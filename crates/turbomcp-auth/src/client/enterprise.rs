//! Enterprise-Managed Authorization (`io.modelcontextprotocol/enterprise-managed-authorization`,
//! SEP-990): the organization's identity provider decides which MCP servers
//! an employee may use, and the client never sends the user through the MCP
//! server's own authorization screen.
//!
//! The client already holds an identity assertion from the user's SSO login
//! (an OpenID ID Token, or a SAML assertion). [`EnterpriseAuthorization`]
//! turns it into an access token for one MCP server in two exchanges:
//!
//! 1. **Token exchange at the IdP** (RFC 8693): the assertion for an
//!    *Identity Assertion JWT Authorization Grant*, an ID-JAG, issued for the
//!    MCP server's authorization server. The IdP evaluates its policy here; a
//!    user who may not use the server gets no ID-JAG.
//! 2. **JWT bearer grant at the MCP authorization server** (RFC 7523): the
//!    ID-JAG for an access token, audience-restricted to the MCP server.
//!
//! Discovery is the MCP server's own (RFC 9728, then RFC 8414), and the
//! authorization server must advertise the ID-JAG profile
//! (`authorization_grant_profiles_supported` containing
//! [`ID_JAG_PROFILE`]) or the flow is refused before anything is sent.
//! Every endpoint passes the same [`NetworkPolicy`](crate::NetworkPolicy)
//! and HTTPS checks as the authorization-code flow.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use zeroize::Zeroizing;

use super::OAuthClientError;
use super::TokenSet;
use super::challenge::BearerChallenge;
use super::discovery::{
    AuthorizationServerMetadata, discover_protected_resource_with_policy,
    fetch_authorization_server, require_secure_url,
};
use super::flow::Discovered;
use super::registration::ClientCredentials;

/// The extension's identifier, declared in the client's capabilities.
pub const EXTENSION_ID: &str = "io.modelcontextprotocol/enterprise-managed-authorization";

/// The grant profile an authorization server advertises when it accepts
/// ID-JAGs.
pub const ID_JAG_PROFILE: &str = "urn:ietf:params:oauth:grant-profile:id-jag";

const TOKEN_EXCHANGE_GRANT: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const JWT_BEARER_GRANT: &str = "urn:ietf:params:oauth:grant-type:jwt-bearer";
const ID_JAG_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:id-jag";

/// What kind of identity assertion the client holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AssertionKind {
    /// An OpenID Connect ID Token.
    IdToken,
    /// A SAML 2.0 assertion.
    Saml2,
    /// A refresh token the IdP issued with the SSO login.
    RefreshToken,
}

impl AssertionKind {
    fn token_type(self) -> &'static str {
        match self {
            Self::IdToken => "urn:ietf:params:oauth:token-type:id_token",
            Self::Saml2 => "urn:ietf:params:oauth:token-type:saml2",
            Self::RefreshToken => "urn:ietf:params:oauth:token-type:refresh_token",
        }
    }
}

/// The user's identity assertion from SSO. Wiped from memory when dropped;
/// `Debug` never renders it.
#[derive(Clone)]
pub struct IdentityAssertion {
    token: Zeroizing<String>,
    kind: AssertionKind,
}

impl std::fmt::Debug for IdentityAssertion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityAssertion")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl IdentityAssertion {
    /// An assertion of `kind`.
    #[must_use]
    pub fn new(token: impl Into<String>, kind: AssertionKind) -> Self {
        Self {
            token: Zeroizing::new(token.into()),
            kind,
        }
    }

    /// An OpenID Connect ID Token.
    #[must_use]
    pub fn id_token(token: impl Into<String>) -> Self {
        Self::new(token, AssertionKind::IdToken)
    }
}

/// The organization's identity provider: its token endpoint, and the
/// client's credentials there (the ones it uses for the SSO login).
#[derive(Clone, Debug)]
pub struct IdentityProvider {
    token_endpoint: String,
    credentials: ClientCredentials,
    auth: ClientAuth,
}

impl IdentityProvider {
    /// The IdP whose token endpoint is `token_endpoint`, where this client is
    /// `credentials`. "If the IdP requires client authentication when the
    /// MCP Client performs OpenID Connect for single sign-on, then client
    /// authentication of the Token Exchange request is also required": give
    /// the secret here if the login used one.
    #[must_use]
    pub fn new(token_endpoint: impl Into<String>, credentials: ClientCredentials) -> Self {
        Self {
            token_endpoint: token_endpoint.into(),
            credentials,
            auth: ClientAuth::Basic,
        }
    }

    /// Send the client secret in the request body (`client_secret_post`)
    /// rather than an `Authorization: Basic` header, for an IdP that accepts
    /// only that.
    #[must_use]
    pub fn secret_in_body(mut self) -> Self {
        self.auth = ClientAuth::Post;
        self
    }
}

/// An ID-JAG, issued by the IdP for one authorization server. Wiped from
/// memory when dropped; `Debug` never renders it.
#[derive(Clone)]
pub struct IdJag {
    /// The JWT.
    pub token: Zeroizing<String>,
    /// The scopes the IdP granted, when it said.
    pub scopes: Option<Vec<String>>,
}

impl std::fmt::Debug for IdJag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdJag")
            .field("scopes", &self.scopes)
            .finish_non_exhaustive()
    }
}

/// The Enterprise-Managed Authorization flow for one MCP server. See
/// [the module docs](self).
#[derive(Clone, Debug)]
pub struct EnterpriseAuthorization {
    resource: String,
    idp: IdentityProvider,
    credentials: ClientCredentials,
    http: reqwest::Client,
    network: crate::NetworkPolicy,
}

impl EnterpriseAuthorization {
    /// The flow for the MCP server `resource`, through `idp`, where this
    /// client is `credentials` at the MCP server's authorization server (a
    /// registered client, or a Client ID Metadata Document URL as a public
    /// client).
    ///
    /// Endpoints must be HTTPS on public addresses, as for
    /// [`OAuthClient`](super::OAuthClient); widen with
    /// [`with_network_policy`](Self::with_network_policy).
    ///
    /// # Panics
    /// If the TLS backend can't initialize, which the HTTP client needs.
    #[must_use]
    pub fn new(
        resource: impl Into<String>,
        idp: IdentityProvider,
        credentials: ClientCredentials,
    ) -> Self {
        let network = crate::NetworkPolicy::public_only();
        Self {
            resource: resource.into(),
            idp,
            credentials,
            http: network.http_client().expect("HTTP client initialization"),
            network,
        }
    }

    /// Reach endpoints under `policy` instead.
    ///
    /// # Errors
    /// The HTTP client failing to build under the policy.
    pub fn with_network_policy(
        mut self,
        policy: crate::NetworkPolicy,
    ) -> Result<Self, OAuthClientError> {
        self.http = policy
            .http_client()
            .map_err(|e| OAuthClientError::Discovery(format!("http client: {e}")))?;
        self.network = policy;
        Ok(self)
    }

    /// The MCP server this flow authorizes for.
    #[must_use]
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// Discover the MCP server's authorization server (from the 401
    /// challenge's `resource_metadata` when given), and check that it
    /// accepts ID-JAGs.
    ///
    /// # Errors
    /// Discovery failures, or [`OAuthClientError::Discovery`] when the
    /// authorization server does not advertise [`ID_JAG_PROFILE`].
    pub async fn discover(
        &self,
        challenge: Option<&BearerChallenge>,
    ) -> Result<Discovered, OAuthClientError> {
        let resource = discover_protected_resource_with_policy(
            &self.http,
            &self.resource,
            challenge.and_then(|c| c.resource_metadata.as_deref()),
            &self.network,
        )
        .await?;
        let issuer = resource.authorization_servers[0].clone();
        let server = fetch_authorization_server(&self.http, &issuer, &self.network).await?;
        if !accepts_id_jag(&server) {
            return Err(OAuthClientError::Discovery(format!(
                "authorization server {issuer} does not accept ID-JAGs \
                 (no {ID_JAG_PROFILE} in authorization_grant_profiles_supported)"
            )));
        }
        Ok(Discovered { resource, server })
    }

    /// Exchange `assertion` at the IdP for an ID-JAG for `discovered`'s
    /// authorization server, asking for `scopes` (RFC 8693).
    ///
    /// # Errors
    /// [`OAuthClientError::TokenExchange`] when the IdP refuses (its policy
    /// said no, or the assertion is stale) or answers with something other
    /// than an ID-JAG.
    pub async fn request_id_jag(
        &self,
        discovered: &Discovered,
        assertion: &IdentityAssertion,
        scopes: &[String],
    ) -> Result<IdJag, OAuthClientError> {
        let scope = scopes.join(" ");
        let mut form = vec![
            ("grant_type", TOKEN_EXCHANGE_GRANT),
            ("requested_token_type", ID_JAG_TOKEN_TYPE),
            // "MUST be the issuer identifier of the Resource Authorization Server"
            ("audience", discovered.server.issuer.as_str()),
            // "if set, MUST be the Resource Identifier of the MCP Server"
            ("resource", discovered.resource.resource.as_str()),
            ("subject_token", assertion.token.as_str()),
            ("subject_token_type", assertion.kind.token_type()),
        ];
        if !scope.is_empty() {
            form.push(("scope", &scope));
        }
        let response: TokenExchangeResponse = self
            .post_form(
                &self.idp.token_endpoint,
                "the IdP token endpoint",
                &self.idp.credentials,
                self.idp.auth,
                form,
            )
            .await?;
        if response.issued_token_type != ID_JAG_TOKEN_TYPE {
            return Err(OAuthClientError::TokenExchange(format!(
                "the IdP issued `{}`, not an ID-JAG",
                response.issued_token_type
            )));
        }
        Ok(IdJag {
            token: response.access_token,
            scopes: response.scope.map(|s| split_scopes(&s)),
        })
    }

    /// Present `id_jag` at `discovered`'s authorization server for an access
    /// token (RFC 7523 JWT bearer grant).
    ///
    /// # Errors
    /// [`OAuthClientError::TokenExchange`] when the authorization server
    /// refuses the grant.
    pub async fn exchange_id_jag(
        &self,
        discovered: &Discovered,
        id_jag: &IdJag,
    ) -> Result<TokenSet, OAuthClientError> {
        let form = vec![
            ("grant_type", JWT_BEARER_GRANT),
            ("assertion", id_jag.token.as_str()),
        ];
        let auth = ClientAuth::for_server(&discovered.server);
        let response: AccessTokenResponse = self
            .post_form(
                &discovered.server.token_endpoint,
                "the token endpoint",
                &self.credentials,
                auth,
                form,
            )
            .await?;
        let expires_at_epoch_secs = response.expires_in.map(|ttl| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + ttl
        });
        Ok(TokenSet {
            access_token: response.access_token,
            refresh_token: response.refresh_token,
            expires_at_epoch_secs,
            scopes: response
                .scope
                .map(|s| split_scopes(&s))
                .or_else(|| id_jag.scopes.clone())
                .unwrap_or_default(),
        })
    }

    /// The whole flow: discover, exchange `assertion` for an ID-JAG, and the
    /// ID-JAG for an access token. The scopes are the challenge's, else the
    /// server's advertised minimum, as for the authorization-code flow.
    ///
    /// # Errors
    /// As [`discover`](Self::discover), [`request_id_jag`](Self::request_id_jag)
    /// and [`exchange_id_jag`](Self::exchange_id_jag).
    pub async fn authorize(
        &self,
        assertion: &IdentityAssertion,
        challenge: Option<&BearerChallenge>,
    ) -> Result<TokenSet, OAuthClientError> {
        let discovered = self.discover(challenge).await?;
        let scopes = super::OAuthClient::select_scopes(challenge, &discovered);
        let id_jag = self.request_id_jag(&discovered, assertion, &scopes).await?;
        self.exchange_id_jag(&discovered, &id_jag).await
    }

    /// POST `form` to `endpoint`, authenticated as `credentials`, and parse
    /// the JSON answer (or the RFC 6749 §5.2 error).
    async fn post_form<T: serde::de::DeserializeOwned>(
        &self,
        endpoint: &str,
        what: &str,
        credentials: &ClientCredentials,
        auth: ClientAuth,
        mut form: Vec<(&str, &str)>,
    ) -> Result<T, OAuthClientError> {
        require_secure_url(endpoint, what)?;
        self.network
            .validate_url(endpoint)
            .map_err(OAuthClientError::Discovery)?;
        let mut request = self
            .http
            .post(endpoint)
            .header("accept", "application/json");
        match (&credentials.client_secret, auth) {
            (Some(secret), ClientAuth::Basic) => {
                request = request.basic_auth(
                    form_urlencode(&credentials.client_id),
                    Some(form_urlencode(secret)),
                );
            }
            (Some(secret), ClientAuth::Post) => {
                form.push(("client_id", &credentials.client_id));
                form.push(("client_secret", secret.as_str()));
            }
            (None, _) => form.push(("client_id", &credentials.client_id)),
        }
        let response = self
            .network
            .send(&self.http, request.form(&form))
            .await
            .map_err(OAuthClientError::TokenExchange)?;
        let status = response.status();
        let body = self
            .network
            .body(response)
            .await
            .map_err(OAuthClientError::TokenExchange)?;
        if !status.is_success() {
            let detail = serde_json::from_slice::<TokenError>(&body).map_or_else(
                |_| format!("http status {status}"),
                |e| match e.error_description {
                    Some(d) => format!("{}: {d}", e.error),
                    None => e.error,
                },
            );
            return Err(OAuthClientError::TokenExchange(format!("{what}: {detail}")));
        }
        serde_json::from_slice(&body)
            .map_err(|e| OAuthClientError::TokenExchange(format!("{what}: malformed answer: {e}")))
    }
}

/// Whether `server` advertises the ID-JAG grant profile.
#[must_use]
pub fn accepts_id_jag(server: &AuthorizationServerMetadata) -> bool {
    server
        .authorization_grant_profiles_supported
        .as_ref()
        .is_some_and(|profiles| profiles.iter().any(|p| p == ID_JAG_PROFILE))
}

/// Where the client secret goes.
#[derive(Clone, Copy, Debug)]
enum ClientAuth {
    Basic,
    Post,
}

impl ClientAuth {
    /// RFC 8414 §2's default is Basic; an authorization server that offers
    /// only `client_secret_post` gets the secret in the body.
    fn for_server(server: &AuthorizationServerMetadata) -> Self {
        let Some(methods) = &server.token_endpoint_auth_methods_supported else {
            return Self::Basic;
        };
        let supports = |name: &str| methods.iter().any(|m| m == name);
        if supports("client_secret_basic") || !supports("client_secret_post") {
            Self::Basic
        } else {
            Self::Post
        }
    }
}

/// RFC 6749 §2.3.1: Basic credentials are form-urlencoded before encoding.
fn form_urlencode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

fn split_scopes(scope: &str) -> Vec<String> {
    scope.split_whitespace().map(str::to_owned).collect()
}

#[derive(Deserialize)]
struct TokenExchangeResponse {
    access_token: Zeroizing<String>,
    issued_token_type: String,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
struct AccessTokenResponse {
    access_token: Zeroizing<String>,
    #[serde(default)]
    refresh_token: Option<Zeroizing<String>>,
    #[serde(default)]
    expires_in: Option<u64>,
    #[serde(default)]
    scope: Option<String>,
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}
