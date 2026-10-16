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
    /// The authorization server said the refresh token is dead
    /// (`invalid_grant`): only a new authorization replaces this token set.
    refresh_dead: bool,
}

/// How many times one scope set may send the user to authorize within
/// [`PROMPT_WINDOW`]: "Clients SHOULD implement retry limits and SHOULD track
/// scope upgrade attempts to avoid repeated failures".
const MAX_PROMPTS: u32 = 3;

/// The window [`MAX_PROMPTS`] counts within.
const PROMPT_WINDOW: Duration = Duration::from_secs(10 * 60);

/// One resource's OAuth lifecycle. Attach with `HttpClientTransport::with_bearer_source`.
/// Token values and application callbacks are deliberately absent from Debug.
pub struct OAuthSession {
    engine: OAuthClient,
    handler: Arc<dyn AuthorizationHandler>,
    state: tokio::sync::Mutex<Option<Authorized>>,
    /// Held across an interactive authorization, so concurrent challenges
    /// send the user to the browser once; `state` is not, so requests keep
    /// their bearer while the user decides.
    interactive: tokio::sync::Mutex<()>,
    /// The longest an interactive authorization may take.
    authorization_timeout: Duration,
    /// Prompts per scope set: how many, and since when.
    prompts: parking_lot::Mutex<std::collections::HashMap<Vec<String>, (u32, std::time::Instant)>>,
}

impl OAuthSession {
    /// Build a coordinator using the engine's network policy and credential store.
    #[must_use]
    pub fn new(engine: OAuthClient, handler: Arc<dyn AuthorizationHandler>) -> Self {
        Self {
            engine,
            handler,
            state: tokio::sync::Mutex::new(None),
            interactive: tokio::sync::Mutex::new(()),
            authorization_timeout: Duration::from_secs(5 * 60),
            prompts: parking_lot::Mutex::default(),
        }
    }

    /// The longest the application's [`AuthorizationHandler`] may take to
    /// return the user's callback (default 5 minutes). Past it the request
    /// that needed authorization fails, rather than waiting on a browser tab
    /// the user closed.
    #[must_use]
    pub fn authorization_timeout(mut self, timeout: Duration) -> Self {
        self.authorization_timeout = timeout;
        self
    }

    /// Redeem `current`'s refresh token. `Ok(false)` when there is none to
    /// use or the server said it is dead; `Err` for a failure that may pass.
    async fn try_refresh(&self, current: &mut Authorized) -> Result<bool, String> {
        if current.refresh_dead || current.tokens.refresh_token.is_none() {
            return Ok(false);
        }
        match self
            .engine
            .refresh(&current.discovered, &current.credentials, &current.tokens)
            .await
        {
            Ok(tokens) => {
                current.tokens = tokens;
                Ok(true)
            }
            // A definitive answer: only the user can replace this grant.
            Err(e) if e.is_invalid_grant() || e.is_invalid_client() => {
                current.refresh_dead = true;
                Ok(false)
            }
            // The network, the server's own error: the refresh token is
            // still good, so it is kept for the next try.
            Err(e) => Err(e.to_string()),
        }
    }

    /// Count a prompt for `scopes`, refusing past [`MAX_PROMPTS`].
    fn count_prompt(&self, scopes: &[String]) -> Result<(), String> {
        let mut key = scopes.to_vec();
        key.sort();
        let mut prompts = self.prompts.lock();
        let now = std::time::Instant::now();
        let (count, since) = prompts.entry(key).or_insert((0, now));
        if now.duration_since(*since) > PROMPT_WINDOW {
            *count = 0;
            *since = now;
        }
        if *count >= MAX_PROMPTS {
            return Err(format!(
                "authorization for [{}] already failed {MAX_PROMPTS} times; not asking again yet",
                scopes.join(" ")
            ));
        }
        *count += 1;
        Ok(())
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
        if current.tokens.expires_within(EXPIRY_SKEW) {
            match self.try_refresh(current).await {
                Ok(true) => {}
                // Nothing to refresh with: the token goes until the server
                // refuses it, and that challenge starts a new authorization.
                Ok(false) if current.refresh_dead => return None,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(%error, "token refresh failed; will retry");
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
        let step_up = challenge
            .as_ref()
            .is_some_and(BearerChallenge::is_insufficient_scope);
        if status == 403 && !step_up {
            return Ok(false);
        }
        let covered = |tokens: &TokenSet| {
            challenge
                .as_ref()
                .is_none_or(|c| c.scopes().iter().all(|s| tokens.scopes.contains(s)))
        };
        {
            let mut state = self.state.lock().await;
            if let Some(current) = state.as_mut() {
                // Another request replaced the token while this one was in
                // flight.
                if Some(current.tokens.access_token.as_str()) != rejected
                    && !current.tokens.expires_within(EXPIRY_SKEW)
                    && covered(&current.tokens)
                {
                    return Ok(true);
                }
                // A `401` for the token we hold: the refresh token comes
                // before the user (an expiry the server noticed first, a
                // token issued without `expires_in`).
                if !step_up && self.try_refresh(current).await? {
                    return Ok(true);
                }
            }
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
        let selected = OAuthClient::with_offline_access(
            OAuthClient::select_scopes(challenge.as_ref(), &discovered),
            &discovered,
        );
        let previous = {
            let state = self.state.lock().await;
            state
                .as_ref()
                .filter(|s| s.discovered.server.issuer == discovered.server.issuer)
                .map(|s| s.tokens.scopes.clone())
        };
        // What this operation asked for, before the union with what was
        // already granted: the unit prompts are counted in.
        let asked = selected.clone();
        let scopes = match &previous {
            Some(previous) => OAuthClient::step_up_scopes(previous, &selected),
            None => selected,
        };

        // A durable store may hold a token set from an earlier run: live, it
        // is used; expired with a refresh token, it is refreshed. Either beats
        // sending the user to the browser.
        if previous.is_none()
            && let Some(tokens) = self.engine.stored_tokens(&discovered).await
            && scopes.iter().all(|scope| tokens.scopes.contains(scope))
            && Some(tokens.access_token.as_str()) != rejected
        {
            let mut restored = Authorized {
                discovered: discovered.clone(),
                credentials: credentials.clone(),
                tokens,
                refresh_dead: false,
            };
            let usable = !restored.tokens.expires_within(EXPIRY_SKEW)
                || self.try_refresh(&mut restored).await.unwrap_or(false);
            if usable {
                *self.state.lock().await = Some(restored);
                return Ok(true);
            }
        }

        // The user, once, however many requests were refused.
        let _prompting = self.interactive.lock().await;
        // Another request finished an authorization while this one waited.
        if let Some(current) = self.state.lock().await.as_ref()
            && Some(current.tokens.access_token.as_str()) != rejected
            && !current.tokens.expires_within(EXPIRY_SKEW)
            && covered(&current.tokens)
        {
            return Ok(true);
        }
        self.count_prompt(&asked)?;
        let pending = self
            .engine
            .begin(&discovered, &credentials, &scopes)
            .map_err(|e| e.to_string())?;
        let callback = tokio::time::timeout(
            self.authorization_timeout,
            self.handler.authorize(&pending.authorize_url),
        )
        .await
        .map_err(|_| {
            format!(
                "authorization was not completed within {:?}",
                self.authorization_timeout
            )
        })??;
        let tokens = self
            .engine
            .complete(&discovered, &credentials, pending, &callback)
            .await
            .map_err(|e| e.to_string())?;
        *self.state.lock().await = Some(Authorized {
            discovered,
            credentials,
            tokens,
            refresh_dead: false,
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

    /// A fingerprint of what the grant is made from (the user token it
    /// exchanges, the assertion it presents), when it is made from something
    /// that can change. A token granted from an older one is replaced: the
    /// newest the user presented governs, so one narrowed or replaced
    /// downstream doesn't keep working upstream until the grant expires.
    async fn basis(&self) -> Option<u64> {
        None
    }
}

/// An in-process fingerprint of a credential, to tell when it changed.
/// Never stored or sent anywhere.
fn fingerprint(secret: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    secret.hash(&mut hasher);
    hasher.finish()
}

/// A bearer source over a repeatable [`Grant`]: an expiring token is
/// replaced outright rather than refreshed, a `401` re-grants, and a
/// `403 insufficient_scope` re-grants with the scopes the server named.
struct Regranting<G> {
    grant: G,
    /// The token set, and the [basis](Grant::basis) it was granted from.
    tokens: tokio::sync::Mutex<Option<(TokenSet, Option<u64>)>>,
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
        let basis = self.grant.basis().await;
        let live = tokens
            .as_ref()
            .is_some_and(|(t, from)| !t.expires_within(EXPIRY_SKEW) && *from == basis);
        if !live {
            let previous = tokens.as_ref().map(|(t, _)| t);
            match self.grant.grant(None, previous).await {
                Ok(fresh) => *tokens = Some((fresh, basis)),
                Err(e) => {
                    tracing::warn!(error = %e, "could not obtain an access token");
                    *tokens = None;
                }
            }
        }
        tokens.as_ref().map(|(t, _)| t.access_token.clone())
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
        if let Some((current, _)) = tokens.as_ref()
            && Some(current.access_token.as_str()) != rejected
            && !current.expires_within(EXPIRY_SKEW)
            && challenge
                .as_ref()
                .is_none_or(|c| c.scopes().iter().all(|s| current.scopes.contains(s)))
        {
            return Ok(true);
        }
        let basis = self.grant.basis().await;
        let fresh = self
            .grant
            .grant(challenge.as_ref(), tokens.as_ref().map(|(t, _)| t))
            .await?;
        *tokens = Some((fresh, basis));
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

    async fn basis(&self) -> Option<u64> {
        let assertion = self.assertions.identity_assertion().await?;
        Some(fingerprint(assertion.secret()))
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

    /// The user's token, when this exchanges one: a token the user replaced
    /// (a narrower one, a re-login) is exchanged anew.
    async fn basis(&self) -> Option<u64> {
        let token = self.subject.as_ref()?.subject_token().await?;
        Some(fingerprint(token.as_str()))
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
