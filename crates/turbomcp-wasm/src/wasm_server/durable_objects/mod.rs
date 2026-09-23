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

use serde::de::{Deserialize, DeserializeOwned, Deserializer, IgnoredAny};

/// The reply to a Durable Object command whose body carries nothing.
///
/// The handlers documented in this module answer writes with `{}`, and a
/// handler may just as well answer with no body. Those replies used to be
/// decoded as `()`, which serde only reads from `null`, so every write —
/// session create/update, event store, state set, token store — reported a
/// deserialization error after the Durable Object had already applied it,
/// and `initialize` over Streamable HTTP answered 500. Any body is accepted.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Ack;

impl<'de> Deserialize<'de> for Ack {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        IgnoredAny::deserialize(deserializer)?;
        Ok(Ack)
    }
}

/// Why a Durable Object's reply could not be used.
#[derive(Debug)]
pub(crate) enum DoReplyError {
    /// The object answered with a non-2xx status.
    Status(worker::Error),
    /// The body was not the JSON the caller expected.
    Body(serde_json::Error),
}

/// Decode a Durable Object's reply.
///
/// A non-2xx status is a failure whatever the body says — a `404` or `500`
/// page used to be handed to the JSON decoder and reported as a confusing
/// deserialization error, or worse, decoded. An empty body reads as `null`.
pub(crate) fn decode_reply<T: DeserializeOwned>(
    status: u16,
    body: &str,
) -> Result<T, DoReplyError> {
    if !(200..300).contains(&status) {
        return Err(DoReplyError::Status(worker::Error::RustError(format!(
            "Durable Object answered HTTP {status}: {}",
            body.chars().take(200).collect::<String>()
        ))));
    }
    let body = if body.trim().is_empty() { "null" } else { body };
    serde_json::from_str(body).map_err(DoReplyError::Body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acknowledgements_accept_whatever_the_object_answers() {
        for body in ["{}", "", "null", r#"{"ok":true}"#, "[]"] {
            assert!(decode_reply::<Ack>(200, body).is_ok(), "{body:?}");
        }
        assert!(decode_reply::<Ack>(204, "").is_ok());
    }

    #[test]
    fn error_statuses_fail_whatever_the_body() {
        assert!(matches!(
            decode_reply::<Ack>(500, "{}"),
            Err(DoReplyError::Status(_))
        ));
        assert!(matches!(
            decode_reply::<Option<u32>>(404, "null"),
            Err(DoReplyError::Status(_))
        ));
    }

    #[test]
    fn typed_replies_still_decode() {
        #[derive(serde::Deserialize)]
        struct Get {
            session: Option<u32>,
        }
        let get: Get = decode_reply(200, r#"{"session":7}"#).unwrap();
        assert_eq!(get.session, Some(7));
        assert!(matches!(
            decode_reply::<Get>(200, "not json"),
            Err(DoReplyError::Body(_))
        ));
    }
}
