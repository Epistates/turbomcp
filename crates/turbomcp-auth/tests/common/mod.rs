//! Common test utilities for integration tests
//!
//! This module provides shared infrastructure for testing OAuth2 flows,
//! DPoP proofs, JWT validation, and other authentication scenarios.

#![allow(dead_code)]

use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};
use wiremock::{
    matchers::{method, path},
    Mock, MockServer, ResponseTemplate,
};

/// OAuth2 mock server configuration
pub struct MockOAuth2Server {
    pub server: MockServer,
    pub token_endpoint: String,
    pub authorize_endpoint: String,
    pub jwks_endpoint: String,
}

impl MockOAuth2Server {
    /// Create a new mock OAuth2 authorization server
    pub async fn start() -> Self {
        let server = MockServer::start().await;
        let base_url = server.uri();

        Self {
            server,
            token_endpoint: format!("{}/token", base_url),
            authorize_endpoint: format!("{}/authorize", base_url),
            jwks_endpoint: format!("{}/jwks", base_url),
        }
    }

    /// Mock successful token endpoint response (OAuth2 token exchange)
    pub async fn mock_token_success(&self, access_token: &str, refresh_token: Option<&str>) {
        let mut response_body = json!({
            "access_token": access_token,
            "token_type": "Bearer",
            "expires_in": 3600,
            "scope": "openid profile email",
        });

        if let Some(refresh) = refresh_token {
            response_body["refresh_token"] = json!(refresh);
        }

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(response_body))
            .mount(&self.server)
            .await;
    }

    /// Mock token endpoint with DPoP support
    pub async fn mock_token_with_dpop(&self, access_token: &str, dpop_nonce: Option<&str>) {
        let mut response = ResponseTemplate::new(200).set_body_json(json!({
            "access_token": access_token,
            "token_type": "DPoP",
            "expires_in": 3600,
        }));

        if let Some(nonce) = dpop_nonce {
            response = response.insert_header("DPoP-Nonce", nonce);
        }

        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(response)
            .mount(&self.server)
            .await;
    }

    /// Mock token endpoint error response
    pub async fn mock_token_error(&self, error: &str, description: &str) {
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(
                ResponseTemplate::new(400).set_body_json(json!({
                    "error": error,
                    "error_description": description,
                })),
            )
            .mount(&self.server)
            .await;
    }

    /// Mock JWKS endpoint with a sample RSA key
    pub async fn mock_jwks(&self, jwk: serde_json::Value) {
        Mock::given(method("GET"))
            .and(path("/jwks"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({
                    "keys": [jwk]
                })),
            )
            .mount(&self.server)
            .await;
    }

    /// Mock authorization endpoint (for testing redirect flows)
    pub async fn mock_authorize_redirect(&self, redirect_uri: &str, code: &str, state: &str) {
        Mock::given(method("GET"))
            .and(path("/authorize"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", format!("{}?code={}&state={}", redirect_uri, code, state)),
            )
            .mount(&self.server)
            .await;
    }
}

/// Generate a test JWT with custom claims
pub fn generate_test_jwt(
    claims: serde_json::Value,
    private_key: &[u8],
    algorithm: jsonwebtoken::Algorithm,
) -> String {
    use jsonwebtoken::{encode, EncodingKey, Header};

    let key = match algorithm {
        jsonwebtoken::Algorithm::RS256 => {
            EncodingKey::from_rsa_pem(private_key).expect("Invalid RSA key")
        }
        jsonwebtoken::Algorithm::ES256 => {
            EncodingKey::from_ec_pem(private_key).expect("Invalid EC key")
        }
        _ => panic!("Unsupported algorithm for test JWT"),
    };

    let mut header = Header::new(algorithm);
    header.typ = Some("JWT".to_string());

    encode(&header, &claims, &key).expect("Failed to encode test JWT")
}

/// Generate a test RSA key pair (PEM format) for testing
pub fn generate_test_rsa_keypair() -> (Vec<u8>, Vec<u8>) {
    use rsa::{pkcs8::EncodePrivateKey, pkcs8::EncodePublicKey, RsaPrivateKey};
    use rsa::pkcs8::LineEnding;

    let mut rng = rand::thread_rng();
    let bits = 2048;
    let private_key = RsaPrivateKey::new(&mut rng, bits).expect("Failed to generate RSA key");
    let public_key = private_key.to_public_key();

    let private_pem = private_key
        .to_pkcs8_pem(LineEnding::LF)
        .expect("Failed to encode private key")
        .as_bytes()
        .to_vec();

    let public_pem = public_key
        .to_public_key_pem(LineEnding::LF)
        .expect("Failed to encode public key")
        .as_bytes()
        .to_vec();

    (private_pem, public_pem)
}

/// Get current Unix timestamp
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

/// Create test JWT claims with standard fields
pub fn test_jwt_claims(
    sub: &str,
    iss: &str,
    aud: &str,
    exp_offset_secs: i64,
) -> serde_json::Value {
    let now = current_timestamp();
    json!({
        "sub": sub,
        "iss": iss,
        "aud": aud,
        "exp": (now as i64 + exp_offset_secs) as u64,
        "iat": now,
        "nbf": now,
    })
}

/// Calculate SHA-256 hash (for DPoP ath claim)
pub fn sha256_hash(data: &str) -> String {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    let result = hasher.finalize();
    URL_SAFE_NO_PAD.encode(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_server_startup() {
        let mock = MockOAuth2Server::start().await;
        assert!(mock.token_endpoint.contains("/token"));
        assert!(mock.authorize_endpoint.contains("/authorize"));
    }

    #[test]
    fn test_current_timestamp() {
        let ts = current_timestamp();
        assert!(ts > 1700000000); // After Nov 2023
    }

    #[test]
    fn test_sha256_hash() {
        let hash = sha256_hash("test_access_token");
        assert!(!hash.is_empty());
        assert!(!hash.contains('=')); // URL-safe, no padding
    }
}
