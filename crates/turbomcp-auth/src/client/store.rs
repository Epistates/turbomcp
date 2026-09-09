//! The issuer-keyed credential store (spec §Authorization Server Binding).
//!
//! Client identifiers are unique to the authorization server that issued
//! them: persisted credentials MUST be keyed by the AS's `issuer` identifier
//! and MUST NOT be reused when the resource's authorization server changes.
//! Tokens are bound the same way. The trait is async and dyn-compatible so
//! implementations can live in OS keychains, files, or databases; the
//! bundled [`MemoryCredentialStore`] holds them for the process lifetime.

use std::collections::HashMap;

use futures::future::BoxFuture;
use std::sync::Mutex;

use super::TokenSet;
use super::registration::ClientCredentials;

/// Per-issuer persistence for client credentials and token sets.
pub trait CredentialStore: Send + Sync {
    /// The credentials previously registered at `issuer`, if any.
    fn load_client<'a>(&'a self, issuer: &'a str) -> BoxFuture<'a, Option<ClientCredentials>>;

    /// Persist `credentials` as registered at `issuer`.
    fn store_client<'a>(
        &'a self,
        issuer: &'a str,
        credentials: &'a ClientCredentials,
    ) -> BoxFuture<'a, ()>;

    /// The token set previously issued by `issuer` for `resource`, if any.
    fn load_tokens<'a>(
        &'a self,
        issuer: &'a str,
        resource: &'a str,
    ) -> BoxFuture<'a, Option<TokenSet>>;

    /// Persist `tokens` as issued by `issuer` for `resource`.
    fn store_tokens<'a>(
        &'a self,
        issuer: &'a str,
        resource: &'a str,
        tokens: &'a TokenSet,
    ) -> BoxFuture<'a, ()>;

    /// Drop everything stored for `issuer` (e.g. after the resource's
    /// authorization server changed).
    fn clear<'a>(&'a self, issuer: &'a str) -> BoxFuture<'a, ()>;
}

/// In-memory, process-lifetime credential store — the default.
#[derive(Default)]
pub struct MemoryCredentialStore {
    clients: Mutex<HashMap<String, ClientCredentials>>,
    tokens: Mutex<HashMap<(String, String), TokenSet>>,
}

impl core::fmt::Debug for MemoryCredentialStore {
    /// Counts only. Every value in here is a credential — client secrets and
    /// access/refresh tokens — and the issuer keys say who the user talks to,
    /// so none of it belongs in a log line.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MemoryCredentialStore")
            .field(
                "clients",
                &self.clients.lock().map(|c| c.len()).unwrap_or(0),
            )
            .field("tokens", &self.tokens.lock().map(|t| t.len()).unwrap_or(0))
            .finish()
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn load_client<'a>(&'a self, issuer: &'a str) -> BoxFuture<'a, Option<ClientCredentials>> {
        Box::pin(async move {
            self.clients
                .lock()
                .expect("credential store lock poisoned")
                .get(issuer)
                .cloned()
        })
    }

    fn store_client<'a>(
        &'a self,
        issuer: &'a str,
        credentials: &'a ClientCredentials,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.clients
                .lock()
                .expect("credential store lock poisoned")
                .insert(issuer.to_owned(), credentials.clone());
        })
    }

    fn load_tokens<'a>(
        &'a self,
        issuer: &'a str,
        resource: &'a str,
    ) -> BoxFuture<'a, Option<TokenSet>> {
        Box::pin(async move {
            self.tokens
                .lock()
                .expect("credential store lock poisoned")
                .get(&(issuer.to_owned(), resource.to_owned()))
                .cloned()
        })
    }

    fn store_tokens<'a>(
        &'a self,
        issuer: &'a str,
        resource: &'a str,
        tokens: &'a TokenSet,
    ) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.tokens
                .lock()
                .expect("credential store lock poisoned")
                .insert((issuer.to_owned(), resource.to_owned()), tokens.clone());
        })
    }

    fn clear<'a>(&'a self, issuer: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            self.clients
                .lock()
                .expect("credential store lock poisoned")
                .remove(issuer);
            self.tokens
                .lock()
                .expect("credential store lock poisoned")
                .retain(|(iss, _), _| iss != issuer);
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Everything in this store is a credential. `Debug` reports counts, and a
    /// `#[derive(Debug)]` added here later would print access and refresh
    /// tokens into whatever log touched it.
    #[tokio::test]
    async fn debug_reports_counts_and_never_credentials() {
        let store = MemoryCredentialStore::default();
        store
            .store_tokens(
                "https://issuer.example",
                "https://mcp.example",
                &TokenSet {
                    access_token: "at-super-secret".into(),
                    refresh_token: Some("rt-super-secret".into()),
                    expires_at_epoch_secs: None,
                    scopes: vec!["mcp:read".into()],
                },
            )
            .await;

        let shown = format!("{store:?}");
        assert!(!shown.contains("at-super-secret"), "leaked access: {shown}");
        assert!(
            !shown.contains("rt-super-secret"),
            "leaked refresh: {shown}"
        );
        assert!(shown.contains("tokens: 1"), "reports the count: {shown}");
    }
}
