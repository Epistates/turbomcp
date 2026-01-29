//! Cloudflare Durable Objects integration for MCP servers.
//!
//! This module provides first-class support for stateful patterns using
//! Cloudflare Durable Objects, enabling:
//!
//! - **Session Persistence**: Streamable HTTP sessions that survive Worker restarts
//! - **State Management**: Per-user/conversation persistent state
//! - **Rate Limiting**: Per-client rate limiting with sliding window
//! - **OAuth Token Storage**: Secure token storage with automatic expiration
//!
//! # Architecture
//!
//! Durable Objects are accessed through stub bindings. You configure the DO
//! binding in your `wrangler.toml`:
//!
//! ```toml
//! [[durable_objects.bindings]]
//! name = "MCP_SESSIONS"
//! class_name = "McpSessionObject"
//!
//! [[durable_objects.bindings]]
//! name = "MCP_STATE"
//! class_name = "McpStateObject"
//!
//! [[durable_objects.bindings]]
//! name = "MCP_RATE_LIMIT"
//! class_name = "McpRateLimitObject"
//! ```
//!
//! # Example: Session Store
//!
//! ```rust,ignore
//! use turbomcp_wasm::wasm_server::durable_objects::DurableObjectSessionStore;
//! use turbomcp_wasm::wasm_server::streamable::StreamableHandler;
//!
//! #[event(fetch)]
//! async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
//!     let sessions = env.durable_object("MCP_SESSIONS")?;
//!     let session_store = DurableObjectSessionStore::new(sessions);
//!
//!     let server = MyServer::new()
//!         .into_mcp_server()
//!         .into_streamable()
//!         .with_session_store(session_store);
//!
//!     server.handle(req).await
//! }
//! ```
//!
//! # Example: State Store
//!
//! ```rust,ignore
//! use turbomcp_wasm::wasm_server::durable_objects::DurableObjectStateStore;
//!
//! async fn my_tool(ctx: Arc<RequestContext>, args: MyArgs) -> Result<String, ToolError> {
//!     let state_store = DurableObjectStateStore::from_env(&env, "MCP_STATE")?;
//!
//!     // Get conversation history
//!     let history: Vec<Message> = state_store
//!         .get(&ctx.session_id().unwrap(), "history")
//!         .await
//!         .unwrap_or_default();
//!
//!     // Process and update
//!     let mut history = history;
//!     history.push(Message::user(&args.input));
//!     state_store.set(&ctx.session_id().unwrap(), "history", &history).await?;
//!
//!     Ok("Done".to_string())
//! }
//! ```
//!
//! # Example: Rate Limiting
//!
//! ```rust,ignore
//! use turbomcp_wasm::wasm_server::durable_objects::DurableObjectRateLimiter;
//!
//! let rate_limiter = DurableObjectRateLimiter::from_env(&env, "MCP_RATE_LIMIT")?
//!     .with_limit(100)      // 100 requests
//!     .with_window(60000);  // per minute
//!
//! // In middleware
//! if !rate_limiter.check(&client_id).await? {
//!     return Err(ToolError::new("Rate limit exceeded"));
//! }
//! ```

mod rate_limiter;
mod session_store;
mod state_store;
mod token_store;

pub use rate_limiter::{DurableObjectRateLimiter, RateLimitConfig, RateLimitResult};
pub use session_store::DurableObjectSessionStore;
pub use state_store::{DurableObjectStateStore, StateStoreError};
pub use token_store::{DurableObjectTokenStore, OAuthTokenData, TokenStoreError};
