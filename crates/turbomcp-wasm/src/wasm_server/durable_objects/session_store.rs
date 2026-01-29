//! Durable Object-backed session store for Streamable HTTP transport.
//!
//! Provides session persistence that survives Worker restarts and enables
//! message replay for client reconnection.

use serde::{Deserialize, Serialize};
#[cfg(target_arch = "wasm32")]
use turbomcp_transport_streamable::SessionId;
#[cfg(target_arch = "wasm32")]
use turbomcp_transport_streamable::SessionStore;
use turbomcp_transport_streamable::{Session, StoredEvent};
use worker::Env;
#[cfg(target_arch = "wasm32")]
use worker::Stub;

/// Session store backed by Cloudflare Durable Objects.
///
/// Each session is stored in its own Durable Object instance, providing:
/// - Strong consistency for session state
/// - Automatic persistence across Worker restarts
/// - Event storage for message replay
///
/// # Setup
///
/// Configure the Durable Object binding in `wrangler.toml`:
///
/// ```toml
/// [[durable_objects.bindings]]
/// name = "MCP_SESSIONS"
/// class_name = "McpSessionObject"
///
/// [[durable_objects.classes]]
/// name = "McpSessionObject"
/// class_name = "McpSessionObject"
/// ```
///
/// # Example
///
/// ```rust,ignore
/// let sessions = env.durable_object("MCP_SESSIONS")?;
/// let store = DurableObjectSessionStore::new(sessions);
///
/// let handler = StreamableHandler::new(server)
///     .with_session_store(store);
/// ```
#[derive(Clone)]
#[allow(dead_code)] // namespace only used on WASM32
pub struct DurableObjectSessionStore {
    namespace: String,
    env: Option<Env>,
}

impl DurableObjectSessionStore {
    /// Create a new session store with the given DO namespace binding name.
    ///
    /// You'll need to call `with_env` before using the store.
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            env: None,
        }
    }

    /// Create a session store from an environment binding.
    pub fn from_env(env: &Env, binding: &str) -> worker::Result<Self> {
        // Validate the binding exists
        let _ = env.durable_object(binding)?;
        Ok(Self {
            namespace: binding.to_string(),
            env: Some(env.clone()),
        })
    }

    /// Set the environment for the store.
    pub fn with_env(mut self, env: Env) -> Self {
        self.env = Some(env);
        self
    }

    /// Get a stub for the session's Durable Object.
    #[cfg(target_arch = "wasm32")]
    fn get_stub(&self, session_id: &str) -> worker::Result<Stub> {
        let env = self
            .env
            .as_ref()
            .ok_or_else(|| worker::Error::RustError("No environment set".into()))?;

        let ns = env.durable_object(&self.namespace)?;
        let id = ns.id_from_name(session_id)?;
        id.get_stub()
    }

    /// Send a request to the Durable Object and parse the response.
    #[cfg(target_arch = "wasm32")]
    async fn do_request<T: for<'de> Deserialize<'de>>(
        &self,
        session_id: &str,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T, DoSessionError> {
        let stub = self.get_stub(session_id).map_err(DoSessionError::Worker)?;

        let mut init = worker::RequestInit::new();
        init.with_method(worker::Method::Post);

        if let Some(body) = body {
            let json = serde_json::to_string(body).map_err(DoSessionError::Serialization)?;
            init.with_body(Some(json.into()));
        }

        let url = format!("https://do-internal{path}");
        let request = worker::Request::new_with_init(&url, &init)?;
        let mut response = stub.fetch_with_request(request).await?;

        let text = response.text().await?;
        serde_json::from_str(&text).map_err(DoSessionError::Deserialization)
    }
}

/// Error type for Durable Object session operations.
#[derive(Debug)]
#[allow(dead_code)] // Some variants only used on WASM
pub enum DoSessionError {
    /// Worker/DO communication error
    Worker(worker::Error),
    /// Serialization error
    Serialization(serde_json::Error),
    /// Deserialization error
    Deserialization(serde_json::Error),
}

impl std::fmt::Display for DoSessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Worker(e) => write!(f, "Worker error: {e:?}"),
            Self::Serialization(e) => write!(f, "Serialization error: {e}"),
            Self::Deserialization(e) => write!(f, "Deserialization error: {e}"),
        }
    }
}

impl std::error::Error for DoSessionError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Worker(e) => Some(e),
            Self::Serialization(e) => Some(e),
            Self::Deserialization(e) => Some(e),
        }
    }
}

impl From<worker::Error> for DoSessionError {
    fn from(e: worker::Error) -> Self {
        Self::Worker(e)
    }
}

// ============================================================================
// SessionStore Implementation (WASM-only)
// ============================================================================

// The SessionStore trait requires MaybeSend futures. On native targets, MaybeSend
// requires Send, but worker crate types use Rc<RefCell> which is not Send.
// On WASM (single-threaded), this works because MaybeSend has no bounds.
#[cfg(target_arch = "wasm32")]
impl SessionStore for DurableObjectSessionStore {
    type Error = DoSessionError;

    async fn create(&self) -> Result<SessionId, Self::Error> {
        let id = SessionId::new();
        let session = Session::new(id.clone());

        // Store the new session
        self.do_request::<()>(id.as_str(), "/session/create", Some(&session))
            .await?;

        Ok(id)
    }

    async fn get(&self, id: &SessionId) -> Result<Option<Session>, Self::Error> {
        #[derive(Deserialize)]
        struct GetResponse {
            session: Option<Session>,
        }

        let response: GetResponse = self
            .do_request(id.as_str(), "/session/get", None::<&()>)
            .await?;

        Ok(response.session)
    }

    async fn update(&self, session: &Session) -> Result<(), Self::Error> {
        self.do_request::<()>(session.id.as_str(), "/session/update", Some(session))
            .await
    }

    async fn store_event(&self, id: &SessionId, event: StoredEvent) -> Result<(), Self::Error> {
        self.do_request::<()>(id.as_str(), "/event/store", Some(&event))
            .await
    }

    async fn replay_from(
        &self,
        id: &SessionId,
        last_event_id: &str,
    ) -> Result<Vec<StoredEvent>, Self::Error> {
        #[derive(Serialize)]
        struct ReplayRequest<'a> {
            last_event_id: &'a str,
        }

        #[derive(Deserialize)]
        struct ReplayResponse {
            events: Vec<StoredEvent>,
        }

        let request = ReplayRequest { last_event_id };
        let response: ReplayResponse = self
            .do_request(id.as_str(), "/event/replay", Some(&request))
            .await?;

        Ok(response.events)
    }

    async fn destroy(&self, id: &SessionId) -> Result<(), Self::Error> {
        self.do_request::<()>(id.as_str(), "/session/destroy", None::<&()>)
            .await
    }

    async fn cleanup_expired(&self, timeout_ms: u64) -> Result<u64, Self::Error> {
        // Durable Objects handle their own cleanup via alarm API
        // This is a no-op for DO-backed stores
        let _ = timeout_ms;
        Ok(0)
    }
}

// ============================================================================
// Durable Object Handler (to be implemented by user)
// ============================================================================

/// Request/response types for the session Durable Object.
///
/// Implement a Durable Object class that handles these routes:
///
/// - `POST /session/create` - Create a new session
/// - `POST /session/get` - Get session by ID
/// - `POST /session/update` - Update session
/// - `POST /session/destroy` - Destroy session
/// - `POST /event/store` - Store an event
/// - `POST /event/replay` - Replay events from a given ID
///
/// # Example Durable Object Implementation
///
/// ```rust,ignore
/// use worker::*;
/// use serde::{Deserialize, Serialize};
/// use turbomcp_transport_streamable::{Session, StoredEvent};
///
/// #[durable_object]
/// pub struct McpSessionObject {
///     state: State,
///     env: Env,
/// }
///
/// #[durable_object]
/// impl DurableObject for McpSessionObject {
///     fn new(state: State, env: Env) -> Self {
///         Self { state, env }
///     }
///
///     async fn fetch(&mut self, req: Request) -> Result<Response> {
///         let path = req.path();
///
///         match path.as_str() {
///             "/session/create" => {
///                 let session: Session = req.json().await?;
///                 self.state.storage().put("session", &session).await?;
///                 Response::ok("{}")
///             }
///             "/session/get" => {
///                 let session: Option<Session> = self.state.storage().get("session").await?;
///                 Response::from_json(&serde_json::json!({ "session": session }))
///             }
///             "/session/update" => {
///                 let session: Session = req.json().await?;
///                 self.state.storage().put("session", &session).await?;
///                 Response::ok("{}")
///             }
///             "/session/destroy" => {
///                 self.state.storage().delete_all().await?;
///                 Response::ok("{}")
///             }
///             "/event/store" => {
///                 let event: StoredEvent = req.json().await?;
///                 let key = format!("event:{}", event.id);
///                 self.state.storage().put(&key, &event).await?;
///                 Response::ok("{}")
///             }
///             "/event/replay" => {
///                 #[derive(Deserialize)]
///                 struct ReplayReq { last_event_id: String }
///                 let req: ReplayReq = req.json().await?;
///                 // List and filter events...
///                 Response::from_json(&serde_json::json!({ "events": [] }))
///             }
///             _ => Response::error("Not found", 404),
///         }
///     }
/// }
/// ```
/// Protocol types for implementing the Durable Object handler.
///
/// These types are used for documentation and should be implemented
/// by the user in their Durable Object class.
#[allow(dead_code)]
pub mod protocol {
    use super::*;

    /// Request to create a session.
    pub type CreateRequest = Session;

    /// Request to get a session.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct GetRequest;

    /// Response from get session.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct GetResponse {
        /// The session, if found
        pub session: Option<Session>,
    }

    /// Request to update a session.
    pub type UpdateRequest = Session;

    /// Request to store an event.
    pub type StoreEventRequest = StoredEvent;

    /// Request to replay events.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct ReplayRequest {
        /// Event ID to replay from
        pub last_event_id: String,
    }

    /// Response from replay events.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct ReplayResponse {
        /// Events that occurred after the given ID
        pub events: Vec<StoredEvent>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_store_creation() {
        let store = DurableObjectSessionStore::new("MCP_SESSIONS");
        assert_eq!(store.namespace, "MCP_SESSIONS");
        assert!(store.env.is_none());
    }

    #[test]
    fn test_do_session_error_display() {
        let err = DoSessionError::Serialization(serde_json::from_str::<()>("invalid").unwrap_err());
        assert!(err.to_string().contains("Serialization error"));
    }
}
