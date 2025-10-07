//! JWT Authentication middleware using well-established jsonwebtoken library
//!
//! This middleware handles JWT token verification and user identity extraction.
//! It follows security best practices for token validation and claim extraction.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{SystemTime, UNIX_EPOCH};

use jsonwebtoken::{Algorithm, DecodingKey, Validation, decode};
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};
use tower::{Layer, Service};
use tracing::{debug, warn};

/// JWT Claims structure
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// Subject (user ID)
    pub sub: String,
    /// User roles for authorization
    pub roles: Vec<String>,
    /// Token expiration time (Unix timestamp)
    pub exp: u64,
    /// Token issued at (Unix timestamp)
    pub iat: u64,
    /// Issuer
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    /// Audience
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
}

impl Claims {
    /// Check if the token is expired
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        self.exp < now
    }

    /// Check if user has a specific role
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.iter().any(|r| r == role)
    }
}

/// Authentication configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// JWT secret key for verification
    pub secret: Secret<String>,
    /// Algorithm to use for verification
    pub algorithm: Algorithm,
    /// Required issuer (optional)
    pub issuer: Option<String>,
    /// Required audience (optional)
    pub audience: Option<String>,
    /// Leeway for clock skew (seconds)
    pub leeway: u64,
    /// Whether to validate expiration
    pub validate_exp: bool,
    /// Whether to validate not before
    pub validate_nbf: bool,
}

impl AuthConfig {
    /// Create new auth config with explicit secret
    ///
    /// # Example
    /// ```rust
    /// use turbomcp_server::middleware::auth::AuthConfig;
    /// use secrecy::Secret;
    ///
    /// let config = AuthConfig::new(Secret::new("your-secret-key".to_string()));
    /// ```
    pub fn new(secret: Secret<String>) -> Self {
        Self {
            secret,
            algorithm: Algorithm::HS256,
            issuer: None,
            audience: None,
            leeway: 60, // 1 minute leeway
            validate_exp: true,
            validate_nbf: true,
        }
    }

    /// Load auth config from environment variables
    ///
    /// Required environment variables:
    /// - `AUTH_JWT_SECRET`: JWT secret key
    ///
    /// Optional environment variables:
    /// - `AUTH_JWT_ALGORITHM`: Algorithm (default: HS256)
    /// - `AUTH_JWT_ISSUER`: Required issuer
    /// - `AUTH_JWT_AUDIENCE`: Required audience
    /// - `AUTH_JWT_LEEWAY`: Clock skew leeway in seconds (default: 60)
    ///
    /// # Example
    /// ```rust,no_run
    /// use turbomcp_server::middleware::auth::AuthConfig;
    ///
    /// let config = AuthConfig::from_env().expect("Failed to load auth config");
    /// ```
    ///
    /// # Errors
    /// Returns error if `AUTH_JWT_SECRET` is not set.
    pub fn from_env() -> Result<Self, String> {
        let secret = std::env::var("AUTH_JWT_SECRET")
            .map_err(|_| "AUTH_JWT_SECRET environment variable not set. See PRODUCTION_DEPLOYMENT.md for configuration guide.".to_string())?;

        let algorithm = std::env::var("AUTH_JWT_ALGORITHM")
            .ok()
            .and_then(|s| match s.as_str() {
                "HS256" => Some(Algorithm::HS256),
                "HS384" => Some(Algorithm::HS384),
                "HS512" => Some(Algorithm::HS512),
                _ => None,
            })
            .unwrap_or(Algorithm::HS256);

        let issuer = std::env::var("AUTH_JWT_ISSUER").ok();
        let audience = std::env::var("AUTH_JWT_AUDIENCE").ok();

        let leeway = std::env::var("AUTH_JWT_LEEWAY")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(60);

        Ok(Self {
            secret: Secret::new(secret),
            algorithm,
            issuer,
            audience,
            leeway,
            validate_exp: true,
            validate_nbf: true,
        })
    }

    /// Set the algorithm
    pub fn with_algorithm(mut self, algorithm: Algorithm) -> Self {
        self.algorithm = algorithm;
        self
    }

    /// Set required issuer
    pub fn with_issuer(mut self, issuer: String) -> Self {
        self.issuer = Some(issuer);
        self
    }

    /// Set required audience
    pub fn with_audience(mut self, audience: String) -> Self {
        self.audience = Some(audience);
        self
    }

    /// Set clock skew leeway
    pub fn with_leeway(mut self, leeway: u64) -> Self {
        self.leeway = leeway;
        self
    }
}

/// JWT Authentication layer
#[derive(Debug, Clone)]
pub struct AuthLayer {
    config: AuthConfig,
}

impl AuthLayer {
    /// Create new authentication layer
    pub fn new(config: AuthConfig) -> Self {
        Self { config }
    }
}

impl<S> Layer<S> for AuthLayer {
    type Service = AuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService {
            inner,
            config: self.config.clone(),
        }
    }
}

/// JWT Authentication service
#[derive(Debug, Clone)]
pub struct AuthService<S> {
    inner: S,
    config: AuthConfig,
}

impl<S, ReqBody> Service<http::Request<ReqBody>> for AuthService<S>
where
    S: Service<http::Request<ReqBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<ReqBody>) -> Self::Future {
        let config = self.config.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Extract and validate JWT token
            match extract_and_validate_token(&req, &config) {
                Ok(claims) => {
                    debug!(user_id = %claims.sub, "Authentication successful");

                    // Add claims to request extensions for downstream use
                    req.extensions_mut().insert(claims);
                }
                Err(error) => {
                    warn!(?error, "Authentication failed");

                    // For now, continue without authentication (graceful degradation)
                    // In a production system, you might want to return 401 Unauthorized
                    // depending on your security requirements
                }
            }

            inner.call(req).await
        })
    }
}

/// JWT authentication error types
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    /// Authorization header is missing from request
    #[error("Missing authorization header")]
    MissingAuthHeader,
    /// Authorization header format is invalid (not Bearer token)
    #[error("Invalid authorization header format")]
    InvalidAuthFormat,
    /// JWT token validation failed
    #[error("Invalid JWT token: {0}")]
    InvalidToken(#[from] jsonwebtoken::errors::Error),
    /// JWT token has expired
    #[error("Token expired")]
    TokenExpired,
}

/// Extract and validate JWT token from request
fn extract_and_validate_token<B>(
    req: &http::Request<B>,
    config: &AuthConfig,
) -> Result<Claims, AuthError> {
    // Extract Authorization header
    let auth_header = req
        .headers()
        .get(http::header::AUTHORIZATION)
        .ok_or(AuthError::MissingAuthHeader)?;

    let auth_str = auth_header
        .to_str()
        .map_err(|_| AuthError::InvalidAuthFormat)?;

    // Check Bearer prefix
    if !auth_str.starts_with("Bearer ") {
        return Err(AuthError::InvalidAuthFormat);
    }

    let token = &auth_str[7..]; // Remove "Bearer " prefix

    // Create validation rules
    let mut validation = Validation::new(config.algorithm);
    validation.leeway = config.leeway;
    validation.validate_exp = config.validate_exp;
    validation.validate_nbf = config.validate_nbf;

    if let Some(ref issuer) = config.issuer {
        validation.set_issuer(&[issuer]);
    }

    if let Some(ref audience) = config.audience {
        validation.set_audience(&[audience]);
    }

    // Decode and validate token
    let decoding_key = DecodingKey::from_secret(config.secret.expose_secret().as_bytes());
    let token_data = decode::<Claims>(token, &decoding_key, &validation)?;

    let claims = token_data.claims;

    // Additional validation
    if claims.is_expired() {
        return Err(AuthError::TokenExpired);
    }

    Ok(claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{EncodingKey, Header, encode};
    use secrecy::Secret;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_test_token(secret: &str, exp_offset: i64) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let claims = Claims {
            sub: "test_user".to_string(),
            roles: vec!["user".to_string()],
            exp: (now + exp_offset) as u64,
            iat: now as u64,
            iss: None,
            aud: None,
        };

        let header = Header::new(Algorithm::HS256);
        let encoding_key = EncodingKey::from_secret(secret.as_bytes());

        encode(&header, &claims, &encoding_key).unwrap()
    }

    #[test]
    fn test_valid_token_extraction() {
        let secret = "test_secret";
        let token = create_test_token(secret, 3600); // Valid for 1 hour

        let config = AuthConfig::new(Secret::new(secret.to_string()));

        let req = http::Request::builder()
            .header("Authorization", format!("Bearer {}", token))
            .body(())
            .unwrap();

        let result = extract_and_validate_token(&req, &config);
        assert!(result.is_ok());

        let claims = result.unwrap();
        assert_eq!(claims.sub, "test_user");
        assert!(claims.has_role("user"));
    }

    #[test]
    fn test_expired_token() {
        let secret = "test_secret";
        let token = create_test_token(secret, -3600); // Expired 1 hour ago

        let config = AuthConfig::new(Secret::new(secret.to_string()));

        let req = http::Request::builder()
            .header("Authorization", format!("Bearer {}", token))
            .body(())
            .unwrap();

        let result = extract_and_validate_token(&req, &config);
        assert!(matches!(result, Err(AuthError::InvalidToken(_))));
    }

    #[test]
    fn test_missing_auth_header() {
        let config = AuthConfig::new(Secret::new("test_secret".to_string()));

        let req = http::Request::builder().body(()).unwrap();

        let result = extract_and_validate_token(&req, &config);
        assert!(matches!(result, Err(AuthError::MissingAuthHeader)));
    }
}
