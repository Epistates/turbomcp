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

    /// The decoding key for `kid` to verify an `alg` signature: a key whose
    /// JWK names another `alg` is not it (RFC 7517 §4.4). The bundled sources
    /// check; the default, for a source of your own, is
    /// [`decoding_key`](Self::decoding_key).
    fn decoding_key_for<'a>(
        &'a self,
        kid: Option<&'a str>,
        alg: jsonwebtoken::Algorithm,
    ) -> BoxFuture<'a, Result<DecodingKey, AuthError>> {
        let _ = alg;
        self.decoding_key(kid)
    }
}

/// The JWK in `set` to verify a token with: by `kid`, or the sole candidate
/// when the token names none. A key published for encryption (`use: enc`)
/// verifies no signature (RFC 7517 §4.2), and one whose `alg` differs from
/// the token's is for another algorithm (§4.4); neither is a candidate.
fn select_key<'s>(
    set: &'s JwkSet,
    kid: Option<&str>,
    alg: Option<jsonwebtoken::Algorithm>,
) -> Option<&'s jsonwebtoken::jwk::Jwk> {
    let usable = |jwk: &&jsonwebtoken::jwk::Jwk| {
        let signing = !matches!(
            jwk.common.public_key_use,
            Some(
                jsonwebtoken::jwk::PublicKeyUse::Encryption
                    | jsonwebtoken::jwk::PublicKeyUse::Other(_)
            )
        );
        let same_alg = match (jwk.common.key_algorithm, alg) {
            (Some(published), Some(alg)) => format!("{published:?}") == format!("{alg:?}"),
            _ => true,
        };
        signing && same_alg
    };
    match kid {
        Some(kid) => set.find(kid).filter(usable),
        None => match set
            .keys
            .iter()
            .filter(usable)
            .collect::<Vec<_>>()
            .as_slice()
        {
            [only] => Some(*only),
            _ => None,
        },
    }
}

/// A fixed set of JWKs. Construct from a JWKS JSON document (the kind an
/// authorization server serves at its `jwks_uri`).
pub struct StaticJwks {
    set: JwkSet,
}

impl core::fmt::Debug for StaticJwks {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StaticJwks")
            .field("keys", &self.set.keys.len())
            .finish()
    }
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
    fn lookup(
        &self,
        kid: Option<&str>,
        alg: Option<jsonwebtoken::Algorithm>,
    ) -> Result<DecodingKey, AuthError> {
        let jwk = select_key(&self.set, kid, alg)
            .ok_or_else(|| AuthError::KeyUnavailable(format!("no JWK for kid {kid:?}")))?;
        DecodingKey::from_jwk(jwk).map_err(|e| AuthError::KeyUnavailable(format!("bad JWK: {e}")))
    }
}

impl JwkSource for StaticJwks {
    fn decoding_key<'a>(
        &'a self,
        kid: Option<&'a str>,
    ) -> BoxFuture<'a, Result<DecodingKey, AuthError>> {
        Box::pin(async move { self.lookup(kid, None) })
    }

    fn decoding_key_for<'a>(
        &'a self,
        kid: Option<&'a str>,
        alg: jsonwebtoken::Algorithm,
    ) -> BoxFuture<'a, Result<DecodingKey, AuthError>> {
        Box::pin(async move { self.lookup(kid, Some(alg)) })
    }
}

#[cfg(feature = "http-jwks")]
#[cfg_attr(docsrs, doc(cfg(feature = "http-jwks")))]
pub use http::HttpJwks;

#[cfg(feature = "http-jwks")]
#[cfg_attr(docsrs, doc(cfg(feature = "http-jwks")))]
mod http {
    use parking_lot::RwLock;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use futures::future::BoxFuture;
    use jsonwebtoken::DecodingKey;
    use jsonwebtoken::jwk::JwkSet;

    use super::JwkSource;
    use crate::error::AuthError;

    /// Fetches a JWKS document from an authorization server's `jwks_uri` and
    /// caches it for `ttl`. A `kid` miss forces one refresh (key rotation),
    /// rate-limited by a cooldown, before giving up.
    pub struct HttpJwks {
        jwks_uri: String,
        client: reqwest::Client,
        ttl: Duration,
        refresh_cooldown: Duration,
        stale_if_error: Duration,
        cache: RwLock<Option<Cached>>,
        refresh: tokio::sync::Mutex<Option<(Instant, String)>>,
        policy: crate::NetworkPolicy,
    }

    impl core::fmt::Debug for HttpJwks {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("HttpJwks")
                .field("jwks_uri", &self.jwks_uri)
                .field("ttl", &self.ttl)
                .field("refresh_cooldown", &self.refresh_cooldown)
                .field(
                    "cached_keys",
                    &self.cache.read().as_ref().map(|c| c.set.keys.len()),
                )
                .finish_non_exhaustive()
        }
    }

    struct Cached {
        /// Shared, so a validation reads the set without copying it.
        set: Arc<JwkSet>,
        fetched: Instant,
    }

    impl HttpJwks {
        /// The default floor between `kid`-miss refreshes. Short enough that a
        /// rotation is picked up promptly, long enough that a flood of unknown
        /// `kid`s can't be turned into a flood of upstream fetches.
        pub const DEFAULT_REFRESH_COOLDOWN: Duration = Duration::from_secs(30);

        /// How long past its TTL a set keeps serving when the issuer can't be
        /// reached, by default.
        pub const DEFAULT_STALE_IF_ERROR: Duration = Duration::from_secs(60 * 60);

        /// A source backed by `jwks_uri`, caching for `ttl` (e.g. 1 hour).
        ///
        /// # Panics
        /// If the TLS backend can't initialize, which the HTTP client needs.
        #[must_use]
        pub fn new(jwks_uri: impl Into<String>, ttl: Duration) -> Self {
            Self {
                jwks_uri: jwks_uri.into(),
                client: crate::NetworkPolicy::default()
                    .http_client()
                    .expect("HTTP client initialization"),
                ttl,
                refresh_cooldown: Self::DEFAULT_REFRESH_COOLDOWN,
                stale_if_error: Self::DEFAULT_STALE_IF_ERROR,
                cache: RwLock::new(None),
                refresh: tokio::sync::Mutex::new(None),
                policy: crate::NetworkPolicy::default(),
            }
        }

        /// Override how often a `kid` miss may force a refresh.
        ///
        /// The trade-off is rotation latency against upstream load: a token
        /// signed with a key newer than the cache is rejected until a refresh
        /// is allowed. `Duration::ZERO` refreshes on every miss — only sane
        /// when the `jwks_uri` is local or the caller is already trusted.
        #[must_use]
        pub fn refresh_cooldown(mut self, cooldown: Duration) -> Self {
            self.refresh_cooldown = cooldown;
            self
        }

        /// Keep serving a set for up to `grace` past its TTL while the
        /// issuer can't be reached (default 1 hour), instead of failing
        /// every token the moment the TTL passes during an outage. Keys
        /// rotate on the issuer's schedule, not on ours: a set that was
        /// right an hour ago is right for every token not signed with a
        /// key minted since. `Duration::ZERO` turns this off.
        #[must_use]
        pub fn stale_if_error(mut self, grace: Duration) -> Self {
            self.stale_if_error = grace;
            self
        }

        /// The cached set if it is younger than `max_age`.
        fn cached_within(&self, max_age: Duration) -> Option<Arc<JwkSet>> {
            let guard = self.cache.read();
            guard
                .as_ref()
                .filter(|c| c.fetched.elapsed() < max_age)
                .map(|c| Arc::clone(&c.set))
        }

        /// Configure outbound network policy and a redirect-disabled HTTP client.
        /// # Errors
        /// Returns an error if HTTP initialization fails.
        pub fn with_network_policy(
            mut self,
            policy: crate::NetworkPolicy,
        ) -> Result<Self, AuthError> {
            self.client = policy
                .http_client()
                .map_err(|e| AuthError::KeyUnavailable(e.to_string()))?;
            self.policy = policy;
            Ok(self)
        }

        /// Supply TLS/proxy customization. The caller must disable redirects
        /// and preserve the configured address policy on this custom client.
        #[must_use]
        pub fn with_http_client(mut self, client: reqwest::Client) -> Self {
            self.client = client;
            self
        }

        async fn fetch(&self) -> Result<Arc<JwkSet>, AuthError> {
            let response = self
                .policy
                .send(&self.client, self.client.get(&self.jwks_uri))
                .await
                .map_err(|e| AuthError::Unavailable(format!("JWKS fetch failed: {e}")))?;
            if !response.status().is_success() {
                return Err(AuthError::Unavailable(format!(
                    "JWKS HTTP {}",
                    response.status()
                )));
            }
            let set: JwkSet = serde_json::from_slice(
                &self
                    .policy
                    .body(response)
                    .await
                    .map_err(AuthError::Unavailable)?,
            )
            .map_err(|e| AuthError::Unavailable(format!("JWKS decode failed: {e}")))?;
            let set = Arc::new(set);
            *self.cache.write() = Some(Cached {
                set: Arc::clone(&set),
                fetched: Instant::now(),
            });
            Ok(set)
        }

        async fn key_set(&self, force: bool) -> Result<Arc<JwkSet>, AuthError> {
            // A `kid` is attacker-chosen — it is read from the token header
            // before any signature is verified — so an unconditional refresh on
            // every miss lets an unauthenticated caller drive one upstream
            // fetch per request. The cooldown bounds that: within it, the
            // forced path is served the cached set and simply fails to find the
            // key, which is the same answer at no cost to the authorization
            // server.
            let max_age = if force {
                self.refresh_cooldown
            } else {
                self.ttl
            };
            if let Some(set) = self.cached_within(max_age) {
                return Ok(set);
            }
            // Expired, not forced: while one caller refreshes, the rest go on
            // with the set they have (stale-while-revalidate) rather than
            // queueing behind a fetch that may take the policy's whole
            // timeout.
            let stale = (!force)
                .then(|| self.cached_within(self.ttl.saturating_add(self.stale_if_error)))
                .flatten();
            let mut last_failure = match (&stale, self.refresh.try_lock()) {
                (_, Ok(guard)) => guard,
                (Some(set), Err(_)) => return Ok(Arc::clone(set)),
                (None, Err(_)) => self.refresh.lock().await,
            };
            // Recheck after joining the in-flight refresh. Only one caller
            // contacts the issuer, including cold-cache and expiry bursts.
            if let Some(set) = self.cached_within(max_age) {
                return Ok(set);
            }
            if let Some((at, error)) = &*last_failure
                && at.elapsed() < self.refresh_cooldown
            {
                return stale.ok_or_else(|| AuthError::Unavailable(error.clone()));
            }
            match self.fetch().await {
                Ok(set) => {
                    *last_failure = None;
                    Ok(set)
                }
                Err(error) => {
                    *last_failure = Some((Instant::now(), error.to_string()));
                    match stale {
                        Some(set) => {
                            tracing::warn!(%error, "JWKS refresh failed; serving the last good set");
                            Ok(set)
                        }
                        None => Err(error),
                    }
                }
            }
        }
    }

    impl HttpJwks {
        async fn key_for(
            &self,
            kid: Option<&str>,
            alg: Option<jsonwebtoken::Algorithm>,
        ) -> Result<DecodingKey, AuthError> {
            // Try the cache, then force one refresh on a miss (rotation).
            for force in [false, true] {
                let set = self.key_set(force).await?;
                if let Some(jwk) = super::select_key(&set, kid, alg) {
                    return DecodingKey::from_jwk(jwk)
                        .map_err(|e| AuthError::KeyUnavailable(format!("bad JWK: {e}")));
                }
            }
            Err(AuthError::KeyUnavailable(format!("no JWK for kid {kid:?}")))
        }
    }

    impl JwkSource for HttpJwks {
        fn decoding_key<'a>(
            &'a self,
            kid: Option<&'a str>,
        ) -> BoxFuture<'a, Result<DecodingKey, AuthError>> {
            Box::pin(self.key_for(kid, None))
        }

        fn decoding_key_for<'a>(
            &'a self,
            kid: Option<&'a str>,
            alg: jsonwebtoken::Algorithm,
        ) -> BoxFuture<'a, Result<DecodingKey, AuthError>> {
            Box::pin(self.key_for(kid, Some(alg)))
        }
    }
}
