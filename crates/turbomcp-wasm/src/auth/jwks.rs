//! JWKS (JSON Web Key Set) fetching and caching for WASM.
//!
//! This module handles fetching and caching JWKS from identity providers.

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
use turbomcp_core::auth::{AuthError, JwtAlgorithm};
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

/// A JSON Web Key (JWK) as defined in RFC 7517.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwk {
    /// Key type (e.g., "RSA", "EC")
    pub kty: String,

    /// Key ID (used to match keys in JWKS)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,

    /// Algorithm (e.g., "RS256", "ES256")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alg: Option<String>,

    /// Key use (e.g., "sig" for signature)
    #[serde(rename = "use", skip_serializing_if = "Option::is_none")]
    pub use_: Option<String>,

    // RSA-specific parameters
    /// RSA modulus (base64url encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,

    /// RSA exponent (base64url encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e: Option<String>,

    // EC-specific parameters
    /// EC curve (e.g., "P-256", "P-384")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,

    /// EC x coordinate (base64url encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,

    /// EC y coordinate (base64url encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,

    // HMAC-specific parameters
    /// Symmetric key value (base64url encoded, for HMAC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub k: Option<String>,
}

impl Jwk {
    /// Get the algorithm for this key
    pub fn algorithm(&self) -> Option<JwtAlgorithm> {
        self.alg.as_ref().and_then(|a| a.parse().ok())
    }

    /// Check if this is an RSA key
    pub fn is_rsa(&self) -> bool {
        self.kty == "RSA" && self.n.is_some() && self.e.is_some()
    }

    /// Check if this is an EC key
    pub fn is_ec(&self) -> bool {
        self.kty == "EC" && self.crv.is_some() && self.x.is_some() && self.y.is_some()
    }

    /// Check if this is a symmetric (HMAC) key
    pub fn is_symmetric(&self) -> bool {
        self.kty == "oct" && self.k.is_some()
    }

    /// Check if this key can be used for signing/verification
    pub fn is_signing_key(&self) -> bool {
        self.use_.as_ref().is_none_or(|u| u == "sig")
    }

    /// Validate that the key type is compatible with the given algorithm.
    ///
    /// # Security
    ///
    /// This prevents algorithm confusion attacks where an attacker might try
    /// to use an RSA public key as an HMAC secret, or vice versa.
    ///
    /// - RSA keys (`kty: "RSA"`) can only be used with RS256, RS384, RS512
    /// - EC keys (`kty: "EC"`) can only be used with ES256, ES384
    /// - Symmetric keys (`kty: "oct"`) can only be used with HS256, HS384, HS512
    pub fn is_compatible_with_algorithm(&self, algorithm: JwtAlgorithm) -> bool {
        match algorithm {
            // RSA algorithms require RSA keys
            JwtAlgorithm::RS256 | JwtAlgorithm::RS384 | JwtAlgorithm::RS512 => self.is_rsa(),

            // ECDSA algorithms require EC keys with matching curves
            JwtAlgorithm::ES256 => self.is_ec() && self.crv.as_deref() == Some("P-256"),
            JwtAlgorithm::ES384 => self.is_ec() && self.crv.as_deref() == Some("P-384"),

            // HMAC algorithms require symmetric keys
            JwtAlgorithm::HS256 | JwtAlgorithm::HS384 | JwtAlgorithm::HS512 => self.is_symmetric(),
        }
    }

    /// Validate key-algorithm compatibility and return a descriptive error.
    ///
    /// # Security
    ///
    /// Always call this before using a key for signature verification to
    /// prevent algorithm confusion attacks.
    pub fn validate_algorithm_compatibility(
        &self,
        algorithm: JwtAlgorithm,
    ) -> Result<(), AuthError> {
        if !self.is_compatible_with_algorithm(algorithm) {
            let key_type = if self.is_rsa() {
                "RSA".to_string()
            } else if self.is_ec() {
                format!("EC ({})", self.crv.as_deref().unwrap_or("unknown curve"))
            } else if self.is_symmetric() {
                "symmetric (oct)".to_string()
            } else {
                "unknown".to_string()
            };

            return Err(AuthError::InvalidCredentialFormat(format!(
                "Key type '{}' is not compatible with algorithm {}. \
                 This may indicate an algorithm confusion attack.",
                key_type, algorithm
            )));
        }
        Ok(())
    }

    /// Validate that this key is suitable for public JWKS endpoints.
    ///
    /// # Security
    ///
    /// Symmetric keys (HMAC, kty: "oct") MUST NOT appear in public JWKS
    /// endpoints because they would expose the shared secret to anyone who
    /// fetches the JWKS. This would allow attackers to forge JWTs.
    ///
    /// Always call this when fetching keys from public JWKS endpoints to
    /// prevent accidental exposure of shared secrets.
    pub fn validate_for_public_jwks(&self) -> Result<(), AuthError> {
        if self.is_symmetric() {
            return Err(AuthError::InvalidCredentialFormat(
                "Symmetric keys (kty: oct) must not appear in public JWKS endpoints. \
                 This would expose the shared secret and allow anyone to forge JWTs. \
                 Use asymmetric keys (RSA, EC) for public JWKS."
                    .to_string(),
            ));
        }
        Ok(())
    }

    /// Convert to a web-sys JsonWebKey for importing
    pub fn to_web_sys_jwk(&self) -> web_sys::JsonWebKey {
        let jwk = web_sys::JsonWebKey::new(&self.kty);

        if let Some(ref alg) = self.alg {
            jwk.set_alg(alg);
        }

        if let Some(ref n) = self.n {
            jwk.set_n(n);
        }

        if let Some(ref e) = self.e {
            jwk.set_e(e);
        }

        if let Some(ref crv) = self.crv {
            jwk.set_crv(crv);
        }

        if let Some(ref x) = self.x {
            jwk.set_x(x);
        }

        if let Some(ref y) = self.y {
            jwk.set_y(y);
        }

        if let Some(ref k) = self.k {
            jwk.set_k(k);
        }

        // Set key_ops for verification
        let key_ops = js_sys::Array::new();
        key_ops.push(&JsValue::from_str("verify"));
        jwk.set_key_ops(&key_ops);

        jwk
    }
}

/// A JSON Web Key Set (JWKS) as defined in RFC 7517.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwkSet {
    /// The keys in the set
    pub keys: Vec<Jwk>,
}

impl JwkSet {
    /// Find a key by key ID
    pub fn find_by_kid(&self, kid: &str) -> Option<&Jwk> {
        self.keys.iter().find(|k| k.kid.as_deref() == Some(kid))
    }

    /// Find a key by algorithm
    pub fn find_by_algorithm(&self, alg: JwtAlgorithm) -> Option<&Jwk> {
        let alg_str = alg.as_str();
        self.keys
            .iter()
            .find(|k| k.alg.as_deref() == Some(alg_str) && k.is_signing_key())
    }

    /// Get the first signing key
    pub fn first_signing_key(&self) -> Option<&Jwk> {
        self.keys.iter().find(|k| k.is_signing_key())
    }

    /// Validate all keys are suitable for public JWKS endpoints.
    ///
    /// # Security
    ///
    /// This filters out symmetric keys (HMAC) which should never appear in
    /// public JWKS endpoints. Returns a new JwkSet with only asymmetric keys.
    ///
    /// Logs a warning for each symmetric key found, as this indicates a
    /// serious security misconfiguration on the authorization server.
    pub fn filter_for_public_jwks(self) -> Self {
        let mut valid_keys = Vec::new();

        for key in self.keys {
            if let Err(e) = key.validate_for_public_jwks() {
                // Log warning about symmetric key in public JWKS
                #[cfg(target_arch = "wasm32")]
                web_sys::console::warn_1(
                    &format!(
                        "⚠️  Security: Rejecting symmetric key from public JWKS (kid: {:?}): {}",
                        key.kid, e
                    )
                    .into(),
                );
                continue;
            }
            valid_keys.push(key);
        }

        Self { keys: valid_keys }
    }
}

/// Cache entry for JWKS with expiration tracking
#[derive(Debug, Clone)]
struct CacheEntry {
    jwks: JwkSet,
    fetched_at: f64, // js_sys::Date timestamp
}

/// Internal fetch error type that tracks retryability without exposing it to clients.
///
/// This allows the retry logic to distinguish transient from permanent failures
/// while returning generic error messages to prevent information leakage.
enum FetchError {
    /// Transient error that should be retried (network failures, 5xx server errors)
    Transient(AuthError),
    /// Permanent error that should not be retried (4xx, validation failures)
    Permanent(AuthError),
}

impl FetchError {
    fn into_auth_error(self) -> AuthError {
        match self {
            Self::Transient(e) | Self::Permanent(e) => e,
        }
    }

    fn is_transient(&self) -> bool {
        matches!(self, Self::Transient(_))
    }
}

/// Maximum cache age in milliseconds (6 hours) regardless of configured TTL.
///
/// # Cloudflare Workers Isolate Lifecycle
///
/// Cloudflare Workers isolates can persist for hours (sometimes days) without
/// restarting. The `Rc<RefCell<>>` cache persists for the isolate's lifetime.
///
/// This hard cap prevents serving stale keys indefinitely, which is critical
/// for security when keys are rotated (e.g., after a compromise).
///
/// Even if the configured TTL is longer (e.g., 24 hours), the cache will
/// force a refresh after 6 hours to ensure reasonable key freshness.
const MAX_CACHE_AGE_MS: f64 = 6.0 * 3600.0 * 1000.0; // 6 hours

/// JWKS cache for efficient key lookups.
///
/// Caches fetched JWKS and automatically refreshes when expired.
/// Includes retry logic with exponential backoff for transient failures.
///
/// # Security
///
/// The JWKS URL **must** use HTTPS to prevent man-in-the-middle attacks
/// on cryptographic key material. HTTP URLs will be rejected unless
/// explicitly allowed via [`JwksCache::allow_insecure_http`] (NOT recommended
/// for production use).
///
/// # Cache Lifetime in Cloudflare Workers
///
/// The cache uses `Rc<RefCell<>>` which persists for the Worker isolate
/// lifetime (potentially hours or days). To prevent serving stale keys
/// indefinitely, a hard maximum cache age of 6 hours is enforced regardless
/// of the configured TTL. This ensures keys are refreshed even if the Worker
/// isolate doesn't restart.
#[derive(Clone)]
pub struct JwksCache {
    /// JWKS endpoint URL
    url: String,

    /// Cache TTL in milliseconds (default: 1 hour, max: 6 hours)
    ttl_ms: f64,

    /// Cached JWKS entries
    cache: Rc<RefCell<Option<CacheEntry>>>,

    /// Maximum retry attempts for transient failures
    max_retries: u32,

    /// Base delay in milliseconds for exponential backoff
    retry_base_delay_ms: f64,

    /// Allow insecure HTTP URLs (NOT recommended for production)
    allow_insecure: bool,
}

impl JwksCache {
    /// Create a new JWKS cache for the given URL.
    ///
    /// # Security
    ///
    /// The URL **must** use HTTPS. HTTP URLs will cause an error when
    /// fetching keys to prevent man-in-the-middle attacks on key material.
    ///
    /// # Panics
    ///
    /// Does not panic, but will return an error on first fetch if the URL
    /// is not HTTPS.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ttl_ms: 3600.0 * 1000.0, // 1 hour
            cache: Rc::new(RefCell::new(None)),
            max_retries: 3,
            retry_base_delay_ms: 100.0,
            allow_insecure: false,
        }
    }

    /// Allow insecure HTTP URLs for JWKS fetching.
    ///
    /// # Security Warning
    ///
    /// **DO NOT USE IN PRODUCTION.** This allows man-in-the-middle attacks
    /// where an attacker could substitute their own keys, completely
    /// bypassing JWT signature validation.
    ///
    /// This should ONLY be used for:
    /// - Local development with localhost URLs
    /// - Testing environments
    ///
    /// ```rust,ignore
    /// // ⚠️ DANGER: Only for development!
    /// let cache = JwksCache::new("http://localhost:8080/.well-known/jwks.json")
    ///     .allow_insecure_http();
    /// ```
    pub fn allow_insecure_http(mut self) -> Self {
        self.allow_insecure = true;
        self
    }

    /// Validate that the URL uses HTTPS (unless insecure mode is enabled).
    fn validate_url(&self) -> Result<(), AuthError> {
        let url_lower = self.url.to_lowercase();

        // Allow localhost for development even without explicit insecure flag
        let is_localhost = url_lower.contains("://localhost")
            || url_lower.contains("://127.0.0.1")
            || url_lower.contains("://[::1]");

        if self.allow_insecure || is_localhost {
            return Ok(());
        }

        if !url_lower.starts_with("https://") {
            return Err(AuthError::KeyFetchError(
                "JWKS URL must use HTTPS to prevent man-in-the-middle attacks. \
                 Use allow_insecure_http() only for local development."
                    .to_string(),
            ));
        }

        Ok(())
    }

    /// Set the cache TTL in seconds
    pub fn with_ttl_seconds(mut self, seconds: u64) -> Self {
        self.ttl_ms = seconds as f64 * 1000.0;
        self
    }

    /// Configure retry behavior for transient failures
    ///
    /// # Arguments
    ///
    /// * `max_retries` - Maximum number of retry attempts (default: 3)
    /// * `base_delay_ms` - Base delay in milliseconds for exponential backoff (default: 100)
    pub fn with_retry_config(mut self, max_retries: u32, base_delay_ms: f64) -> Self {
        self.max_retries = max_retries;
        self.retry_base_delay_ms = base_delay_ms;
        self
    }

    /// Get the JWKS, fetching if needed
    ///
    /// # Cache Expiration
    ///
    /// The cache is considered expired if either:
    /// 1. Age exceeds the configured TTL, OR
    /// 2. Age exceeds the hard maximum (6 hours)
    ///
    /// The hard maximum prevents serving stale keys in long-lived Worker
    /// isolates that might not restart for hours or days.
    pub async fn get_jwks(&self) -> Result<JwkSet, AuthError> {
        // Check cache first
        if let Some(ref entry) = *self.cache.borrow() {
            let now = js_sys::Date::now();
            let age_ms = now - entry.fetched_at;

            // Enforce BOTH the configured TTL and the hard maximum age
            let effective_ttl = self.ttl_ms.min(MAX_CACHE_AGE_MS);

            if age_ms < effective_ttl {
                return Ok(entry.jwks.clone());
            }
        }

        // Fetch fresh JWKS
        let jwks = self.fetch_jwks().await?;

        // Update cache
        *self.cache.borrow_mut() = Some(CacheEntry {
            jwks: jwks.clone(),
            fetched_at: js_sys::Date::now(),
        });

        Ok(jwks)
    }

    /// Force refresh the cache
    pub async fn refresh(&self) -> Result<JwkSet, AuthError> {
        let jwks = self.fetch_jwks().await?;

        *self.cache.borrow_mut() = Some(CacheEntry {
            jwks: jwks.clone(),
            fetched_at: js_sys::Date::now(),
        });

        Ok(jwks)
    }

    /// Find a key by ID, fetching JWKS if needed
    pub async fn find_key(&self, kid: &str) -> Result<Jwk, AuthError> {
        let jwks = self.get_jwks().await?;

        jwks.find_by_kid(kid)
            .cloned()
            .ok_or_else(|| AuthError::KeyNotFound(kid.to_string()))
    }

    /// Fetch JWKS from the endpoint with retry logic
    async fn fetch_jwks(&self) -> Result<JwkSet, AuthError> {
        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            match self.fetch_jwks_internal().await {
                Ok(jwks) => return Ok(jwks),
                Err(fetch_err) => {
                    // Only retry on transient errors (network failures, 5xx)
                    if !fetch_err.is_transient() {
                        return Err(fetch_err.into_auth_error());
                    }

                    last_error = Some(fetch_err.into_auth_error());

                    // Don't sleep after last attempt
                    if attempt < self.max_retries {
                        let delay_ms = self.retry_base_delay_ms * 2_f64.powi(attempt as i32);
                        Self::sleep_ms(delay_ms).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            AuthError::KeyFetchError("Failed to fetch JWKS from authorization server".to_string())
        }))
    }

    /// Internal JWKS fetch with error classification (no retry).
    ///
    /// Returns `FetchError` to classify transient vs permanent failures
    /// without exposing this information in error messages.
    async fn fetch_jwks_internal(&self) -> Result<JwkSet, FetchError> {
        // SECURITY: Validate URL uses HTTPS before fetching key material
        self.validate_url().map_err(FetchError::Permanent)?;

        let window = web_sys::window().ok_or_else(|| {
            FetchError::Permanent(AuthError::Internal(
                "No window object available".to_string(),
            ))
        })?;

        // Create fetch request
        let request = web_sys::Request::new_with_str(&self.url).map_err(|_| {
            // Request creation failure is permanent (bad URL, etc.)
            FetchError::Permanent(AuthError::KeyFetchError(
                "Failed to fetch JWKS from authorization server".to_string(),
            ))
        })?;

        // Execute fetch - network errors are transient
        let promise = window.fetch_with_request(&request);
        let response = JsFuture::from(promise).await.map_err(|e| {
            // Log network error details for operators
            #[cfg(target_arch = "wasm32")]
            web_sys::console::warn_1(&format!("JWKS network fetch failed: {:?}", e).into());
            FetchError::Transient(AuthError::KeyFetchError(
                "Failed to fetch JWKS from authorization server".to_string(),
            ))
        })?;

        let response: web_sys::Response = response.dyn_into().map_err(|_| {
            FetchError::Permanent(AuthError::KeyFetchError(
                "Failed to fetch JWKS from authorization server".to_string(),
            ))
        })?;

        if !response.ok() {
            let status = response.status();
            // Log details for operators only
            #[cfg(target_arch = "wasm32")]
            web_sys::console::warn_1(
                &format!("JWKS fetch failed with HTTP status {}", status).into(),
            );

            // 5xx errors are transient (server issues), 4xx are permanent (client/config issues)
            let error = AuthError::KeyFetchError(
                "Failed to fetch JWKS from authorization server".to_string(),
            );
            return if status >= 500 {
                Err(FetchError::Transient(error))
            } else {
                Err(FetchError::Permanent(error))
            };
        }

        // Parse JSON body - parse errors are permanent
        let json_promise = response.json().map_err(|_| {
            FetchError::Permanent(AuthError::KeyFetchError(
                "Failed to fetch JWKS from authorization server".to_string(),
            ))
        })?;

        let json_value = JsFuture::from(json_promise).await.map_err(|_| {
            FetchError::Permanent(AuthError::KeyFetchError(
                "Failed to fetch JWKS from authorization server".to_string(),
            ))
        })?;

        // Convert to our JwkSet type - format errors are permanent
        let jwks: JwkSet = serde_wasm_bindgen::from_value(json_value).map_err(|e| {
            #[cfg(target_arch = "wasm32")]
            web_sys::console::warn_1(&format!("JWKS format invalid: {:?}", e).into());
            FetchError::Permanent(AuthError::KeyFetchError(
                "Failed to fetch JWKS from authorization server".to_string(),
            ))
        })?;

        // SECURITY: Filter out symmetric keys from public JWKS endpoints.
        // Symmetric keys would expose the shared secret, allowing anyone to forge JWTs.
        let jwks = jwks.filter_for_public_jwks();

        Ok(jwks)
    }

    /// Sleep for the specified milliseconds.
    ///
    /// Uses `setTimeout` via the global object, which works in both browsers
    /// and Cloudflare Workers (which don't have a `window` object).
    async fn sleep_ms(ms: f64) {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            // Use global() instead of window() for Workers compatibility.
            // Both browsers and Workers support setTimeout on the global object.
            let global = js_sys::global();
            let set_timeout = js_sys::Reflect::get(&global, &"setTimeout".into())
                .ok()
                .and_then(|v| v.dyn_into::<js_sys::Function>().ok());

            if let Some(timeout_fn) = set_timeout {
                let _ = timeout_fn.call2(&global, &resolve, &(ms as i32).into());
            } else {
                // Fallback: resolve immediately if setTimeout not available
                let _ = resolve.call0(&JsValue::undefined());
            }
        });
        let _ = JsFuture::from(promise).await;
    }

    /// Clear the cache
    pub fn clear(&self) {
        *self.cache.borrow_mut() = None;
    }
}

impl std::fmt::Debug for JwksCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwksCache")
            .field("url", &self.url)
            .field("ttl_ms", &self.ttl_ms)
            .finish()
    }
}

/// Fetch JWKS from a URL (standalone function for simple use cases)
pub async fn fetch_jwks(url: &str) -> Result<JwkSet, AuthError> {
    let cache = JwksCache::new(url);
    cache.get_jwks().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwk_is_rsa() {
        let jwk = Jwk {
            kty: "RSA".to_string(),
            kid: Some("key1".to_string()),
            alg: Some("RS256".to_string()),
            use_: Some("sig".to_string()),
            n: Some("modulus".to_string()),
            e: Some("AQAB".to_string()),
            crv: None,
            x: None,
            y: None,
            k: None,
        };

        assert!(jwk.is_rsa());
        assert!(!jwk.is_ec());
        assert!(jwk.is_signing_key());
        assert_eq!(jwk.algorithm(), Some(JwtAlgorithm::RS256));
    }

    #[test]
    fn test_jwk_is_ec() {
        let jwk = Jwk {
            kty: "EC".to_string(),
            kid: Some("key2".to_string()),
            alg: Some("ES256".to_string()),
            use_: Some("sig".to_string()),
            n: None,
            e: None,
            crv: Some("P-256".to_string()),
            x: Some("x-coord".to_string()),
            y: Some("y-coord".to_string()),
            k: None,
        };

        assert!(!jwk.is_rsa());
        assert!(jwk.is_ec());
        assert!(jwk.is_signing_key());
        assert_eq!(jwk.algorithm(), Some(JwtAlgorithm::ES256));
    }

    #[test]
    fn test_jwks_find_by_kid() {
        let jwks = JwkSet {
            keys: vec![
                Jwk {
                    kty: "RSA".to_string(),
                    kid: Some("key1".to_string()),
                    alg: Some("RS256".to_string()),
                    use_: None,
                    n: Some("n".to_string()),
                    e: Some("e".to_string()),
                    crv: None,
                    x: None,
                    y: None,
                    k: None,
                },
                Jwk {
                    kty: "EC".to_string(),
                    kid: Some("key2".to_string()),
                    alg: Some("ES256".to_string()),
                    use_: None,
                    n: None,
                    e: None,
                    crv: Some("P-256".to_string()),
                    x: Some("x".to_string()),
                    y: Some("y".to_string()),
                    k: None,
                },
            ],
        };

        assert!(jwks.find_by_kid("key1").is_some());
        assert!(jwks.find_by_kid("key2").is_some());
        assert!(jwks.find_by_kid("key3").is_none());
    }

    // ==========================================================================
    // Security Tests: Algorithm Confusion Attack Prevention
    // ==========================================================================

    #[test]
    fn test_jwk_is_symmetric() {
        let jwk = Jwk {
            kty: "oct".to_string(),
            kid: Some("hmac-key".to_string()),
            alg: Some("HS256".to_string()),
            use_: Some("sig".to_string()),
            n: None,
            e: None,
            crv: None,
            x: None,
            y: None,
            k: Some("c2VjcmV0".to_string()), // base64url encoded "secret"
        };

        assert!(jwk.is_symmetric());
        assert!(!jwk.is_rsa());
        assert!(!jwk.is_ec());
    }

    #[test]
    fn test_rsa_key_compatible_with_rs_algorithms() {
        let rsa_jwk = Jwk {
            kty: "RSA".to_string(),
            kid: None,
            alg: None,
            use_: None,
            n: Some("modulus".to_string()),
            e: Some("AQAB".to_string()),
            crv: None,
            x: None,
            y: None,
            k: None,
        };

        // RSA key should be compatible with RS* algorithms
        assert!(rsa_jwk.is_compatible_with_algorithm(JwtAlgorithm::RS256));
        assert!(rsa_jwk.is_compatible_with_algorithm(JwtAlgorithm::RS384));
        assert!(rsa_jwk.is_compatible_with_algorithm(JwtAlgorithm::RS512));

        // RSA key should NOT be compatible with other algorithms
        assert!(!rsa_jwk.is_compatible_with_algorithm(JwtAlgorithm::ES256));
        assert!(!rsa_jwk.is_compatible_with_algorithm(JwtAlgorithm::ES384));
        assert!(!rsa_jwk.is_compatible_with_algorithm(JwtAlgorithm::HS256));
        assert!(!rsa_jwk.is_compatible_with_algorithm(JwtAlgorithm::HS384));
        assert!(!rsa_jwk.is_compatible_with_algorithm(JwtAlgorithm::HS512));
    }

    #[test]
    fn test_ec_key_compatible_with_es_algorithms() {
        let ec_p256_jwk = Jwk {
            kty: "EC".to_string(),
            kid: None,
            alg: None,
            use_: None,
            n: None,
            e: None,
            crv: Some("P-256".to_string()),
            x: Some("x-coord".to_string()),
            y: Some("y-coord".to_string()),
            k: None,
        };

        // P-256 key should only be compatible with ES256
        assert!(ec_p256_jwk.is_compatible_with_algorithm(JwtAlgorithm::ES256));
        assert!(!ec_p256_jwk.is_compatible_with_algorithm(JwtAlgorithm::ES384));

        // EC key should NOT be compatible with RS* or HS* algorithms
        assert!(!ec_p256_jwk.is_compatible_with_algorithm(JwtAlgorithm::RS256));
        assert!(!ec_p256_jwk.is_compatible_with_algorithm(JwtAlgorithm::HS256));

        // P-384 key should only be compatible with ES384
        let ec_p384_jwk = Jwk {
            kty: "EC".to_string(),
            kid: None,
            alg: None,
            use_: None,
            n: None,
            e: None,
            crv: Some("P-384".to_string()),
            x: Some("x-coord".to_string()),
            y: Some("y-coord".to_string()),
            k: None,
        };

        assert!(!ec_p384_jwk.is_compatible_with_algorithm(JwtAlgorithm::ES256));
        assert!(ec_p384_jwk.is_compatible_with_algorithm(JwtAlgorithm::ES384));
    }

    #[test]
    fn test_symmetric_key_compatible_with_hs_algorithms() {
        let hmac_jwk = Jwk {
            kty: "oct".to_string(),
            kid: None,
            alg: None,
            use_: None,
            n: None,
            e: None,
            crv: None,
            x: None,
            y: None,
            k: Some("c2VjcmV0".to_string()),
        };

        // Symmetric key should be compatible with HS* algorithms
        assert!(hmac_jwk.is_compatible_with_algorithm(JwtAlgorithm::HS256));
        assert!(hmac_jwk.is_compatible_with_algorithm(JwtAlgorithm::HS384));
        assert!(hmac_jwk.is_compatible_with_algorithm(JwtAlgorithm::HS512));

        // Symmetric key should NOT be compatible with asymmetric algorithms
        assert!(!hmac_jwk.is_compatible_with_algorithm(JwtAlgorithm::RS256));
        assert!(!hmac_jwk.is_compatible_with_algorithm(JwtAlgorithm::ES256));
    }

    #[test]
    fn test_algorithm_confusion_attack_prevention() {
        // This test verifies that the classic RS256 -> HS256 attack is prevented.
        // In this attack, the attacker changes the algorithm to HS256 and uses
        // the RSA public key as the HMAC secret.
        let rsa_public_key = Jwk {
            kty: "RSA".to_string(),
            kid: None,
            alg: Some("RS256".to_string()), // Key advertises RS256
            use_: Some("sig".to_string()),
            n: Some("modulus".to_string()),
            e: Some("AQAB".to_string()),
            crv: None,
            x: None,
            y: None,
            k: None,
        };

        // Attacker tries to use RSA public key with HS256 algorithm
        let result = rsa_public_key.validate_algorithm_compatibility(JwtAlgorithm::HS256);
        assert!(result.is_err(), "RSA key should not be usable with HS256");

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("not compatible") && err_msg.contains("algorithm confusion"),
            "Error should mention algorithm confusion attack: {}",
            err_msg
        );
    }

    // ==========================================================================
    // Security Tests: JWKS URL Validation
    // ==========================================================================

    #[test]
    fn test_jwks_url_https_required() {
        // HTTPS URL should be allowed
        let https_cache = JwksCache::new("https://auth.example.com/.well-known/jwks.json");
        assert!(https_cache.validate_url().is_ok());
    }

    #[test]
    fn test_jwks_url_http_rejected() {
        // HTTP URL should be rejected
        let http_cache = JwksCache::new("http://auth.example.com/.well-known/jwks.json");
        let result = http_cache.validate_url();
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("HTTPS"),
            "Error should mention HTTPS requirement: {}",
            err
        );
    }

    #[test]
    fn test_jwks_url_localhost_allowed() {
        // Localhost URLs should be allowed for development
        let localhost_cache = JwksCache::new("http://localhost:8080/.well-known/jwks.json");
        assert!(localhost_cache.validate_url().is_ok());

        let localhost_127 = JwksCache::new("http://127.0.0.1:8080/.well-known/jwks.json");
        assert!(localhost_127.validate_url().is_ok());

        let localhost_ipv6 = JwksCache::new("http://[::1]:8080/.well-known/jwks.json");
        assert!(localhost_ipv6.validate_url().is_ok());
    }

    #[test]
    fn test_jwks_url_insecure_mode() {
        // With allow_insecure_http(), HTTP should be allowed
        let cache =
            JwksCache::new("http://test-server/.well-known/jwks.json").allow_insecure_http();
        assert!(cache.validate_url().is_ok());
    }

    // ==========================================================================
    // Security Tests: Symmetric Key Rejection in Public JWKS
    // ==========================================================================

    #[test]
    fn test_jwk_validate_for_public_jwks_rejects_symmetric() {
        let hmac_key = Jwk {
            kty: "oct".to_string(),
            kid: Some("hmac-key-1".to_string()),
            alg: Some("HS256".to_string()),
            use_: Some("sig".to_string()),
            n: None,
            e: None,
            crv: None,
            x: None,
            y: None,
            k: Some("c2VjcmV0".to_string()),
        };

        let result = hmac_key.validate_for_public_jwks();
        assert!(result.is_err(), "Symmetric key should be rejected");

        let err = result.unwrap_err();
        let err_msg = err.to_string();
        assert!(
            err_msg.contains("Symmetric keys") && err_msg.contains("must not appear"),
            "Error should explain symmetric key prohibition: {}",
            err_msg
        );
    }

    #[test]
    fn test_jwk_validate_for_public_jwks_accepts_asymmetric() {
        // RSA key should be accepted
        let rsa_key = Jwk {
            kty: "RSA".to_string(),
            kid: Some("rsa-key-1".to_string()),
            alg: Some("RS256".to_string()),
            use_: Some("sig".to_string()),
            n: Some("modulus".to_string()),
            e: Some("AQAB".to_string()),
            crv: None,
            x: None,
            y: None,
            k: None,
        };
        assert!(rsa_key.validate_for_public_jwks().is_ok());

        // EC key should be accepted
        let ec_key = Jwk {
            kty: "EC".to_string(),
            kid: Some("ec-key-1".to_string()),
            alg: Some("ES256".to_string()),
            use_: Some("sig".to_string()),
            n: None,
            e: None,
            crv: Some("P-256".to_string()),
            x: Some("x-coord".to_string()),
            y: Some("y-coord".to_string()),
            k: None,
        };
        assert!(ec_key.validate_for_public_jwks().is_ok());
    }

    #[test]
    fn test_jwks_filter_for_public_jwks() {
        let jwks = JwkSet {
            keys: vec![
                // Valid RSA key
                Jwk {
                    kty: "RSA".to_string(),
                    kid: Some("rsa-1".to_string()),
                    alg: Some("RS256".to_string()),
                    use_: Some("sig".to_string()),
                    n: Some("n".to_string()),
                    e: Some("e".to_string()),
                    crv: None,
                    x: None,
                    y: None,
                    k: None,
                },
                // Invalid HMAC key (should be filtered out)
                Jwk {
                    kty: "oct".to_string(),
                    kid: Some("hmac-1".to_string()),
                    alg: Some("HS256".to_string()),
                    use_: Some("sig".to_string()),
                    n: None,
                    e: None,
                    crv: None,
                    x: None,
                    y: None,
                    k: Some("secret".to_string()),
                },
                // Valid EC key
                Jwk {
                    kty: "EC".to_string(),
                    kid: Some("ec-1".to_string()),
                    alg: Some("ES256".to_string()),
                    use_: Some("sig".to_string()),
                    n: None,
                    e: None,
                    crv: Some("P-256".to_string()),
                    x: Some("x".to_string()),
                    y: Some("y".to_string()),
                    k: None,
                },
            ],
        };

        let filtered = jwks.filter_for_public_jwks();

        // Should only have 2 keys (RSA and EC, HMAC filtered out)
        assert_eq!(filtered.keys.len(), 2);
        assert!(filtered.find_by_kid("rsa-1").is_some());
        assert!(filtered.find_by_kid("hmac-1").is_none()); // Filtered out
        assert!(filtered.find_by_kid("ec-1").is_some());
    }
}
