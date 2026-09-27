//! Structured log messages to the client (`notifications/message`, the
//! `logging` capability).
//!
//! Strictly opt-in on both ends. The server enables the capability with
//! [`ServerBuilder::with_logging`](crate::ServerBuilder::with_logging); the
//! client opts into delivery — per session via `logging/setLevel` on
//! `2025-11-25`, per request via the `_meta` key
//! `io.modelcontextprotocol/logLevel` on the draft (which deprecates the
//! whole feature, SEP-2577 — it stays functional here, like every deprecated
//! draft surface). Without both opt-ins, [`LogSender`] drops everything: the
//! draft spec's MUST NOT, and our chosen policy for un-opted legacy sessions.
//!
//! Delivery rides the originating request's own stream (the draft forbids any
//! other stream for request-scoped messages); the legacy family may fall back
//! to the session's `GET` stream.

use serde_json::{Value, json};
use std::sync::Arc;
use turbomcp_core::{JsonRpcNotification, LogLevel};
use turbomcp_protocol::methods;

use crate::subscriptions::Route;

/// Sends `notifications/message` for one in-flight request. Cheap to clone.
///
/// Messages below the client's requested minimum severity are dropped here,
/// so handlers can log unconditionally.
#[derive(Clone, Debug)]
pub struct LogSender {
    inner: Option<Arc<Inner>>,
}

#[derive(Debug)]
struct Inner {
    /// The client's requested minimum severity.
    min: LogLevel,
    /// Where its messages go: the request's own stream, then (legacy only)
    /// the session's `GET` stream.
    route: Route,
}

impl LogSender {
    /// A sender for a request with no logging opt-in: everything is dropped.
    #[must_use]
    pub(crate) fn disabled() -> Self {
        Self { inner: None }
    }

    /// A live sender filtering below `min`.
    #[must_use]
    pub(crate) fn new(min: LogLevel, route: Route) -> Self {
        Self {
            inner: Some(Arc::new(Inner { min, route })),
        }
    }

    /// Whether a message at `level` would currently be delivered. Handlers
    /// MAY skip expensive message assembly when it wouldn't.
    #[must_use]
    pub fn would_log(&self, level: LogLevel) -> bool {
        self.inner.as_ref().is_some_and(|i| level >= i.min)
    }

    /// Send one log message: any JSON-serializable `data`, an optional logger
    /// name. Dropped silently when the client didn't opt in, the severity is
    /// below the requested minimum, or the stream is gone — logging is
    /// best-effort by design, so the call is infallible.
    pub async fn log_with(&self, level: LogLevel, logger: Option<&str>, data: Value) {
        let Some(inner) = &self.inner else { return };
        if level < inner.min {
            return;
        }
        let Some(writer) = inner.route.peer() else {
            tracing::debug!("no stream for log notification; dropped");
            return;
        };
        let mut params = json!({ "level": level, "data": data });
        if let (Some(obj), Some(logger)) = (params.as_object_mut(), logger) {
            obj.insert("logger".to_owned(), json!(logger));
        }
        if writer
            .send(JsonRpcNotification::new(methods::notification::MESSAGE, Some(params)).into())
            .await
            .is_err()
        {
            tracing::debug!("request stream closed before log notification; dropped");
        }
    }

    /// [`log_with`](Self::log_with) without a logger name.
    pub async fn log(&self, level: LogLevel, data: Value) {
        self.log_with(level, None, data).await;
    }

    /// Log at `debug` severity.
    pub async fn debug(&self, data: Value) {
        self.log(LogLevel::Debug, data).await;
    }

    /// Log at `info` severity.
    pub async fn info(&self, data: Value) {
        self.log(LogLevel::Info, data).await;
    }

    /// Log at `warning` severity.
    pub async fn warning(&self, data: Value) {
        self.log(LogLevel::Warning, data).await;
    }

    /// Log at `error` severity.
    pub async fn error(&self, data: Value) {
        self.log(LogLevel::Error, data).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_sender_is_inert() {
        let sender = LogSender::disabled();
        assert!(!sender.would_log(LogLevel::Emergency));
        sender.error(json!("nope")).await; // must not panic
    }

    #[tokio::test]
    async fn severity_filter_and_shape() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let sender = LogSender::new(
            LogLevel::Info,
            Route::to(turbomcp_service::Peer::new("log-test-conn", &tx)),
        );

        assert!(!sender.would_log(LogLevel::Debug));
        assert!(sender.would_log(LogLevel::Error));

        sender.debug(json!("filtered")).await;
        sender
            .log_with(LogLevel::Error, Some("db"), json!({ "oops": true }))
            .await;

        let only = rx.try_recv().expect("error message sent");
        assert!(rx.try_recv().is_err(), "debug was filtered");
        let turbomcp_core::JsonRpcMessage::Notification(n) = only else {
            panic!("expected a notification");
        };
        assert_eq!(n.method, "notifications/message");
        let params = n.params.unwrap();
        assert_eq!(params["level"], "error");
        assert_eq!(params["logger"], "db");
        assert_eq!(params["data"]["oops"], true);
    }
}
