//! Coordinated OAuth authorization and token refresh for
//! [`HttpClientTransport`](crate::HttpClientTransport), driven by the
//! server's challenges.
//!
//! Share one [`OAuthSession`] as the transport's bearer source. Consent is
//! application-owned; concurrent challenges and refreshes are serialized.
use std::{sync::Arc, time::Duration};
use turbomcp_auth::client::{
    BearerChallenge, CallbackParams, ClientCredentials, Discovered, EnterpriseAuthorization,
    IdentityAssertion, MachineAuthorization, OAuthClient, TokenSet, parse_bearer_challenge,
};
use zeroize::Zeroizing;

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

/// How close to expiry a token is replaced rather than presented.
const EXPIRY_SKEW: Duration = Duration::from_secs(30);

/// A grant that can be repeated with no user in the loop.
#[async_trait::async_trait]
trait Grant: Send + Sync + 'static {
    /// A fresh token set, for `challenge` when the server sent one, given
    /// the `previous` set (whose scopes a step-up widens).
    async fn grant(
        &self,
        challenge: Option<&BearerChallenge>,
        previous: Option<&TokenSet>,
    ) -> Result<TokenSet, String>;
}

/// A bearer source over a repeatable [`Grant`]: an expiring token is
/// replaced outright rather than refreshed, a `401` re-grants, and a
/// `403 insufficient_scope` re-grants with the scopes the server named.
struct Regranting<G> {
    grant: G,
    tokens: tokio::sync::Mutex<Option<TokenSet>>,
}

impl<G: Grant> Regranting<G> {
    fn new(grant: G) -> Self {
        Self {
            grant,
            tokens: tokio::sync::Mutex::new(None),
        }
    }

    async fn bearer(&self) -> Option<Zeroizing<String>> {
        let mut tokens = self.tokens.lock().await;
        let live = tokens
            .as_ref()
            .is_some_and(|t| !t.expires_within(EXPIRY_SKEW));
        if !live {
            match self.grant.grant(None, tokens.as_ref()).await {
                Ok(fresh) => *tokens = Some(fresh),
                Err(e) => {
                    tracing::warn!(error = %e, "could not obtain an access token");
                    *tokens = None;
                }
            }
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
        // Another request already replaced the rejected token, with every
        // scope this challenge names.
        if let Some(current) = tokens.as_ref()
            && Some(current.access_token.as_str()) != rejected
            && !current.expires_within(EXPIRY_SKEW)
            && challenge
                .as_ref()
                .is_none_or(|c| c.scopes().iter().all(|s| current.scopes.contains(s)))
        {
            return Ok(true);
        }
        let fresh = self
            .grant
            .grant(challenge.as_ref(), tokens.as_ref())
            .await?;
        *tokens = Some(fresh);
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
    inner: Regranting<EnterpriseGrant>,
}

struct EnterpriseGrant {
    engine: EnterpriseAuthorization,
    assertions: Arc<dyn AssertionSource>,
}

impl std::fmt::Debug for EnterpriseSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnterpriseSession")
            .field("resource", &self.inner.grant.engine.resource())
            .finish_non_exhaustive()
    }
}

impl EnterpriseSession {
    /// Authorize through `engine`, with assertions from `assertions`.
    #[must_use]
    pub fn new(engine: EnterpriseAuthorization, assertions: Arc<dyn AssertionSource>) -> Self {
        Self {
            inner: Regranting::new(EnterpriseGrant { engine, assertions }),
        }
    }
}

#[async_trait::async_trait]
impl Grant for EnterpriseGrant {
    async fn grant(
        &self,
        challenge: Option<&BearerChallenge>,
        _previous: Option<&TokenSet>,
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
    async fn bearer(&self) -> Option<Zeroizing<String>> {
        self.inner.bearer().await
    }

    async fn on_challenge(
        &self,
        status: u16,
        header: Option<&str>,
        rejected: Option<&str>,
    ) -> Result<bool, String> {
        self.inner.on_challenge(status, header, rejected).await
    }
}

/// Supplies the access token of the user a [`MachineSession`] acts for, to
/// exchange (RFC 8693). Consulted whenever a new upstream token is needed,
/// so the latest one the user presented is the one exchanged.
#[async_trait::async_trait]
pub trait SubjectSource: Send + Sync + 'static {
    /// The user's current access token, or `None` when there isn't one.
    async fn subject_token(&self) -> Option<Zeroizing<String>>;
}

/// Authorization with no user in the loop as the transport's bearer source:
/// a service calling the MCP server as itself (client credentials), or for
/// a user it serves (token exchange). Tokens are re-granted when they
/// expire, on a `401`, and with wider scopes on a `403 insufficient_scope`
/// (keeping the ones already granted). Attach with
/// `HttpClientTransport::with_bearer_source`.
pub struct MachineSession {
    inner: Regranting<MachineGrant>,
}

struct MachineGrant {
    engine: MachineAuthorization,
    subject: Option<Arc<dyn SubjectSource>>,
    scopes: Option<Vec<String>>,
    discovered: tokio::sync::Mutex<Option<Discovered>>,
}

impl std::fmt::Debug for MachineSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MachineSession")
            .field("resource", &self.inner.grant.engine.resource())
            .field("exchange", &self.inner.grant.subject.is_some())
            .finish_non_exhaustive()
    }
}

impl MachineSession {
    fn with(engine: MachineAuthorization, subject: Option<Arc<dyn SubjectSource>>) -> Self {
        Self {
            inner: Regranting::new(MachineGrant {
                engine,
                subject,
                scopes: None,
                discovered: tokio::sync::Mutex::new(None),
            }),
        }
    }

    /// Call as the client itself (RFC 6749 §4.4).
    #[must_use]
    pub fn client_credentials(engine: MachineAuthorization) -> Self {
        Self::with(engine, None)
    }

    /// Call for the user whose tokens `subject` supplies, by exchanging them
    /// (RFC 8693).
    #[must_use]
    pub fn token_exchange(engine: MachineAuthorization, subject: Arc<dyn SubjectSource>) -> Self {
        Self::with(engine, Some(subject))
    }

    /// Ask for `scopes` (default: what the server's challenge names, else
    /// the scopes its metadata advertises).
    #[must_use]
    pub fn with_scopes(mut self, scopes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.inner.grant.scopes = Some(scopes.into_iter().map(Into::into).collect());
        self
    }
}

#[async_trait::async_trait]
impl Grant for MachineGrant {
    async fn grant(
        &self,
        challenge: Option<&BearerChallenge>,
        previous: Option<&TokenSet>,
    ) -> Result<TokenSet, String> {
        let discovered = {
            let mut cached = self.discovered.lock().await;
            // A challenge naming its metadata is fresher word than ours.
            let rediscover =
                cached.is_none() || challenge.is_some_and(|c| c.resource_metadata.is_some());
            if rediscover {
                *cached = Some(
                    self.engine
                        .discover(challenge)
                        .await
                        .map_err(|e| e.to_string())?,
                );
            }
            cached.clone().ok_or("no authorization server discovered")?
        };
        let challenged = challenge.map(BearerChallenge::scopes).unwrap_or_default();
        let selected = match (&self.scopes, challenged.is_empty()) {
            (Some(configured), true) => configured.clone(),
            _ => OAuthClient::select_scopes(challenge, &discovered),
        };
        let scopes = match previous {
            Some(previous) => OAuthClient::step_up_scopes(&previous.scopes, &selected),
            None => selected,
        };
        let granted = match &self.subject {
            None => self.engine.client_credentials(&discovered, &scopes).await,
            Some(subject) => {
                let token = subject
                    .subject_token()
                    .await
                    .ok_or("no user token to exchange")?;
                self.engine.exchange(&discovered, &token, &scopes).await
            }
        };
        granted.map_err(|e| e.to_string())
    }
}

#[async_trait::async_trait]
impl crate::BearerSource for MachineSession {
    async fn bearer(&self) -> Option<Zeroizing<String>> {
        self.inner.bearer().await
    }

    async fn on_challenge(
        &self,
        status: u16,
        header: Option<&str>,
        rejected: Option<&str>,
    ) -> Result<bool, String> {
        self.inner.on_challenge(status, header, rejected).await
    }
}
