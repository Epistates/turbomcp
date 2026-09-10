//! Challenge-driven OAuth for [`crate::HttpClientTransport`].
//!
//! Share one [`OAuthSession`](crate::oauth::OAuthSession) as the transport's bearer source. Consent is
//! application-owned; concurrent challenges and refreshes are serialized.
use std::{sync::Arc, time::Duration};
use turbomcp_auth::client::{
    CallbackParams, ClientCredentials, Discovered, OAuthClient, TokenSet, parse_bearer_challenge,
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
    async fn bearer(&self) -> Option<String> {
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
