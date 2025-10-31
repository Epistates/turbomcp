//! JWKS (JSON Web Key Set) fetching and caching
//!
//! This module implements secure JWKS fetching with intelligent caching
//! per industry best practices (2025):
//!
//! - **TTL-based caching**: Default 10 minutes (configurable 5-30 min)
//! - **Refresh on errors**: Auto-refresh if validation fails
//! - **Rate limiting**: Prevents DoS on authorization servers
//! - **Observability**: Comprehensive logging and metrics
//!
//! # Security Considerations
//!
//! - HTTPS required for JWKS endpoints (prevents MITM)
//! - Cache prevents authorization server overload
//! - TTL balances security (key rotation) vs performance
//! - Refresh-on-error handles key rotation edge cases

use jsonwebtoken::jwk::JwkSet;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use turbomcp_protocol::{Error as McpError, Result as McpResult};

/// JWKS cache entry with metadata
#[derive(Debug, Clone)]
struct CachedJwks {
    /// The JWK set
    jwks: JwkSet,
    /// When this was cached
    cached_at: SystemTime,
    /// TTL for this cache entry
    ttl: Duration,
}

impl CachedJwks {
    /// Check if this cache entry is still valid
    fn is_valid(&self) -> bool {
        match SystemTime::now().duration_since(self.cached_at) {
            Ok(age) => age < self.ttl,
            Err(_) => false, // Clock went backwards, invalidate
        }
    }
}

/// JWKS client for fetching and caching JSON Web Key Sets
///
/// # Example
///
/// ```rust,no_run
/// # use turbomcp_auth::jwt::JwksClient;
/// # tokio_test::block_on(async {
/// let client = JwksClient::new("https://auth.example.com/.well-known/jwks.json".to_string());
///
/// // Fetch JWKS (cached for 10 minutes by default)
/// let jwks = client.get_jwks().await?;
///
/// // Find key by ID
/// if let Some(key) = jwks.find("key-id-123") {
///     // Use key for validation
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # });
/// ```
#[derive(Debug, Clone)]
pub struct JwksClient {
    /// JWKS endpoint URL
    jwks_uri: String,
    /// Cached JWKS
    cache: Arc<RwLock<Option<CachedJwks>>>,
    /// HTTP client
    http_client: reqwest::Client,
    /// Cache TTL (default: 10 minutes)
    cache_ttl: Duration,
    /// Minimum refresh interval (rate limiting)
    min_refresh_interval: Duration,
    /// Last refresh attempt
    last_refresh: Arc<RwLock<Option<SystemTime>>>,
}

impl JwksClient {
    /// Create a new JWKS client with default settings
    ///
    /// # Arguments
    ///
    /// * `jwks_uri` - JWKS endpoint URL (must be HTTPS in production)
    ///
    /// # Default Settings
    ///
    /// - Cache TTL: 10 minutes (balance between security and performance)
    /// - Min refresh interval: 5 seconds (rate limiting)
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_auth::jwt::JwksClient;
    ///
    /// let client = JwksClient::new(
    ///     "https://accounts.google.com/.well-known/openid-configuration/jwks".to_string()
    /// );
    /// ```
    pub fn new(jwks_uri: String) -> Self {
        Self {
            jwks_uri,
            cache: Arc::new(RwLock::new(None)),
            http_client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            cache_ttl: Duration::from_secs(600), // 10 minutes (industry standard)
            min_refresh_interval: Duration::from_secs(5), // Rate limiting
            last_refresh: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a JWKS client with custom cache TTL
    ///
    /// # Arguments
    ///
    /// * `jwks_uri` - JWKS endpoint URL
    /// * `cache_ttl` - Cache time-to-live (recommended: 5-30 minutes)
    ///
    /// # Security Note
    ///
    /// Shorter TTL = more secure (faster key rotation detection)
    /// Longer TTL = better performance (fewer network requests)
    /// Industry standard: 5-30 minutes
    pub fn with_ttl(jwks_uri: String, cache_ttl: Duration) -> Self {
        let mut client = Self::new(jwks_uri);
        client.cache_ttl = cache_ttl;
        client
    }

    /// Get JWKS (from cache or fetch if needed)
    ///
    /// This method automatically handles caching and refresh logic:
    /// - Returns cached JWKS if valid
    /// - Fetches fresh JWKS if cache expired
    /// - Rate limits refresh attempts
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - JWKS endpoint is unreachable
    /// - Response is not valid JWKS JSON
    /// - Network timeout (10 seconds)
    pub async fn get_jwks(&self) -> McpResult<JwkSet> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.as_ref()
                && cached.is_valid()
            {
                debug!(jwks_uri = %self.jwks_uri, "Using cached JWKS");
                return Ok(cached.jwks.clone());
            }
        }

        // Cache expired or missing, fetch fresh JWKS
        self.fetch_and_cache().await
    }

    /// Force refresh JWKS (ignoring cache)
    ///
    /// Use this when token validation fails - the key may have been rotated.
    ///
    /// # Rate Limiting
    ///
    /// This method enforces a minimum refresh interval to prevent DoS attacks
    /// on the authorization server. If called too frequently, it returns the
    /// cached value (if available) or errors.
    pub async fn refresh(&self) -> McpResult<JwkSet> {
        // Check rate limiting
        {
            let last_refresh = self.last_refresh.read().await;
            if let Some(last) = *last_refresh
                && let Ok(since_last) = SystemTime::now().duration_since(last)
                && since_last < self.min_refresh_interval
            {
                warn!(
                    jwks_uri = %self.jwks_uri,
                    since_last_ms = since_last.as_millis(),
                    "JWKS refresh rate limited, using cache"
                );
                return self.get_jwks().await;
            }
        }

        self.fetch_and_cache().await
    }

    /// Fetch JWKS from endpoint and update cache
    async fn fetch_and_cache(&self) -> McpResult<JwkSet> {
        info!(jwks_uri = %self.jwks_uri, "Fetching JWKS from endpoint");

        // Validate HTTPS in production
        if !self.jwks_uri.starts_with("https://") && !self.jwks_uri.starts_with("http://localhost")
        {
            return Err(McpError::validation(
                "JWKS endpoint must use HTTPS (HTTP only allowed for localhost)".to_string(),
            ));
        }

        // Fetch JWKS
        let response = self
            .http_client
            .get(&self.jwks_uri)
            .send()
            .await
            .map_err(|e| {
                error!(jwks_uri = %self.jwks_uri, error = %e, "Failed to fetch JWKS");
                McpError::internal(format!("JWKS fetch failed: {e}"))
            })?;

        if !response.status().is_success() {
            error!(
                jwks_uri = %self.jwks_uri,
                status = %response.status(),
                "JWKS endpoint returned error status"
            );
            return Err(McpError::internal(format!(
                "JWKS endpoint returned status {}",
                response.status()
            )));
        }

        // Parse JWKS
        let jwks: JwkSet = response.json().await.map_err(|e| {
            error!(jwks_uri = %self.jwks_uri, error = %e, "Failed to parse JWKS JSON");
            McpError::internal(format!("Invalid JWKS format: {e}"))
        })?;

        info!(
            jwks_uri = %self.jwks_uri,
            key_count = jwks.keys.len(),
            "Successfully fetched JWKS"
        );

        // Update cache
        {
            let mut cache = self.cache.write().await;
            *cache = Some(CachedJwks {
                jwks: jwks.clone(),
                cached_at: SystemTime::now(),
                ttl: self.cache_ttl,
            });
        }

        // Update last refresh time
        {
            let mut last_refresh = self.last_refresh.write().await;
            *last_refresh = Some(SystemTime::now());
        }

        Ok(jwks)
    }

    /// Get the JWKS endpoint URI
    pub fn jwks_uri(&self) -> &str {
        &self.jwks_uri
    }

    /// Clear the cache (for testing or manual refresh)
    pub async fn clear_cache(&self) {
        let mut cache = self.cache.write().await;
        *cache = None;
        debug!(jwks_uri = %self.jwks_uri, "JWKS cache cleared");
    }
}

/// JWKS cache for managing multiple authorization servers
///
/// This is a higher-level abstraction that manages JWKS clients for
/// multiple issuers. Use this when validating tokens from various providers.
///
/// # Example
///
/// ```rust,no_run
/// # use turbomcp_auth::jwt::JwksCache;
/// # tokio_test::block_on(async {
/// let cache = JwksCache::new();
///
/// // Get JWKS for a specific issuer
/// let jwks = cache.get_jwks_for_issuer("https://accounts.google.com").await?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// # });
/// ```
#[derive(Debug, Default)]
pub struct JwksCache {
    /// Map of issuer -> JWKS client
    clients: Arc<RwLock<std::collections::HashMap<String, Arc<JwksClient>>>>,
}

impl JwksCache {
    /// Create a new JWKS cache
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Get or create a JWKS client for an issuer
    ///
    /// # Arguments
    ///
    /// * `issuer` - The issuer URL (e.g., "https://accounts.google.com")
    ///
    /// # JWKS Discovery
    ///
    /// This method performs OpenID Connect discovery by appending
    /// `/.well-known/openid-configuration/jwks` to the issuer URL.
    pub async fn get_client_for_issuer(&self, issuer: &str) -> Arc<JwksClient> {
        let mut clients = self.clients.write().await;

        if let Some(client) = clients.get(issuer) {
            return Arc::clone(client);
        }

        // Create new client with OIDC discovery
        let jwks_uri = format!("{issuer}/.well-known/openid-configuration/jwks");
        let client = Arc::new(JwksClient::new(jwks_uri));

        clients.insert(issuer.to_string(), Arc::clone(&client));

        client
    }

    /// Get JWKS for an issuer
    ///
    /// Convenience method that gets the client and fetches JWKS in one call.
    pub async fn get_jwks_for_issuer(&self, issuer: &str) -> McpResult<JwkSet> {
        let client = self.get_client_for_issuer(issuer).await;
        client.get_jwks().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwks_client_creation() {
        let client = JwksClient::new("https://auth.example.com/jwks".to_string());
        assert_eq!(client.jwks_uri(), "https://auth.example.com/jwks");
        assert_eq!(client.cache_ttl, Duration::from_secs(600));
    }

    #[test]
    fn test_jwks_client_with_custom_ttl() {
        let client = JwksClient::with_ttl(
            "https://auth.example.com/jwks".to_string(),
            Duration::from_secs(300),
        );
        assert_eq!(client.cache_ttl, Duration::from_secs(300));
    }

    #[test]
    fn test_cached_jwks_validity() {
        let jwks = JwkSet { keys: vec![] };
        let cached = CachedJwks {
            jwks,
            cached_at: SystemTime::now(),
            ttl: Duration::from_secs(600),
        };

        assert!(cached.is_valid());
    }

    #[test]
    fn test_cached_jwks_expired() {
        let jwks = JwkSet { keys: vec![] };
        let cached = CachedJwks {
            jwks,
            cached_at: SystemTime::now() - Duration::from_secs(700),
            ttl: Duration::from_secs(600),
        };

        assert!(!cached.is_valid());
    }

    #[tokio::test]
    async fn test_jwks_cache_creation() {
        let cache = JwksCache::new();
        let client1 = cache
            .get_client_for_issuer("https://auth.example.com")
            .await;
        let client2 = cache
            .get_client_for_issuer("https://auth.example.com")
            .await;

        // Should return same client instance
        assert!(Arc::ptr_eq(&client1, &client2));
    }

    #[tokio::test]
    async fn test_jwks_cache_different_issuers() {
        let cache = JwksCache::new();
        let client1 = cache
            .get_client_for_issuer("https://auth1.example.com")
            .await;
        let client2 = cache
            .get_client_for_issuer("https://auth2.example.com")
            .await;

        // Should return different client instances
        assert!(!Arc::ptr_eq(&client1, &client2));
    }

    #[tokio::test]
    async fn test_clear_cache() {
        let client = JwksClient::new("https://auth.example.com/jwks".to_string());
        client.clear_cache().await;

        let cache = client.cache.read().await;
        assert!(cache.is_none());
    }
}
