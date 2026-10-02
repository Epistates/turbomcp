//! Legacy (`2025-11-25`) session state — the stateful half of the dual-stack
//! server (PLAN §11, D9: sessions are a service-layer concern, not transport).
//!
//! The store itself is transport-agnostic: it maps an opaque session id to the
//! state negotiated at `initialize`. *Who mints the id* is the transport's
//! business (the HTTP runner derives it for the `Mcp-Session-Id` header; the
//! stdio [`LegacySessionAdapter`] mints one per connection) — the id reaches
//! the dispatcher as a [`SessionId`](turbomcp_core::SessionId) attached to the
//! request.
//!
//! The dispatcher reaches session state only through the [`SessionBackend`]
//! trait, so the storage is pluggable (`ServerBuilder::with_session_backend`).
//! [`SessionStore`] is the bundled in-memory backend and the default. A
//! backend outside the process (Redis, …) stores [`SessionState`], which
//! serializes, and builds it back with [`SessionState::new`].
//!
//! A shared store is not by itself enough to spread one `2025-11-25` session
//! across replicas: inline elicitation and sampling answers, cancellation and
//! progress are routed through the process that started the request, so the
//! legacy wire needs sticky routing on `Mcp-Session-Id`. `2026-07-28` has no
//! sessions (see `docs/DEPLOYMENT.md`).
//!
//! [`LegacySessionAdapter`]: crate::LegacySessionAdapter

use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use moka::notification::RemovalCause;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use turbomcp_core::{Implementation, LogLevel, ProtocolVersion};
use turbomcp_service::ProtocolError;

/// What `initialize` negotiated for one session.
///
/// Serializable, so a backend outside the process can store it and build it
/// back; construct one with [`new`](Self::new).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SessionState {
    /// Issuer-and-subject key of the creating principal. `None` binds this
    /// session to anonymous callers, never to the next authenticated caller.
    pub owner: Option<String>,
    /// The protocol version the server answered with.
    pub version: ProtocolVersion,
    /// The client's `clientInfo`.
    pub client_info: Implementation,
    /// The client's declared capabilities (kept as raw JSON; the dispatcher
    /// injects it into [`turbomcp_core::RequestContext::client_capabilities`]).
    pub client_capabilities: Value,
    /// The minimum severity the client opted into via `logging/setLevel`.
    /// `None` ⇒ no opt-in yet ⇒ this server sends no `notifications/message`
    /// (the spec leaves un-opted behavior to the server; we choose opt-in).
    pub log_level: Option<LogLevel>,
    /// When `initialize` created it, for the session's duration. Wall-clock,
    /// so a session a shared backend hands to another replica keeps it.
    #[serde(default = "SystemTime::now")]
    pub created_at: SystemTime,
}

impl SessionState {
    /// What `initialize` agreed on: an anonymous session at `version` with no
    /// log level chosen yet.
    #[must_use]
    pub fn new(
        version: ProtocolVersion,
        client_info: Implementation,
        client_capabilities: Value,
    ) -> Self {
        Self {
            owner: None,
            version,
            client_info,
            client_capabilities,
            log_level: None,
            created_at: SystemTime::now(),
        }
    }

    /// How long ago `initialize` created it.
    #[must_use]
    pub fn age(&self) -> Duration {
        self.created_at.elapsed().unwrap_or_default()
    }

    /// Bind the session to the principal that created it (an issuer-and-subject
    /// key; `None` is anonymous).
    #[must_use]
    pub fn with_owner(mut self, owner: Option<String>) -> Self {
        self.owner = owner;
        self
    }

    /// Set the `logging/setLevel` choice.
    #[must_use]
    pub fn with_log_level(mut self, level: Option<LogLevel>) -> Self {
        self.log_level = level;
        self
    }
}

/// A session a backend reclaimed for idling ([`SessionBackend::sweep_expired`]).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ExpiredSession {
    /// Its id.
    pub id: String,
    /// Its state as it expired, when the backend still has it.
    pub state: Option<Arc<SessionState>>,
}

impl ExpiredSession {
    /// Session `id` expired, with `state` if the backend kept it.
    #[must_use]
    pub fn new(id: impl Into<String>, state: Option<Arc<SessionState>>) -> Self {
        Self {
            id: id.into(),
            state,
        }
    }
}

/// A [`SessionBackend`] could not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SessionError {
    /// The store is unreachable or failing. Temporary; the request is answered
    /// `503`, not as an unknown session, so clients don't all re-`initialize`
    /// at once against a store that is already down.
    #[error("session store unavailable: {0}")]
    Unavailable(String),
    /// The store is full and will not take another session right now.
    #[error("session store at capacity ({0} sessions)")]
    AtCapacity(usize),
}

impl From<SessionError> for ProtocolError {
    fn from(e: SessionError) -> Self {
        ProtocolError::Unavailable(e.to_string())
    }
}

/// The bundled in-memory session table: bounded, with an idle timeout.
///
/// Built on [`moka`]: reads are lock-free, and each read refreshes the
/// session's idle clock. A full table **refuses** a new session
/// ([`SessionError::AtCapacity`], `503`) rather than evicting a live one: with
/// least-recently-used eviction, anyone able to call `initialize` (which is
/// unauthenticated unless you add auth) could flood the table and push out
/// every real session. Idle sessions expire and make room on their own.
///
/// Sessions that expire are reported by [`sweep_expired`](Self::sweep_expired),
/// so the dispatcher can tear down their subscription routes, `GET` streams
/// and tasks.
pub struct SessionStore {
    cache: moka::sync::Cache<String, Arc<SessionState>>,
    capacity: usize,
    idle_timeout: Option<Duration>,
    expired: Arc<Mutex<Vec<ExpiredSession>>>,
}

impl core::fmt::Debug for SessionStore {
    /// Counts, never ids: a session id is a bearer-equivalent handle that the
    /// transports spec requires be treated as a secret.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SessionStore")
            .field("capacity", &self.capacity)
            .field("idle_timeout", &self.idle_timeout)
            .field("live", &self.cache.entry_count())
            .finish()
    }
}

impl SessionStore {
    /// Default maximum number of live sessions.
    pub const DEFAULT_CAPACITY: usize = 16_384;

    /// Default idle timeout: a session unused this long is gone, and its client
    /// re-`initialize`s.
    pub const DEFAULT_IDLE_TIMEOUT: Duration = Duration::from_secs(60 * 60);

    /// A store bounded to `capacity` sessions, with the
    /// [default idle timeout](Self::DEFAULT_IDLE_TIMEOUT).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self::build(capacity.max(1), Some(Self::DEFAULT_IDLE_TIMEOUT))
    }

    /// Set the idle timeout: a session not used within `timeout` expires.
    /// `None` keeps sessions until they are deleted or the server restarts.
    #[must_use]
    pub fn with_idle_timeout(self, timeout: Option<Duration>) -> Self {
        Self::build(self.capacity, timeout)
    }

    fn build(capacity: usize, idle_timeout: Option<Duration>) -> Self {
        let expired: Arc<Mutex<Vec<ExpiredSession>>> = Arc::default();
        let reported = Arc::clone(&expired);
        let mut builder = moka::sync::Cache::builder()
            // A backstop: inserts are refused at capacity first, so this
            // only trims what concurrent `initialize`s squeeze past the check.
            .max_capacity(capacity as u64)
            .eviction_listener(move |id: Arc<String>, state, cause| {
                if matches!(cause, RemovalCause::Expired | RemovalCause::Size) {
                    reported
                        .lock()
                        .expect("expired-session list poisoned")
                        .push(ExpiredSession::new(id.as_ref().clone(), Some(state)));
                }
            });
        if let Some(timeout) = idle_timeout {
            builder = builder.time_to_idle(timeout);
        }
        Self {
            cache: builder.build(),
            capacity,
            idle_timeout,
            expired,
        }
    }

    /// Store (or replace) `state` under `id`.
    ///
    /// # Errors
    /// [`SessionError::AtCapacity`] when the table is full and `id` is new.
    pub fn insert(&self, id: impl Into<String>, state: SessionState) -> Result<(), SessionError> {
        let id = id.into();
        if !self.cache.contains_key(&id) {
            // The count is maintained lazily; settle it before reading it. A
            // new session is minted once per `initialize`, so this is not on
            // the per-request path.
            self.cache.run_pending_tasks();
            if self.cache.entry_count() >= self.capacity as u64 {
                return Err(SessionError::AtCapacity(self.capacity));
            }
        }
        self.cache.insert(id, Arc::new(state));
        Ok(())
    }

    /// Look up a session, refreshing its idle clock. `None` means expired,
    /// deleted, or never created; the caller answers "unknown session".
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Arc<SessionState>> {
        self.cache.get(id)
    }

    /// Every session that has expired since the last call. The dispatcher
    /// calls this where sessions are minted and tears down each one's routes.
    ///
    /// # Panics
    /// If another thread panicked while recording an expiry.
    #[must_use]
    pub fn sweep_expired(&self) -> Vec<ExpiredSession> {
        self.cache.run_pending_tasks();
        std::mem::take(&mut *self.expired.lock().expect("expired-session list poisoned"))
    }

    /// Whether `id` is a live session (does not refresh it).
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.cache.contains_key(id)
    }

    /// Record the session's `logging/setLevel` choice. Returns whether the
    /// session exists.
    pub fn set_log_level(&self, id: &str, level: LogLevel) -> bool {
        let Some(state) = self.cache.get(id) else {
            return false;
        };
        let mut state = SessionState::clone(&state);
        state.log_level = Some(level);
        self.cache.insert(id.to_owned(), Arc::new(state));
        true
    }

    /// Terminate a session, returning it if it existed.
    pub fn remove(&self, id: &str) -> Option<Arc<SessionState>> {
        self.cache.remove(id)
    }

    /// Number of live sessions (settles pending maintenance first).
    #[must_use]
    pub fn len(&self) -> usize {
        self.cache.run_pending_tasks();
        usize::try_from(self.cache.entry_count()).unwrap_or(usize::MAX)
    }

    /// Whether no sessions are live.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }
}

/// Pluggable storage for legacy (`2025-06-18` / `2025-11-25`) session state.
///
/// The dispatcher only ever touches sessions through this trait, so the state
/// can live anywhere: the bundled [`SessionStore`] keeps it in process memory;
/// a shared backend keeps sessions across restarts and lets a replica that
/// receives a session's request (with sticky routing, see the module docs)
/// find it. Register one with `ServerBuilder::with_session_backend`.
///
/// Contract notes for implementors:
/// - [`get`](Self::get) refreshes the session's idle clock (it gates every
///   legacy request). `Ok(None)` means expired, deleted, or never created: the
///   client is answered "unknown session" and re-`initialize`s.
/// - `Err` means the store itself is failing, and is answered `503`, which
///   clients retry. Never report an outage as `Ok(None)`: that sends every
///   client to re-`initialize` at once.
/// - Eviction policy (capacity, TTL) belongs to the backend.
///   [`sweep_expired`](Self::sweep_expired) reports reclaimed ids so the
///   dispatcher can tear down their routes; a backend that expires internally
///   (e.g. Redis TTLs) may return only what it can enumerate, or nothing.
#[async_trait]
pub trait SessionBackend: Send + Sync {
    /// Store (or replace) `state` under `id`. Refusing a new session (at
    /// capacity) is an `Err`, which answers that `initialize` with `503`.
    async fn insert(&self, id: &str, state: SessionState) -> Result<(), SessionError>;

    /// Look up a session, refreshing its idle clock.
    async fn get(&self, id: &str) -> Result<Option<Arc<SessionState>>, SessionError>;

    /// Record the session's `logging/setLevel` choice. Returns whether the
    /// session exists.
    async fn set_log_level(&self, id: &str, level: LogLevel) -> Result<bool, SessionError>;

    /// Terminate a session, returning its state if it existed (`Some` of a
    /// backend that can't read it back on removal is fine too, as long as it
    /// says the session existed).
    async fn remove(&self, id: &str) -> Result<Option<Arc<SessionState>>, SessionError>;

    /// Reclaim expired sessions (the dispatcher tears down each one's routes
    /// and reports how long it lived, when the state comes back with it).
    async fn sweep_expired(&self) -> Result<Vec<ExpiredSession>, SessionError>;
}

#[async_trait]
impl SessionBackend for SessionStore {
    async fn insert(&self, id: &str, state: SessionState) -> Result<(), SessionError> {
        SessionStore::insert(self, id, state)
    }

    async fn get(&self, id: &str) -> Result<Option<Arc<SessionState>>, SessionError> {
        Ok(SessionStore::get(self, id))
    }

    async fn set_log_level(&self, id: &str, level: LogLevel) -> Result<bool, SessionError> {
        Ok(SessionStore::set_log_level(self, id, level))
    }

    async fn remove(&self, id: &str) -> Result<Option<Arc<SessionState>>, SessionError> {
        Ok(SessionStore::remove(self, id))
    }

    async fn sweep_expired(&self) -> Result<Vec<ExpiredSession>, SessionError> {
        Ok(SessionStore::sweep_expired(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SessionState {
        SessionState::new(
            ProtocolVersion::V2025_11_25,
            Implementation::new("test-client", "1.0"),
            serde_json::json!({}),
        )
    }

    #[test]
    fn insert_get_remove_round_trip() {
        let store = SessionStore::default();
        store.insert("a", state()).unwrap();
        assert!(store.contains("a"));
        assert_eq!(store.get("a").unwrap().client_info.name, "test-client");
        assert!(store.remove("a").is_some());
        assert!(store.get("a").is_none());
        assert!(store.remove("a").is_none());
    }

    /// A full table refuses a newcomer rather than evicting a live session:
    /// with LRU eviction, an `initialize` flood pushed out every real
    /// session.
    #[test]
    fn a_full_store_refuses_rather_than_evicts() {
        let store = SessionStore::with_capacity(2);
        store.insert("a", state()).unwrap();
        store.insert("b", state()).unwrap();
        assert_eq!(store.insert("c", state()), Err(SessionError::AtCapacity(2)));
        assert!(store.contains("a") && store.contains("b"));
        // Replacing an existing one is fine at capacity.
        store.insert("a", state()).unwrap();
        // And a deletion makes room.
        assert!(store.remove("b").is_some());
        store.insert("c", state()).unwrap();
    }

    #[test]
    fn idle_sessions_expire_and_are_reported_once() {
        // Margins wide enough for a slow CI runner: the fresh session must
        // not expire before it is checked, the old ones must.
        let store =
            SessionStore::with_capacity(8).with_idle_timeout(Some(Duration::from_millis(500)));
        store.insert("a", state()).unwrap();
        store.insert("b", state()).unwrap();
        std::thread::sleep(Duration::from_millis(800));
        store.insert("c", state()).unwrap();
        assert!(store.get("a").is_none(), "idle past the timeout → gone");
        let mut swept: Vec<String> = store
            .sweep_expired()
            .into_iter()
            .map(|expired| {
                assert!(expired.state.is_some(), "the state comes back with it");
                expired.id
            })
            .collect();
        swept.sort();
        assert_eq!(swept, vec!["a".to_owned(), "b".to_owned()]);
        assert!(store.contains("c"));
        assert!(store.sweep_expired().is_empty(), "reported once");
    }

    #[test]
    fn use_keeps_a_session_alive() {
        // Each gap is well inside the timeout, and together they are well
        // past it: only the reads can have kept it alive.
        let store = SessionStore::with_capacity(8).with_idle_timeout(Some(Duration::from_secs(1)));
        store.insert("a", state()).unwrap();
        for _ in 0..5 {
            std::thread::sleep(Duration::from_millis(300));
            assert!(store.get("a").is_some());
        }
    }

    #[test]
    fn no_idle_timeout_keeps_sessions() {
        let store = SessionStore::with_capacity(8).with_idle_timeout(None);
        store.insert("a", state()).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        assert!(store.sweep_expired().is_empty());
        assert!(store.contains("a"));
    }

    #[test]
    fn set_log_level_updates_the_stored_state() {
        let store = SessionStore::default();
        store.insert("a", state()).unwrap();
        assert!(store.set_log_level("a", LogLevel::Warning));
        assert_eq!(store.get("a").unwrap().log_level, Some(LogLevel::Warning));
        assert!(!store.set_log_level("missing", LogLevel::Warning));
    }

    /// An external backend stores the state as bytes and builds it back.
    #[test]
    fn session_state_round_trips_through_json() {
        let original = state()
            .with_owner(Some("https://issuer#alice".into()))
            .with_log_level(Some(LogLevel::Info));
        let bytes = serde_json::to_vec(&original).unwrap();
        let back: SessionState = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back, original);
    }
}
