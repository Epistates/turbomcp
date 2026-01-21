//! JWT validation for WASM environments using Web Crypto API.
//!
//! This module provides JWT validation for Cloudflare Workers and other
//! WASM environments that support the Web Crypto API.

use super::jwks::{Jwk, JwksCache};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use turbomcp_core::auth::{
    AuthError, Authenticator, Credential, CredentialExtractor, JwtAlgorithm, JwtConfig, Principal,
};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// WASM JWT Authenticator using Web Crypto API.
///
/// Validates JWTs by verifying signatures using the Web Crypto API
/// and checking standard claims.
///
/// # Example
///
/// ```ignore
/// use turbomcp_wasm::auth::{WasmJwtAuthenticator, JwtConfig};
///
/// let config = JwtConfig::new()
///     .issuer("https://auth.example.com")
///     .audience("my-api");
///
/// let auth = WasmJwtAuthenticator::with_jwks(
///     "https://auth.example.com/.well-known/jwks.json",
///     config,
/// );
///
/// let principal = auth.authenticate(&Credential::bearer("eyJ...")).await?;
/// ```
#[derive(Clone)]
pub struct WasmJwtAuthenticator {
    /// JWKS cache for fetching and caching public keys
    jwks_cache: JwksCache,

    /// JWT validation configuration
    config: JwtConfig,
}

impl WasmJwtAuthenticator {
    /// Create a new authenticator with a JWKS endpoint
    pub fn with_jwks(jwks_url: impl Into<String>, config: JwtConfig) -> Self {
        Self {
            jwks_cache: JwksCache::new(jwks_url),
            config,
        }
    }

    /// Create a new authenticator with a JWKS cache
    pub fn with_cache(cache: JwksCache, config: JwtConfig) -> Self {
        Self {
            jwks_cache: cache,
            config,
        }
    }

    /// Parse a JWT into its header, payload, and signature parts
    fn parse_jwt(token: &str) -> Result<(JwtHeader, JwtPayload, Vec<u8>, String), AuthError> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(AuthError::InvalidCredentialFormat(
                "JWT must have 3 parts".to_string(),
            ));
        }

        // Decode header
        let header_bytes = base64_url_decode(parts[0])?;
        let header: JwtHeader = serde_json::from_slice(&header_bytes)
            .map_err(|e| AuthError::InvalidCredentialFormat(format!("Invalid header: {}", e)))?;

        // Decode payload
        let payload_bytes = base64_url_decode(parts[1])?;
        let payload: JwtPayload = serde_json::from_slice(&payload_bytes)
            .map_err(|e| AuthError::InvalidCredentialFormat(format!("Invalid payload: {}", e)))?;

        // Decode signature
        let signature = base64_url_decode(parts[2])?;

        // The signing input is header.payload (raw, not decoded)
        let signing_input = format!("{}.{}", parts[0], parts[1]);

        Ok((header, payload, signature, signing_input))
    }

    /// Verify JWT signature using Web Crypto API
    async fn verify_signature(
        &self,
        jwk: &Jwk,
        algorithm: JwtAlgorithm,
        signing_input: &str,
        signature: &[u8],
    ) -> Result<bool, AuthError> {
        let window = web_sys::window()
            .ok_or_else(|| AuthError::Internal("No window object available".to_string()))?;

        let crypto = window
            .crypto()
            .map_err(|_| AuthError::Internal("No crypto object available".to_string()))?;

        let subtle = crypto.subtle();

        // Import the key
        let crypto_key = self.import_key(&subtle, jwk, algorithm).await?;

        // Create algorithm object for verification
        let algo = self.create_verify_algorithm(algorithm)?;

        // Verify the signature
        let data = js_sys::Uint8Array::from(signing_input.as_bytes());
        let sig = js_sys::Uint8Array::from(signature);

        let promise = subtle
            .verify_with_object_and_buffer_source_and_buffer_source(&algo, &crypto_key, &sig, &data)
            .map_err(|e| AuthError::Internal(format!("Verify call failed: {:?}", e)))?;

        let result = JsFuture::from(promise)
            .await
            .map_err(|e| AuthError::Internal(format!("Verification failed: {:?}", e)))?;

        Ok(result.as_bool().unwrap_or(false))
    }

    /// Import a JWK as a CryptoKey
    async fn import_key(
        &self,
        subtle: &web_sys::SubtleCrypto,
        jwk: &Jwk,
        algorithm: JwtAlgorithm,
    ) -> Result<web_sys::CryptoKey, AuthError> {
        let web_jwk = jwk.to_web_sys_jwk();
        let algo = self.create_import_algorithm(algorithm)?;
        let usages = js_sys::Array::new();
        usages.push(&JsValue::from_str("verify"));

        let promise = subtle
            .import_key_with_object("jwk", &web_jwk, &algo, false, &usages)
            .map_err(|e| AuthError::Internal(format!("Import key failed: {:?}", e)))?;

        let result = JsFuture::from(promise)
            .await
            .map_err(|e| AuthError::Internal(format!("Key import failed: {:?}", e)))?;

        result
            .dyn_into::<web_sys::CryptoKey>()
            .map_err(|_| AuthError::Internal("Failed to convert to CryptoKey".to_string()))
    }

    /// Create algorithm object for key import
    fn create_import_algorithm(
        &self,
        algorithm: JwtAlgorithm,
    ) -> Result<js_sys::Object, AuthError> {
        let algo = js_sys::Object::new();

        match algorithm {
            JwtAlgorithm::RS256 | JwtAlgorithm::RS384 | JwtAlgorithm::RS512 => {
                js_sys::Reflect::set(&algo, &"name".into(), &"RSASSA-PKCS1-v1_5".into())
                    .map_err(|_| AuthError::Internal("Failed to set algorithm name".to_string()))?;

                let hash = match algorithm {
                    JwtAlgorithm::RS256 => "SHA-256",
                    JwtAlgorithm::RS384 => "SHA-384",
                    JwtAlgorithm::RS512 => "SHA-512",
                    _ => unreachable!(),
                };

                let hash_obj = js_sys::Object::new();
                js_sys::Reflect::set(&hash_obj, &"name".into(), &hash.into())
                    .map_err(|_| AuthError::Internal("Failed to set hash name".to_string()))?;
                js_sys::Reflect::set(&algo, &"hash".into(), &hash_obj)
                    .map_err(|_| AuthError::Internal("Failed to set hash object".to_string()))?;
            }
            JwtAlgorithm::ES256 | JwtAlgorithm::ES384 => {
                js_sys::Reflect::set(&algo, &"name".into(), &"ECDSA".into())
                    .map_err(|_| AuthError::Internal("Failed to set algorithm name".to_string()))?;

                let curve = match algorithm {
                    JwtAlgorithm::ES256 => "P-256",
                    JwtAlgorithm::ES384 => "P-384",
                    _ => unreachable!(),
                };

                js_sys::Reflect::set(&algo, &"namedCurve".into(), &curve.into())
                    .map_err(|_| AuthError::Internal("Failed to set curve".to_string()))?;
            }
            JwtAlgorithm::HS256 | JwtAlgorithm::HS384 | JwtAlgorithm::HS512 => {
                js_sys::Reflect::set(&algo, &"name".into(), &"HMAC".into())
                    .map_err(|_| AuthError::Internal("Failed to set algorithm name".to_string()))?;

                let hash = match algorithm {
                    JwtAlgorithm::HS256 => "SHA-256",
                    JwtAlgorithm::HS384 => "SHA-384",
                    JwtAlgorithm::HS512 => "SHA-512",
                    _ => unreachable!(),
                };

                let hash_obj = js_sys::Object::new();
                js_sys::Reflect::set(&hash_obj, &"name".into(), &hash.into())
                    .map_err(|_| AuthError::Internal("Failed to set hash name".to_string()))?;
                js_sys::Reflect::set(&algo, &"hash".into(), &hash_obj)
                    .map_err(|_| AuthError::Internal("Failed to set hash object".to_string()))?;
            }
        }

        Ok(algo)
    }

    /// Create algorithm object for signature verification
    fn create_verify_algorithm(
        &self,
        algorithm: JwtAlgorithm,
    ) -> Result<js_sys::Object, AuthError> {
        let algo = js_sys::Object::new();

        match algorithm {
            JwtAlgorithm::RS256 | JwtAlgorithm::RS384 | JwtAlgorithm::RS512 => {
                js_sys::Reflect::set(&algo, &"name".into(), &"RSASSA-PKCS1-v1_5".into())
                    .map_err(|_| AuthError::Internal("Failed to set algorithm name".to_string()))?;
            }
            JwtAlgorithm::ES256 | JwtAlgorithm::ES384 => {
                js_sys::Reflect::set(&algo, &"name".into(), &"ECDSA".into())
                    .map_err(|_| AuthError::Internal("Failed to set algorithm name".to_string()))?;

                let hash = match algorithm {
                    JwtAlgorithm::ES256 => "SHA-256",
                    JwtAlgorithm::ES384 => "SHA-384",
                    _ => unreachable!(),
                };

                let hash_obj = js_sys::Object::new();
                js_sys::Reflect::set(&hash_obj, &"name".into(), &hash.into())
                    .map_err(|_| AuthError::Internal("Failed to set hash name".to_string()))?;
                js_sys::Reflect::set(&algo, &"hash".into(), &hash_obj)
                    .map_err(|_| AuthError::Internal("Failed to set hash object".to_string()))?;
            }
            JwtAlgorithm::HS256 | JwtAlgorithm::HS384 | JwtAlgorithm::HS512 => {
                js_sys::Reflect::set(&algo, &"name".into(), &"HMAC".into())
                    .map_err(|_| AuthError::Internal("Failed to set algorithm name".to_string()))?;
            }
        }

        Ok(algo)
    }

    /// Validate JWT claims
    fn validate_claims(&self, payload: &JwtPayload) -> Result<(), AuthError> {
        let now = (js_sys::Date::now() / 1000.0) as u64;

        // Validate expiration
        if self.config.validate_exp
            && let Some(exp) = payload.exp
            && now > exp + self.config.leeway_seconds
        {
            return Err(AuthError::TokenExpired);
        }

        // Validate not-before
        if self.config.validate_nbf
            && let Some(nbf) = payload.nbf
            && now + self.config.leeway_seconds < nbf
        {
            return Err(AuthError::InvalidClaims(
                "Token not yet valid (nbf)".to_string(),
            ));
        }

        // Validate issuer
        if let Some(ref expected_iss) = self.config.issuer {
            if let Some(ref actual_iss) = payload.iss {
                if actual_iss != expected_iss {
                    return Err(AuthError::InvalidIssuer {
                        expected: expected_iss.clone(),
                        actual: actual_iss.clone(),
                    });
                }
            } else {
                return Err(AuthError::InvalidClaims("Missing issuer claim".to_string()));
            }
        }

        // Validate audience
        if let Some(ref expected_aud) = self.config.audience {
            let valid = match &payload.aud {
                Some(Audience::Single(aud)) => aud == expected_aud,
                Some(Audience::Multiple(auds)) => auds.iter().any(|a| a == expected_aud),
                None => false,
            };
            if !valid {
                let actual = payload
                    .aud
                    .as_ref()
                    .map(|a| match a {
                        Audience::Single(s) => s.clone(),
                        Audience::Multiple(v) => v.join(", "),
                    })
                    .unwrap_or_default();
                return Err(AuthError::InvalidAudience {
                    expected: expected_aud.clone(),
                    actual,
                });
            }
        }

        Ok(())
    }

    /// Convert validated payload to Principal
    fn payload_to_principal(&self, payload: JwtPayload) -> Principal {
        let subject = payload.sub.clone().unwrap_or_else(|| "unknown".to_string());

        let mut principal = Principal::new(subject);

        if let Some(iss) = payload.iss {
            principal = principal.with_issuer(iss);
        }

        if let Some(ref aud) = payload.aud {
            let aud_str = match aud {
                Audience::Single(s) => s.clone(),
                Audience::Multiple(v) => v.first().cloned().unwrap_or_default(),
            };
            principal = principal.with_audience(aud_str);
        }

        if let Some(exp) = payload.exp {
            principal = principal.with_expires_at(exp);
        }

        if let Some(email) = payload.email {
            principal = principal.with_email(email);
        }

        if let Some(name) = payload.name {
            principal = principal.with_name(name);
        }

        // Add extra claims
        for (key, value) in payload.extra {
            principal = principal.with_claim(key, value);
        }

        principal
    }
}

impl std::fmt::Debug for WasmJwtAuthenticator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmJwtAuthenticator")
            .field("jwks_cache", &self.jwks_cache)
            .field("config", &self.config)
            .finish()
    }
}

impl Authenticator for WasmJwtAuthenticator {
    type Error = AuthError;

    async fn authenticate(&self, credential: &Credential) -> Result<Principal, Self::Error> {
        // Extract the token
        let token = credential
            .as_bearer()
            .ok_or(AuthError::UnsupportedCredentialType)?;

        // Parse the JWT
        let (header, payload, signature, signing_input) = Self::parse_jwt(token)?;

        // Get the algorithm
        let algorithm = header
            .alg
            .as_ref()
            .and_then(|a| a.parse().ok())
            .ok_or_else(|| {
                AuthError::InvalidCredentialFormat("Missing or invalid algorithm".to_string())
            })?;

        // Check if algorithm is allowed
        if !self.config.algorithms.is_empty() && !self.config.algorithms.contains(&algorithm) {
            return Err(AuthError::InvalidCredentialFormat(format!(
                "Algorithm {} not allowed",
                algorithm
            )));
        }

        // Try to verify signature, with automatic key rotation handling
        let result = self
            .verify_with_key_rotation(&header, algorithm, &signing_input, &signature)
            .await;

        match result {
            Ok(true) => {
                // Validate claims
                self.validate_claims(&payload)?;
                // Convert to principal
                Ok(self.payload_to_principal(payload))
            }
            Ok(false) => Err(AuthError::InvalidSignature),
            Err(e) => Err(e),
        }
    }
}

impl WasmJwtAuthenticator {
    /// Verify signature with automatic key rotation handling.
    ///
    /// If signature verification fails and a `kid` is specified, refreshes the
    /// JWKS cache and retries once. This handles the case where keys were
    /// rotated (Cloudflare rotates every 6 weeks).
    async fn verify_with_key_rotation(
        &self,
        header: &JwtHeader,
        algorithm: JwtAlgorithm,
        signing_input: &str,
        signature: &[u8],
    ) -> Result<bool, AuthError> {
        // Get the signing key
        let jwk = if let Some(ref kid) = header.kid {
            self.jwks_cache.find_key(kid).await?
        } else {
            // No kid, try to find a key by algorithm
            let jwks = self.jwks_cache.get_jwks().await?;
            jwks.find_by_algorithm(algorithm)
                .or_else(|| jwks.first_signing_key())
                .cloned()
                .ok_or_else(|| AuthError::KeyNotFound("No suitable key found".to_string()))?
        };

        // First attempt at signature verification
        let valid = self
            .verify_signature(&jwk, algorithm, signing_input, signature)
            .await?;

        if valid {
            return Ok(true);
        }

        // If signature failed and we have a kid, try refreshing the JWKS
        // This handles key rotation scenarios
        if let Some(ref kid) = header.kid {
            // Force refresh the JWKS cache
            if self.jwks_cache.refresh().await.is_ok() {
                // Try to find the key again
                if let Ok(refreshed_jwk) = self.jwks_cache.find_key(kid).await {
                    // Retry verification with the refreshed key
                    return self
                        .verify_signature(&refreshed_jwk, algorithm, signing_input, signature)
                        .await;
                }
            }
        }

        Ok(false)
    }
}

/// Cloudflare Access authenticator.
///
/// Validates JWTs from Cloudflare Access using the team's JWKS endpoint.
///
/// # Example
///
/// ```ignore
/// use turbomcp_wasm::auth::CloudflareAccessAuthenticator;
///
/// let auth = CloudflareAccessAuthenticator::new(
///     "your-team",
///     "your-audience-tag",
/// );
///
/// // Extract from Cf-Access-Jwt-Assertion header
/// let token = request.headers().get("Cf-Access-Jwt-Assertion")?;
/// let principal = auth.authenticate(&Credential::bearer(token)).await?;
/// ```
#[derive(Clone, Debug)]
pub struct CloudflareAccessAuthenticator {
    inner: WasmJwtAuthenticator,
}

impl CloudflareAccessAuthenticator {
    /// Create a new Cloudflare Access authenticator
    ///
    /// # Arguments
    ///
    /// * `team_name` - Your Cloudflare One team name
    /// * `audience` - Application Audience (AUD) tag
    pub fn new(team_name: impl Into<String>, audience: impl Into<String>) -> Self {
        let team_name = team_name.into();
        let audience = audience.into();

        let jwks_url = format!(
            "https://{}.cloudflareaccess.com/cdn-cgi/access/certs",
            team_name
        );

        let issuer = format!("https://{}.cloudflareaccess.com", team_name);

        let config = JwtConfig::new()
            .issuer(issuer)
            .audience(audience)
            .algorithms(vec![JwtAlgorithm::RS256]);

        Self {
            inner: WasmJwtAuthenticator::with_jwks(jwks_url, config),
        }
    }

    /// Create authenticator with custom configuration
    pub fn with_config(team_name: impl Into<String>, config: JwtConfig) -> Self {
        let team_name = team_name.into();
        let jwks_url = format!(
            "https://{}.cloudflareaccess.com/cdn-cgi/access/certs",
            team_name
        );

        Self {
            inner: WasmJwtAuthenticator::with_jwks(jwks_url, config),
        }
    }

    /// Authenticate a request by extracting the Cf-Access-Jwt-Assertion header
    ///
    /// This is a convenience method for Cloudflare Workers that extracts
    /// the token from the correct header.
    pub async fn authenticate_request(
        &self,
        request: &worker::Request,
    ) -> Result<Principal, AuthError> {
        let headers = request.headers();

        // Try the JWT assertion header (recommended)
        let token = headers
            .get("Cf-Access-Jwt-Assertion")
            .ok()
            .flatten()
            .or_else(|| {
                // Fall back to Authorization header
                headers
                    .get("Authorization")
                    .ok()
                    .flatten()
                    .and_then(|h| h.strip_prefix("Bearer ").map(String::from))
            })
            .ok_or(AuthError::MissingCredentials)?;

        self.authenticate(&Credential::bearer(token)).await
    }
}

impl Authenticator for CloudflareAccessAuthenticator {
    type Error = AuthError;

    async fn authenticate(&self, credential: &Credential) -> Result<Principal, Self::Error> {
        self.inner.authenticate(credential).await
    }
}

/// Credential extractor for Cloudflare Access.
///
/// Extracts credentials from the `Cf-Access-Jwt-Assertion` header
/// or falls back to the `Authorization: Bearer` header.
#[derive(Debug, Clone, Copy, Default)]
pub struct CloudflareAccessExtractor;

impl CredentialExtractor for CloudflareAccessExtractor {
    fn extract<F>(&self, get_header: F) -> Option<Credential>
    where
        F: Fn(&str) -> Option<String>,
    {
        // Try CF Access header first
        if let Some(token) = get_header("cf-access-jwt-assertion") {
            return Some(Credential::bearer(token));
        }

        // Fall back to Authorization header
        if let Some(auth) = get_header("authorization")
            && let Some(token) = auth
                .strip_prefix("Bearer ")
                .or_else(|| auth.strip_prefix("bearer "))
        {
            return Some(Credential::bearer(token.trim()));
        }

        None
    }
}

// ============================================================================
// JWT Types
// ============================================================================

/// JWT header
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JwtHeader {
    /// Algorithm
    alg: Option<String>,

    /// Key ID
    kid: Option<String>,

    /// Token type
    typ: Option<String>,
}

/// JWT audience (single or array)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Audience {
    Single(String),
    Multiple(Vec<String>),
}

/// JWT payload with standard and extra claims
#[derive(Debug, Clone, Serialize, Deserialize)]
struct JwtPayload {
    /// Subject
    #[serde(skip_serializing_if = "Option::is_none")]
    sub: Option<String>,

    /// Issuer
    #[serde(skip_serializing_if = "Option::is_none")]
    iss: Option<String>,

    /// Audience
    #[serde(skip_serializing_if = "Option::is_none")]
    aud: Option<Audience>,

    /// Expiration
    #[serde(skip_serializing_if = "Option::is_none")]
    exp: Option<u64>,

    /// Not before
    #[serde(skip_serializing_if = "Option::is_none")]
    nbf: Option<u64>,

    /// Issued at
    #[serde(skip_serializing_if = "Option::is_none")]
    iat: Option<u64>,

    /// JWT ID
    #[serde(skip_serializing_if = "Option::is_none")]
    jti: Option<String>,

    /// Email (common claim)
    #[serde(skip_serializing_if = "Option::is_none")]
    email: Option<String>,

    /// Name (common claim)
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,

    /// Extra claims
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

// ============================================================================
// Base64 URL Decoding
// ============================================================================

/// Decode base64url-encoded data
fn base64_url_decode(input: &str) -> Result<Vec<u8>, AuthError> {
    // Add padding if needed
    let padded = match input.len() % 4 {
        2 => format!("{}==", input),
        3 => format!("{}=", input),
        _ => input.to_string(),
    };

    // Convert from base64url to standard base64
    let standard: String = padded
        .chars()
        .map(|c| match c {
            '-' => '+',
            '_' => '/',
            c => c,
        })
        .collect();

    // Decode using browser's atob
    let window =
        web_sys::window().ok_or_else(|| AuthError::Internal("No window object".to_string()))?;

    let decoded = window
        .atob(&standard)
        .map_err(|_| AuthError::InvalidCredentialFormat("Invalid base64".to_string()))?;

    // Convert to bytes
    Ok(decoded.bytes().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_url_decode() {
        // This test would need to run in a browser/wasm environment
        // since it uses window.atob
    }

    #[test]
    fn test_jwt_header_parse() {
        let header_json = r#"{"alg":"RS256","kid":"key1","typ":"JWT"}"#;
        let header: JwtHeader = serde_json::from_str(header_json).unwrap();
        assert_eq!(header.alg, Some("RS256".to_string()));
        assert_eq!(header.kid, Some("key1".to_string()));
    }

    #[test]
    fn test_jwt_payload_parse() {
        let payload_json = r#"{
            "sub": "user123",
            "iss": "https://auth.example.com",
            "aud": "my-api",
            "exp": 1704067200,
            "email": "user@example.com",
            "custom_claim": "custom_value"
        }"#;
        let payload: JwtPayload = serde_json::from_str(payload_json).unwrap();
        assert_eq!(payload.sub, Some("user123".to_string()));
        assert_eq!(payload.iss, Some("https://auth.example.com".to_string()));
        assert!(payload.extra.contains_key("custom_claim"));
    }

    #[test]
    fn test_cloudflare_access_extractor() {
        let extractor = CloudflareAccessExtractor;

        // Test CF Access header
        let cred = extractor.extract(|name| {
            if name == "cf-access-jwt-assertion" {
                Some("my-cf-token".to_string())
            } else {
                None
            }
        });
        assert_eq!(cred, Some(Credential::bearer("my-cf-token")));

        // Test fallback to Authorization
        let cred2 = extractor.extract(|name| {
            if name == "authorization" {
                Some("Bearer my-bearer-token".to_string())
            } else {
                None
            }
        });
        assert_eq!(cred2, Some(Credential::bearer("my-bearer-token")));
    }
}
