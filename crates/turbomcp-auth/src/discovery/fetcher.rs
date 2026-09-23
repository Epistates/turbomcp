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
    #[error("All discovery endpoints failed: {attempts:?}")]
    AllEndpointsFailed {
        /// `(url, error)` for every endpoint tried, in priority order
        attempts: Vec<(String, String)>,
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
    ///
    /// NOTE: requests now go through [`crate::ssrf::SsrfValidator`]'s
    /// DNS-pinned client (see `fetch_pinned`) rather than a client built from
    /// this config, so the *effective* timeout is
    /// `SsrfPolicy::request_timeout` on the validator passed to
    /// [`DiscoveryFetcher::new`]/[`DiscoveryFetcher::with_config`]. Both
    /// default to 5s; set the SSRF policy's timeout if you need a different
    /// value.
    pub request_timeout: Duration,

    /// Default cache TTL if no cache headers present (default: 1 hour)
    pub default_cache_ttl: Duration,

    /// Maximum cache TTL (default: 24 hours)
    pub max_cache_ttl: Duration,

    /// User agent for HTTP requests
    ///
    /// NOTE: not currently applied — see the note on `request_timeout`.
    pub user_agent: String,

    /// Whether to try OIDC discovery if RFC 8414 fails (default: true)
    ///
    /// Only consulted for issuers *without* a path component (RFC 8414 is the
    /// only mandatory endpoint there per the MCP spec); for issuers with a
    /// path, both OIDC forms are always tried after RFC 8414, per the spec's
    /// discovery priority order.
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
/// - SSRF protection (DNS-pinned fetches, see `fetch_pinned`)
/// - Multi-endpoint discovery, MCP 2025-11-25 priority order (see `fetch`)
/// - HTTP caching (respects Cache-Control headers)
/// - Response size limits
pub struct DiscoveryFetcher {
    /// SSRF validator. Also the source of the HTTP client used for every
    /// request — see `fetch_pinned` for why there's no separately-built
    /// `reqwest::Client` here.
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
        Ok(Self {
            ssrf_validator: Arc::new(ssrf_validator),
            config,
            cache: Arc::new(DashMap::new()),
        })
    }

    /// Fetch discovery metadata from an issuer URL
    ///
    /// Implements the endpoint priority order the MCP 2025-11-25 spec requires
    /// ("Authorization Server Metadata Discovery"), which differs depending on
    /// whether the issuer URL has a path component:
    ///
    /// For an issuer *with* a path (e.g. `https://auth.example.com/tenant1`):
    /// 1. RFC 8414 with path insertion: `/.well-known/oauth-authorization-server/tenant1`
    /// 2. OIDC Discovery with path insertion: `/.well-known/openid-configuration/tenant1`
    /// 3. OIDC Discovery with path appending: `/tenant1/.well-known/openid-configuration`
    ///
    /// For an issuer *without* a path:
    /// 1. RFC 8414: `/.well-known/oauth-authorization-server`
    /// 2. OIDC Discovery: `/.well-known/openid-configuration`
    ///
    /// Every endpoint is tried in order until one succeeds; this is the only
    /// discovery algorithm in the crate (see [`crate::jwt::JwtValidator`],
    /// which is built on top of this fetcher rather than reimplementing its
    /// own, narrower version).
    ///
    /// # Errors
    ///
    /// Returns [`FetcherError::AllEndpointsFailed`] if every endpoint fails.
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

        let has_path = {
            let path = issuer_url.path().trim_end_matches('/');
            !path.is_empty() && path != "/"
        };

        // (is_oidc, url) pairs, in the spec-mandated priority order.
        let mut endpoints = vec![(false, self.build_oauth2_discovery_url(&issuer_url)?)];
        if has_path {
            endpoints.push((true, self.build_oidc_path_insertion_url(&issuer_url)?));
            endpoints.push((true, self.build_oidc_path_appending_url(&issuer_url)?));
        } else if self.config.fallback_to_oidc {
            endpoints.push((true, self.build_oidc_path_insertion_url(&issuer_url)?));
        }

        let mut attempts = Vec::with_capacity(endpoints.len());
        for (is_oidc, url) in endpoints {
            debug!(url = %url, "Trying authorization server discovery endpoint");
            let result = if is_oidc {
                self.fetch_oidc(&url, issuer).await
            } else {
                self.fetch_oauth2(&url, issuer).await
            };

            match result {
                Ok(metadata) => {
                    debug!(url = %url, "Successfully fetched authorization server metadata");
                    return Ok(metadata);
                }
                Err(e) => attempts.push((url, e.to_string())),
            }
        }

        warn!(issuer = %issuer, "All authorization server discovery endpoints failed");
        Err(FetcherError::AllEndpointsFailed { attempts })
    }

    /// Build RFC 8414 discovery URL (path-insertion form)
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

    /// Build the OIDC Discovery URL, path-insertion form.
    ///
    /// For issuer without path: `https://example.com/.well-known/openid-configuration`
    /// For issuer with path: `https://example.com/.well-known/openid-configuration/path`
    fn build_oidc_path_insertion_url(&self, issuer: &url::Url) -> Result<String, FetcherError> {
        let mut url = issuer.clone();

        let path = url.path().trim_end_matches('/');
        let discovery_path = if path.is_empty() || path == "/" {
            "/.well-known/openid-configuration".to_string()
        } else {
            format!("/.well-known/openid-configuration{}", path)
        };

        url.set_path(&discovery_path);
        Ok(url.to_string())
    }

    /// Build the OIDC Discovery URL, path-appending form (only meaningful for
    /// issuers with a path component): `https://example.com/path/.well-known/openid-configuration`
    fn build_oidc_path_appending_url(&self, issuer: &url::Url) -> Result<String, FetcherError> {
        let mut url = issuer.clone();

        let path = url.path().trim_end_matches('/');
        url.set_path(&format!("{}/.well-known/openid-configuration", path));
        Ok(url.to_string())
    }

    /// Fetch OAuth 2.0 Authorization Server Metadata (RFC 8414)
    async fn fetch_oauth2(
        &self,
        discovery_url: &str,
        issuer: &str,
    ) -> Result<ValidatedDiscoveryMetadata, FetcherError> {
        // Validate URL, then fetch over a client whose DNS resolution is
        // pinned to the IPs that were just validated (see `fetch_pinned` doc
        // comment for why: validating a URL and then fetching it over an
        // unrelated connection is a TOCTOU gap — DNS can resolve differently
        // between the two).
        let response = self.fetch_pinned(discovery_url).await?;

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
        let response = self.fetch_pinned(discovery_url).await?;

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

    /// Validate a URL against the SSRF policy and fetch it over a client
    /// whose DNS resolution is pinned to the IP addresses that validation
    /// just checked.
    ///
    /// `self.client` (built once, in `with_config`, from `FetcherConfig`) is
    /// deliberately *not* used for the actual request: resolving the
    /// hostname during `validate_url` and then handing the same hostname to
    /// an independent client for the real connection is a TOCTOU gap — an
    /// attacker who controls DNS for the issuer's host can return a benign IP
    /// for the validation lookup and a private/metadata IP for the follow-up
    /// lookup the independent client performs milliseconds later (DNS
    /// rebinding). `SsrfValidator::create_pinned_client` resolves once,
    /// validates every resolved IP, and returns a client hard-pinned to that
    /// address, so the request physically cannot land anywhere else.
    ///
    /// Note this means the request's timeout and redirect policy come from
    /// `self.ssrf_validator`'s [`crate::ssrf::SsrfPolicy`], not from
    /// `self.config` — both default to 5s / no-redirects, but a caller who
    /// customizes only `FetcherConfig::request_timeout` won't see it applied
    /// here.
    async fn fetch_pinned(&self, url: &str) -> Result<reqwest::Response, FetcherError> {
        self.ssrf_validator.validate_url(url)?;
        let (client, pinned_url) = self.ssrf_validator.create_pinned_client(url)?;
        client
            .get(&pinned_url)
            .send()
            .await
            .map_err(|e| FetcherError::HttpError(format!("Request failed: {}", e)))
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

    /// AU-10: the OIDC path-insertion form must insert the issuer's path
    /// after `/.well-known/openid-configuration`, not drop it — dropping it
    /// means two different tenants under the same host resolve to the same
    /// discovery document.
    #[test]
    fn test_oidc_path_insertion_url_building() {
        let validator = SsrfValidator::default();
        let fetcher = DiscoveryFetcher::new(validator).unwrap();

        let issuer = url::Url::parse("https://example.com").unwrap();
        let url = fetcher.build_oidc_path_insertion_url(&issuer).unwrap();
        assert_eq!(url, "https://example.com/.well-known/openid-configuration");

        let issuer = url::Url::parse("https://auth.example.com/tenant1").unwrap();
        let url = fetcher.build_oidc_path_insertion_url(&issuer).unwrap();
        assert_eq!(
            url,
            "https://auth.example.com/.well-known/openid-configuration/tenant1"
        );
    }

    /// AU-10: the third priority-order endpoint for path-bearing issuers.
    #[test]
    fn test_oidc_path_appending_url_building() {
        let validator = SsrfValidator::default();
        let fetcher = DiscoveryFetcher::new(validator).unwrap();

        let issuer = url::Url::parse("https://auth.example.com/tenant1").unwrap();
        let url = fetcher.build_oidc_path_appending_url(&issuer).unwrap();
        assert_eq!(
            url,
            "https://auth.example.com/tenant1/.well-known/openid-configuration"
        );
    }

    /// AU-10: the MCP 2025-11-25 spec's exact priority order — path-bearing
    /// issuers try 3 endpoints (RFC 8414, then OIDC path-insertion, then OIDC
    /// path-appending); path-free issuers try 2 (RFC 8414, then OIDC).
    #[test]
    fn test_discovery_priority_order_matches_spec() {
        let validator = SsrfValidator::default();
        let fetcher = DiscoveryFetcher::new(validator).unwrap();

        let with_path = url::Url::parse("https://auth.example.com/tenant1").unwrap();
        assert_eq!(
            vec![
                fetcher.build_oauth2_discovery_url(&with_path).unwrap(),
                fetcher.build_oidc_path_insertion_url(&with_path).unwrap(),
                fetcher.build_oidc_path_appending_url(&with_path).unwrap(),
            ],
            vec![
                "https://auth.example.com/.well-known/oauth-authorization-server/tenant1",
                "https://auth.example.com/.well-known/openid-configuration/tenant1",
                "https://auth.example.com/tenant1/.well-known/openid-configuration",
            ]
        );

        let no_path = url::Url::parse("https://auth.example.com").unwrap();
        assert_eq!(
            vec![
                fetcher.build_oauth2_discovery_url(&no_path).unwrap(),
                fetcher.build_oidc_path_insertion_url(&no_path).unwrap(),
            ],
            vec![
                "https://auth.example.com/.well-known/oauth-authorization-server",
                "https://auth.example.com/.well-known/openid-configuration",
            ]
        );
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
