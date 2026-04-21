//! Server-side context re-exports.
//!
//! `turbomcp-server` used to carry its own `RequestContext` and `McpSession`
//! trait (with a `to_core_context()` conversion back to `turbomcp-core`'s
//! weaker type). v3.2 collapses both into the single canonical type defined
//! in `turbomcp-core`, so `#[tool]` handlers and internal dispatch see the
//! same context — the one that carries the session handle and exposes
//! `sample()` / `elicit_form()` / `elicit_url()` / `notify_client()`.
//!
//! The `Cancellable` blanket impl for `tokio_util::sync::CancellationToken`
//! lives in `turbomcp-core` (gated on the `std` feature) so the orphan rule
//! doesn't force us into a newtype wrapper here.

pub use turbomcp_core::context::{RequestContext, TransportType};
#[allow(unused_imports)]
pub use turbomcp_core::session::{Cancellable, McpSession, SessionFuture};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio_util::sync::CancellationToken;

    #[test]
    fn cancellation_token_adapts_to_cancellable() {
        let token = CancellationToken::new();
        let ctx = RequestContext::new()
            .with_cancellation_token(Arc::new(token.clone()) as Arc<dyn Cancellable>);
        assert!(!ctx.is_cancelled());
        token.cancel();
        assert!(ctx.is_cancelled());
    }

    #[test]
    fn new_generates_uuid_request_id() {
        let ctx = RequestContext::new();
        assert!(!ctx.request_id().is_empty());
    }
}
