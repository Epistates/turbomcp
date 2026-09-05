//! The OAuth 2.1 authorization-code + PKCE flow, MCP-shaped.
//!
//! [`OAuthClient`] drives everything after the interactive step: discovery
//! ([RFC 9728]/[RFC 8414]), registration, PKCE (`S256`, via the `oauth2`
//! crate — never hand-rolled), the RFC 8707 `resource` parameter on both the
//! authorization and token requests, RFC 9207 `iss` validation of the
//! authorization response, token exchange, and refresh. The one thing it
//! cannot do is open a browser: [`begin`](OAuthClient::begin) hands back the
//! authorization URL, the embedding application delivers the user to it and
//! returns the redirect callback to [`complete`](OAuthClient::complete).
//!
//! [RFC 9728]: https://datatracker.ietf.org/doc/html/rfc9728
//! [RFC 8414]: https://datatracker.ietf.org/doc/html/rfc8414

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use oauth2::basic::BasicClient;
use oauth2::{
    AuthType, AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
};

use super::challenge::BearerChallenge;
use super::discovery::{
    AuthorizationServerMetadata, ProtectedResourceMetadata, discover_authorization_server,
    discover_protected_resource,
};
use super::registration::{ClientCredentials, RegistrationStrategy, obtain_credentials};
use super::store::{CredentialStore, MemoryCredentialStore};
use super::{OAuthClientError, TokenSet};

/// Everything discovery produced for one MCP server: its protected-resource
/// metadata and the selected authorization server's validated metadata.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Discovered {
    /// The MCP server's RFC 9728 document.
    pub resource: ProtectedResourceMetadata,
    /// The selected authorization server's validated metadata.
    pub server: AuthorizationServerMetadata,
}

/// The per-authorization record the client MUST keep between opening the
/// browser and receiving the callback: PKCE verifier, `state`, and the
/// recorded expected issuer (RFC 9207 depends on this being authentic).
///
/// `Debug` is redacted: the PKCE verifier is a secret (a leaked verifier lets
/// an attacker who also has the code complete the exchange), so it never
/// renders.
#[non_exhaustive]
pub struct PendingAuthorization {
    /// Where to send the user's browser.
    pub authorize_url: String,
    /// The `state` value bound to this authorization.
    pub state: String,
    pkce_verifier: String,
    expected_issuer: String,
    iss_advertised: bool,
    scopes: Vec<String>,
}

impl std::fmt::Debug for PendingAuthorization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingAuthorization")
            .field("authorize_url", &self.authorize_url)
            .field("state", &self.state)
            .field("pkce_verifier", &"<redacted>")
            .field("expected_issuer", &self.expected_issuer)
            .field("iss_advertised", &self.iss_advertised)
            .field("scopes", &self.scopes)
            .finish()
    }
}

/// The parameters of the authorization-response redirect, parsed from its
/// query string.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct CallbackParams {
    /// `code` — the authorization code.
    pub code: Option<String>,
    /// `state` — must match [`PendingAuthorization::state`].
    pub state: Option<String>,
    /// `iss` — RFC 9207 issuer identification.
    pub iss: Option<String>,
    /// `error` / `error_description` — an error response.
    pub error: Option<String>,
    /// Human-readable error detail.
    pub error_description: Option<String>,
}

impl CallbackParams {
    /// Parse from a query string (with or without the leading `?`) or a full
    /// redirect URL.
    #[must_use]
    pub fn from_query(query_or_url: &str) -> Self {
        let query = query_or_url
            .split_once('?')
            .map_or(query_or_url, |(_, q)| q);
        let mut out = Self::default();
        for (k, v) in url::form_urlencoded::parse(query.as_bytes()) {
            match k.as_ref() {
                "code" => out.code = Some(v.into_owned()),
                "state" => out.state = Some(v.into_owned()),
                "iss" => out.iss = Some(v.into_owned()),
                "error" => out.error = Some(v.into_owned()),
                "error_description" => out.error_description = Some(v.into_owned()),
                _ => {}
            }
        }
        out
    }
}

/// The MCP OAuth 2.1 client engine. See the [module docs](self).
pub struct OAuthClient {
    http: reqwest::Client,
    resource: String,
    redirect_uri: String,
    strategy: RegistrationStrategy,
    store: Arc<dyn CredentialStore>,
}

impl std::fmt::Debug for OAuthClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthClient")
            .field("resource", &self.resource)
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

impl OAuthClient {
    /// An engine for the MCP server at `resource` (its canonical RFC 8707
    /// URI — e.g. `https://mcp.example.com/mcp`), redirecting the browser
    /// back to `redirect_uri`, registering via `strategy`. Credentials and
    /// tokens persist in a process-lifetime [`MemoryCredentialStore`] until
    /// [`with_store`](Self::with_store) supplies something durable.
    #[must_use]
    pub fn new(
        resource: impl Into<String>,
        redirect_uri: impl Into<String>,
        strategy: RegistrationStrategy,
    ) -> Self {
        Self {
            http: reqwest::Client::new(),
            resource: resource.into(),
            redirect_uri: redirect_uri.into(),
            strategy,
            store: Arc::new(MemoryCredentialStore::default()),
        }
    }

    /// Persist credentials/tokens in `store` (issuer-keyed; see
    /// [`CredentialStore`]).
    #[must_use]
    pub fn with_store(mut self, store: Arc<dyn CredentialStore>) -> Self {
        self.store = store;
        self
    }

    /// Use a custom HTTP client (proxies, TLS pinning, timeouts).
    #[must_use]
    pub fn with_http_client(mut self, http: reqwest::Client) -> Self {
        self.http = http;
        self
    }

    /// The canonical resource URI this engine requests tokens for.
    #[must_use]
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// Run discovery: Protected Resource Metadata (from the 401 challenge's
    /// `resource_metadata` URL when given, else the well-known fallbacks),
    /// then the first advertised authorization server's metadata (validated;
    /// PKCE support required).
    ///
    /// # Errors
    /// Discovery/validation failures; [`OAuthClientError::PkceUnsupported`]
    /// when the AS does not advertise PKCE.
    pub async fn discover(
        &self,
        challenge: Option<&BearerChallenge>,
    ) -> Result<Discovered, OAuthClientError> {
        let resource_meta = discover_protected_resource(
            &self.http,
            &self.resource,
            challenge.and_then(|c| c.resource_metadata.as_deref()),
        )
        .await?;
        // Selection among multiple advertised servers is the client's call
        // (RFC 9728 §7.6); we take the first. Wrap the engine to override.
        let issuer = resource_meta.authorization_servers[0].clone();
        let server = discover_authorization_server(&self.http, &issuer).await?;
        Ok(Discovered {
            resource: resource_meta,
            server,
        })
    }

    /// The credentials to use at the discovered server: previously stored for
    /// this issuer, else obtained via the registration strategy and stored.
    ///
    /// # Errors
    /// Registration failures ([`OAuthClientError::Registration`] /
    /// [`OAuthClientError::IssuerChanged`]).
    pub async fn credentials(
        &self,
        discovered: &Discovered,
    ) -> Result<ClientCredentials, OAuthClientError> {
        if let Some(stored) = self.store.load_client(&discovered.server.issuer).await {
            return Ok(stored);
        }
        let credentials =
            obtain_credentials(&self.http, &discovered.server, &self.strategy).await?;
        self.store
            .store_client(&discovered.server.issuer, &credentials)
            .await;
        Ok(credentials)
    }

    /// Compute the scope set to request, per the spec's selection strategy:
    /// the challenge's `scope` when present, else the resource metadata's
    /// `scopes_supported`, else empty (omit the parameter).
    #[must_use]
    pub fn select_scopes(
        challenge: Option<&BearerChallenge>,
        discovered: &Discovered,
    ) -> Vec<String> {
        if let Some(c) = challenge {
            let scopes = c.scopes();
            if !scopes.is_empty() {
                return scopes;
            }
        }
        discovered
            .resource
            .scopes_supported
            .clone()
            .unwrap_or_default()
    }

    /// The step-up scope set: the union of previously requested scopes and
    /// the new challenge's scopes (preserving previously granted permissions,
    /// spec §Step-Up Authorization Flow).
    #[must_use]
    pub fn step_up_scopes(previous: &[String], challenged: &[String]) -> Vec<String> {
        let mut seen: BTreeMap<&str, ()> = BTreeMap::new();
        let mut out = Vec::new();
        for scope in previous.iter().chain(challenged) {
            if seen.insert(scope, ()).is_none() {
                out.push(scope.clone());
            }
        }
        out
    }

    /// Build the authorization request: fresh PKCE (`S256`) + `state`, the
    /// RFC 8707 `resource` parameter, and the recorded expected issuer. Send
    /// the user's browser to [`PendingAuthorization::authorize_url`], then
    /// feed the redirect to [`complete`](Self::complete).
    ///
    /// # Errors
    /// Malformed endpoint/redirect URLs from discovery/config.
    pub fn begin(
        &self,
        discovered: &Discovered,
        credentials: &ClientCredentials,
        scopes: &[String],
    ) -> Result<PendingAuthorization, OAuthClientError> {
        let client = build_oauth2_client(discovered, credentials, &self.redirect_uri)?;
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let mut request = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(pkce_challenge)
            .add_extra_param("resource", &self.resource);
        for scope in scopes {
            request = request.add_scope(Scope::new(scope.clone()));
        }
        let (authorize_url, state) = request.url();
        Ok(PendingAuthorization {
            authorize_url: authorize_url.to_string(),
            state: state.secret().clone(),
            pkce_verifier: pkce_verifier.secret().clone(),
            expected_issuer: discovered.server.issuer.clone(),
            iss_advertised: discovered
                .server
                .authorization_response_iss_parameter_supported
                == Some(true),
            scopes: scopes.to_vec(),
        })
    }

    /// Validate the authorization response and exchange the code for tokens.
    ///
    /// Validation order (each a MUST): `state` binds the response to
    /// `pending`; the RFC 9207 `iss` table runs **before** the code touches
    /// any token endpoint (and before error params are trusted — a mismatched
    /// response's `error` is never surfaced); then an error response is
    /// reported; then the code is exchanged with the PKCE verifier and the
    /// `resource` parameter. Tokens are persisted issuer-keyed.
    ///
    /// # Errors
    /// [`OAuthClientError::Authorization`] on state/iss/error-response
    /// failures; [`OAuthClientError::TokenExchange`] when the exchange fails.
    pub async fn complete(
        &self,
        discovered: &Discovered,
        credentials: &ClientCredentials,
        pending: PendingAuthorization,
        callback: &CallbackParams,
    ) -> Result<TokenSet, OAuthClientError> {
        // state: discard mismatches (CSRF / open-redirect protection). The
        // compare is constant-time — the redirect is typically received by a
        // localhost HTTP listener, so a variable-time check would expose a
        // timing oracle on the expected `state` to anything that can hit it.
        let state_ok = callback
            .state
            .as_deref()
            .is_some_and(|s| ct_eq(s.as_bytes(), pending.state.as_bytes()));
        if !state_ok {
            return Err(OAuthClientError::Authorization(
                "authorization response state mismatch; response discarded".into(),
            ));
        }
        // RFC 9207 §2.4 (the MCP table). Simple string comparison — no
        // normalization of scheme/host case, ports, slashes, or escaping.
        match (&callback.iss, pending.iss_advertised) {
            (Some(iss), _) => {
                if *iss != pending.expected_issuer {
                    // On mismatch: MUST NOT act on or display error params.
                    return Err(OAuthClientError::IssuerMismatch {
                        expected: pending.expected_issuer.clone(),
                        got: iss.clone(),
                    });
                }
            }
            (None, true) => {
                return Err(OAuthClientError::Authorization(
                    "authorization server advertises iss identification but the response omitted it"
                        .into(),
                ));
            }
            (None, false) => {}
        }
        if let Some(error) = &callback.error {
            return Err(OAuthClientError::Authorization(format!(
                "authorization failed: {error}{}",
                callback
                    .error_description
                    .as_deref()
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default()
            )));
        }
        let Some(code) = &callback.code else {
            return Err(OAuthClientError::Authorization(
                "authorization response carried no code".into(),
            ));
        };

        let client = build_oauth2_client(discovered, credentials, &self.redirect_uri)?;
        let http = self.http.clone();
        let token = client
            .exchange_code(AuthorizationCode::new(code.clone()))
            .set_pkce_verifier(PkceCodeVerifier::new(pending.pkce_verifier.clone()))
            .add_extra_param("resource", &self.resource)
            .request_async(&move |req| {
                let http = http.clone();
                async move { oauth_http_call(&http, req).await }
            })
            .await
            .map_err(|e| OAuthClientError::TokenExchange(e.to_string()))?;

        let tokens = to_token_set(&token, &pending.scopes);
        self.store
            .store_tokens(&discovered.server.issuer, &self.resource, &tokens)
            .await;
        Ok(tokens)
    }

    /// Redeem a refresh token (with the `resource` parameter) and persist the
    /// rotated token set.
    ///
    /// # Errors
    /// [`OAuthClientError::TokenExchange`] when the refresh is rejected
    /// (fall back to a fresh interactive authorization).
    pub async fn refresh(
        &self,
        discovered: &Discovered,
        credentials: &ClientCredentials,
        tokens: &TokenSet,
    ) -> Result<TokenSet, OAuthClientError> {
        let Some(refresh_token) = &tokens.refresh_token else {
            return Err(OAuthClientError::TokenExchange(
                "no refresh token; re-authorization required".into(),
            ));
        };
        let client = build_oauth2_client(discovered, credentials, &self.redirect_uri)?;
        let http = self.http.clone();
        let token = client
            .exchange_refresh_token(&RefreshToken::new(refresh_token.clone()))
            .add_extra_param("resource", &self.resource)
            .request_async(&move |req| {
                let http = http.clone();
                async move { oauth_http_call(&http, req).await }
            })
            .await
            .map_err(|e| OAuthClientError::TokenExchange(e.to_string()))?;

        let mut rotated = to_token_set(&token, &tokens.scopes);
        // An AS that doesn't rotate the refresh token keeps the old one live.
        if rotated.refresh_token.is_none() {
            rotated.refresh_token = tokens.refresh_token.clone();
        }
        self.store
            .store_tokens(&discovered.server.issuer, &self.resource, &rotated)
            .await;
        Ok(rotated)
    }

    /// The stored token set for the discovered issuer + this resource.
    pub async fn stored_tokens(&self, discovered: &Discovered) -> Option<TokenSet> {
        self.store
            .load_tokens(&discovered.server.issuer, &self.resource)
            .await
    }
}

/// Constant-time byte-slice equality: no early return on the first differing
/// byte, so comparing a secret-ish token (here the CSRF `state`) leaks no
/// timing signal about how much of it matched. Length still differs fast — the
/// `state` length is fixed by `CsrfToken::new_random`, so that is not sensitive.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Assemble the `oauth2` client from discovered endpoints + credentials.
fn build_oauth2_client(
    discovered: &Discovered,
    credentials: &ClientCredentials,
    redirect_uri: &str,
) -> Result<
    BasicClient<
        oauth2::EndpointSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
    >,
    OAuthClientError,
> {
    let bad_url =
        |what: &str, e: url::ParseError| OAuthClientError::Discovery(format!("{what}: {e}"));
    let mut client = BasicClient::new(ClientId::new(credentials.client_id.clone()))
        .set_auth_uri(
            AuthUrl::new(discovered.server.authorization_endpoint.clone())
                .map_err(|e| bad_url("authorization_endpoint", e))?,
        )
        .set_token_uri(
            TokenUrl::new(discovered.server.token_endpoint.clone())
                .map_err(|e| bad_url("token_endpoint", e))?,
        )
        .set_redirect_uri(
            RedirectUrl::new(redirect_uri.to_owned()).map_err(|e| bad_url("redirect_uri", e))?,
        );
    if let Some(secret) = &credentials.client_secret {
        client = client
            .set_client_secret(ClientSecret::new(secret.clone()))
            .set_auth_type(token_endpoint_auth_type(&discovered.server));
    }
    Ok(client)
}

/// Where to put the client secret at the token endpoint.
///
/// RFC 8414 §2 makes `client_secret_basic` the default when the AS advertises
/// nothing, and `oauth2` defaults the same way — but an AS that supports *only*
/// `client_secret_post` rejects a Basic header outright, so the advertised list
/// has to be honoured when there is one. Basic stays preferred wherever both
/// are offered, per OAuth 2.1's own preference.
fn token_endpoint_auth_type(server: &AuthorizationServerMetadata) -> AuthType {
    let Some(methods) = &server.token_endpoint_auth_methods_supported else {
        return AuthType::BasicAuth;
    };
    let supports = |name: &str| methods.iter().any(|m| m == name);
    if supports("client_secret_basic") || !supports("client_secret_post") {
        AuthType::BasicAuth
    } else {
        AuthType::RequestBody
    }
}

fn to_token_set(
    token: &oauth2::basic::BasicTokenResponse,
    requested_scopes: &[String],
) -> TokenSet {
    let expires_at_epoch_secs = token.expires_in().map(|ttl: Duration| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + ttl.as_secs()
    });
    // Granted scopes when the AS echoed them; else what we asked for.
    let scopes = token
        .scopes()
        .map(|s| s.iter().map(|v| v.to_string()).collect())
        .unwrap_or_else(|| requested_scopes.to_vec());
    TokenSet {
        access_token: token.access_token().secret().clone(),
        refresh_token: token.refresh_token().map(|t| t.secret().clone()),
        expires_at_epoch_secs,
        scopes,
    }
}

/// Bridge `oauth2`'s HTTP request/response types over our `reqwest` client
/// (byte-level conversions, so the two crates' `http` versions never need to
/// agree).
async fn oauth_http_call(
    http: &reqwest::Client,
    request: oauth2::HttpRequest,
) -> Result<oauth2::HttpResponse, OAuthClientError> {
    let (parts, body) = request.into_parts();
    let method = reqwest::Method::from_bytes(parts.method.as_str().as_bytes())
        .map_err(|e| OAuthClientError::TokenExchange(format!("bad method: {e}")))?;
    let mut req = http.request(method, parts.uri.to_string());
    for (name, value) in &parts.headers {
        req = req.header(name.as_str(), value.as_bytes());
    }
    let resp = req
        .body(body)
        .send()
        .await
        .map_err(|e| OAuthClientError::TokenExchange(e.to_string()))?;

    let status = resp.status().as_u16();
    let headers = resp.headers().clone();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| OAuthClientError::TokenExchange(e.to_string()))?;

    let mut builder = oauth2::http::Response::builder().status(status);
    for (name, value) in &headers {
        builder = builder.header(name.as_str(), value.as_bytes());
    }
    builder
        .body(bytes.to_vec())
        .map_err(|e| OAuthClientError::TokenExchange(format!("response conversion: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_params_parse_from_query_and_url() {
        let p = CallbackParams::from_query(
            "http://127.0.0.1:7777/cb?code=abc&state=xyz&iss=https%3A%2F%2Fas.example",
        );
        assert_eq!(p.code.as_deref(), Some("abc"));
        assert_eq!(p.state.as_deref(), Some("xyz"));
        assert_eq!(p.iss.as_deref(), Some("https://as.example"));

        let p = CallbackParams::from_query("error=access_denied&state=s");
        assert_eq!(p.error.as_deref(), Some("access_denied"));
    }

    #[test]
    fn step_up_unions_scopes_preserving_grants() {
        let previous = vec!["files:read".to_owned(), "profile".to_owned()];
        let challenged = vec!["files:write".to_owned(), "files:read".to_owned()];
        assert_eq!(
            OAuthClient::step_up_scopes(&previous, &challenged),
            vec!["files:read", "profile", "files:write"]
        );
    }

    #[test]
    fn secret_bearing_debug_is_redacted() {
        // A stray `{:?}` on any of these must not spill the secret into logs.
        let tokens = TokenSet {
            access_token: "at-super-secret".into(),
            refresh_token: Some("rt-super-secret".into()),
            expires_at_epoch_secs: Some(42),
            scopes: vec!["files:read".into()],
        };
        let dbg = format!("{tokens:?}");
        assert!(!dbg.contains("super-secret"), "token leaked: {dbg}");
        assert!(dbg.contains("<redacted>") && dbg.contains("files:read"));

        let creds = ClientCredentials {
            client_id: "public-id".into(),
            client_secret: Some("cs-super-secret".into()),
        };
        let dbg = format!("{creds:?}");
        assert!(!dbg.contains("super-secret"), "client secret leaked: {dbg}");
        assert!(dbg.contains("public-id") && dbg.contains("<redacted>"));

        let pending = PendingAuthorization {
            authorize_url: "https://as.example/authorize?state=s".into(),
            state: "s".into(),
            pkce_verifier: "pv-super-secret".into(),
            expected_issuer: "https://as.example".into(),
            iss_advertised: true,
            scopes: vec![],
        };
        let dbg = format!("{pending:?}");
        assert!(!dbg.contains("super-secret"), "pkce verifier leaked: {dbg}");
        assert!(dbg.contains("<redacted>"));

        // A constant-time state compare still accepts the exact match and
        // rejects a differing one.
        assert!(ct_eq(b"abc123", b"abc123"));
        assert!(!ct_eq(b"abc123", b"abc124"));
        assert!(!ct_eq(b"abc", b"abcd"));
    }
}
