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

/// How the client authenticates at the token endpoint.
#[derive(Clone, Copy, Debug)]
pub(crate) enum ClientAuth<'a> {
    /// The secret in `Authorization: Basic`.
    Basic,
    /// The secret in the body (`client_secret_post`).
    Post,
    /// A signed assertion (`private_key_jwt`, RFC 7523), audienced to the
    /// named issuer.
    #[cfg(feature = "jwt")]
    PrivateKeyJwt(&'a ClientAssertion, &'a str),
    #[cfg(not(feature = "jwt"))]
    #[allow(dead_code)]
    Never(core::marker::PhantomData<&'a ()>),
}

/// A key this client signs `private_key_jwt` assertions with (RFC 7523
/// §2.2): the authorization server holds the matching public key, so no
/// shared secret travels or sits in configuration. The OAuth client
/// credentials extension recommends it over `client_secret`.
///
/// `Debug` names the algorithm and key id, never the key.
#[cfg(feature = "jwt")]
#[derive(Clone)]
pub struct ClientAssertion {
    key: jsonwebtoken::EncodingKey,
    algorithm: jsonwebtoken::Algorithm,
    key_id: Option<String>,
}

#[cfg(feature = "jwt")]
impl ClientAssertion {
    /// Sign with `key` using `algorithm` (e.g.
    /// `EncodingKey::from_rsa_pem(…)` with `Algorithm::RS256`, or an EC key
    /// with `ES256`).
    #[must_use]
    pub fn new(key: jsonwebtoken::EncodingKey, algorithm: jsonwebtoken::Algorithm) -> Self {
        Self {
            key,
            algorithm,
            key_id: None,
        }
    }

    /// Name the key (`kid`) the authorization server should verify with.
    #[must_use]
    pub fn with_key_id(mut self, kid: impl Into<String>) -> Self {
        self.key_id = Some(kid.into());
        self
    }

    /// A fresh assertion for `client_id` at `audience` (the authorization
    /// server's issuer, as RFC 7523bis directs): one minute's life and a
    /// random `jti`, so a captured one is useless soon and can't be replayed
    /// where the server tracks them.
    pub(crate) fn sign(&self, client_id: &str, audience: &str) -> Result<String, OAuthClientError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mut header = jsonwebtoken::Header::new(self.algorithm);
        header.kid.clone_from(&self.key_id);
        let claims = serde_json::json!({
            "iss": client_id,
            "sub": client_id,
            "aud": audience,
            "iat": now,
            "exp": now + 60,
            "jti": oauth2::CsrfToken::new_random().secret(),
        });
        jsonwebtoken::encode(&header, &claims, &self.key).map_err(|e| {
            OAuthClientError::TokenExchange(format!("could not sign the client assertion: {e}"))
        })
    }
}

#[cfg(feature = "jwt")]
impl core::fmt::Debug for ClientAssertion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ClientAssertion")
            .field("algorithm", &self.algorithm)
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

/// The assertion type `private_key_jwt` sends (RFC 7523 §2.2).
#[cfg(feature = "jwt")]
const JWT_BEARER_ASSERTION: &str = "urn:ietf:params:oauth:client-assertion-type:jwt-bearer";

impl ClientAuth<'_> {
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
    auth: ClientAuth<'_>,
    mut form: Vec<(&str, &str)>,
) -> Result<T, OAuthClientError> {
    require_secure_url(endpoint, what)?;
    network
        .validate_url(endpoint)
        .map_err(OAuthClientError::Discovery)?;
    let mut request = http.post(endpoint).header("accept", "application/json");
    #[cfg(feature = "jwt")]
    let assertion;
    match (&credentials.client_secret, auth) {
        #[cfg(feature = "jwt")]
        (_, ClientAuth::PrivateKeyJwt(key, audience)) => {
            assertion = key.sign(&credentials.client_id, audience)?;
            form.push(("client_id", &credentials.client_id));
            form.push(("client_assertion_type", JWT_BEARER_ASSERTION));
            form.push(("client_assertion", &assertion));
        }
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
        #[cfg(not(feature = "jwt"))]
        (Some(_), ClientAuth::Never(_)) => unreachable!("never constructed"),
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
        // An OAuth error body on a 4xx is the server's answer; anything else
        // (a 5xx, an HTML error page) is a failure to get one.
        return Err(match serde_json::from_slice::<TokenError>(&body) {
            Ok(e) if status.is_client_error() => OAuthClientError::TokenRejected {
                error: e.error,
                description: e.error_description.map(|d| format!("{what}: {d}")),
            },
            Ok(e) => OAuthClientError::TokenExchange(format!("{what}: {}", e.error)),
            Err(_) => OAuthClientError::TokenExchange(format!("{what}: http status {status}")),
        });
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
