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

    /// Check if this key can be used for signing/verification
    pub fn is_signing_key(&self) -> bool {
        self.use_.as_ref().is_none_or(|u| u == "sig")
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
}

/// Cache entry for JWKS with expiration tracking
#[derive(Debug, Clone)]
struct CacheEntry {
    jwks: JwkSet,
    fetched_at: f64, // js_sys::Date timestamp
}

/// JWKS cache for efficient key lookups.
///
/// Caches fetched JWKS and automatically refreshes when expired.
/// Includes retry logic with exponential backoff for transient failures.
#[derive(Clone)]
pub struct JwksCache {
    /// JWKS endpoint URL
    url: String,

    /// Cache TTL in milliseconds (default: 1 hour)
    ttl_ms: f64,

    /// Cached JWKS entries
    cache: Rc<RefCell<Option<CacheEntry>>>,

    /// Maximum retry attempts for transient failures
    max_retries: u32,

    /// Base delay in milliseconds for exponential backoff
    retry_base_delay_ms: f64,
}

impl JwksCache {
    /// Create a new JWKS cache for the given URL
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ttl_ms: 3600.0 * 1000.0, // 1 hour
            cache: Rc::new(RefCell::new(None)),
            max_retries: 3,
            retry_base_delay_ms: 100.0,
        }
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
    pub async fn get_jwks(&self) -> Result<JwkSet, AuthError> {
        // Check cache first
        if let Some(ref entry) = *self.cache.borrow() {
            let now = js_sys::Date::now();
            if now - entry.fetched_at < self.ttl_ms {
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
            match self.fetch_jwks_once().await {
                Ok(jwks) => return Ok(jwks),
                Err(e) => {
                    // Only retry on transient errors (network failures, 5xx)
                    if !Self::is_retryable_error(&e) {
                        return Err(e);
                    }

                    last_error = Some(e);

                    // Don't sleep after last attempt
                    if attempt < self.max_retries {
                        let delay_ms = self.retry_base_delay_ms * 2_f64.powi(attempt as i32);
                        Self::sleep_ms(delay_ms).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            AuthError::KeyFetchError("JWKS fetch failed after retries".to_string())
        }))
    }

    /// Single JWKS fetch attempt (no retry)
    async fn fetch_jwks_once(&self) -> Result<JwkSet, AuthError> {
        let window = web_sys::window()
            .ok_or_else(|| AuthError::Internal("No window object available".to_string()))?;

        // Create fetch request
        let request = web_sys::Request::new_with_str(&self.url)
            .map_err(|e| AuthError::KeyFetchError(format!("Failed to create request: {:?}", e)))?;

        // Execute fetch
        let promise = window.fetch_with_request(&request);
        let response = JsFuture::from(promise)
            .await
            .map_err(|e| AuthError::KeyFetchError(format!("Fetch failed: {:?}", e)))?;

        let response: web_sys::Response = response.dyn_into().map_err(|_| {
            AuthError::KeyFetchError("Response is not a Response object".to_string())
        })?;

        if !response.ok() {
            let status = response.status();
            return Err(AuthError::KeyFetchError(format!(
                "HTTP error: {} (retryable: {})",
                status,
                status >= 500
            )));
        }

        // Parse JSON body
        let json_promise = response
            .json()
            .map_err(|e| AuthError::KeyFetchError(format!("Failed to get JSON: {:?}", e)))?;

        let json_value = JsFuture::from(json_promise)
            .await
            .map_err(|e| AuthError::KeyFetchError(format!("JSON parse failed: {:?}", e)))?;

        // Convert to our JwkSet type
        let jwks: JwkSet = serde_wasm_bindgen::from_value(json_value)
            .map_err(|e| AuthError::KeyFetchError(format!("Invalid JWKS format: {:?}", e)))?;

        Ok(jwks)
    }

    /// Check if an error is retryable (transient)
    fn is_retryable_error(error: &AuthError) -> bool {
        match error {
            AuthError::KeyFetchError(msg) => {
                // Retry on network errors and 5xx server errors
                msg.contains("Fetch failed") || msg.contains("HTTP error: 5")
            }
            _ => false,
        }
    }

    /// Sleep for the specified milliseconds using setTimeout
    async fn sleep_ms(ms: f64) {
        let promise = js_sys::Promise::new(&mut |resolve, _| {
            let window = web_sys::window().expect("no window");
            let _ =
                window.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms as i32);
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
}
