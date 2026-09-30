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
use url::Url;

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
    /// Optional SSRF validator applied before fetching the JWKS URI
    ssrf_validator: Option<Arc<crate::ssrf::SsrfValidator>>,
    /// Serializes network fetches so concurrent callers coalesce onto one
    /// request instead of stampeding the JWKS endpoint (single-flight), and
    /// so the rate-limit check-then-set in `refresh()` is atomic rather than
    /// a TOCTOU race between the read of `last_refresh` and the write of it.
    fetch_lock: Arc<tokio::sync::Mutex<()>>,
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
    /// let client = JwksClient::new("https://accounts.google.com/.well-known/jwks.json".to_string());
    /// ```
    pub fn new(jwks_uri: String) -> Self {
        Self {
            jwks_uri,
            cache: Arc::new(RwLock::new(None)),
            http_client: Self::http_client_builder()
                .build()
                .expect("Failed to create HTTP client"),
            cache_ttl: Duration::from_secs(600), // 10 minutes (industry standard)
            min_refresh_interval: Duration::from_secs(5), // Rate limiting
            last_refresh: Arc::new(RwLock::new(None)),
            ssrf_validator: None,
            fetch_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    fn http_client_builder() -> reqwest::ClientBuilder {
        reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            // A JWKS URI can be attacker-influenced (multi-issuer setups,
            // JWT-driven discovery). Following redirects would let a
            // malicious or compromised endpoint hand back keys from an
            // arbitrary host, defeating both the HTTPS check below and any
            // SSRF policy applied to the original URI.
            .redirect(reqwest::redirect::Policy::none())
    }

    /// Create a JWKS client with SSRF protection
    ///
    /// The SSRF validator is applied to the JWKS URI before each fetch attempt,
    /// preventing server-side request forgery via user-controlled JWKS endpoints.
    /// The fetch then connects to an address the validator checked, rather
    /// than resolving the host a second time.
    pub fn with_ssrf_validator(
        jwks_uri: String,
        ssrf_validator: Arc<crate::ssrf::SsrfValidator>,
    ) -> Self {
        let mut client = Self::new(jwks_uri);
        client.ssrf_validator = Some(ssrf_validator);
        client
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

        // Cache expired or missing. Serialize the fetch: hold `fetch_lock` for
        // the whole "decide, then maybe fetch" sequence so concurrent callers
        // that all missed the cache coalesce onto one network request
        // (single-flight) instead of each firing their own — the previous
        // version had no such coordination, so N concurrent callers with a
        // cold cache made N requests.
        let _fetch_guard = self.fetch_lock.lock().await;

        // Re-check the cache: whoever held the lock ahead of us may already
        // have refreshed it.
        {
            let cache = self.cache.read().await;
            if let Some(cached) = cache.as_ref()
                && cached.is_valid()
            {
                debug!(jwks_uri = %self.jwks_uri, "Using cached JWKS (filled while waiting on fetch lock)");
                return Ok(cached.jwks.clone());
            }
        }

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
        // Hold `fetch_lock` across the "check last_refresh, then maybe fetch
        // and update last_refresh" sequence. The previous version read
        // `last_refresh` and wrote it back in two separate, unguarded steps —
        // concurrent callers could both pass the rate-limit check before
        // either updated the timestamp, each firing a request within the
        // window the check exists to prevent (TOCTOU).
        let _fetch_guard = self.fetch_lock.lock().await;

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
                let cache = self.cache.read().await;
                return match cache.as_ref() {
                    Some(cached) => Ok(cached.jwks.clone()),
                    None => Err(McpError::internal(
                        "JWKS refresh rate limited and no cached JWKS available".to_string(),
                    )),
                };
            }
        }

        self.fetch_and_cache().await
    }

    /// Fetch JWKS from endpoint and update cache
    ///
    /// Callers MUST hold `fetch_lock` before calling this — it does not
    /// acquire it itself so `get_jwks`/`refresh` can share one critical
    /// section that covers both the cache/rate-limit check and the fetch.
    async fn fetch_and_cache(&self) -> McpResult<JwkSet> {
        info!(jwks_uri = %self.jwks_uri, "Fetching JWKS from endpoint");

        if !Self::is_allowed_jwks_uri(&self.jwks_uri) {
            return Err(McpError::invalid_params(
                "JWKS endpoint must use HTTPS (HTTP only allowed for localhost)".to_string(),
            ));
        }

        // Validate URI against SSRF policy before fetching, and pin the
        // connection to an address that validation checked: letting the client
        // resolve the host again afterwards is a DNS-rebinding gap.
        let http_client = match &self.ssrf_validator {
            Some(validator) => {
                let (host, pinned) = validator
                    .validate_url(&self.jwks_uri)
                    .and_then(|()| validator.resolve_for_pinning(&self.jwks_uri))
                    .map_err(|e| {
                        McpError::authentication(format!(
                            "SSRF validation failed for JWKS URI: {e}"
                        ))
                    })?;
                Self::http_client_builder()
                    .resolve(&host, pinned)
                    .build()
                    .map_err(|e| McpError::internal(format!("JWKS client: {e}")))?
            }
            None => self.http_client.clone(),
        };

        // Fetch JWKS
        let mut response = http_client.get(&self.jwks_uri).send().await.map_err(|e| {
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

        // Read the response with a size limit to prevent memory exhaustion,
        // enforced as it arrives rather than after buffering all of it.
        const MAX_JWKS_RESPONSE_SIZE: usize = 65_536; // 64 KB — sufficient for hundreds of keys
        let too_large = |size: u64| {
            error!(
                jwks_uri = %self.jwks_uri,
                size,
                max = MAX_JWKS_RESPONSE_SIZE,
                "JWKS response exceeds size limit"
            );
            McpError::internal(format!(
                "JWKS response too large: at least {size} bytes (max: {MAX_JWKS_RESPONSE_SIZE} bytes)"
            ))
        };
        if let Some(length) = response.content_length()
            && length > MAX_JWKS_RESPONSE_SIZE as u64
        {
            return Err(too_large(length));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| {
            error!(jwks_uri = %self.jwks_uri, error = %e, "Failed to read JWKS response body");
            McpError::internal(format!("Failed to read JWKS response: {e}"))
        })? {
            if bytes.len() + chunk.len() > MAX_JWKS_RESPONSE_SIZE {
                return Err(too_large((bytes.len() + chunk.len()) as u64));
            }
            bytes.extend_from_slice(&chunk);
        }
        let jwks: JwkSet = serde_json::from_slice(&bytes).map_err(|e| {
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

    fn is_allowed_jwks_uri(jwks_uri: &str) -> bool {
        let Ok(parsed) = Url::parse(jwks_uri) else {
            return false;
        };

        match parsed.scheme() {
            "https" => true,
            "http" => matches!(parsed.host_str(), Some("localhost" | "127.0.0.1" | "::1")),
            _ => false,
        }
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
    /// Optional SSRF validator applied when creating new clients
    ssrf_validator: Option<Arc<crate::ssrf::SsrfValidator>>,
}

impl JwksCache {
    /// Create a new JWKS cache
    pub fn new() -> Self {
        Self {
            clients: Arc::new(RwLock::new(std::collections::HashMap::new())),
            ssrf_validator: None,
        }
    }

    /// Create a new JWKS cache with SSRF protection
    ///
    /// All JWKS clients created by this cache will validate the JWKS URI against
    /// the provided SSRF policy before fetching.
    pub fn with_ssrf_validator(ssrf_validator: Arc<crate::ssrf::SsrfValidator>) -> Self {
        Self {
            clients: Arc::new(RwLock::new(std::collections::HashMap::new())),
            ssrf_validator: Some(ssrf_validator),
        }
    }

    /// Get or create a JWKS client for an issuer
    ///
    /// # Arguments
    ///
    /// * `issuer` - The issuer URL (e.g., "<https://accounts.google.com>")
    ///
    /// # JWKS Discovery
    ///
    /// This method uses the conventional direct JWKS endpoint
    /// `/.well-known/jwks.json` when discovery metadata is not available.
    pub async fn get_client_for_issuer(&self, issuer: &str) -> Arc<JwksClient> {
        let mut clients = self.clients.write().await;

        if let Some(client) = clients.get(issuer) {
            return Arc::clone(client);
        }

        // Create new client with standard JWKS endpoint
        let jwks_uri = Url::parse(issuer)
            .and_then(|base| base.join(".well-known/jwks.json"))
            .map(|u| u.to_string())
            .unwrap_or_else(|_| format!("{issuer}/.well-known/jwks.json"));

        let client = Arc::new(if let Some(ref validator) = self.ssrf_validator {
            JwksClient::with_ssrf_validator(jwks_uri, Arc::clone(validator))
        } else {
            JwksClient::new(jwks_uri)
        });

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

    fn localhost_policy() -> Arc<crate::ssrf::SsrfValidator> {
        Arc::new(crate::ssrf::SsrfValidator::new(crate::ssrf::SsrfPolicy {
            allow_localhost: true,
            allow_private_networks: true,
            require_https: false,
            ip_denylist: Vec::new(),
            ..Default::default()
        }))
    }

    /// With an SSRF policy the fetch goes to the address the policy checked.
    #[tokio::test]
    async fn an_ssrf_checked_fetch_uses_the_validated_address() {
        use wiremock::matchers::method;
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "keys": []
            })))
            .mount(&server)
            .await;

        let client = JwksClient::with_ssrf_validator(server.uri(), localhost_policy());
        let jwks = client
            .get_jwks()
            .await
            .expect("the policy allows localhost");
        assert!(jwks.keys.is_empty());

        let strict = JwksClient::with_ssrf_validator(
            server.uri(),
            Arc::new(crate::ssrf::SsrfValidator::default()),
        );
        assert!(
            strict.get_jwks().await.is_err(),
            "the default policy refuses it"
        );
    }

    /// The size limit applies before the body is buffered, including when the
    /// server sends no `Content-Length`.
    #[tokio::test]
    async fn an_oversized_jwks_is_refused() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let uri = format!("http://{}/jwks", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = socket.read(&mut request).await;
            // Chunked, so there is no length to check up front.
            let _ = socket
                .write_all(b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\n\r\n")
                .await;
            let chunk = vec![b' '; 16 * 1024];
            for _ in 0..8 {
                let _ = socket
                    .write_all(format!("{:x}\r\n", chunk.len()).as_bytes())
                    .await;
                let _ = socket.write_all(&chunk).await;
                let _ = socket.write_all(b"\r\n").await;
            }
            let _ = socket.write_all(b"0\r\n\r\n").await;
        });

        let error = JwksClient::new(uri).get_jwks().await.unwrap_err();
        assert!(error.to_string().contains("too large"), "{error}");
    }
}
