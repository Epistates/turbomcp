//! Authorization with no user in the loop: a service calling an MCP server as
//! itself (the client credentials grant, RFC 6749 §4.4), or on behalf of the
//! user it is serving (token exchange, RFC 8693).
//!
//! Both start from the MCP server's own discovery (RFC 9728, then RFC 8414)
//! and ask for a token bound to it with the `resource` parameter (RFC 8707),
//! as the MCP authorization specification requires of every token request.
//! Every endpoint passes the same [`NetworkPolicy`](crate::NetworkPolicy)
//! and HTTPS checks as the authorization-code flow.
//!
//! Token exchange is what lets a gateway act for its caller without passing
//! the caller's token on, which the specification forbids ("MUST NOT pass
//! through the token it received from the MCP client"): the gateway presents
//! the caller's token to the authorization server as the `subject_token`,
//! and gets back a token issued for the upstream server. Whether that is
//! allowed for which callers, and with which scopes, is the authorization
//! server's policy.

use zeroize::Zeroizing;

use super::challenge::BearerChallenge;
use super::discovery::discover_with_policy;
use super::flow::Discovered;
use super::registration::ClientCredentials;
use super::token_endpoint::{ClientAuth, TokenResponse, post_form, require_grant};
use super::{OAuthClientError, TokenSet};

const CLIENT_CREDENTIALS_GRANT: &str = "client_credentials";
const TOKEN_EXCHANGE_GRANT: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
const ACCESS_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:access_token";

/// Machine-to-machine authorization for one MCP server. See
/// [the module docs](self).
#[derive(Clone, Debug)]
pub struct MachineAuthorization {
    resource: String,
    credentials: ClientCredentials,
    http: reqwest::Client,
    network: crate::NetworkPolicy,
}

impl MachineAuthorization {
    /// Authorization for the MCP server `resource`, as the confidential
    /// client `credentials` at its authorization server (both grants
    /// authenticate the client, so it needs a secret).
    ///
    /// Endpoints must be HTTPS on public addresses, as for
    /// [`OAuthClient`](super::OAuthClient); widen with
    /// [`with_network_policy`](Self::with_network_policy).
    ///
    /// # Panics
    /// If the TLS backend can't initialize, which the HTTP client needs.
    #[must_use]
    pub fn new(resource: impl Into<String>, credentials: ClientCredentials) -> Self {
        let network = crate::NetworkPolicy::public_only();
        Self {
            resource: resource.into(),
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

    /// The MCP server this authorizes for.
    #[must_use]
    pub fn resource(&self) -> &str {
        &self.resource
    }

    /// Discover the MCP server's authorization server (from the 401
    /// challenge's `resource_metadata` when given).
    ///
    /// # Errors
    /// Discovery failures.
    pub async fn discover(
        &self,
        challenge: Option<&BearerChallenge>,
    ) -> Result<Discovered, OAuthClientError> {
        discover_with_policy(&self.http, &self.resource, challenge, &self.network).await
    }

    /// A token for this client itself, with `scopes` (RFC 6749 §4.4).
    ///
    /// # Errors
    /// [`OAuthClientError::Discovery`] when the client has no secret or the
    /// authorization server doesn't offer the grant;
    /// [`OAuthClientError::TokenExchange`] when it refuses the request.
    pub async fn client_credentials(
        &self,
        discovered: &Discovered,
        scopes: &[String],
    ) -> Result<TokenSet, OAuthClientError> {
        self.require_secret(CLIENT_CREDENTIALS_GRANT)?;
        require_grant(&discovered.server, CLIENT_CREDENTIALS_GRANT)?;
        let scope = scopes.join(" ");
        let mut form = vec![
            ("grant_type", CLIENT_CREDENTIALS_GRANT),
            ("resource", discovered.resource.resource.as_str()),
        ];
        if !scope.is_empty() {
            form.push(("scope", &scope));
        }
        let response: TokenResponse = self.post_form(discovered, form).await?;
        Ok(response.into_token_set(scopes))
    }

    /// A token for the MCP server in exchange for `subject_token`, an access
    /// token the authorization server issued to a user this client serves,
    /// with `scopes` (RFC 8693).
    ///
    /// # Errors
    /// As [`client_credentials`](Self::client_credentials), and
    /// [`OAuthClientError::TokenExchange`] when the answer isn't an access
    /// token.
    pub async fn exchange(
        &self,
        discovered: &Discovered,
        subject_token: &Zeroizing<String>,
        scopes: &[String],
    ) -> Result<TokenSet, OAuthClientError> {
        self.require_secret(TOKEN_EXCHANGE_GRANT)?;
        require_grant(&discovered.server, TOKEN_EXCHANGE_GRANT)?;
        let scope = scopes.join(" ");
        let mut form = vec![
            ("grant_type", TOKEN_EXCHANGE_GRANT),
            ("resource", discovered.resource.resource.as_str()),
            ("requested_token_type", ACCESS_TOKEN_TYPE),
            ("subject_token", subject_token.as_str()),
            ("subject_token_type", ACCESS_TOKEN_TYPE),
        ];
        if !scope.is_empty() {
            form.push(("scope", &scope));
        }
        let response: TokenResponse = self.post_form(discovered, form).await?;
        // RFC 8693 §2.2.1: `issued_token_type` is REQUIRED. A token of
        // another type isn't one to present as a bearer.
        if response.issued_token_type.as_deref() != Some(ACCESS_TOKEN_TYPE) {
            return Err(OAuthClientError::TokenExchange(format!(
                "the token endpoint issued `{}`, not an access token",
                response
                    .issued_token_type
                    .as_deref()
                    .unwrap_or("an untyped token")
            )));
        }
        Ok(response.into_token_set(scopes))
    }

    fn require_secret(&self, grant: &str) -> Result<(), OAuthClientError> {
        if self.credentials.client_secret.is_none() {
            return Err(OAuthClientError::Discovery(format!(
                "the `{grant}` grant authenticates the client, and client `{}` has no secret",
                self.credentials.client_id
            )));
        }
        Ok(())
    }

    async fn post_form<T: serde::de::DeserializeOwned>(
        &self,
        discovered: &Discovered,
        form: Vec<(&str, &str)>,
    ) -> Result<T, OAuthClientError> {
        post_form(
            &self.http,
            &self.network,
            &discovered.server.token_endpoint,
            "the token endpoint",
            &self.credentials,
            ClientAuth::for_server(&discovered.server),
            form,
        )
        .await
    }
}
