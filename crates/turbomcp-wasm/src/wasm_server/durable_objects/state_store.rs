//! Durable Object-backed state store for per-user/conversation persistent state.
//!
//! Provides strongly consistent state storage using Cloudflare Durable Objects.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use worker::Env;

/// State store backed by Cloudflare Durable Objects.
///
/// Each namespace (user, conversation, etc.) gets its own Durable Object
/// instance for strong consistency and isolation.
///
/// # Setup
///
/// Configure the Durable Object binding in `wrangler.toml`:
///
/// ```toml
/// [[durable_objects.bindings]]
/// name = "MCP_STATE"
/// class_name = "McpStateObject"
///
/// [[durable_objects.classes]]
/// name = "McpStateObject"
/// class_name = "McpStateObject"
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_wasm::wasm_server::durable_objects::DurableObjectStateStore;
///
/// let store = DurableObjectStateStore::from_env(&env, "MCP_STATE")?;
///
/// // Store conversation history
/// store.set("user:alice", "history", &messages).await?;
///
/// // Retrieve later
/// let history: Vec<Message> = store.get("user:alice", "history").await?.unwrap_or_default();
///
/// // Atomic update
/// store.transaction("user:alice", |state| {
///     let mut count: i32 = state.get("count")?.unwrap_or(0);
///     count += 1;
///     state.set("count", &count)?;
///     Ok(())
/// }).await?;
/// ```
#[derive(Clone)]
pub struct DurableObjectStateStore {
    namespace: String,
    env: Option<Env>,
}

impl DurableObjectStateStore {
    /// Create a new state store with the given DO namespace binding name.
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            env: None,
        }
    }

    /// Create a state store from an environment binding.
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

    /// Get a value from the state store.
    ///
    /// # Arguments
    ///
    /// * `namespace_id` - The namespace/user/conversation ID
    /// * `key` - The key within that namespace
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let prefs: Option<UserPrefs> = store.get("user:alice", "preferences").await?;
    /// ```
    pub async fn get<T: DeserializeOwned>(
        &self,
        namespace_id: &str,
        key: &str,
    ) -> Result<Option<T>, StateStoreError> {
        #[derive(Serialize)]
        struct GetRequest<'a> {
            key: &'a str,
        }

        #[derive(Deserialize)]
        struct GetResponse<T> {
            value: Option<T>,
        }

        let request = GetRequest { key };
        let response: GetResponse<T> = self
            .do_request(namespace_id, "/state/get", Some(&request))
            .await?;

        Ok(response.value)
    }

    /// Set a value in the state store.
    ///
    /// # Arguments
    ///
    /// * `namespace_id` - The namespace/user/conversation ID
    /// * `key` - The key within that namespace
    /// * `value` - The value to store (must be serializable)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// store.set("user:alice", "preferences", &prefs).await?;
    /// ```
    pub async fn set<T: Serialize>(
        &self,
        namespace_id: &str,
        key: &str,
        value: &T,
    ) -> Result<(), StateStoreError> {
        #[derive(Serialize)]
        struct SetRequest<'a, T> {
            key: &'a str,
            value: &'a T,
        }

        let request = SetRequest { key, value };
        self.do_request::<()>(namespace_id, "/state/set", Some(&request))
            .await
    }

    /// Delete a value from the state store.
    ///
    /// # Arguments
    ///
    /// * `namespace_id` - The namespace/user/conversation ID
    /// * `key` - The key to delete
    ///
    /// # Returns
    ///
    /// `true` if a value was deleted, `false` if the key didn't exist.
    pub async fn delete(&self, namespace_id: &str, key: &str) -> Result<bool, StateStoreError> {
        #[derive(Serialize)]
        struct DeleteRequest<'a> {
            key: &'a str,
        }

        #[derive(Deserialize)]
        struct DeleteResponse {
            deleted: bool,
        }

        let request = DeleteRequest { key };
        let response: DeleteResponse = self
            .do_request(namespace_id, "/state/delete", Some(&request))
            .await?;

        Ok(response.deleted)
    }

    /// List all keys in a namespace.
    ///
    /// # Arguments
    ///
    /// * `namespace_id` - The namespace/user/conversation ID
    /// * `prefix` - Optional prefix to filter keys
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// let keys = store.list("user:alice", Some("chat:")).await?;
    /// ```
    pub async fn list(
        &self,
        namespace_id: &str,
        prefix: Option<&str>,
    ) -> Result<Vec<String>, StateStoreError> {
        #[derive(Serialize)]
        struct ListRequest<'a> {
            prefix: Option<&'a str>,
        }

        #[derive(Deserialize)]
        struct ListResponse {
            keys: Vec<String>,
        }

        let request = ListRequest { prefix };
        let response: ListResponse = self
            .do_request(namespace_id, "/state/list", Some(&request))
            .await?;

        Ok(response.keys)
    }

    /// Clear all state for a namespace.
    ///
    /// **Warning**: This deletes ALL data for the given namespace.
    pub async fn clear(&self, namespace_id: &str) -> Result<(), StateStoreError> {
        self.do_request::<()>(namespace_id, "/state/clear", None::<&()>)
            .await
    }

    /// Send a request to the Durable Object.
    async fn do_request<T: for<'de> Deserialize<'de>>(
        &self,
        namespace_id: &str,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T, StateStoreError> {
        let env = self.env.as_ref().ok_or(StateStoreError::NoEnvironment)?;

        let ns = env
            .durable_object(&self.namespace)
            .map_err(StateStoreError::Worker)?;
        let id = ns
            .id_from_name(namespace_id)
            .map_err(StateStoreError::Worker)?;
        let stub = id.get_stub().map_err(StateStoreError::Worker)?;

        let mut init = worker::RequestInit::new();
        init.with_method(worker::Method::Post);

        if let Some(body) = body {
            let json = serde_json::to_string(body).map_err(StateStoreError::Serialization)?;
            init.with_body(Some(json.into()));
        }

        let url = format!("https://do-internal{path}");
        let request =
            worker::Request::new_with_init(&url, &init).map_err(StateStoreError::Worker)?;
        let mut response = stub
            .fetch_with_request(request)
            .await
            .map_err(StateStoreError::Worker)?;

        let text = response.text().await.map_err(StateStoreError::Worker)?;
        serde_json::from_str(&text).map_err(StateStoreError::Deserialization)
    }
}

/// Error type for state store operations.
#[derive(Debug)]
pub enum StateStoreError {
    /// No environment has been set
    NoEnvironment,
    /// Worker/DO communication error
    Worker(worker::Error),
    /// Serialization error
    Serialization(serde_json::Error),
    /// Deserialization error
    Deserialization(serde_json::Error),
}

impl std::fmt::Display for StateStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEnvironment => write!(f, "No environment set"),
            Self::Worker(e) => write!(f, "Worker error: {e:?}"),
            Self::Serialization(e) => write!(f, "Serialization error: {e}"),
            Self::Deserialization(e) => write!(f, "Deserialization error: {e}"),
        }
    }
}

impl std::error::Error for StateStoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Worker(e) => Some(e),
            Self::Serialization(e) => Some(e),
            Self::Deserialization(e) => Some(e),
            Self::NoEnvironment => None,
        }
    }
}

impl From<worker::Error> for StateStoreError {
    fn from(e: worker::Error) -> Self {
        Self::Worker(e)
    }
}

// ============================================================================
// Protocol Types
// ============================================================================

/// Request/response types for the state Durable Object.
///
/// Implement a Durable Object class that handles these routes:
///
/// - `POST /state/get` - Get a value by key
/// - `POST /state/set` - Set a value by key
/// - `POST /state/delete` - Delete a value by key
/// - `POST /state/list` - List keys with optional prefix
/// - `POST /state/clear` - Clear all state
///
/// # Example Durable Object Implementation
///
/// ```rust,ignore
/// #[durable_object]
/// pub struct McpStateObject {
///     state: State,
/// }
///
/// #[durable_object]
/// impl DurableObject for McpStateObject {
///     fn new(state: State, _env: Env) -> Self {
///         Self { state }
///     }
///
///     async fn fetch(&mut self, req: Request) -> Result<Response> {
///         match req.path().as_str() {
///             "/state/get" => {
///                 #[derive(Deserialize)]
///                 struct Req { key: String }
///                 let req: Req = req.json().await?;
///                 let value: Option<JsValue> = self.state.storage().get(&req.key).await?;
///                 Response::from_json(&json!({ "value": value }))
///             }
///             "/state/set" => {
///                 #[derive(Deserialize)]
///                 struct Req { key: String, value: JsValue }
///                 let req: Req = req.json().await?;
///                 self.state.storage().put(&req.key, &req.value).await?;
///                 Response::ok("{}")
///             }
///             "/state/delete" => {
///                 #[derive(Deserialize)]
///                 struct Req { key: String }
///                 let req: Req = req.json().await?;
///                 let deleted = self.state.storage().delete(&req.key).await?;
///                 Response::from_json(&json!({ "deleted": deleted }))
///             }
///             "/state/list" => {
///                 #[derive(Deserialize)]
///                 struct Req { prefix: Option<String> }
///                 let req: Req = req.json().await?;
///                 let keys = self.state.storage()
///                     .list_with_options(ListOptions::new().prefix(&req.prefix.unwrap_or_default()))
///                     .await?
///                     .keys()
///                     .map(|k| k.to_string())
///                     .collect::<Vec<_>>();
///                 Response::from_json(&json!({ "keys": keys }))
///             }
///             "/state/clear" => {
///                 self.state.storage().delete_all().await?;
///                 Response::ok("{}")
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

    /// Request to get a value.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct GetRequest {
        /// Key to retrieve
        pub key: String,
    }

    /// Response from get value.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct GetResponse<T> {
        /// The value, if found
        pub value: Option<T>,
    }

    /// Request to set a value.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct SetRequest<T> {
        /// Key to set
        pub key: String,
        /// Value to store
        pub value: T,
    }

    /// Request to delete a value.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct DeleteRequest {
        /// Key to delete
        pub key: String,
    }

    /// Response from delete.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct DeleteResponse {
        /// Whether a value was deleted
        pub deleted: bool,
    }

    /// Request to list keys.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct ListRequest {
        /// Optional prefix to filter keys
        pub prefix: Option<String>,
    }

    /// Response from list keys.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct ListResponse {
        /// List of keys
        pub keys: Vec<String>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_store_creation() {
        let store = DurableObjectStateStore::new("MCP_STATE");
        assert_eq!(store.namespace, "MCP_STATE");
        assert!(store.env.is_none());
    }

    #[test]
    fn test_state_store_error_display() {
        let err = StateStoreError::NoEnvironment;
        assert_eq!(err.to_string(), "No environment set");

        let err =
            StateStoreError::Serialization(serde_json::from_str::<()>("invalid").unwrap_err());
        assert!(err.to_string().contains("Serialization error"));
    }
}
