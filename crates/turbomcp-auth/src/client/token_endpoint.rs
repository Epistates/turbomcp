//! Requests to an authorization server's token endpoint for the grants that
//! need no user (RFC 6749 §4.4, RFC 7523, RFC 8693): one POST, one JSON
//! answer, under the same network policy and HTTPS checks as every other
//! authorization request.

use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use zeroize::Zeroizing;

use super::discovery::{AuthorizationServerMetadata, require_secure_url};
use super::registration::ClientCredentials;
use super::{OAuthClientError, TokenSet};

/// Where the client secret goes.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ClientAuth {
    Basic,
    Post,
}

impl ClientAuth {
    /// RFC 8414 §2's default is Basic; an authorization server that offers
    /// only `client_secret_post` gets the secret in the body.
    pub(crate) fn for_server(server: &AuthorizationServerMetadata) -> Self {
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

/// A token endpoint's success answer (RFC 6749 §5.1, with RFC 8693's
/// `issued_token_type`).
#[derive(Deserialize)]
pub(crate) struct TokenResponse {
    pub(crate) access_token: Zeroizing<String>,
    #[serde(default)]
    pub(crate) issued_token_type: Option<String>,
    #[serde(default)]
    pub(crate) refresh_token: Option<Zeroizing<String>>,
    #[serde(default)]
    pub(crate) expires_in: Option<u64>,
    #[serde(default)]
    pub(crate) scope: Option<String>,
}

impl TokenResponse {
    /// The token set it grants; `requested` stands for the scopes when the
    /// server didn't say which it granted (RFC 6749 §5.1: it need not when
    /// they are the requested ones).
    pub(crate) fn into_token_set(self, requested: &[String]) -> TokenSet {
        let expires_at_epoch_secs = self.expires_in.map(|ttl| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
                + ttl
        });
        TokenSet {
            access_token: self.access_token,
            refresh_token: self.refresh_token,
            expires_at_epoch_secs,
            scopes: self
                .scope
                .map_or_else(|| requested.to_vec(), |s| split_scopes(&s)),
        }
    }
}

#[derive(Deserialize)]
struct TokenError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

/// POST `form` to `endpoint`, authenticated as `credentials`, and parse the
/// JSON answer (or the RFC 6749 §5.2 error).
pub(crate) async fn post_form<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    network: &crate::NetworkPolicy,
    endpoint: &str,
    what: &str,
    credentials: &ClientCredentials,
    auth: ClientAuth,
    mut form: Vec<(&str, &str)>,
) -> Result<T, OAuthClientError> {
    require_secure_url(endpoint, what)?;
    network
        .validate_url(endpoint)
        .map_err(OAuthClientError::Discovery)?;
    let mut request = http.post(endpoint).header("accept", "application/json");
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
    let response = network
        .send(http, request.form(&form))
        .await
        .map_err(OAuthClientError::TokenExchange)?;
    let status = response.status();
    let body = network
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

/// Refuse a grant the server says it doesn't support (RFC 8414
/// `grant_types_supported`; absent, nothing is known and the request is
/// made).
pub(crate) fn require_grant(
    server: &AuthorizationServerMetadata,
    grant_type: &str,
) -> Result<(), OAuthClientError> {
    match &server.grant_types_supported {
        Some(grants) if !grants.iter().any(|g| g == grant_type) => {
            Err(OAuthClientError::Discovery(format!(
                "authorization server {} does not support the `{grant_type}` grant",
                server.issuer
            )))
        }
        _ => Ok(()),
    }
}

/// RFC 6749 §2.3.1: Basic credentials are form-urlencoded before encoding.
fn form_urlencode(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

pub(crate) fn split_scopes(scope: &str) -> Vec<String> {
    scope.split_whitespace().map(str::to_owned).collect()
}
