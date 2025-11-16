//! Authentication middleware for API keys and JWT validation
//!
//! Implements MCP specification requirements for authentication:
//! - Stateless token validation on every request
//! - WWW-Authenticate header on 401 (RFC 9728)
//! - Audience validation (RFC 8707)

use axum::{
    extract::State,
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;

use crate::axum::config::AuthConfig;

#[cfg(feature = "auth")]
use turbomcp_auth::server::WwwAuthenticateBuilder;

#[cfg(feature = "jwt-validation")]
use crate::axum::config::{JwtAlgorithm, JwtConfig};

#[cfg(feature = "jwt-validation")]
use crate::axum::middleware::jwks::JwksCache;

#[cfg(feature = "jwt-validation")]
use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};

#[cfg(feature = "jwt-validation")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "jwt-validation")]
use dashmap::DashMap;

#[cfg(feature = "jwt-validation")]
use once_cell::sync::Lazy;

#[cfg(feature = "jwt-validation")]
use std::sync::Arc;

/// Authentication error with WWW-Authenticate header support
///
/// Per MCP specification (RFC 9728), 401 responses MUST include
/// WWW-Authenticate header with resource metadata URI.
#[derive(Debug)]
struct AuthError {
    status: StatusCode,
    www_authenticate: Option<String>,
    body: serde_json::Value,
}

impl AuthError {
    /// Create 401 Unauthorized error with WWW-Authenticate header
    ///
    /// # MCP Specification Compliance
    ///
    /// Per RFC 9728 Section 5.1, servers MUST return WWW-Authenticate header:
    /// ```text
    /// WWW-Authenticate: Bearer resource_metadata="https://api.example.com/.well-known/oauth-protected-resource"
    /// ```
    fn unauthorized(metadata_uri: Option<&str>, scope: Option<&str>) -> Self {
        #[cfg(feature = "auth")]
        let www_authenticate = metadata_uri.map(|uri| {
            let mut builder = WwwAuthenticateBuilder::new(uri.to_string());
            if let Some(s) = scope
                && !s.is_empty()
            {
                builder = builder.with_scope(s.to_string());
            }
            builder.build()
        });

        #[cfg(not(feature = "auth"))]
        let www_authenticate = None;

        Self {
            status: StatusCode::UNAUTHORIZED,
            www_authenticate,
            body: json!({
                "error": "unauthorized",
                "error_description": "Valid bearer token required",
            }),
        }
    }

    fn bad_request(message: &str) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            www_authenticate: None,
            body: json!({
                "error": "bad_request",
                "error_description": message,
            }),
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let mut resp = (self.status, axum::Json(self.body)).into_response();

        if let Some(header_value) = self.www_authenticate
            && let Ok(value) = header::HeaderValue::from_str(&header_value)
        {
            resp.headers_mut().insert(header::WWW_AUTHENTICATE, value);
        }

        resp
    }
}

/// JWT Claims structure for validation
#[cfg(feature = "jwt-validation")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwtClaims {
    /// Subject (user ID)
    pub sub: String,
    /// Expiration time (seconds since epoch)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exp: Option<u64>,
    /// Not before time (seconds since epoch)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nbf: Option<u64>,
    /// Issued at (seconds since epoch)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iat: Option<u64>,
    /// Issuer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    /// Audience
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<serde_json::Value>,
    /// Additional claims
    #[serde(flatten)]
    pub additional: std::collections::HashMap<String, serde_json::Value>,
}

/// Global JWKS cache manager
///
/// Maintains separate caches for each JWKS URI to support multiple
/// OAuth providers simultaneously.
#[cfg(feature = "jwt-validation")]
static JWKS_CACHES: Lazy<DashMap<String, Arc<JwksCache>>> = Lazy::new(DashMap::new);

/// Get or create JWKS cache for a given URI
#[cfg(feature = "jwt-validation")]
fn get_jwks_cache(uri: &str) -> Arc<JwksCache> {
    JWKS_CACHES
        .entry(uri.to_string())
        .or_insert_with(|| Arc::new(JwksCache::new(uri.to_string())))
        .clone()
}

/// Extract Bearer token from Authorization header
fn extract_bearer_token(request: &axum::http::Request<axum::body::Body>) -> Option<String> {
    let auth_header = request.headers().get("Authorization")?;
    let auth_str = auth_header.to_str().ok()?;
    auth_str.strip_prefix("Bearer ").map(|s| s.to_string())
}

/// Extract kid (key ID) from JWT header
#[cfg(feature = "jwt-validation")]
fn extract_kid_from_token(token: &str) -> Option<String> {
    use jsonwebtoken::decode_header;

    let header = decode_header(token).ok()?;
    header.kid
}

/// Validate JWT token with proper validation (supports both symmetric and asymmetric)
#[cfg(feature = "jwt-validation")]
async fn validate_jwt_token(token: &str, jwt_config: &JwtConfig) -> Result<JwtClaims, StatusCode> {
    // Map algorithm enum to jsonwebtoken Algorithm
    let algorithm = match jwt_config.algorithm {
        JwtAlgorithm::HS256 => Algorithm::HS256,
        JwtAlgorithm::HS384 => Algorithm::HS384,
        JwtAlgorithm::HS512 => Algorithm::HS512,
        JwtAlgorithm::RS256 => Algorithm::RS256,
        JwtAlgorithm::RS384 => Algorithm::RS384,
        JwtAlgorithm::RS512 => Algorithm::RS512,
        JwtAlgorithm::ES256 => Algorithm::ES256,
        JwtAlgorithm::ES384 => Algorithm::ES384,
    };

    // Create validation config
    let mut validation = Validation::new(algorithm);

    // Configure audience validation
    if let Some(audience) = &jwt_config.audience {
        validation.set_audience(audience);
    }

    // Configure issuer validation
    if let Some(issuer) = &jwt_config.issuer {
        validation.set_issuer(issuer);
    }

    // Configure time-based validations
    validation.validate_exp = jwt_config.validate_exp;
    validation.validate_nbf = jwt_config.validate_nbf;
    validation.leeway = jwt_config.leeway;

    // Get decoding key based on algorithm type
    let decoding_key = if let Some(secret) = &jwt_config.secret {
        // Symmetric key (HS256/HS384/HS512)
        DecodingKey::from_secret(secret.as_bytes())
    } else if let Some(jwks_uri) = &jwt_config.jwks_uri {
        // Asymmetric key (RS256/ES256/etc.) - fetch from JWKS
        // Extract kid from token header
        let kid = extract_kid_from_token(token).ok_or(StatusCode::UNAUTHORIZED)?;

        // Get or create JWKS cache for this provider
        let jwks_cache = get_jwks_cache(jwks_uri);

        // Fetch key from JWKS (cached)
        jwks_cache
            .get_key(&kid)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, kid = %kid, jwks_uri = %jwks_uri, "Failed to fetch JWKS key");
                StatusCode::UNAUTHORIZED
            })?
    } else {
        // No key provided
        tracing::error!("JWT validation configured but no secret or JWKS URI provided");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    // Decode and validate token
    let token_data = decode::<JwtClaims>(token, &decoding_key, &validation).map_err(|e| {
        tracing::debug!(error = %e, "JWT validation failed");
        StatusCode::UNAUTHORIZED
    })?;

    Ok(token_data.claims)
}

/// Authentication middleware - validates tokens and API keys
///
/// # MCP Specification Compliance
///
/// Implements MCP authentication requirements:
/// - Stateless token validation on EVERY request
/// - WWW-Authenticate header on 401 (RFC 9728 Section 5.1)
/// - JWT validation with proper claims (HS256, RS256, ES256, etc.)
/// - API key validation
/// - Clock skew tolerance
///
/// # Example
///
/// ```rust,no_run
/// use turbomcp_transport::axum::AuthConfig;
/// use axum::Router;
///
/// let config = AuthConfig::jwt("secret".to_string())
///     .with_metadata_uri("https://api.example.com/.well-known/oauth-protected-resource")
///     .with_required_scopes(vec!["mcp:tools".to_string()]);
/// ```
pub async fn authentication_middleware(
    State(auth_config): State<AuthConfig>,
    mut request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Response {
    let metadata_uri = auth_config.resource_metadata_uri.as_deref();
    let scope = if !auth_config.required_scopes.is_empty() {
        Some(auth_config.required_scopes.join(" "))
    } else {
        None
    };

    // Check for JWT authentication
    #[cfg(feature = "jwt-validation")]
    if let Some(jwt_config) = &auth_config.jwt {
        if let Some(token) = extract_bearer_token(&request) {
            // Step 1: Validate JWT signature and claims
            let claims = match validate_jwt_token(&token, jwt_config).await {
                Ok(c) => c,
                Err(_) => {
                    return AuthError::unauthorized(metadata_uri, scope.as_deref()).into_response();
                }
            };

            // Step 2: Optional introspection for real-time revocation checking
            #[cfg(feature = "auth")]
            if let Some(ref introspection_endpoint) = jwt_config.introspection_endpoint {
                use turbomcp_auth::introspection::IntrospectionClient;

                let client = IntrospectionClient::new(
                    introspection_endpoint.clone(),
                    jwt_config
                        .introspection_client_id
                        .clone()
                        .unwrap_or_default(),
                    jwt_config.introspection_client_secret.clone(),
                );

                let is_active = match client.is_token_active(&token).await {
                    Ok(active) => active,
                    Err(e) => {
                        tracing::error!(error = %e, "Token introspection failed");
                        return AuthError::unauthorized(metadata_uri, scope.as_deref())
                            .into_response();
                    }
                };

                if !is_active {
                    tracing::warn!(token = %token, "Token revoked per introspection");
                    return AuthError::unauthorized(metadata_uri, scope.as_deref()).into_response();
                }
            }

            // Add authenticated user context to request extensions
            request.extensions_mut().insert(claims);
        } else if auth_config.enabled {
            return AuthError::unauthorized(metadata_uri, scope.as_deref()).into_response();
        }
    }

    // Check for API key authentication
    if let Some(api_key_header) = &auth_config.api_key_header {
        if let Some(provided_key) = request.headers().get(api_key_header) {
            // Validate API key format
            let key_str = match provided_key.to_str() {
                Ok(s) => s,
                Err(_) => return AuthError::bad_request("Invalid API key header").into_response(),
            };

            if key_str.is_empty() {
                return AuthError::unauthorized(metadata_uri, scope.as_deref()).into_response();
            }

            // Clone the key string to drop the immutable borrow before we need mutable access
            let key_string = key_str.to_string();

            // IMPORTANT: This middleware performs basic format validation only.
            //
            // API key VERIFICATION against a store/database must be implemented by the application:
            // - This transport layer should NOT block requests based on invalid API keys
            //   (that is the application's responsibility)
            // - The application handler can use turbomcp_auth::ApiKeyProvider or custom logic
            //   to verify the key against a database/KV store
            // - See: turbomcp_auth::providers::ApiKeyProvider for a reference implementation
            //
            // Rationale: Transport layer provides format validation for MCP compliance.
            // Application layer handles business logic (which keys are valid, rate limits, etc.)

            // Add API key to request extensions for application-layer validation
            request.extensions_mut().insert(key_string);
            tracing::debug!("API key header found, delegating validation to application layer");
        } else if auth_config.enabled && auth_config.jwt.is_none() {
            // Only require API key if JWT is not configured
            return AuthError::unauthorized(metadata_uri, scope.as_deref()).into_response();
        }
    }

    // Continue processing
    next.run(request).await
}

#[cfg(all(test, feature = "jwt-validation"))]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};

    #[tokio::test]
    async fn test_jwt_validation_hs256() {
        let secret = "test-secret";
        let claims = JwtClaims {
            sub: "user123".to_string(),
            exp: Some((chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as u64),
            nbf: None,
            iat: Some(chrono::Utc::now().timestamp() as u64),
            iss: Some("test-issuer".to_string()),
            aud: Some(serde_json::json!("test-audience")),
            additional: std::collections::HashMap::new(),
        };

        // Create token
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        // Create config
        let jwt_config = JwtConfig {
            secret: Some(secret.to_string()),
            algorithm: JwtAlgorithm::HS256,
            audience: Some(vec!["test-audience".to_string()]),
            issuer: Some(vec!["test-issuer".to_string()]),
            ..Default::default()
        };

        // Validate
        let result = validate_jwt_token(&token, &jwt_config).await;
        assert!(result.is_ok());

        let validated_claims = result.unwrap();
        assert_eq!(validated_claims.sub, "user123");
    }

    #[tokio::test]
    async fn test_jwt_validation_expired() {
        let secret = "test-secret";
        let claims = JwtClaims {
            sub: "user123".to_string(),
            exp: Some((chrono::Utc::now() - chrono::Duration::hours(1)).timestamp() as u64),
            nbf: None,
            iat: Some(chrono::Utc::now().timestamp() as u64),
            iss: None,
            aud: None,
            additional: std::collections::HashMap::new(),
        };

        // Create token
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        // Create config
        let jwt_config = JwtConfig {
            secret: Some(secret.to_string()),
            algorithm: JwtAlgorithm::HS256,
            ..Default::default()
        };

        // Validate (should fail due to expiration)
        let result = validate_jwt_token(&token, &jwt_config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_jwt_validation_invalid_audience() {
        let secret = "test-secret";
        let claims = JwtClaims {
            sub: "user123".to_string(),
            exp: Some((chrono::Utc::now() + chrono::Duration::hours(1)).timestamp() as u64),
            nbf: None,
            iat: Some(chrono::Utc::now().timestamp() as u64),
            iss: None,
            aud: Some(serde_json::json!("wrong-audience")),
            additional: std::collections::HashMap::new(),
        };

        // Create token
        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(secret.as_bytes()),
        )
        .unwrap();

        // Create config with different audience
        let jwt_config = JwtConfig {
            secret: Some(secret.to_string()),
            algorithm: JwtAlgorithm::HS256,
            audience: Some(vec!["expected-audience".to_string()]),
            ..Default::default()
        };

        // Validate (should fail due to audience mismatch)
        let result = validate_jwt_token(&token, &jwt_config).await;
        assert!(result.is_err());
    }
}
