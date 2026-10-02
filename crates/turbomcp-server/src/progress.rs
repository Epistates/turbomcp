//! Progress reporting (`notifications/progress`).
//!
//! A request opts in by carrying `_meta.progressToken` (progress spec
//! §Progress Flow); the handler's context then exposes a [`ProgressReporter`]
//! whose notifications ride the originating request's own stream — the stdio
//! pipe, or the request-scoped POST SSE response on HTTP (the transports spec
//! routes request-related messages there). Without a token the reporter is
//! inert: reports are silently dropped, which the spec allows ("the receiver
//! is not obligated to provide these notifications" — and the sender here is
//! never obligated to ask).
//!
//! The wire shape is identical on both protocol versions.
//!
//! A call running as a task reports into the task too: each report becomes
//! its `statusMessage`. On `2026-07-28` that is all it does, since
//! "`notifications/progress` ... are not supported on tasks"; on
//! `2025-11-25` "the `progressToken` provided in the initial request remains
//! valid throughout the task lifetime", so notifications keep going too.

use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use turbomcp_core::JsonRpcNotification;
use turbomcp_protocol::methods;

use crate::subscriptions::Route;
use crate::task_handle::TaskHandle;

/// Reports progress for one in-flight request. Cheap to clone; all clones
/// share the monotonicity guard.
///
/// Available on the work-doing contexts ([`CallToolContext`](crate::CallToolContext),
/// [`ReadResourceContext`](crate::ReadResourceContext),
/// [`GetPromptContext`](crate::GetPromptContext)). Spec constraints enforced
/// here so handlers can't violate them:
///
/// - notifications only reference a token the client provided (no token →
///   inert reporter);
/// - `progress` strictly increases — a non-increasing report is dropped with
///   a warning;
/// - notifications stop after completion (the per-request channel closes with
///   the request).
#[derive(Clone, Debug)]
pub struct ProgressReporter {
    inner: Option<Arc<Inner>>,
}

#[derive(Debug)]
struct Inner {
    /// Where notifications go, when the client asked for them.
    notify: Option<Notify>,
    /// The task the call runs as, if it becomes one: reports become its
    /// status message.
    task: TaskHandle,
    /// Last reported value (spec: MUST increase with each notification).
    last: Mutex<Option<f64>>,
}

#[derive(Clone, Debug)]
struct Notify {
    /// The client's opaque token (string or integer), echoed verbatim.
    token: Value,
    /// Where reports go: the request's own stream, then (legacy only) the
    /// session's `GET` stream. The draft forbids delivering request-scoped
    /// messages anywhere but the request's own stream.
    route: Route,
}

impl ProgressReporter {
    /// A reporter for a request that carried no `progressToken`: every report
    /// is dropped.
    #[must_use]
    pub(crate) fn disabled() -> Self {
        Self { inner: None }
    }

    /// A live reporter delivering along `route`.
    #[must_use]
    pub(crate) fn new(token: Value, route: Route) -> Self {
        Self::build(Some(Notify { token, route }), TaskHandle::disabled())
    }

    /// A reporter for a call that may run as `task`, and notifies no one
    /// (`2026-07-28`: tasks get no progress notifications).
    #[must_use]
    pub(crate) fn for_task(task: TaskHandle) -> Self {
        Self::build(None, task)
    }

    /// This reporter, reporting into `task` as well once the call runs as
    /// one (`2025-11-25`, where the notifications keep going).
    #[must_use]
    pub(crate) fn with_task(self, task: TaskHandle) -> Self {
        let notify = self.inner.and_then(|inner| inner.notify.clone());
        Self::build(notify, task)
    }

    fn build(notify: Option<Notify>, task: TaskHandle) -> Self {
        Self {
            inner: Some(Arc::new(Inner {
                notify,
                task,
                last: Mutex::new(None),
            })),
        }
    }

    /// Whether anyone hears these reports: the client asked for progress on
    /// this request, or the call runs as a task, whose status message they
    /// become. Handlers MAY skip expensive bookkeeping when not.
    #[must_use]
    pub fn is_requested(&self) -> bool {
        self.inner
            .as_ref()
            .is_some_and(|inner| inner.notify.is_some() || inner.task.is_task())
    }

    /// Send one `notifications/progress` for this request, and, when the call
    /// runs as a task, make it the task's status message: `message`, or
    /// `progress/total` without one.
    ///
    /// `progress` must be strictly greater than the previous report (spec
    /// MUST); violations are dropped with a warning rather than sent. A
    /// missing token or a closed stream makes this a no-op — progress is
    /// best-effort by design, so the call is infallible.
    ///
    /// # Panics
    /// If another thread panicked while reporting on this call.
    pub async fn report(&self, progress: f64, total: Option<f64>, message: Option<&str>) {
        let Some(inner) = &self.inner else { return };
        if inner.notify.is_none() && !inner.task.is_task() {
            return;
        }
        // JSON has no NaN or infinity: serde writes `null`, which the schema
        // rejects, and a NaN would also poison the guard below (every
        // comparison with it is false, so later reports could go down).
        if !progress.is_finite() {
            tracing::warn!(progress, "progress must be a finite number; report dropped");
            return;
        }
        let total = total.filter(|t| {
            let finite = t.is_finite();
            if !finite {
                tracing::warn!(total = t, "progress total must be a finite number; omitted");
            }
            finite
        });

        {
            let mut last = inner.last.lock().expect("progress guard poisoned");
            if last.is_some_and(|prev| progress <= prev) {
                tracing::warn!(
                    progress,
                    previous = last.unwrap_or_default(),
                    "progress must strictly increase (spec MUST); report dropped"
                );
                return;
            }
            *last = Some(progress);
        }

        if inner.task.is_task() {
            let status = match (message, total) {
                (Some(message), _) => message.to_owned(),
                (None, Some(total)) => format!("{progress}/{total}"),
                (None, None) => progress.to_string(),
            };
            inner.task.set_status_message(status).await;
        }

        let Some(notify) = &inner.notify else { return };
        let Some(writer) = notify.route.peer() else {
            tracing::debug!("no stream for progress notification; dropped");
            return;
        };
        let mut params = json!({
            "progressToken": notify.token,
            "progress": progress,
        });
        if let Some(obj) = params.as_object_mut() {
            if let Some(total) = total {
                obj.insert("total".to_owned(), json!(total));
            }
            if let Some(message) = message {
                obj.insert("message".to_owned(), json!(message));
            }
        }
        if writer
            .send(JsonRpcNotification::new(methods::notification::PROGRESS, Some(params)).into())
            .await
            .is_err()
        {
            tracing::debug!("request stream closed before progress notification; dropped");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disabled_reporter_is_inert() {
        let reporter = ProgressReporter::disabled();
        assert!(!reporter.is_requested());
        reporter.report(1.0, None, None).await; // must not panic
    }

    #[tokio::test]
    async fn non_increasing_reports_are_dropped() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let reporter = ProgressReporter::new(
            json!("t1"),
            Route::to(turbomcp_service::Peer::new("prog-test-conn", &tx)),
        );

        reporter.report(2.0, Some(10.0), None).await;
        reporter.report(2.0, None, None).await; // equal: dropped
        reporter.report(1.0, None, None).await; // lower: dropped
        reporter.report(3.0, None, Some("step 3")).await;

        let first = rx.try_recv().expect("first report sent");
        let second = rx.try_recv().expect("second report sent");
        assert!(rx.try_recv().is_err(), "exactly two notifications");
        let (first, second) = match (first, second) {
            (
                turbomcp_core::JsonRpcMessage::Notification(a),
                turbomcp_core::JsonRpcMessage::Notification(b),
            ) => (a, b),
            other => panic!("expected notifications, got {other:?}"),
        };
        assert_eq!(first.params.as_ref().unwrap()["progress"], 2.0);
        assert_eq!(first.params.as_ref().unwrap()["total"], 10.0);
        assert_eq!(second.params.as_ref().unwrap()["progress"], 3.0);
        assert_eq!(second.params.as_ref().unwrap()["message"], "step 3");
        assert_eq!(second.params.as_ref().unwrap()["progressToken"], "t1");
    }

    /// `done / total` with `total == 0` is NaN. It used to go out as
    /// `"progress": null` and then disarm the monotonic guard, letting later
    /// reports go down.
    #[tokio::test]
    async fn non_finite_progress_is_dropped_and_does_not_disarm_the_guard() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let reporter = ProgressReporter::new(
            json!("t2"),
            Route::to(turbomcp_service::Peer::new("prog-nan-conn", &tx)),
        );

        reporter.report(f64::NAN, None, None).await;
        reporter.report(5.0, Some(f64::INFINITY), None).await;
        reporter.report(3.0, None, None).await; // lower than 5: still dropped

        let turbomcp_core::JsonRpcMessage::Notification(only) = rx.try_recv().expect("one report")
        else {
            panic!("expected a notification");
        };
        assert!(
            rx.try_recv().is_err(),
            "the NaN and the decrease were dropped"
        );
        let params = only.params.unwrap();
        assert_eq!(params["progress"], 5.0);
        assert!(
            params.get("total").is_none(),
            "a non-finite total is omitted"
        );
    }
}
