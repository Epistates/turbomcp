//! Turning a panicking handler into a JSON-RPC error response.
//!
//! JSON-RPC requires that every request be answered. A `#[tool]` body that
//! panics would otherwise strand its request: the spawned task dies, and the
//! peer waits out its own timeout with no way to tell a hung server from a
//! buggy one. [`catch_handler_panic`] wraps a dispatch future so the panic
//! becomes a `-32603` response instead — the request fails loudly and the
//! connection stays usable for every other request.
//!
//! Wrap the future where it is *created*, not where it is awaited: the HTTP
//! transport polls the same call future from several places (inline, the
//! lazy SSE upgrade, the upgraded stream), and one wrap at construction covers
//! all of them.
//!
//! This relies on unwinding. Under `panic = "abort"` the process aborts before
//! any of this runs, which is what that profile asks for.

use std::any::Any;
use std::panic::AssertUnwindSafe;

use futures::FutureExt as _;
use turbomcp_core::{JsonRpcMessage, RequestId};

use crate::ProtocolError;

/// Run a dispatch future, converting a panic into an internal-error response.
///
/// `id` is the request's id, or `None` for a notification (nothing to answer,
/// so a panic is logged and swallowed). The panic payload is logged but never
/// put on the wire — it can carry internal detail the peer has no business
/// seeing; the response says only that the handler panicked.
pub async fn catch_handler_panic<F>(
    id: Option<RequestId>,
    fut: F,
) -> Result<Option<JsonRpcMessage>, ProtocolError>
where
    F: Future<Output = Result<Option<JsonRpcMessage>, ProtocolError>>,
{
    // `AssertUnwindSafe`: the only state that outlives the unwind is `id`, and
    // the caller drops the service clone the future borrowed from.
    match AssertUnwindSafe(fut).catch_unwind().await {
        Ok(outcome) => outcome,
        Err(payload) => {
            let detail = panic_detail(&*payload);
            match id {
                Some(id) => {
                    tracing::error!(
                        panic = detail,
                        request_id = ?id,
                        "handler panicked; answering -32603"
                    );
                    let err = ProtocolError::Internal("handler panicked".to_owned());
                    Ok(Some(err.into_response(id).into()))
                }
                None => {
                    tracing::error!(panic = detail, "handler panicked on a notification");
                    Ok(None)
                }
            }
        }
    }
}

/// Best-effort readable form of a panic payload (`panic!` produces a `&str` or
/// a `String`; anything else came from `panic_any`).
fn panic_detail(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&'static str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("<non-string panic payload>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_core::JsonRpcResponse;

    #[tokio::test]
    async fn a_panicking_request_becomes_an_internal_error() {
        let id = RequestId::from(7i64);
        let out = catch_handler_panic(Some(id.clone()), async { panic!("boom") })
            .await
            .expect("panic is converted, not propagated");

        let Some(JsonRpcMessage::Response(r)) = out else {
            panic!("expected a response");
        };
        assert_eq!(r.id, id);
        let err = r.error.expect("error response");
        assert_eq!(err.code, -32603);
        // The payload ("boom") stays in the log, not on the wire.
        assert!(!err.message.contains("boom"), "leaked payload: {err:?}");
    }

    #[tokio::test]
    async fn a_panicking_notification_is_swallowed() {
        let out = catch_handler_panic(None, async { panic!("boom") })
            .await
            .expect("panic is converted, not propagated");
        assert!(out.is_none());
    }

    #[tokio::test]
    async fn a_normal_outcome_passes_through_untouched() {
        let reply: JsonRpcMessage =
            JsonRpcResponse::success(RequestId::from(1i64), serde_json::json!("ok")).into();
        let out = catch_handler_panic(Some(RequestId::from(1i64)), async { Ok(Some(reply)) })
            .await
            .unwrap();
        assert!(matches!(out, Some(JsonRpcMessage::Response(_))));

        let err = catch_handler_panic(Some(RequestId::from(1i64)), async {
            Err(ProtocolError::Internal("real error".to_owned()))
        })
        .await;
        assert!(matches!(err, Err(ProtocolError::Internal(_))));
    }

    #[tokio::test]
    async fn a_non_string_payload_still_answers() {
        let out = catch_handler_panic(Some(RequestId::from(2i64)), async {
            std::panic::panic_any(42u32)
        })
        .await
        .expect("panic is converted, not propagated");
        assert!(matches!(out, Some(JsonRpcMessage::Response(_))));
    }
}
