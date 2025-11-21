//! # Client Metadata Document Fetcher
//!
//! HTTP fetcher for Client ID Metadata Documents with SSRF protection,
//! caching, and rate limiting as required by MCP 2025-11-25 specification.

use super::types::{ClientMetadata, ClientMetadataError, ValidatedClientMetadata};
use crate::ssrf::{SsrfError, SsrfValidator};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tracing::{debug, warn};

/// Metadata fetcher errors
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

    /// Metadata validation failed
    #[error("Metadata validation failed: {0}")]
    ValidationFailed(#[from] ClientMetadataError),

    /// Rate limit exceeded
    #[error("Rate limit exceeded for client_id: {0}")]
    RateLimitExceeded(String),

    /// Cache error
    #[error("Cache error: {0}")]
    CacheError(String),
}

/// Cache entry for metadata documents
#[derive(Debug, Clone)]
struct CacheEntry {
    /// The validated metadata
    metadata: ValidatedClientMetadata,

    /// When this entry expires
    expires_at: SystemTime,
}

/// Rate limit entry
#[derive(Debug, Clone)]
struct RateLimitEntry {
    /// Number of requests in current window
    count: u32,

    /// When the current window started
    window_start: SystemTime,
}

/// Configuration for metadata fetcher
#[derive(Debug, Clone)]
pub struct FetcherConfig {
    /// Maximum response size in bytes (default: 5KB per MCP spec)
    pub max_response_size: usize,

    /// Request timeout (default: 5 seconds)
    pub request_timeout: Duration,

    /// Default cache TTL if no cache headers present (default: 1 hour)
    pub default_cache_ttl: Duration,

    /// Maximum cache TTL (default: 24 hours)
    pub max_cache_ttl: Duration,

    /// Rate limit: max requests per window (default: 10)
    pub rate_limit_max_requests: u32,

    /// Rate limit window duration (default: 1 minute)
    pub rate_limit_window: Duration,

    /// User agent for HTTP requests
    pub user_agent: String,
}

impl Default for FetcherConfig {
    fn default() -> Self {
        Self {
            max_response_size: 5 * 1024, // 5 KB (MCP spec recommendation)
            request_timeout: Duration::from_secs(5),
            default_cache_ttl: Duration::from_secs(3600), // 1 hour
            max_cache_ttl: Duration::from_secs(86400),    // 24 hours
            rate_limit_max_requests: 10,
            rate_limit_window: Duration::from_secs(60), // 1 minute
            user_agent: format!("TurboMCP/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// Client metadata document fetcher
///
/// Fetches and caches OAuth 2.0 Client ID Metadata Documents with:
/// - SSRF protection
/// - HTTP caching (respects Cache-Control headers)
/// - Rate limiting (per-client_id)
/// - Response size limits
/// - Request timeouts
pub struct MetadataFetcher {
    /// HTTP client
    client: reqwest::Client,

    /// SSRF validator
    ssrf_validator: Arc<SsrfValidator>,

    /// Configuration
    config: FetcherConfig,

    /// Metadata cache
    cache: Arc<DashMap<String, CacheEntry>>,

    /// Rate limit tracker
    rate_limits: Arc<DashMap<String, RateLimitEntry>>,
}

impl MetadataFetcher {
    /// Create a new metadata fetcher with default configuration
    ///
    /// # Errors
    ///
    /// Returns error if HTTP client creation fails
    pub fn new(ssrf_validator: SsrfValidator) -> Result<Self, FetcherError> {
        Self::with_config(ssrf_validator, FetcherConfig::default())
    }

    /// Create a new metadata fetcher with custom configuration
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
            rate_limits: Arc::new(DashMap::new()),
        })
    }

    /// Fetch metadata document from a client_id URL
    ///
    /// This method:
    /// 1. Validates the URL with SSRF protection
    /// 2. Checks rate limits
    /// 3. Checks cache for valid entry
    /// 4. Fetches from network if not cached
    /// 5. Validates the response
    /// 6. Caches the validated metadata
    ///
    /// # Errors
    ///
    /// Returns [`FetcherError`] if fetch or validation fails
    pub async fn fetch(
        &self,
        client_id_url: &str,
    ) -> Result<ValidatedClientMetadata, FetcherError> {
        // 1. Validate URL with SSRF protection
        debug!("Validating client_id URL: {}", client_id_url);
        self.ssrf_validator.validate_url(client_id_url)?;

        // 2. Check rate limits
        self.check_rate_limit(client_id_url)?;

        // 3. Check cache
        if let Some(cached) = self.get_cached(client_id_url) {
            debug!("Returning cached metadata for: {}", client_id_url);
            return Ok(cached);
        }

        // 4. Fetch from network
        debug!("Fetching metadata from network: {}", client_id_url);
        let response = self
            .client
            .get(client_id_url)
            .send()
            .await
            .map_err(|e| FetcherError::HttpError(format!("Request failed: {}", e)))?;

        // Check response status
        if !response.status().is_success() {
            warn!(
                "Non-success status {} for client_id: {}",
                response.status(),
                client_id_url
            );
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

        // 5. Parse and validate JSON
        let metadata: ClientMetadata = serde_json::from_slice(&body)
            .map_err(|e| FetcherError::InvalidJson(format!("Failed to parse JSON: {}", e)))?;

        let validated = ValidatedClientMetadata::new(metadata, client_id_url.to_string())?;

        // 6. Cache the validated metadata
        self.cache_metadata(client_id_url, validated.clone(), cache_ttl);

        Ok(validated)
    }

    /// Check if a client_id is rate limited
    fn check_rate_limit(&self, client_id: &str) -> Result<(), FetcherError> {
        let now = SystemTime::now();

        let mut entry = self
            .rate_limits
            .entry(client_id.to_string())
            .or_insert(RateLimitEntry {
                count: 0,
                window_start: now,
            });

        // Check if we're in a new window
        if let Ok(elapsed) = now.duration_since(entry.window_start)
            && elapsed >= self.config.rate_limit_window
        {
            // Start new window
            entry.count = 0;
            entry.window_start = now;
        }

        // Check if limit exceeded
        if entry.count >= self.config.rate_limit_max_requests {
            warn!("Rate limit exceeded for client_id: {}", client_id);
            return Err(FetcherError::RateLimitExceeded(client_id.to_string()));
        }

        // Increment counter
        entry.count += 1;

        Ok(())
    }

    /// Get cached metadata if valid
    fn get_cached(&self, client_id: &str) -> Option<ValidatedClientMetadata> {
        if let Some(entry) = self.cache.get(client_id) {
            let now = SystemTime::now();
            if now < entry.expires_at {
                return Some(entry.metadata.clone());
            } else {
                // Entry expired, remove it
                drop(entry); // Release the lock
                self.cache.remove(client_id);
            }
        }
        None
    }

    /// Cache metadata with TTL
    fn cache_metadata(&self, client_id: &str, metadata: ValidatedClientMetadata, ttl: Duration) {
        let expires_at = SystemTime::now() + ttl;

        debug!(
            "Caching metadata for {} with TTL of {}s",
            client_id,
            ttl.as_secs()
        );

        self.cache.insert(
            client_id.to_string(),
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
    use crate::ssrf::SsrfPolicy;

    #[test]
    fn test_fetcher_creation() {
        let validator = SsrfValidator::default();
        let fetcher = MetadataFetcher::new(validator);
        assert!(fetcher.is_ok());
    }

    #[test]
    fn test_cache_ttl_parsing() {
        let validator = SsrfValidator::default();
        let fetcher = MetadataFetcher::new(validator).unwrap();

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
        let fetcher = MetadataFetcher::new(validator).unwrap();

        let stats = fetcher.cache_stats();
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.valid_entries, 0);
        assert_eq!(stats.expired_entries, 0);
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let validator = SsrfValidator::new(SsrfPolicy {
            allow_private_networks: true,
            allow_localhost: true,
            ..Default::default()
        });

        let config = FetcherConfig {
            rate_limit_max_requests: 2,
            rate_limit_window: Duration::from_secs(60),
            ..Default::default()
        };

        let fetcher = MetadataFetcher::with_config(validator, config).unwrap();

        let client_id = "https://example.com/metadata.json";

        // First two requests should succeed
        assert!(fetcher.check_rate_limit(client_id).is_ok());
        assert!(fetcher.check_rate_limit(client_id).is_ok());

        // Third request should fail (rate limited)
        assert!(matches!(
            fetcher.check_rate_limit(client_id),
            Err(FetcherError::RateLimitExceeded(_))
        ));
    }
}
