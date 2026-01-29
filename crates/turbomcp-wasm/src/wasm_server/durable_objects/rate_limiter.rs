//! Durable Object-backed rate limiter for per-client rate limiting.
//!
//! Provides sliding window rate limiting using Cloudflare Durable Objects
//! for consistent enforcement across Worker instances.

use serde::{Deserialize, Serialize};
use worker::Env;

/// Rate limiter backed by Cloudflare Durable Objects.
///
/// Uses a sliding window algorithm for smooth rate limiting that doesn't
/// have the burst issues of fixed windows.
///
/// # Setup
///
/// Configure the Durable Object binding in `wrangler.toml`:
///
/// ```toml
/// [[durable_objects.bindings]]
/// name = "MCP_RATE_LIMIT"
/// class_name = "McpRateLimitObject"
///
/// [[durable_objects.classes]]
/// name = "McpRateLimitObject"
/// class_name = "McpRateLimitObject"
/// ```
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_wasm::wasm_server::durable_objects::DurableObjectRateLimiter;
///
/// // Create a rate limiter: 100 requests per minute
/// let limiter = DurableObjectRateLimiter::from_env(&env, "MCP_RATE_LIMIT")?
///     .with_config(RateLimitConfig {
///         limit: 100,
///         window_ms: 60_000, // 1 minute
///     });
///
/// // Check before processing a request
/// let result = limiter.check("client-123").await?;
/// if !result.allowed {
///     return Err(ToolError::new(format!(
///         "Rate limit exceeded. Retry after {}ms",
///         result.retry_after_ms.unwrap_or(0)
///     )));
/// }
///
/// // Process the request...
/// ```
#[derive(Clone)]
pub struct DurableObjectRateLimiter {
    namespace: String,
    env: Option<Env>,
    config: RateLimitConfig,
}

/// Configuration for rate limiting.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum number of requests allowed in the window
    pub limit: u64,
    /// Time window in milliseconds
    pub window_ms: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            limit: 100,        // 100 requests
            window_ms: 60_000, // per minute
        }
    }
}

impl RateLimitConfig {
    /// Create a new rate limit configuration.
    pub fn new(limit: u64, window_ms: u64) -> Self {
        Self { limit, window_ms }
    }

    /// Create a per-second rate limit.
    pub fn per_second(limit: u64) -> Self {
        Self::new(limit, 1_000)
    }

    /// Create a per-minute rate limit.
    pub fn per_minute(limit: u64) -> Self {
        Self::new(limit, 60_000)
    }

    /// Create a per-hour rate limit.
    pub fn per_hour(limit: u64) -> Self {
        Self::new(limit, 3_600_000)
    }
}

/// Result of a rate limit check.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RateLimitResult {
    /// Whether the request is allowed
    pub allowed: bool,
    /// Number of requests remaining in the current window
    pub remaining: u64,
    /// Total limit
    pub limit: u64,
    /// Milliseconds until the rate limit resets
    pub reset_ms: u64,
    /// Milliseconds to wait before retrying (only if not allowed)
    pub retry_after_ms: Option<u64>,
}

impl DurableObjectRateLimiter {
    /// Create a new rate limiter with the given DO namespace binding name.
    pub fn new(namespace: impl Into<String>) -> Self {
        Self {
            namespace: namespace.into(),
            env: None,
            config: RateLimitConfig::default(),
        }
    }

    /// Create a rate limiter from an environment binding.
    pub fn from_env(env: &Env, binding: &str) -> worker::Result<Self> {
        // Validate the binding exists
        let _ = env.durable_object(binding)?;
        Ok(Self {
            namespace: binding.to_string(),
            env: Some(env.clone()),
            config: RateLimitConfig::default(),
        })
    }

    /// Set the environment for the limiter.
    pub fn with_env(mut self, env: Env) -> Self {
        self.env = Some(env);
        self
    }

    /// Set the rate limit configuration.
    pub fn with_config(mut self, config: RateLimitConfig) -> Self {
        self.config = config;
        self
    }

    /// Set the rate limit (requests per window).
    pub fn with_limit(mut self, limit: u64) -> Self {
        self.config.limit = limit;
        self
    }

    /// Set the time window in milliseconds.
    pub fn with_window_ms(mut self, window_ms: u64) -> Self {
        self.config.window_ms = window_ms;
        self
    }

    /// Check if a request is allowed for the given client ID.
    ///
    /// If allowed, this also records the request in the rate limiter.
    ///
    /// # Arguments
    ///
    /// * `client_id` - Unique identifier for the client (e.g., IP, API key, session ID)
    ///
    /// # Returns
    ///
    /// A `RateLimitResult` indicating whether the request is allowed.
    pub async fn check(&self, client_id: &str) -> Result<RateLimitResult, RateLimitError> {
        #[derive(Serialize)]
        struct CheckRequest<'a> {
            limit: u64,
            window_ms: u64,
            record: bool,
            client_id: &'a str,
        }

        let request = CheckRequest {
            limit: self.config.limit,
            window_ms: self.config.window_ms,
            record: true,
            client_id,
        };

        self.do_request(client_id, "/rate-limit/check", Some(&request))
            .await
    }

    /// Check the rate limit without recording a request.
    ///
    /// Useful for pre-flight checks or displaying remaining quota.
    pub async fn peek(&self, client_id: &str) -> Result<RateLimitResult, RateLimitError> {
        #[derive(Serialize)]
        struct CheckRequest<'a> {
            limit: u64,
            window_ms: u64,
            record: bool,
            client_id: &'a str,
        }

        let request = CheckRequest {
            limit: self.config.limit,
            window_ms: self.config.window_ms,
            record: false,
            client_id,
        };

        self.do_request(client_id, "/rate-limit/check", Some(&request))
            .await
    }

    /// Reset the rate limit for a client.
    ///
    /// Useful for administrative purposes.
    pub async fn reset(&self, client_id: &str) -> Result<(), RateLimitError> {
        self.do_request::<()>(client_id, "/rate-limit/reset", None::<&()>)
            .await
    }

    /// Send a request to the Durable Object.
    async fn do_request<T: for<'de> Deserialize<'de>>(
        &self,
        client_id: &str,
        path: &str,
        body: Option<&impl Serialize>,
    ) -> Result<T, RateLimitError> {
        let env = self.env.as_ref().ok_or(RateLimitError::NoEnvironment)?;

        let ns = env
            .durable_object(&self.namespace)
            .map_err(RateLimitError::Worker)?;
        let id = ns.id_from_name(client_id).map_err(RateLimitError::Worker)?;
        let stub = id.get_stub().map_err(RateLimitError::Worker)?;

        let mut init = worker::RequestInit::new();
        init.with_method(worker::Method::Post);

        if let Some(body) = body {
            let json = serde_json::to_string(body).map_err(RateLimitError::Serialization)?;
            init.with_body(Some(json.into()));
        }

        let url = format!("https://do-internal{path}");
        let request =
            worker::Request::new_with_init(&url, &init).map_err(RateLimitError::Worker)?;
        let mut response = stub
            .fetch_with_request(request)
            .await
            .map_err(RateLimitError::Worker)?;

        let text = response.text().await.map_err(RateLimitError::Worker)?;
        serde_json::from_str(&text).map_err(RateLimitError::Deserialization)
    }
}

/// Error type for rate limiter operations.
#[derive(Debug)]
pub enum RateLimitError {
    /// No environment has been set
    NoEnvironment,
    /// Worker/DO communication error
    Worker(worker::Error),
    /// Serialization error
    Serialization(serde_json::Error),
    /// Deserialization error
    Deserialization(serde_json::Error),
}

impl std::fmt::Display for RateLimitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoEnvironment => write!(f, "No environment set"),
            Self::Worker(e) => write!(f, "Worker error: {e:?}"),
            Self::Serialization(e) => write!(f, "Serialization error: {e}"),
            Self::Deserialization(e) => write!(f, "Deserialization error: {e}"),
        }
    }
}

impl std::error::Error for RateLimitError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Worker(e) => Some(e),
            Self::Serialization(e) => Some(e),
            Self::Deserialization(e) => Some(e),
            Self::NoEnvironment => None,
        }
    }
}

impl From<worker::Error> for RateLimitError {
    fn from(e: worker::Error) -> Self {
        Self::Worker(e)
    }
}

// ============================================================================
// Protocol Types
// ============================================================================

/// Request/response types for the rate limit Durable Object.
///
/// Implement a Durable Object class that handles these routes:
///
/// - `POST /rate-limit/check` - Check and optionally record a request
/// - `POST /rate-limit/reset` - Reset the rate limit
///
/// # Example Durable Object Implementation
///
/// ```rust,ignore
/// use std::collections::VecDeque;
///
/// #[durable_object]
/// pub struct McpRateLimitObject {
///     state: State,
///     timestamps: VecDeque<u64>,
/// }
///
/// #[durable_object]
/// impl DurableObject for McpRateLimitObject {
///     fn new(state: State, _env: Env) -> Self {
///         Self {
///             state,
///             timestamps: VecDeque::new(),
///         }
///     }
///
///     async fn fetch(&mut self, req: Request) -> Result<Response> {
///         match req.path().as_str() {
///             "/rate-limit/check" => {
///                 #[derive(Deserialize)]
///                 struct Req {
///                     limit: u64,
///                     window_ms: u64,
///                     record: bool,
///                 }
///                 let req: Req = req.json().await?;
///                 let now = js_sys::Date::now() as u64;
///                 let cutoff = now.saturating_sub(req.window_ms);
///
///                 // Remove expired timestamps
///                 while self.timestamps.front().map(|&t| t < cutoff).unwrap_or(false) {
///                     self.timestamps.pop_front();
///                 }
///
///                 let count = self.timestamps.len() as u64;
///                 let allowed = count < req.limit;
///                 let remaining = req.limit.saturating_sub(count + if allowed && req.record { 1 } else { 0 });
///
///                 if allowed && req.record {
///                     self.timestamps.push_back(now);
///                 }
///
///                 let reset_ms = self.timestamps.front()
///                     .map(|&t| req.window_ms.saturating_sub(now.saturating_sub(t)))
///                     .unwrap_or(0);
///
///                 Response::from_json(&json!({
///                     "allowed": allowed,
///                     "remaining": remaining,
///                     "limit": req.limit,
///                     "reset_ms": reset_ms,
///                     "retry_after_ms": if allowed { null } else { Some(reset_ms) }
///                 }))
///             }
///             "/rate-limit/reset" => {
///                 self.timestamps.clear();
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

    /// Request to check rate limit.
    #[derive(Debug, Serialize, Deserialize)]
    pub struct CheckRequest {
        /// Maximum allowed requests
        pub limit: u64,
        /// Time window in milliseconds
        pub window_ms: u64,
        /// Whether to record this request
        pub record: bool,
        /// Client identifier
        pub client_id: String,
    }

    /// Response from rate limit check.
    pub type CheckResponse = RateLimitResult;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limiter_creation() {
        let limiter = DurableObjectRateLimiter::new("MCP_RATE_LIMIT")
            .with_limit(50)
            .with_window_ms(30_000);

        assert_eq!(limiter.namespace, "MCP_RATE_LIMIT");
        assert_eq!(limiter.config.limit, 50);
        assert_eq!(limiter.config.window_ms, 30_000);
    }

    #[test]
    fn test_rate_limit_config_presets() {
        let per_second = RateLimitConfig::per_second(10);
        assert_eq!(per_second.limit, 10);
        assert_eq!(per_second.window_ms, 1_000);

        let per_minute = RateLimitConfig::per_minute(100);
        assert_eq!(per_minute.limit, 100);
        assert_eq!(per_minute.window_ms, 60_000);

        let per_hour = RateLimitConfig::per_hour(1000);
        assert_eq!(per_hour.limit, 1000);
        assert_eq!(per_hour.window_ms, 3_600_000);
    }

    #[test]
    fn test_rate_limit_error_display() {
        let err = RateLimitError::NoEnvironment;
        assert_eq!(err.to_string(), "No environment set");
    }
}
