//! Opaque access tokens, validated by asking the authorization server
//! (RFC 7662 OAuth 2.0 Token Introspection; feature `introspection`).
//!
//! A JWT carries its own proof; an opaque token is a handle only its issuer
//! can read. [`IntrospectionValidator`] posts it to the authorization
//! server's introspection endpoint, authenticated as this resource server,
//! and accepts it only if the answer is `active` and was issued for this
//! resource: "MCP servers MUST validate that access tokens were issued
//! specifically for them as the intended audience", so an answer naming no
//! audience, or another one, is refused. `iss`, `exp` and `nbf` are checked
//! too where present (and `iss` required when configured).
//!
//! Every call that misses the cache is a round trip, so answers are cached,
//! keyed by a SHA-256 of the token (never the token itself), for a short
//! time and never past the token's own `exp`. The trade is revocation
//! latency: a token revoked at the authorization server is honoured here for
//! up to that long ("the protected resource ... MAY cache the response"
//! with exactly this caveat, RFC 7662 §4). `cache_ttl(Duration::ZERO)` asks
//! every time.
//!
//! Failure closes: an endpoint that can't be reached, answers an error, or
//! answers something that isn't an introspection response rejects the token.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use futures::future::BoxFuture;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::NetworkPolicy;
use crate::error::AuthError;
use crate::validator::{AuthPrincipal, BearerValidator, extract_scopes};

/// How the resource server authenticates to the introspection endpoint ("the
/// protected resource MUST also be authenticated", RFC 7662 §2.1).
#[derive(Clone)]
#[non_exhaustive]
pub enum ClientAuth {
    /// HTTP Basic with the form-encoded client id and secret
    /// (`client_secret_basic`, RFC 6749 §2.3.1): the default most servers
    /// expect.
    Basic {
        /// The resource server's client id.
        client_id: String,
        /// Its client secret, wiped from memory when dropped.
        client_secret: Zeroizing<String>,
    },
    /// The client id and secret in the form body (`client_secret_post`).
    Post {
        /// The resource server's client id.
        client_id: String,
        /// Its client secret, wiped from memory when dropped.
        client_secret: Zeroizing<String>,
    },
    /// A bearer token of the resource server's own, wiped from memory when
    /// dropped.
    Bearer(Zeroizing<String>),
}

impl ClientAuth {
    /// `client_secret_basic`.
    #[must_use]
    pub fn basic(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self::Basic {
            client_id: client_id.into(),
            client_secret: Zeroizing::new(client_secret.into()),
        }
    }

    /// `client_secret_post`.
    #[must_use]
    pub fn post(client_id: impl Into<String>, client_secret: impl Into<String>) -> Self {
        Self::Post {
            client_id: client_id.into(),
            client_secret: Zeroizing::new(client_secret.into()),
        }
    }

    /// A bearer token.
    #[must_use]
    pub fn bearer(token: impl Into<String>) -> Self {
        Self::Bearer(Zeroizing::new(token.into()))
    }
}

impl core::fmt::Debug for ClientAuth {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Basic { client_id, .. } => f
                .debug_struct("Basic")
                .field("client_id", client_id)
                .finish_non_exhaustive(),
            Self::Post { client_id, .. } => f
                .debug_struct("Post")
                .field("client_id", client_id)
                .finish_non_exhaustive(),
            Self::Bearer(_) => f.write_str("Bearer(..)"),
        }
    }
}

#[derive(Clone)]
struct Cached {
    principal: AuthPrincipal,
    /// The token's own `exp`, which bounds the entry whatever the cache's TTL.
    exp: Option<u64>,
}

/// A [`BearerValidator`] for opaque tokens, by RFC 7662 introspection.
///
/// ```no_run
/// # use turbomcp_auth::{ClientAuth, IntrospectionValidator, ResourceMetadata, ResourceServer};
/// let validator = IntrospectionValidator::new(
///     "https://auth.example.com/oauth2/introspect",
///     "https://mcp.example.com",
///     ClientAuth::basic("mcp-server", "secret-from-your-vault"),
/// )
/// .require_issuer("https://auth.example.com");
/// let resource_server = ResourceServer::new(
///     validator,
///     ResourceMetadata::new("https://mcp.example.com", ["https://auth.example.com"]),
///     "https://mcp.example.com/.well-known/oauth-protected-resource",
/// );
/// ```
pub struct IntrospectionValidator {
    endpoint: String,
    auth: ClientAuth,
    audiences: Vec<String>,
    issuers: Vec<String>,
    leeway: u64,
    cache_ttl: Duration,
    cache: moka::sync::Cache<[u8; 32], Cached>,
    client: reqwest::Client,
    policy: NetworkPolicy,
}

impl core::fmt::Debug for IntrospectionValidator {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("IntrospectionValidator")
            .field("endpoint", &self.endpoint)
            .field("auth", &self.auth)
            .field("audiences", &self.audiences)
            .field("issuers", &self.issuers)
            .field("cache_ttl", &self.cache_ttl)
            .finish_non_exhaustive()
    }
}

impl IntrospectionValidator {
    /// How long an answer is reused by default.
    pub const DEFAULT_CACHE_TTL: Duration = Duration::from_secs(60);
    /// How many answers the cache holds.
    pub const CACHE_CAPACITY: u64 = 10_000;

    /// Introspect at `endpoint`, authenticated with `auth`, accepting tokens
    /// issued for `audience` (this resource's canonical URI).
    ///
    /// # Panics
    /// If the TLS backend can't initialize, which the HTTP client needs.
    #[must_use]
    pub fn new(endpoint: impl Into<String>, audience: impl Into<String>, auth: ClientAuth) -> Self {
        let policy = NetworkPolicy::default();
        Self {
            endpoint: endpoint.into(),
            auth,
            audiences: vec![audience.into()],
            issuers: Vec::new(),
            leeway: 60,
            cache_ttl: Self::DEFAULT_CACHE_TTL,
            cache: Self::build_cache(Self::DEFAULT_CACHE_TTL),
            client: policy.http_client().expect("HTTP client initialization"),
            policy,
        }
    }

    fn build_cache(ttl: Duration) -> moka::sync::Cache<[u8; 32], Cached> {
        moka::sync::Cache::builder()
            .max_capacity(Self::CACHE_CAPACITY)
            .time_to_live(ttl.max(Duration::from_millis(1)))
            .build()
    }

    /// Accept tokens issued for this audience too.
    #[must_use]
    pub fn add_audience(mut self, audience: impl Into<String>) -> Self {
        self.audiences.push(audience.into());
        self
    }

    /// Require the answer's `iss` to be `issuer` (or another required one).
    /// Without any, `iss` isn't checked: RFC 7662 makes it optional, and the
    /// endpoint is the issuer's own.
    #[must_use]
    pub fn require_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuers.push(issuer.into());
        self
    }

    /// Clock-skew leeway for `exp` and `nbf`, in seconds (default 60).
    #[must_use]
    pub fn leeway(mut self, seconds: u64) -> Self {
        self.leeway = seconds;
        self
    }

    /// Reuse an answer for up to `ttl` (default 60 s), never past the token's
    /// `exp`. `Duration::ZERO` introspects every request.
    #[must_use]
    pub fn cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self.cache = Self::build_cache(ttl);
        self
    }

    /// Reach the endpoint under `policy` (default: [`NetworkPolicy::default`],
    /// which allows a private authorization server, since the URL is the
    /// operator's own).
    ///
    /// # Errors
    /// [`AuthError::KeyUnavailable`] if the HTTP client can't be built.
    pub fn with_network_policy(mut self, policy: NetworkPolicy) -> Result<Self, AuthError> {
        self.client = policy
            .http_client()
            .map_err(|e| AuthError::KeyUnavailable(e.to_string()))?;
        self.policy = policy;
        Ok(self)
    }

    /// Use `client` (TLS or proxy settings). It must not follow redirects,
    /// and must keep the network policy's address restrictions itself.
    #[must_use]
    pub fn with_http_client(mut self, client: reqwest::Client) -> Self {
        self.client = client;
        self
    }

    async fn validate_inner(&self, token: &str) -> Result<AuthPrincipal, AuthError> {
        if token.len() > 64 * 1024 {
            return Err(AuthError::InvalidToken("token exceeds byte limit".into()));
        }
        let key: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        if let Some(cached) = self.cache.get(&key) {
            if cached.exp.is_none_or(|exp| exp + self.leeway > now()) {
                return Ok(cached.principal);
            }
            self.cache.invalidate(&key);
        }
        let claims = self.introspect(token).await?;
        let (principal, exp) = self.accept(claims)?;
        if !self.cache_ttl.is_zero() {
            self.cache.insert(
                key,
                Cached {
                    principal: principal.clone(),
                    exp,
                },
            );
        }
        Ok(principal)
    }

    /// Post the token and read the answer (RFC 7662 §2.1–2.2).
    async fn introspect(&self, token: &str) -> Result<Map<String, Value>, AuthError> {
        let mut form = vec![("token", token), ("token_type_hint", "access_token")];
        let mut request = self
            .client
            .post(&self.endpoint)
            .header(reqwest::header::ACCEPT, "application/json");
        match &self.auth {
            ClientAuth::Basic {
                client_id,
                client_secret,
            } => {
                // RFC 6749 §2.3.1: each part form-encoded before Base64.
                let encode = |s: &str| {
                    url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>()
                };
                let credentials = format!("{}:{}", encode(client_id), encode(client_secret));
                let basic = base64::engine::general_purpose::STANDARD.encode(credentials);
                request = request.header(reqwest::header::AUTHORIZATION, format!("Basic {basic}"));
            }
            ClientAuth::Post {
                client_id,
                client_secret,
            } => {
                form.push(("client_id", client_id));
                form.push(("client_secret", client_secret));
            }
            ClientAuth::Bearer(bearer) => request = request.bearer_auth(bearer.as_str()),
        }
        let unavailable = |e: String| AuthError::KeyUnavailable(format!("introspection: {e}"));
        let response = self
            .policy
            .send(&self.client, request.form(&form))
            .await
            .map_err(unavailable)?;
        let status = response.status();
        if !status.is_success() {
            return Err(unavailable(format!("HTTP {status}")));
        }
        let body = self.policy.body(response).await.map_err(unavailable)?;
        match serde_json::from_slice(&body) {
            Ok(Value::Object(claims)) => Ok(claims),
            _ => Err(unavailable("the answer is not a JSON object".into())),
        }
    }

    /// Hold an introspection answer to this resource: active, for this
    /// audience, from a required issuer, within its validity window.
    fn accept(
        &self,
        claims: Map<String, Value>,
    ) -> Result<(AuthPrincipal, Option<u64>), AuthError> {
        let invalid = |why: &str| Err(AuthError::InvalidToken(why.to_owned()));
        // "active" is the one required member; anything but `true` is not.
        if claims.get("active") != Some(&Value::Bool(true)) {
            return invalid("the token is not active");
        }
        let now = now();
        let time = |name: &str| claims.get(name).and_then(Value::as_u64);
        let exp = time("exp");
        if exp.is_some_and(|exp| exp + self.leeway <= now) {
            return invalid("the token has expired");
        }
        if time("nbf").is_some_and(|nbf| nbf > now + self.leeway) {
            return invalid("the token is not yet valid");
        }
        let audiences: Vec<&str> = match claims.get("aud") {
            Some(Value::String(aud)) => vec![aud.as_str()],
            Some(Value::Array(auds)) => auds.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        };
        if !audiences
            .iter()
            .any(|aud| self.audiences.iter().any(|ours| ours == aud))
        {
            return invalid("the token was not issued for this resource");
        }
        if !self.issuers.is_empty() {
            let iss = claims.get("iss").and_then(Value::as_str);
            if !iss.is_some_and(|iss| self.issuers.iter().any(|ours| ours == iss)) {
                return invalid("the token's issuer is not trusted");
            }
        }
        // A client-credentials token may name no user: its client is who
        // the caller is.
        let subject = claims
            .get("sub")
            .or_else(|| claims.get("client_id"))
            .and_then(Value::as_str)
            .ok_or_else(|| AuthError::InvalidToken("the token names no subject".into()))?
            .to_owned();
        let scopes = extract_scopes(&claims);
        Ok((
            AuthPrincipal {
                subject,
                scopes,
                claims,
            },
            exp,
        ))
    }
}

impl BearerValidator for IntrospectionValidator {
    fn validate<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<AuthPrincipal, AuthError>> {
        Box::pin(self.validate_inner(token))
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
