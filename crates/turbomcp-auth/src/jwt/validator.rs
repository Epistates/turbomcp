//! JWT validation with JWKS support
//!
//! This module implements MCP-compliant JWT validation with:
//! - Audience validation (RFC 8707 requirement)
//! - Issuer validation
//! - Clock skew tolerance (60 seconds per MCP spec)
//! - Algorithm validation (ES256, RS256, PS256)
//! - JWKS-based signature verification
//!
//! # MCP Security Requirements
//!
//! Per MCP specification (RFC 9728):
//! - Servers MUST validate access tokens were issued for them (audience check)
//! - Servers MUST validate token signatures against issuer's public keys
//! - Servers MUST reject expired tokens
//! - Servers SHOULD allow 60 seconds of clock skew

use super::{JwksCache, JwksClient, StandardClaims};
use jsonwebtoken::{Algorithm, DecodingKey, TokenData, Validation, decode, decode_header};
use serde::Deserialize;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::OnceCell;
use tracing::{debug, error, info, warn};
use turbomcp_protocol::{Error as McpError, Result as McpResult};

/// OpenID Connect Discovery Document (RFC 8414)
///
/// This is a minimal representation containing only the fields we need.
/// The full discovery document contains many more optional fields.
#[derive(Debug, Clone, Deserialize)]
struct OidcDiscoveryDocument {
    /// JWKS URI - the only field we actually need
    jwks_uri: String,

    /// All other fields are optional and ignored
    #[serde(flatten)]
    _additional: serde_json::Value,
}

/// JWT validation result containing validated claims
#[derive(Debug, Clone)]
pub struct JwtValidationResult {
    /// The validated claims
    pub claims: StandardClaims,
    /// Algorithm used for signing
    pub algorithm: Algorithm,
    /// Key ID (kid) from JWT header
    pub key_id: Option<String>,
    /// When the token was issued
    pub issued_at: Option<SystemTime>,
    /// When the token expires
    pub expires_at: Option<SystemTime>,
}

/// JWT validator with JWKS support
///
/// # Example
///
/// ```rust,no_run
/// # use turbomcp_auth::jwt::JwtValidator;
/// # tokio_test::block_on(async {
/// let validator = JwtValidator::new(
///     "https://accounts.google.com".to_string(),  // issuer
///     "https://mcp.example.com".to_string(),      // expected audience
/// ).await?;
///
/// let token = "eyJ0eXAiOiJKV1QiLCJhbGc...";
/// let result = validator.validate(token).await?;
///
/// println!("Token valid for: {}", result.claims.sub.unwrap());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # });
/// ```
pub struct JwtValidator {
    /// Expected issuer (iss claim)
    expected_issuer: String,
    /// Expected audience (aud claim) - typically the MCP server URI
    expected_audience: String,
    /// JWKS client for fetching keys
    jwks_client: Arc<JwksClient>,
    /// Clock skew tolerance (default: 60 seconds per MCP spec)
    clock_skew_leeway: Duration,
    /// Supported algorithms (default: ES256, RS256, PS256)
    allowed_algorithms: Vec<Algorithm>,
    /// Discovered JWKS URI (cached after first discovery)
    discovered_jwks_uri: OnceCell<String>,
}

// Manual Debug impl to prevent discovered_jwks_uri from exposing internal state
impl std::fmt::Debug for JwtValidator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtValidator")
            .field("expected_issuer", &self.expected_issuer)
            .field("expected_audience", &self.expected_audience)
            .field("jwks_client", &self.jwks_client)
            .field("clock_skew_leeway", &self.clock_skew_leeway)
            .field("allowed_algorithms", &self.allowed_algorithms)
            .field(
                "discovered_jwks_uri",
                &self.discovered_jwks_uri.get().map(|_| "<cached>"),
            )
            .finish()
    }
}

impl JwtValidator {
    /// Discover JWKS URI via RFC 8414 OpenID Connect Discovery
    ///
    /// # Discovery Process
    ///
    /// 1. Fetch `{issuer}/.well-known/openid-configuration`
    /// 2. Parse the discovery document
    /// 3. Extract `jwks_uri` field
    /// 4. If discovery fails, fall back to hardcoded pattern
    ///
    /// # Errors
    ///
    /// Returns error if both discovery and fallback fail
    async fn discover_jwks_uri(issuer: &str) -> McpResult<String> {
        let discovery_url = format!("{}/.well-known/openid-configuration", issuer);

        debug!(
            issuer = issuer,
            discovery_url = %discovery_url,
            "Attempting RFC 8414 OIDC discovery"
        );

        // Try RFC 8414 discovery first
        match reqwest::get(&discovery_url).await {
            Ok(response) if response.status().is_success() => {
                match response.json::<OidcDiscoveryDocument>().await {
                    Ok(doc) => {
                        info!(
                            issuer = issuer,
                            jwks_uri = %doc.jwks_uri,
                            "Successfully discovered JWKS URI via RFC 8414"
                        );
                        return Ok(doc.jwks_uri);
                    }
                    Err(e) => {
                        warn!(
                            error = %e,
                            issuer = issuer,
                            "Failed to parse OIDC discovery document, trying fallback"
                        );
                    }
                }
            }
            Ok(response) => {
                warn!(
                    status = %response.status(),
                    issuer = issuer,
                    "OIDC discovery endpoint returned non-success status, trying fallback"
                );
            }
            Err(e) => {
                warn!(
                    error = %e,
                    issuer = issuer,
                    "Failed to fetch OIDC discovery document, trying fallback"
                );
            }
        }

        // Fallback: Use hardcoded pattern for non-OIDC providers
        let fallback_uri = format!("{}/.well-known/openid-configuration/jwks", issuer);
        info!(
            issuer = issuer,
            jwks_uri = %fallback_uri,
            "Using fallback JWKS URI pattern (RFC 8414 discovery failed)"
        );
        Ok(fallback_uri)
    }

    /// Create a new JWT validator with RFC 8414 discovery
    ///
    /// # Arguments
    ///
    /// * `expected_issuer` - The expected iss claim (e.g., "https://accounts.google.com")
    /// * `expected_audience` - The expected aud claim (typically your MCP server URI)
    ///
    /// # Default Settings
    ///
    /// - Clock skew: 60 seconds (MCP specification)
    /// - Algorithms: ES256, RS256, PS256 (industry standard)
    ///
    /// # RFC 8414 Discovery
    ///
    /// This method performs OpenID Connect Discovery to find the JWKS endpoint:
    /// 1. Fetches `{issuer}/.well-known/openid-configuration`
    /// 2. Extracts `jwks_uri` from the discovery document
    /// 3. Falls back to `{issuer}/.well-known/openid-configuration/jwks` if discovery fails
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use turbomcp_auth::jwt::JwtValidator;
    ///
    /// # tokio_test::block_on(async {
    /// let validator = JwtValidator::new(
    ///     "https://auth.example.com".to_string(),
    ///     "https://mcp.example.com".to_string(),
    /// ).await?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// # });
    /// ```
    pub async fn new(expected_issuer: String, expected_audience: String) -> McpResult<Self> {
        // Perform RFC 8414 discovery to get JWKS URI
        let jwks_uri = Self::discover_jwks_uri(&expected_issuer).await?;
        let jwks_client = Arc::new(JwksClient::new(jwks_uri.clone()));

        Ok(Self {
            expected_issuer,
            expected_audience,
            jwks_client,
            clock_skew_leeway: Duration::from_secs(60), // MCP spec: 60s leeway
            allowed_algorithms: vec![
                Algorithm::ES256, // ECDSA P-256 (recommended)
                Algorithm::RS256, // RSA-SHA256 (widely supported)
                Algorithm::PS256, // RSA-PSS (modern RSA)
            ],
            discovered_jwks_uri: OnceCell::new_with(Some(jwks_uri)),
        })
    }

    /// Create a new JWT validator without discovery (for testing or custom JWKS URIs)
    ///
    /// Use this when you already know the JWKS URI or want to avoid the discovery roundtrip.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_auth::jwt::JwtValidator;
    ///
    /// let validator = JwtValidator::with_jwks_uri(
    ///     "https://auth.example.com".to_string(),
    ///     "https://mcp.example.com".to_string(),
    ///     "https://auth.example.com/jwks".to_string(),
    /// );
    /// ```
    pub fn with_jwks_uri(
        expected_issuer: String,
        expected_audience: String,
        jwks_uri: String,
    ) -> Self {
        let jwks_client = Arc::new(JwksClient::new(jwks_uri.clone()));

        Self {
            expected_issuer,
            expected_audience,
            jwks_client,
            clock_skew_leeway: Duration::from_secs(60),
            allowed_algorithms: vec![Algorithm::ES256, Algorithm::RS256, Algorithm::PS256],
            discovered_jwks_uri: OnceCell::new_with(Some(jwks_uri)),
        }
    }

    /// Create a validator with custom JWKS client
    ///
    /// Use this when you need custom JWKS caching or multiple validators
    /// sharing the same JWKS cache.
    pub fn with_jwks_client(
        expected_issuer: String,
        expected_audience: String,
        jwks_client: Arc<JwksClient>,
    ) -> Self {
        Self {
            expected_issuer,
            expected_audience,
            jwks_client,
            clock_skew_leeway: Duration::from_secs(60),
            allowed_algorithms: vec![Algorithm::ES256, Algorithm::RS256, Algorithm::PS256],
            discovered_jwks_uri: OnceCell::new(), // No discovery performed in this constructor
        }
    }

    /// Set custom clock skew tolerance
    ///
    /// Default is 60 seconds per MCP specification. Only change if you have
    /// specific requirements (e.g., testing with mock clocks).
    pub fn with_clock_skew(mut self, leeway: Duration) -> Self {
        self.clock_skew_leeway = leeway;
        self
    }

    /// Set allowed algorithms
    ///
    /// Default is ES256, RS256, PS256. Only change if you have specific
    /// security requirements.
    ///
    /// # Security Warning
    ///
    /// Never allow the "none" algorithm. Only use asymmetric algorithms
    /// (ES256, RS256, PS256, etc.) for token validation.
    pub fn with_algorithms(mut self, algorithms: Vec<Algorithm>) -> Self {
        self.allowed_algorithms = algorithms;
        self
    }

    /// Validate a JWT token
    ///
    /// This performs comprehensive validation including:
    /// - Signature verification (using JWKS)
    /// - Audience validation (aud claim)
    /// - Issuer validation (iss claim)
    /// - Expiration check (exp claim)
    /// - Not-before check (nbf claim)
    /// - Algorithm validation
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Token is malformed
    /// - Signature is invalid
    /// - Audience doesn't match
    /// - Issuer doesn't match
    /// - Token is expired
    /// - Token not yet valid (nbf)
    /// - Algorithm not allowed
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use turbomcp_auth::jwt::JwtValidator;
    /// # tokio_test::block_on(async {
    /// let validator = JwtValidator::new(
    ///     "https://auth.example.com".to_string(),
    ///     "https://mcp.example.com".to_string(),
    /// ).await?;
    ///
    /// match validator.validate("eyJ0eXAi...").await {
    ///     Ok(result) => println!("Valid token for: {}", result.claims.sub.unwrap()),
    ///     Err(e) => println!("Invalid token: {}", e),
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// # });
    /// ```
    pub async fn validate(&self, token: &str) -> McpResult<JwtValidationResult> {
        // Decode header to get algorithm and key ID
        let header = decode_header(token).map_err(|e| {
            debug!(error = %e, "Failed to decode JWT header");
            McpError::invalid_params(format!("Invalid JWT format: {e}"))
        })?;

        // Validate algorithm is allowed
        if !self.allowed_algorithms.contains(&header.alg) {
            error!(
                algorithm = ?header.alg,
                allowed = ?self.allowed_algorithms,
                "JWT algorithm not allowed"
            );
            return Err(McpError::invalid_params(format!(
                "Algorithm {:?} not allowed",
                header.alg
            )));
        }

        // Get key ID
        let key_id = header.kid.clone().ok_or_else(|| {
            error!("JWT missing kid (key ID) in header");
            McpError::invalid_params("JWT must include kid (key ID) in header".to_string())
        })?;

        // Fetch JWKS and find the key
        let decoding_key = self.get_decoding_key(&key_id, header.alg).await?;

        // Set up validation rules
        let mut validation = Validation::new(header.alg);
        validation.set_audience(&[&self.expected_audience]);
        validation.set_issuer(&[&self.expected_issuer]);
        validation.leeway = self.clock_skew_leeway.as_secs();

        // Validate and decode token
        let token_data: TokenData<StandardClaims> = decode(token, &decoding_key, &validation)
            .map_err(|e| {
                warn!(
                    error = %e,
                    issuer = %self.expected_issuer,
                    audience = %self.expected_audience,
                    "JWT validation failed"
                );
                McpError::invalid_params(format!("JWT validation failed: {e}"))
            })?;

        // Extract timestamps
        let issued_at = token_data
            .claims
            .iat
            .map(|iat| UNIX_EPOCH + Duration::from_secs(iat));
        let expires_at = token_data
            .claims
            .exp
            .map(|exp| UNIX_EPOCH + Duration::from_secs(exp));

        debug!(
            issuer = %self.expected_issuer,
            audience = %self.expected_audience,
            subject = ?token_data.claims.sub,
            algorithm = ?header.alg,
            "JWT validation successful"
        );

        Ok(JwtValidationResult {
            claims: token_data.claims,
            algorithm: header.alg,
            key_id: Some(key_id),
            issued_at,
            expires_at,
        })
    }

    /// Validate a JWT token with automatic JWKS refresh on failure
    ///
    /// This method handles key rotation gracefully:
    /// 1. Try validation with cached JWKS
    /// 2. If validation fails, refresh JWKS and retry
    /// 3. Return error if second validation fails
    ///
    /// Use this as the primary validation method in production.
    pub async fn validate_with_refresh(&self, token: &str) -> McpResult<JwtValidationResult> {
        // First attempt with cached JWKS
        match self.validate(token).await {
            Ok(result) => Ok(result),
            Err(first_error) => {
                // Validation failed, refresh JWKS and retry
                warn!(
                    error = %first_error,
                    "JWT validation failed, refreshing JWKS and retrying"
                );

                self.jwks_client.refresh().await?;

                // Second attempt with fresh JWKS
                self.validate(token).await.map_err(|e| {
                    error!(error = %e, "JWT validation failed after JWKS refresh");
                    e
                })
            }
        }
    }

    /// Get decoding key from JWKS
    async fn get_decoding_key(
        &self,
        key_id: &str,
        _algorithm: Algorithm,
    ) -> McpResult<DecodingKey> {
        let jwks = self.jwks_client.get_jwks().await?;

        // Find the key with matching kid
        let jwk = jwks.find(key_id).ok_or_else(|| {
            error!(key_id = key_id, "Key ID not found in JWKS");
            McpError::invalid_params(format!("Key ID '{key_id}' not found in JWKS"))
        })?;

        // Convert JWK to DecodingKey
        DecodingKey::from_jwk(jwk).map_err(|e| {
            error!(key_id = key_id, error = %e, "Failed to create decoding key from JWK");
            McpError::internal(format!("Invalid JWK: {e}"))
        })
    }

    /// Get the expected issuer
    pub fn expected_issuer(&self) -> &str {
        &self.expected_issuer
    }

    /// Get the expected audience
    pub fn expected_audience(&self) -> &str {
        &self.expected_audience
    }
}

/// Multi-issuer JWT validator
///
/// Use this when you need to validate tokens from multiple authorization servers.
/// It manages separate validators for each issuer.
///
/// # Example
///
/// ```rust,no_run
/// # use turbomcp_auth::jwt::validator::MultiIssuerValidator;
/// # tokio_test::block_on(async {
/// let mut validator = MultiIssuerValidator::new("https://mcp.example.com".to_string());
///
/// // Add supported issuers
/// validator.add_issuer("https://accounts.google.com".to_string());
/// validator.add_issuer("https://login.microsoftonline.com".to_string());
///
/// // Validate token (issuer auto-detected from JWT)
/// # let token = "example.jwt.token";
/// let result = validator.validate(token).await?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # });
/// ```
#[derive(Debug)]
pub struct MultiIssuerValidator {
    /// Expected audience (same for all issuers)
    expected_audience: String,
    /// Map of issuer -> validator
    validators: std::collections::HashMap<String, Arc<JwtValidator>>,
    /// Shared JWKS cache (reserved for future use)
    #[allow(dead_code)]
    jwks_cache: Arc<JwksCache>,
}

impl MultiIssuerValidator {
    /// Create a new multi-issuer validator
    pub fn new(expected_audience: String) -> Self {
        Self {
            expected_audience,
            validators: std::collections::HashMap::new(),
            jwks_cache: Arc::new(JwksCache::new()),
        }
    }

    /// Add a supported issuer with RFC 8414 discovery
    ///
    /// This creates a validator for the issuer using RFC 8414 discovery to find
    /// the JWKS URI. Falls back to hardcoded pattern if discovery fails.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// # use turbomcp_auth::jwt::validator::MultiIssuerValidator;
    /// # tokio_test::block_on(async {
    /// let mut validator = MultiIssuerValidator::new("https://mcp.example.com".into());
    /// validator.add_issuer("https://accounts.google.com".into()).await?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// # });
    /// ```
    pub async fn add_issuer(&mut self, issuer: String) -> McpResult<()> {
        // Use RFC 8414 discovery to find JWKS URI
        let jwks_uri = JwtValidator::discover_jwks_uri(&issuer).await?;
        let jwks_client = Arc::new(JwksClient::new(jwks_uri));

        let validator = Arc::new(JwtValidator::with_jwks_client(
            issuer.clone(),
            self.expected_audience.clone(),
            jwks_client,
        ));

        self.validators.insert(issuer, validator);
        Ok(())
    }

    /// Add a supported issuer with a known JWKS URI (no discovery)
    ///
    /// Use this when you already know the JWKS URI or want to avoid the discovery roundtrip.
    pub fn add_issuer_with_jwks_uri(&mut self, issuer: String, jwks_uri: String) {
        let jwks_client = Arc::new(JwksClient::new(jwks_uri));

        let validator = Arc::new(JwtValidator::with_jwks_client(
            issuer.clone(),
            self.expected_audience.clone(),
            jwks_client,
        ));

        self.validators.insert(issuer, validator);
    }

    /// Validate a token (auto-detect issuer from JWT claims)
    ///
    /// v2.3.6: Added algorithm allowlist validation to prevent algorithm confusion attacks
    pub async fn validate(&self, token: &str) -> McpResult<JwtValidationResult> {
        // Decode header to check algorithm BEFORE any other processing
        // This prevents algorithm confusion attacks (e.g., none, HS256 with public key)
        let header = decode_header(token)
            .map_err(|e| McpError::invalid_params(format!("Invalid JWT format: {e}")))?;

        // SECURITY: Validate algorithm is in allowlist before proceeding
        // Only asymmetric algorithms are allowed for multi-issuer validation
        const ALLOWED_ALGORITHMS: &[Algorithm] = &[
            Algorithm::ES256,
            Algorithm::ES384,
            Algorithm::RS256,
            Algorithm::RS384,
            Algorithm::RS512,
            Algorithm::PS256,
            Algorithm::PS384,
            Algorithm::PS512,
        ];

        if !ALLOWED_ALGORITHMS.contains(&header.alg) {
            error!(algorithm = ?header.alg, "JWT algorithm not in allowlist");
            return Err(McpError::invalid_params(format!(
                "JWT algorithm {:?} not allowed. Only asymmetric algorithms (ES*, RS*, PS*) are permitted.",
                header.alg
            )));
        }

        // We need to peek at the payload to get the issuer
        // This is safe because we'll validate the signature next
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(McpError::invalid_params("Invalid JWT format".to_string()));
        }

        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

        let payload = URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|e| McpError::invalid_params(format!("Invalid JWT payload encoding: {e}")))?;

        let claims: StandardClaims = serde_json::from_slice(&payload)
            .map_err(|e| McpError::invalid_params(format!("Invalid JWT claims: {e}")))?;

        let issuer = claims.iss.ok_or_else(|| {
            McpError::invalid_params("JWT missing iss (issuer) claim".to_string())
        })?;

        // Find validator for this issuer
        let validator = self.validators.get(&issuer).ok_or_else(|| {
            error!(issuer = %issuer, "Unknown issuer");
            McpError::invalid_params(format!("Issuer '{}' not supported", issuer))
        })?;

        // Validate with the appropriate validator
        validator.validate_with_refresh(token).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_validator_creation_with_jwks_uri() {
        let validator = JwtValidator::with_jwks_uri(
            "https://auth.example.com".to_string(),
            "https://mcp.example.com".to_string(),
            "https://auth.example.com/jwks".to_string(),
        );

        assert_eq!(validator.expected_issuer(), "https://auth.example.com");
        assert_eq!(validator.expected_audience(), "https://mcp.example.com");
        assert_eq!(validator.clock_skew_leeway, Duration::from_secs(60));
        assert_eq!(validator.allowed_algorithms.len(), 3);
    }

    #[test]
    fn test_jwt_validator_custom_clock_skew() {
        let validator = JwtValidator::with_jwks_uri(
            "https://auth.example.com".to_string(),
            "https://mcp.example.com".to_string(),
            "https://auth.example.com/jwks".to_string(),
        )
        .with_clock_skew(Duration::from_secs(30));

        assert_eq!(validator.clock_skew_leeway, Duration::from_secs(30));
    }

    #[test]
    fn test_jwt_validator_custom_algorithms() {
        let validator = JwtValidator::with_jwks_uri(
            "https://auth.example.com".to_string(),
            "https://mcp.example.com".to_string(),
            "https://auth.example.com/jwks".to_string(),
        )
        .with_algorithms(vec![Algorithm::ES256]);

        assert_eq!(validator.allowed_algorithms, vec![Algorithm::ES256]);
    }

    #[test]
    fn test_multi_issuer_validator_creation() {
        let validator = MultiIssuerValidator::new("https://mcp.example.com".to_string());
        assert_eq!(validator.expected_audience, "https://mcp.example.com");
        assert_eq!(validator.validators.len(), 0);
    }

    #[test]
    fn test_multi_issuer_validator_add_issuer() {
        let mut validator = MultiIssuerValidator::new("https://mcp.example.com".to_string());
        validator.add_issuer_with_jwks_uri(
            "https://auth.example.com".to_string(),
            "https://auth.example.com/jwks".to_string(),
        );

        assert_eq!(validator.validators.len(), 1);
        assert!(
            validator
                .validators
                .contains_key("https://auth.example.com")
        );
    }
}
