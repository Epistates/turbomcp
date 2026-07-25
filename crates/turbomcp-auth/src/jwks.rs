//! JWKS sources — where the validator gets signing keys.
//!
//! [`JwkSource`] is the seam: [`StaticJwks`] holds a fixed key set (tests, or
//! servers that pin keys), and — behind the `http-jwks` feature — [`HttpJwks`]
//! fetches and caches a JWKS document from the authorization server's
//! `jwks_uri`.

use futures::future::BoxFuture;
use jsonwebtoken::DecodingKey;
use jsonwebtoken::jwk::JwkSet;

use crate::error::AuthError;

/// Resolves a verification key for a token, by its header `kid`.
pub trait JwkSource: Send + Sync {
    /// The decoding key for `kid` (or the sole key when the source has exactly
    /// one and `kid` is absent). Async so HTTP-backed sources can fetch.
    fn decoding_key<'a>(
        &'a self,
        kid: Option<&'a str>,
    ) -> BoxFuture<'a, Result<DecodingKey, AuthError>>;
}

/// A fixed set of JWKs. Construct from a JWKS JSON document (the kind an
/// authorization server serves at its `jwks_uri`).
pub struct StaticJwks {
    set: JwkSet,
}

impl StaticJwks {
    /// Parse a JWKS JSON document (`{ "keys": [ … ] }`).
    ///
    /// # Errors
    /// Returns [`AuthError::KeyUnavailable`] if the document doesn't parse.
    pub fn from_json(json: &str) -> Result<Self, AuthError> {
        let set: JwkSet = serde_json::from_str(json)
            .map_err(|e| AuthError::KeyUnavailable(format!("malformed JWKS: {e}")))?;
        Ok(Self { set })
    }

    /// Wrap an already-parsed [`JwkSet`].
    #[must_use]
    pub fn new(set: JwkSet) -> Self {
        Self { set }
    }

    /// Resolve `kid` (or the sole key) against this set.
    fn lookup(&self, kid: Option<&str>) -> Result<DecodingKey, AuthError> {
        let jwk = match kid {
            Some(kid) => self.set.find(kid),
            // No `kid`: only unambiguous when the set has exactly one key.
            None => match self.set.keys.as_slice() {
                [only] => Some(only),
                _ => None,
            },
        }
        .ok_or_else(|| AuthError::KeyUnavailable(format!("no JWK for kid {kid:?}")))?;
        DecodingKey::from_jwk(jwk).map_err(|e| AuthError::KeyUnavailable(format!("bad JWK: {e}")))
    }
}

impl JwkSource for StaticJwks {
    fn decoding_key<'a>(
        &'a self,
        kid: Option<&'a str>,
    ) -> BoxFuture<'a, Result<DecodingKey, AuthError>> {
        Box::pin(async move { self.lookup(kid) })
    }
}

#[cfg(feature = "http-jwks")]
#[cfg_attr(docsrs, doc(cfg(feature = "http-jwks")))]
pub use http::HttpJwks;

#[cfg(feature = "http-jwks")]
#[cfg_attr(docsrs, doc(cfg(feature = "http-jwks")))]
mod http {
    use std::sync::RwLock;
    use std::time::{Duration, Instant};

    use futures::future::BoxFuture;
    use jsonwebtoken::DecodingKey;
    use jsonwebtoken::jwk::JwkSet;

    use super::JwkSource;
    use crate::error::AuthError;

    /// Fetches a JWKS document from an authorization server's `jwks_uri` and
    /// caches it for `ttl`. A `kid` miss forces one refresh (key rotation) per
    /// the cooldown before giving up.
    pub struct HttpJwks {
        jwks_uri: String,
        client: reqwest::Client,
        ttl: Duration,
        cache: RwLock<Option<Cached>>,
    }

    struct Cached {
        set: JwkSet,
        fetched: Instant,
    }

    impl HttpJwks {
        /// A source backed by `jwks_uri`, caching for `ttl` (e.g. 1 hour).
        #[must_use]
        pub fn new(jwks_uri: impl Into<String>, ttl: Duration) -> Self {
            Self {
                jwks_uri: jwks_uri.into(),
                client: reqwest::Client::new(),
                ttl,
                cache: RwLock::new(None),
            }
        }

        fn cached_fresh(&self) -> Option<JwkSet> {
            let guard = self.cache.read().expect("jwks cache poisoned");
            guard
                .as_ref()
                .filter(|c| c.fetched.elapsed() < self.ttl)
                .map(|c| c.set.clone())
        }

        async fn fetch(&self) -> Result<JwkSet, AuthError> {
            let set: JwkSet = self
                .client
                .get(&self.jwks_uri)
                .send()
                .await
                .map_err(|e| AuthError::KeyUnavailable(format!("JWKS fetch failed: {e}")))?
                .json()
                .await
                .map_err(|e| AuthError::KeyUnavailable(format!("JWKS decode failed: {e}")))?;
            *self.cache.write().expect("jwks cache poisoned") = Some(Cached {
                set: set.clone(),
                fetched: Instant::now(),
            });
            Ok(set)
        }

        async fn key_set(&self, force: bool) -> Result<JwkSet, AuthError> {
            if !force && let Some(set) = self.cached_fresh() {
                return Ok(set);
            }
            self.fetch().await
        }
    }

    impl JwkSource for HttpJwks {
        fn decoding_key<'a>(
            &'a self,
            kid: Option<&'a str>,
        ) -> BoxFuture<'a, Result<DecodingKey, AuthError>> {
            Box::pin(async move {
                // Try the cache, then force one refresh on a miss (rotation).
                for force in [false, true] {
                    let set = self.key_set(force).await?;
                    let found = match kid {
                        Some(kid) => set.find(kid).cloned(),
                        None => match set.keys.as_slice() {
                            [only] => Some(only.clone()),
                            _ => None,
                        },
                    };
                    if let Some(jwk) = found {
                        return DecodingKey::from_jwk(&jwk)
                            .map_err(|e| AuthError::KeyUnavailable(format!("bad JWK: {e}")));
                    }
                }
                Err(AuthError::KeyUnavailable(format!("no JWK for kid {kid:?}")))
            })
        }
    }
}
