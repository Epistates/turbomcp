//! JWKS (JSON Web Key Set) fetching and caching
//!
//! Provides production-ready JWKS support for asymmetric JWT validation:
//! - Fetches JWK Sets from provider URIs (Google, GitHub, Auth0, etc.)
//! - Caches keys with configurable TTL
//! - Thread-safe concurrent access
//! - Automatic key rotation support
//!
//! Supports RSA (RS256/384/512) and ECDSA (ES256/384) algorithms.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::DecodingKey;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, error, info, warn};

/// JWKS fetch error
#[derive(Debug, thiserror::Error)]
pub enum JwksError {
    /// HTTP request to JWKS endpoint failed
    #[error("HTTP request failed: {0}")]
    HttpError(#[from] reqwest::Error),

    /// JWKS response was invalid or malformed
    #[error("Invalid JWKS response: {0}")]
    InvalidResponse(String),

    /// Requested key ID not found in JWKS
    #[error("Key not found: {kid}")]
    KeyNotFound {
        /// The key ID that was not found
        kid: String,
    },

    /// Key type is not supported (only RSA and EC are supported)
    #[error("Unsupported key type: {kty}")]
    UnsupportedKeyType {
        /// The unsupported key type
        kty: String,
    },

    /// Key format is invalid or incomplete
    #[error("Invalid key format: {0}")]
    InvalidKeyFormat(String),

    /// Base64 decoding of key parameters failed
    #[error("Base64 decode error: {0}")]
    Base64Error(#[from] base64::DecodeError),
}

/// JSON Web Key (JWK) structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jwk {
    /// Key Type (RSA, EC, oct, OKP)
    pub kty: String,

    /// Key ID (optional, used for key selection)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kid: Option<String>,

    /// Public Key Use (sig, enc)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "use")]
    pub key_use: Option<String>,

    /// Algorithm (RS256, ES256, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alg: Option<String>,

    // RSA parameters (kty = "RSA")
    /// RSA modulus (base64url encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,

    /// RSA exponent (base64url encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub e: Option<String>,

    // ECDSA parameters (kty = "EC")
    /// Elliptic Curve name (P-256, P-384, P-521)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crv: Option<String>,

    /// ECDSA X coordinate (base64url encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub x: Option<String>,

    /// ECDSA Y coordinate (base64url encoded)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub y: Option<String>,
}

impl Jwk {
    /// Convert JWK to jsonwebtoken DecodingKey
    pub fn to_decoding_key(&self) -> Result<DecodingKey, JwksError> {
        match self.kty.as_str() {
            "RSA" => self.to_rsa_key(),
            "EC" => self.to_ec_key(),
            _ => Err(JwksError::UnsupportedKeyType {
                kty: self.kty.clone(),
            }),
        }
    }

    /// Convert RSA JWK to DecodingKey
    fn to_rsa_key(&self) -> Result<DecodingKey, JwksError> {
        let n = self
            .n
            .as_ref()
            .ok_or_else(|| JwksError::InvalidKeyFormat("RSA key missing 'n' parameter".into()))?;
        let e = self
            .e
            .as_ref()
            .ok_or_else(|| JwksError::InvalidKeyFormat("RSA key missing 'e' parameter".into()))?;

        // Decode base64url (RFC 4648 §5 - URL-safe base64 without padding)
        let _n_bytes = URL_SAFE_NO_PAD.decode(n)?;
        let _e_bytes = URL_SAFE_NO_PAD.decode(e)?;

        // Convert to DecodingKey using RSA components
        DecodingKey::from_rsa_components(n, e)
            .map_err(|e| JwksError::InvalidKeyFormat(format!("Failed to create RSA key: {}", e)))
    }

    /// Convert ECDSA JWK to DecodingKey
    fn to_ec_key(&self) -> Result<DecodingKey, JwksError> {
        let x = self
            .x
            .as_ref()
            .ok_or_else(|| JwksError::InvalidKeyFormat("EC key missing 'x' parameter".into()))?;
        let y = self
            .y
            .as_ref()
            .ok_or_else(|| JwksError::InvalidKeyFormat("EC key missing 'y' parameter".into()))?;

        // Use jsonwebtoken's from_ec_components which accepts base64url-encoded strings directly
        // The library handles all DER/ASN.1 encoding internally.
        DecodingKey::from_ec_components(x, y)
            .map_err(|e| JwksError::InvalidKeyFormat(format!("Failed to create EC key: {}", e)))
    }
}

/// JSON Web Key Set (JWKS) response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JwkSet {
    /// Array of JWKs
    pub keys: Vec<Jwk>,
}

/// Cached JWKS data
#[derive(Clone)]
struct CachedJwks {
    /// Parsed keys indexed by kid
    keys: HashMap<String, DecodingKey>,
    /// When the cache entry was created
    fetched_at: Instant,
    /// TTL for this cache entry
    ttl: Duration,
}

impl CachedJwks {
    /// Check if cache entry is expired
    fn is_expired(&self) -> bool {
        self.fetched_at.elapsed() > self.ttl
    }
}

/// JWKS cache with automatic fetching and TTL management
pub struct JwksCache {
    /// JWKS URI to fetch from
    uri: String,

    /// HTTP client for fetching
    client: reqwest::Client,

    /// Cached keys
    cache: Arc<RwLock<Option<CachedJwks>>>,

    /// Cache TTL (default: 1 hour)
    ttl: Duration,
}

impl std::fmt::Debug for JwksCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwksCache")
            .field("uri", &self.uri)
            .field("ttl", &self.ttl)
            .field("cache", &"<cached keys>")
            .finish()
    }
}

impl JwksCache {
    /// Create new JWKS cache
    pub fn new(uri: String) -> Self {
        Self::with_ttl(uri, Duration::from_secs(3600))
    }

    /// Create new JWKS cache with custom TTL
    pub fn with_ttl(uri: String, ttl: Duration) -> Self {
        Self {
            uri,
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            cache: Arc::new(RwLock::new(None)),
            ttl,
        }
    }

    /// Get decoding key by kid, fetching if necessary
    pub async fn get_key(&self, kid: &str) -> Result<DecodingKey, JwksError> {
        // Fast path: check cache with read lock
        {
            let cache = self.cache.read();
            if let Some(cached) = cache.as_ref() {
                if !cached.is_expired() {
                    if let Some(key) = cached.keys.get(kid) {
                        debug!(kid = %kid, "JWKS cache hit");
                        return Ok(key.clone());
                    }
                } else {
                    debug!(kid = %kid, "JWKS cache expired");
                }
            }
        }

        // Cache miss or expired - fetch new JWKS
        info!(uri = %self.uri, "Fetching JWKS");
        let jwk_set = self.fetch_jwks().await?;

        // Parse all keys and build cache
        let mut keys = HashMap::new();
        for jwk in jwk_set.keys {
            let key_id = jwk.kid.clone().unwrap_or_else(|| {
                // Generate synthetic kid if not present
                format!("synthetic_{}", fastrand::u64(..))
            });

            match jwk.to_decoding_key() {
                Ok(key) => {
                    debug!(kid = %key_id, kty = %jwk.kty, alg = ?jwk.alg, "Parsed JWK");
                    keys.insert(key_id, key);
                }
                Err(e) => {
                    warn!(kid = %key_id, error = %e, "Failed to parse JWK");
                }
            }
        }

        if keys.is_empty() {
            return Err(JwksError::InvalidResponse(
                "No valid keys found in JWKS".to_string(),
            ));
        }

        // Update cache with write lock
        {
            let mut cache = self.cache.write();
            *cache = Some(CachedJwks {
                keys: keys.clone(),
                fetched_at: Instant::now(),
                ttl: self.ttl,
            });
        }

        info!(num_keys = keys.len(), "JWKS cache updated");

        // Return requested key
        keys.get(kid)
            .cloned()
            .ok_or_else(|| JwksError::KeyNotFound {
                kid: kid.to_string(),
            })
    }

    /// Fetch JWKS from provider URI
    async fn fetch_jwks(&self) -> Result<JwkSet, JwksError> {
        let response = self.client.get(&self.uri).send().await?;

        if !response.status().is_success() {
            return Err(JwksError::InvalidResponse(format!(
                "HTTP {} from JWKS endpoint",
                response.status()
            )));
        }

        let jwk_set: JwkSet = response
            .json()
            .await
            .map_err(|e| JwksError::InvalidResponse(format!("Failed to parse JWKS JSON: {}", e)))?;

        Ok(jwk_set)
    }

    /// Manually refresh the cache
    pub async fn refresh(&self) -> Result<(), JwksError> {
        info!(uri = %self.uri, "Manually refreshing JWKS cache");
        let jwk_set = self.fetch_jwks().await?;

        let mut keys = HashMap::new();
        for jwk in jwk_set.keys {
            let key_id = jwk
                .kid
                .clone()
                .unwrap_or_else(|| format!("synthetic_{}", fastrand::u64(..)));

            if let Ok(key) = jwk.to_decoding_key() {
                keys.insert(key_id, key);
            }
        }

        let mut cache = self.cache.write();
        *cache = Some(CachedJwks {
            keys,
            fetched_at: Instant::now(),
            ttl: self.ttl,
        });

        Ok(())
    }

    /// Clear the cache
    pub fn clear(&self) {
        let mut cache = self.cache.write();
        *cache = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Example RSA JWK (from RFC 7517 Example A.1, valid key)
    fn example_rsa_jwk() -> Jwk {
        Jwk {
            kty: "RSA".to_string(),
            kid: Some("2011-04-29".to_string()),
            key_use: Some("sig".to_string()),
            alg: Some("RS256".to_string()),
            n: Some("0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw".to_string()),
            e: Some("AQAB".to_string()),
            crv: None,
            x: None,
            y: None,
        }
    }

    /// Example ECDSA JWK (from RFC 7517 Example A.2, valid key)
    fn example_ec_jwk() -> Jwk {
        Jwk {
            kty: "EC".to_string(),
            kid: Some("1".to_string()),
            key_use: Some("sig".to_string()),
            alg: Some("ES256".to_string()),
            n: None,
            e: None,
            crv: Some("P-256".to_string()),
            x: Some("WKn-ZIGevcwGIyyrzFoZNBdaq9_TsqzGl96oc0CWuis".to_string()),
            y: Some("y77t-RvAHRKTsSGdIYUfweuOvwrvDD-Q3Hv5J0fSKbE".to_string()),
        }
    }

    #[test]
    fn test_jwk_rsa_parsing() {
        let jwk = example_rsa_jwk();
        let result = jwk.to_decoding_key();
        assert!(
            result.is_ok(),
            "Failed to parse RSA JWK: {:?}",
            result.err()
        );
    }

    #[test]
    fn test_jwk_ec_parsing() {
        let jwk = example_ec_jwk();
        let result = jwk.to_decoding_key();
        assert!(result.is_ok(), "Failed to parse EC JWK: {:?}", result.err());
    }

    #[test]
    fn test_jwk_unsupported_type() {
        let jwk = Jwk {
            kty: "oct".to_string(), // Symmetric key (not supported for JWT validation)
            kid: Some("symmetric-key".to_string()),
            key_use: None,
            alg: None,
            n: None,
            e: None,
            crv: None,
            x: None,
            y: None,
        };

        let result = jwk.to_decoding_key();
        assert!(matches!(result, Err(JwksError::UnsupportedKeyType { .. })));
    }

    #[test]
    fn test_jwk_missing_rsa_params() {
        let jwk = Jwk {
            kty: "RSA".to_string(),
            kid: Some("incomplete-rsa".to_string()),
            key_use: None,
            alg: None,
            n: Some("modulus".to_string()),
            e: None, // Missing exponent
            crv: None,
            x: None,
            y: None,
        };

        let result = jwk.to_decoding_key();
        assert!(matches!(result, Err(JwksError::InvalidKeyFormat(_))));
    }

    #[test]
    fn test_jwk_missing_ec_params() {
        let jwk = Jwk {
            kty: "EC".to_string(),
            kid: Some("incomplete-ec".to_string()),
            key_use: None,
            alg: None,
            n: None,
            e: None,
            crv: Some("P-256".to_string()),
            x: Some("x-coord".to_string()),
            y: None, // Missing y coordinate
        };

        let result = jwk.to_decoding_key();
        assert!(matches!(result, Err(JwksError::InvalidKeyFormat(_))));
    }

    #[test]
    fn test_jwk_set_deserialization() {
        let json = json!({
            "keys": [
                {
                    "kty": "RSA",
                    "kid": "key-1",
                    "use": "sig",
                    "alg": "RS256",
                    "n": "0vx7agoebGcQSuuPiLJXZptN9nndrQmbXEps2aiAFbWhM78LhWx4cbbfAAtVT86zwu1RK7aPFFxuhDR1L6tSoc_BJECPebWKRXjBZCiFV4n3oknjhMstn64tZ_2W-5JsGY4Hc5n9yBXArwl93lqt7_RN5w6Cf0h4QyQ5v-65YGjQR0_FDW2QvzqY368QQMicAtaSqzs8KJZgnYb9c7d0zgdAZHzu6qMQvRL5hajrn1n91CbOpbISD08qNLyrdkt-bFTWhAI4vMQFh6WeZu0fM4lFd2NcRwr3XPksINHaQ-G_xBniIqbw0Ls1jF44-csFCur-kEgU8awapJzKnqDKgw",
                    "e": "AQAB"
                },
                {
                    "kty": "EC",
                    "kid": "key-2",
                    "use": "sig",
                    "alg": "ES256",
                    "crv": "P-256",
                    "x": "WKn-ZIGevcwGIyyrzFoZNBdaq9_TsqzGl96oc0CWuis",
                    "y": "y77t-RvAHRKTsSGdIYUfweuOvwrvDD-Q3Hv5J0fSKbE"
                }
            ]
        });

        let jwk_set: JwkSet = serde_json::from_value(json).unwrap();
        assert_eq!(jwk_set.keys.len(), 2);
        assert_eq!(jwk_set.keys[0].kty, "RSA");
        assert_eq!(jwk_set.keys[1].kty, "EC");
    }

    #[test]
    fn test_cached_jwks_expiration() {
        let mut keys = HashMap::new();
        keys.insert("test-key".to_string(), DecodingKey::from_secret(b"test"));

        let cached = CachedJwks {
            keys,
            fetched_at: Instant::now() - Duration::from_secs(7200), // 2 hours ago
            ttl: Duration::from_secs(3600),                         // 1 hour TTL
        };

        assert!(cached.is_expired());
    }

    #[test]
    fn test_cached_jwks_not_expired() {
        let mut keys = HashMap::new();
        keys.insert("test-key".to_string(), DecodingKey::from_secret(b"test"));

        let cached = CachedJwks {
            keys,
            fetched_at: Instant::now(),
            ttl: Duration::from_secs(3600), // 1 hour TTL
        };

        assert!(!cached.is_expired());
    }

    #[tokio::test]
    async fn test_jwks_cache_creation() {
        let cache = JwksCache::new("https://example.com/.well-known/jwks.json".to_string());
        // Just verify construction succeeds
        assert_eq!(cache.uri, "https://example.com/.well-known/jwks.json");
    }

    #[tokio::test]
    async fn test_jwks_cache_with_custom_ttl() {
        let ttl = Duration::from_secs(1800); // 30 minutes
        let cache =
            JwksCache::with_ttl("https://example.com/.well-known/jwks.json".to_string(), ttl);
        assert_eq!(cache.ttl, ttl);
    }

    #[test]
    fn test_jwks_error_display() {
        let err = JwksError::KeyNotFound {
            kid: "test-key".to_string(),
        };
        assert_eq!(err.to_string(), "Key not found: test-key");

        let err = JwksError::UnsupportedKeyType {
            kty: "oct".to_string(),
        };
        assert_eq!(err.to_string(), "Unsupported key type: oct");
    }
}
