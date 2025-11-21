//! # Discovery Document Fetcher
//!
//! HTTP fetcher for OAuth 2.0 Authorization Server Metadata (RFC 8414) and
//! OpenID Connect Discovery 1.0 documents with SSRF protection, caching,
//! and multi-endpoint support as required by MCP 2025-11-25 specification.

use super::types::{
    AuthorizationServerMetadata, DiscoveryError, OIDCProviderMetadata, ValidatedDiscoveryMetadata,
};
use crate::ssrf::{SsrfError, SsrfValidator};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tracing::{debug, warn};

/// Discovery fetcher errors
#[derive(Debug, Error)]
pub enum FetcherError {
    /// SSRF protection blocked the request
    #[error("SSRF protection blocked request: {0}")]
    SsrfBlocked(#[from] SsrfError),

    /// HTTP request failed
    #[error("HTTP request failed: {0}")]
    HttpError(String),

    /// Response size limit exceeded
    #[error("Response size limit exceeded")]
    ResponseTooLarge,

    /// Invalid JSON response
    #[error("Invalid JSON response: {0}")]
    InvalidJson(String),

    /// Discovery validation failed
    #[error("Discovery validation failed: {0}")]
    ValidationFailed(#[from] DiscoveryError),

    /// All discovery endpoints failed
    #[error("All discovery endpoints failed. RFC 8414: {oauth2_error}, OIDC: {oidc_error}")]
    AllEndpointsFailed {
        oauth2_error: String,
        oidc_error: String,
    },

    /// Invalid issuer URL
    #[error("Invalid issuer URL: {0}")]
    InvalidIssuer(String),

    /// Cache error
    #[error("Cache error: {0}")]
    CacheError(String),
}

/// Cache entry for discovery documents
#[derive(Debug, Clone)]
struct CacheEntry {
    /// The validated metadata
    metadata: ValidatedDiscoveryMetadata,

    /// When this entry expires
    expires_at: SystemTime,
}

/// Configuration for discovery fetcher
#[derive(Debug, Clone)]
pub struct FetcherConfig {
    /// Maximum response size in bytes (default: 10KB - larger than CIMD)
    pub max_response_size: usize,

    /// Request timeout (default: 5 seconds)
    pub request_timeout: Duration,

    /// Default cache TTL if no cache headers present (default: 1 hour)
    pub default_cache_ttl: Duration,

    /// Maximum cache TTL (default: 24 hours)
    pub max_cache_ttl: Duration,

    /// User agent for HTTP requests
    pub user_agent: String,

    /// Whether to try OIDC discovery if RFC 8414 fails (default: true)
    pub fallback_to_oidc: bool,
}

impl Default for FetcherConfig {
    fn default() -> Self {
        Self {
            max_response_size: 10 * 1024, // 10 KB
            request_timeout: Duration::from_secs(5),
            default_cache_ttl: Duration::from_secs(3600), // 1 hour
            max_cache_ttl: Duration::from_secs(86400),    // 24 hours
            user_agent: format!("TurboMCP/{}", env!("CARGO_PKG_VERSION")),
            fallback_to_oidc: true,
        }
    }
}

/// Discovery document fetcher
///
/// Fetches and caches OAuth 2.0 Authorization Server Metadata (RFC 8414) and
/// OpenID Connect Discovery 1.0 documents with:
/// - SSRF protection
/// - Multi-endpoint discovery (RFC 8414 first, OIDC Discovery as fallback)
/// - HTTP caching (respects Cache-Control headers)
/// - Response size limits
/// - Request timeouts
///
/// ## Discovery Endpoint Priority
///
/// 1. RFC 8414: `/.well-known/oauth-authorization-server[/path]`
/// 2. OIDC Discovery: `/.well-known/openid-configuration` (if fallback enabled)
pub struct DiscoveryFetcher {
    /// HTTP client
    client: reqwest::Client,

    /// SSRF validator
    ssrf_validator: Arc<SsrfValidator>,

    /// Configuration
    config: FetcherConfig,

    /// Metadata cache
    cache: Arc<DashMap<String, CacheEntry>>,
}

impl DiscoveryFetcher {
    /// Create a new discovery fetcher with default configuration
    ///
    /// # Errors
    ///
    /// Returns error if HTTP client creation fails
    pub fn new(ssrf_validator: SsrfValidator) -> Result<Self, FetcherError> {
        Self::with_config(ssrf_validator, FetcherConfig::default())
    }

    /// Create a new discovery fetcher with custom configuration
    ///
    /// # Errors
    ///
    /// Returns error if HTTP client creation fails
    pub fn with_config(
        ssrf_validator: SsrfValidator,
        config: FetcherConfig,
    ) -> Result<Self, FetcherError> {
        let client = reqwest::Client::builder()
            .timeout(config.request_timeout)
            .user_agent(&config.user_agent)
            .redirect(reqwest::redirect::Policy::none()) // Don't follow redirects (security)
            .build()
            .map_err(|e| FetcherError::HttpError(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            client,
            ssrf_validator: Arc::new(ssrf_validator),
            config,
            cache: Arc::new(DashMap::new()),
        })
    }

    /// Fetch discovery metadata from an issuer URL
    ///
    /// This method tries multiple discovery endpoints in priority order:
    /// 1. RFC 8414: `/.well-known/oauth-authorization-server[/path]`
    /// 2. OIDC Discovery: `/.well-known/openid-configuration` (if enabled)
    ///
    /// # Errors
    ///
    /// Returns [`FetcherError`] if all discovery endpoints fail
    pub async fn fetch(&self, issuer: &str) -> Result<ValidatedDiscoveryMetadata, FetcherError> {
        // Validate issuer URL
        let issuer_url = url::Url::parse(issuer)
            .map_err(|e| FetcherError::InvalidIssuer(format!("Invalid URL: {}", e)))?;

        if issuer_url.scheme() != "https" {
            return Err(FetcherError::InvalidIssuer(
                "Issuer MUST use https scheme".to_string(),
            ));
        }

        // Check cache
        if let Some(cached) = self.get_cached(issuer) {
            debug!("Returning cached discovery metadata for: {}", issuer);
            return Ok(cached);
        }

        // Try RFC 8414 first
        let oauth2_url = self.build_oauth2_discovery_url(&issuer_url)?;
        debug!("Trying RFC 8414 discovery: {}", oauth2_url);

        match self.fetch_oauth2(&oauth2_url, issuer).await {
            Ok(metadata) => {
                debug!("Successfully fetched RFC 8414 metadata for: {}", issuer);
                Ok(metadata)
            }
            Err(e) => {
                debug!("RFC 8414 discovery failed: {}", e);

                // Try OIDC Discovery as fallback if enabled
                if self.config.fallback_to_oidc {
                    let oidc_url = self.build_oidc_discovery_url(&issuer_url)?;
                    debug!("Trying OIDC Discovery fallback: {}", oidc_url);

                    match self.fetch_oidc(&oidc_url, issuer).await {
                        Ok(metadata) => {
                            debug!("Successfully fetched OIDC metadata for: {}", issuer);
                            Ok(metadata)
                        }
                        Err(oidc_error) => {
                            warn!(
                                "Both RFC 8414 and OIDC Discovery failed for issuer: {}",
                                issuer
                            );
                            Err(FetcherError::AllEndpointsFailed {
                                oauth2_error: e.to_string(),
                                oidc_error: oidc_error.to_string(),
                            })
                        }
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Build RFC 8414 discovery URL
    ///
    /// For issuer without path: `https://example.com/.well-known/oauth-authorization-server`
    /// For issuer with path: `https://example.com/.well-known/oauth-authorization-server/path`
    fn build_oauth2_discovery_url(&self, issuer: &url::Url) -> Result<String, FetcherError> {
        let mut url = issuer.clone();

        // Get the path component (empty string if no path)
        let path = url.path().trim_end_matches('/');

        // Build discovery path
        let discovery_path = if path.is_empty() || path == "/" {
            "/.well-known/oauth-authorization-server".to_string()
        } else {
            format!("/.well-known/oauth-authorization-server{}", path)
        };

        url.set_path(&discovery_path);
        Ok(url.to_string())
    }

    /// Build OIDC Discovery URL
    ///
    /// Always: `https://example.com/.well-known/openid-configuration`
    fn build_oidc_discovery_url(&self, issuer: &url::Url) -> Result<String, FetcherError> {
        let mut url = issuer.clone();
        url.set_path("/.well-known/openid-configuration");
        Ok(url.to_string())
    }

    /// Fetch OAuth 2.0 Authorization Server Metadata (RFC 8414)
    async fn fetch_oauth2(
        &self,
        discovery_url: &str,
        issuer: &str,
    ) -> Result<ValidatedDiscoveryMetadata, FetcherError> {
        // Validate URL with SSRF protection
        self.ssrf_validator.validate_url(discovery_url)?;

        // Fetch from network
        let response = self
            .client
            .get(discovery_url)
            .send()
            .await
            .map_err(|e| FetcherError::HttpError(format!("Request failed: {}", e)))?;

        // Check response status
        if !response.status().is_success() {
            return Err(FetcherError::HttpError(format!(
                "HTTP {} {}",
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("Unknown")
            )));
        }

        // Extract cache headers before consuming response
        let cache_ttl = self.parse_cache_headers(&response);

        // Check content length
        if let Some(content_length) = response.content_length()
            && content_length > self.config.max_response_size as u64
        {
            return Err(FetcherError::ResponseTooLarge);
        }

        // Read response body with size limit
        let body = response
            .bytes()
            .await
            .map_err(|e| FetcherError::HttpError(format!("Failed to read response: {}", e)))?;

        if body.len() > self.config.max_response_size {
            return Err(FetcherError::ResponseTooLarge);
        }

        // Parse and validate JSON
        let metadata: AuthorizationServerMetadata = serde_json::from_slice(&body)
            .map_err(|e| FetcherError::InvalidJson(format!("Failed to parse JSON: {}", e)))?;

        let validated = ValidatedDiscoveryMetadata::new_oauth2(metadata, issuer.to_string())?;

        // Cache the validated metadata
        self.cache_metadata(issuer, validated.clone(), cache_ttl);

        Ok(validated)
    }

    /// Fetch OpenID Connect Provider Metadata
    async fn fetch_oidc(
        &self,
        discovery_url: &str,
        issuer: &str,
    ) -> Result<ValidatedDiscoveryMetadata, FetcherError> {
        // Validate URL with SSRF protection
        self.ssrf_validator.validate_url(discovery_url)?;

        // Fetch from network
        let response = self
            .client
            .get(discovery_url)
            .send()
            .await
            .map_err(|e| FetcherError::HttpError(format!("Request failed: {}", e)))?;

        // Check response status
        if !response.status().is_success() {
            return Err(FetcherError::HttpError(format!(
                "HTTP {} {}",
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("Unknown")
            )));
        }

        // Extract cache headers before consuming response
        let cache_ttl = self.parse_cache_headers(&response);

        // Check content length
        if let Some(content_length) = response.content_length()
            && content_length > self.config.max_response_size as u64
        {
            return Err(FetcherError::ResponseTooLarge);
        }

        // Read response body with size limit
        let body = response
            .bytes()
            .await
            .map_err(|e| FetcherError::HttpError(format!("Failed to read response: {}", e)))?;

        if body.len() > self.config.max_response_size {
            return Err(FetcherError::ResponseTooLarge);
        }

        // Parse and validate JSON
        let metadata: OIDCProviderMetadata = serde_json::from_slice(&body)
            .map_err(|e| FetcherError::InvalidJson(format!("Failed to parse JSON: {}", e)))?;

        let validated = ValidatedDiscoveryMetadata::new_oidc(metadata, issuer.to_string())?;

        // Cache the validated metadata
        self.cache_metadata(issuer, validated.clone(), cache_ttl);

        Ok(validated)
    }

    /// Get cached metadata if valid
    fn get_cached(&self, issuer: &str) -> Option<ValidatedDiscoveryMetadata> {
        if let Some(entry) = self.cache.get(issuer) {
            let now = SystemTime::now();
            if now < entry.expires_at {
                return Some(entry.metadata.clone());
            } else {
                // Entry expired, remove it
                drop(entry); // Release the lock
                self.cache.remove(issuer);
            }
        }
        None
    }

    /// Cache metadata with TTL
    fn cache_metadata(&self, issuer: &str, metadata: ValidatedDiscoveryMetadata, ttl: Duration) {
        let expires_at = SystemTime::now() + ttl;

        debug!(
            "Caching discovery metadata for {} with TTL of {}s",
            issuer,
            ttl.as_secs()
        );

        self.cache.insert(
            issuer.to_string(),
            CacheEntry {
                metadata,
                expires_at,
            },
        );
    }

    /// Parse cache headers from HTTP response
    fn parse_cache_headers(&self, response: &reqwest::Response) -> Duration {
        // Parse Cache-Control header
        if let Some(cache_control) = response.headers().get("cache-control")
            && let Ok(value) = cache_control.to_str()
        {
            // Look for max-age directive
            for directive in value.split(',') {
                let directive = directive.trim();
                if let Some(max_age) = directive.strip_prefix("max-age=")
                    && let Ok(seconds) = max_age.parse::<u64>()
                {
                    let ttl = Duration::from_secs(seconds);

                    // Cap at max_cache_ttl
                    return ttl.min(self.config.max_cache_ttl);
                }
            }

            // Check for no-cache or no-store
            if value.contains("no-cache") || value.contains("no-store") {
                // Don't cache, return zero TTL
                return Duration::from_secs(0);
            }
        }

        // Fall back to default TTL
        self.config.default_cache_ttl
    }

    /// Clear the cache
    pub fn clear_cache(&self) {
        self.cache.clear();
    }

    /// Get cache statistics
    pub fn cache_stats(&self) -> CacheStats {
        let total_entries = self.cache.len();
        let mut expired = 0;
        let now = SystemTime::now();

        for entry in self.cache.iter() {
            if now >= entry.expires_at {
                expired += 1;
            }
        }

        CacheStats {
            total_entries,
            expired_entries: expired,
            valid_entries: total_entries - expired,
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Total number of cache entries
    pub total_entries: usize,

    /// Number of expired entries
    pub expired_entries: usize,

    /// Number of valid entries
    pub valid_entries: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ssrf::SsrfValidator;

    #[test]
    fn test_fetcher_creation() {
        let validator = SsrfValidator::default();
        let fetcher = DiscoveryFetcher::new(validator);
        assert!(fetcher.is_ok());
    }

    #[test]
    fn test_oauth2_discovery_url_building() {
        let validator = SsrfValidator::default();
        let fetcher = DiscoveryFetcher::new(validator).unwrap();

        // Issuer without path
        let issuer = url::Url::parse("https://example.com").unwrap();
        let url = fetcher.build_oauth2_discovery_url(&issuer).unwrap();
        assert_eq!(
            url,
            "https://example.com/.well-known/oauth-authorization-server"
        );

        // Issuer with path
        let issuer = url::Url::parse("https://example.com/issuer1").unwrap();
        let url = fetcher.build_oauth2_discovery_url(&issuer).unwrap();
        assert_eq!(
            url,
            "https://example.com/.well-known/oauth-authorization-server/issuer1"
        );
    }

    #[test]
    fn test_oidc_discovery_url_building() {
        let validator = SsrfValidator::default();
        let fetcher = DiscoveryFetcher::new(validator).unwrap();

        // Always uses same path regardless of issuer
        let issuer = url::Url::parse("https://example.com").unwrap();
        let url = fetcher.build_oidc_discovery_url(&issuer).unwrap();
        assert_eq!(url, "https://example.com/.well-known/openid-configuration");

        let issuer = url::Url::parse("https://example.com/issuer1").unwrap();
        let url = fetcher.build_oidc_discovery_url(&issuer).unwrap();
        assert_eq!(url, "https://example.com/.well-known/openid-configuration");
    }

    #[test]
    fn test_cache_ttl_parsing() {
        let validator = SsrfValidator::default();
        let fetcher = DiscoveryFetcher::new(validator).unwrap();

        // Test with max-age
        let response = reqwest::Response::from(
            http::Response::builder()
                .header("cache-control", "max-age=3600")
                .body("")
                .unwrap(),
        );

        let ttl = fetcher.parse_cache_headers(&response);
        assert_eq!(ttl, Duration::from_secs(3600));
    }

    #[test]
    fn test_cache_stats() {
        let validator = SsrfValidator::default();
        let fetcher = DiscoveryFetcher::new(validator).unwrap();

        let stats = fetcher.cache_stats();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.valid_entries, 0);
        assert_eq!(stats.expired_entries, 0);
    }
}
