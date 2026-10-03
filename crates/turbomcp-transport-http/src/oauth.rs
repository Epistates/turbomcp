//! Coordinated OAuth authorization and token refresh for
//! [`HttpClientTransport`](crate::HttpClientTransport), driven by the
//! server's challenges.
//!
//! Share one [`OAuthSession`] as the transport's bearer source. Consent is
//! application-owned; concurrent challenges and refreshes are serialized.
use std::{sync::Arc, time::Duration};
use turbomcp_auth::client::{
    CallbackParams, ClientCredentials, Discovered, EnterpriseAuthorization, IdentityAssertion,
    OAuthClient, TokenSet, parse_bearer_challenge,
};

/// Opens the authorization URL and returns the validated application's callback.
/// The SDK subsequently checks OAuth state, issuer, and PKCE itself.
#[async_trait::async_trait]
pub trait AuthorizationHandler: Send + Sync + 'static {
    /// Obtain user consent. Never follow an arbitrary callback URL server-side.
    async fn authorize(&self, url: &str) -> Result<CallbackParams, String>;
}

struct Authorized {
    discovered: Discovered,
    credentials: ClientCredentials,
    tokens: TokenSet,
    refresh_failed: bool,
}

/// One resource's OAuth lifecycle. Attach with `HttpClientTransport::with_bearer_source`.
/// Token values and application callbacks are deliberately absent from Debug.
pub struct OAuthSession {
    engine: OAuthClient,
    handler: Arc<dyn AuthorizationHandler>,
    state: tokio::sync::Mutex<Option<Authorized>>,
}

impl OAuthSession {
    /// Build a coordinator using the engine's network policy and credential store.
    #[must_use]
    pub fn new(engine: OAuthClient, handler: Arc<dyn AuthorizationHandler>) -> Self {
        Self {
            engine,
            handler,
            state: tokio::sync::Mutex::new(None),
        }
    }
}

impl std::fmt::Debug for OAuthSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthSession")
            .field("resource", &self.engine.resource())
            .finish_non_exhaustive()
    }
}

#[async_trait::async_trait]
impl crate::BearerSource for OAuthSession {
    async fn bearer(&self) -> Option<zeroize::Zeroizing<String>> {
        let mut state = self.state.lock().await;
        let current = state.as_mut()?;
        if current.refresh_failed {
            return None;
        }
        if current.tokens.expires_within(Duration::from_secs(30)) {
            match self
                .engine
                .refresh(&current.discovered, &current.credentials, &current.tokens)
                .await
            {
                Ok(tokens) => current.tokens = tokens,
                // A rejected refresh requires a fresh challenge and user consent.
                Err(_) => {
                    // Waiting callers share the failure as well as success.
                    // The next resource challenge may authorize a new token set.
                    current.refresh_failed = true;
                    return None;
                }
            }
        }
        Some(current.tokens.access_token.clone())
    }

    async fn on_challenge(
        &self,
        status: u16,
        header: Option<&str>,
        rejected: Option<&str>,
    ) -> Result<bool, String> {
        if !matches!(status, 401 | 403) {
            return Ok(false);
        }
        let challenge = header.and_then(parse_bearer_challenge);
        if status == 403
            && challenge.as_ref().and_then(|c| c.error.as_deref()) != Some("insufficient_scope")
        {
            return Ok(false);
        }
        let mut state = self.state.lock().await;
        // Another request completed authorization while this request was in flight.
        if let Some(current) = state.as_ref()
            && Some(current.tokens.access_token.as_str()) != rejected
            && !current.tokens.expires_within(Duration::from_secs(30))
            && challenge
                .as_ref()
                .is_none_or(|c| c.scopes().iter().all(|s| current.tokens.scopes.contains(s)))
        {
            return Ok(true);
        }
        let discovered = self
            .engine
            .discover(challenge.as_ref())
            .await
            .map_err(|e| e.to_string())?;
        let credentials = self
            .engine
            .credentials(&discovered)
            .await
            .map_err(|e| e.to_string())?;
        let selected = OAuthClient::select_scopes(challenge.as_ref(), &discovered);
        let scopes = match state
            .as_ref()
            .filter(|s| s.discovered.server.issuer == discovered.server.issuer)
        {
            Some(current) => OAuthClient::step_up_scopes(&current.tokens.scopes, &selected),
            None => selected,
        };
        if state.is_none()
            && let Some(tokens) = self.engine.stored_tokens(&discovered).await
            && !tokens.expires_within(Duration::from_secs(30))
            && scopes.iter().all(|scope| tokens.scopes.contains(scope))
            && Some(tokens.access_token.as_str()) != rejected
        {
            *state = Some(Authorized {
                discovered,
                credentials,
                tokens,
                refresh_failed: false,
            });
            return Ok(true);
        }
        let pending = self
            .engine
            .begin(&discovered, &credentials, &scopes)
            .map_err(|e| e.to_string())?;
        let callback = self.handler.authorize(&pending.authorize_url).await?;
        let tokens = self
            .engine
            .complete(&discovered, &credentials, pending, &callback)
            .await
            .map_err(|e| e.to_string())?;
        *state = Some(Authorized {
            discovered,
            credentials,
            tokens,
            refresh_failed: false,
        });
        Ok(true)
    }
}

/// Supplies the user's identity assertion from SSO (an OpenID ID Token, or a
/// SAML assertion) for [`EnterpriseSession`]. Consulted whenever a new
/// access token is needed, so a re-login takes effect without rebuilding
/// the transport.
#[async_trait::async_trait]
pub trait AssertionSource: Send + Sync + 'static {
    /// The current assertion, or `None` when the user is not signed in.
    async fn identity_assertion(&self) -> Option<IdentityAssertion>;
}

/// Enterprise-Managed Authorization as the transport's bearer source: the
/// organization's IdP issues an ID-JAG for the MCP server, which the server's
/// authorization server exchanges for the access token presented here. No
/// browser, no consent screen: the IdP's policy decides. Attach with
/// `HttpClientTransport::with_bearer_source`, and declare the extension
/// ([`turbomcp_auth::client::enterprise::EXTENSION_ID`]) on the client.
pub struct EnterpriseSession {
    engine: EnterpriseAuthorization,
    assertions: Arc<dyn AssertionSource>,
    tokens: tokio::sync::Mutex<Option<TokenSet>>,
}

impl std::fmt::Debug for EnterpriseSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnterpriseSession")
            .field("resource", &self.engine.resource())
            .finish_non_exhaustive()
    }
}

impl EnterpriseSession {
    /// Authorize through `engine`, with assertions from `assertions`.
    #[must_use]
    pub fn new(engine: EnterpriseAuthorization, assertions: Arc<dyn AssertionSource>) -> Self {
        Self {
            engine,
            assertions,
            tokens: tokio::sync::Mutex::new(None),
        }
    }

    /// A fresh token set for `challenge` (or the server's defaults).
    async fn authorize(
        &self,
        challenge: Option<&turbomcp_auth::client::BearerChallenge>,
    ) -> Result<TokenSet, String> {
        let assertion = self
            .assertions
            .identity_assertion()
            .await
            .ok_or("no identity assertion: the user is not signed in")?;
        self.engine
            .authorize(&assertion, challenge)
            .await
            .map_err(|e| e.to_string())
    }
}

#[async_trait::async_trait]
impl crate::BearerSource for EnterpriseSession {
    async fn bearer(&self) -> Option<zeroize::Zeroizing<String>> {
        let mut tokens = self.tokens.lock().await;
        let live = tokens
            .as_ref()
            .is_some_and(|t| !t.expires_within(Duration::from_secs(30)));
        if !live {
            // An ID-JAG grant is cheap to repeat and needs no user, so an
            // expiring token is replaced outright rather than refreshed.
            *tokens = self.authorize(None).await.ok();
        }
        tokens.as_ref().map(|t| t.access_token.clone())
    }

    async fn on_challenge(
        &self,
        status: u16,
        header: Option<&str>,
        rejected: Option<&str>,
    ) -> Result<bool, String> {
        if !matches!(status, 401 | 403) {
            return Ok(false);
        }
        let challenge = header.and_then(parse_bearer_challenge);
        if status == 403
            && challenge.as_ref().and_then(|c| c.error.as_deref()) != Some("insufficient_scope")
        {
            return Ok(false);
        }
        let mut tokens = self.tokens.lock().await;
        // Another request already replaced the rejected token.
        if let Some(current) = tokens.as_ref()
            && Some(current.access_token.as_str()) != rejected
            && !current.expires_within(Duration::from_secs(30))
        {
            return Ok(true);
        }
        *tokens = Some(self.authorize(challenge.as_ref()).await?);
        Ok(true)
    }
}
