//! Subscription registry + [`ServerNotifier`] — both protocol versions.
//!
//! **Draft (`subscriptions/listen`):** a subscription is `(connection,
//! listen-request id)` plus the filter subset the server agreed to honor
//! (subscriptions spec: the server **MUST NOT** send notification types the
//! client didn't opt in to). Delivery resolves the connection's ordered writer
//! lazily via [`turbomcp_service::outbound`] — a missing writer means the
//! connection closed, and the subscription is pruned on the spot (on stdio the
//! server holds no subscription state across reconnections, per spec).
//!
//! **Legacy (`2025-11-25`):** subscriptions are per *session* —
//! `resources/subscribe` adds a URI; `*_list_changed` goes to every live
//! legacy session unconditionally (the old protocol has no opt-in filter; the
//! capability advertisement is the contract). Delivery prefers the session's
//! HTTP `GET` SSE stream ([`outbound::session_stream_id`]) and falls back to
//! the byte-pipe connection the session was last seen on (stdio). Routes
//! without a reachable writer are kept — an HTTP client may open its GET
//! stream later — bounded by [`MAX_LEGACY_ROUTES`].
//!
//! `*_list_changed` publishes are coalesced: bursts inside
//! [`COALESCE_WINDOW_MS`] collapse into one notification per kind.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use turbomcp_core::{JsonRpcMessage, JsonRpcNotification, RequestId, meta};
use turbomcp_protocol::draft::types as draft;
use turbomcp_protocol::methods;
use turbomcp_service::outbound;

/// How long a `*_list_changed` burst is allowed to accumulate before the one
/// coalesced notification goes out.
pub(crate) const COALESCE_WINDOW_MS: u64 = 50;

/// The list-changed notification kinds (also the coalescing slots).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ListChangedKind {
    Tools,
    Resources,
    Prompts,
}

impl ListChangedKind {
    fn method(self) -> &'static str {
        match self {
            Self::Tools => methods::notification::TOOLS_LIST_CHANGED,
            Self::Resources => methods::notification::RESOURCES_LIST_CHANGED,
            Self::Prompts => methods::notification::PROMPTS_LIST_CHANGED,
        }
    }

    fn slot(self) -> usize {
        match self {
            Self::Tools => 0,
            Self::Resources => 1,
            Self::Prompts => 2,
        }
    }

    fn wants(self, filter: &draft::SubscriptionFilter) -> bool {
        match self {
            Self::Tools => filter.tools_list_changed == Some(true),
            Self::Resources => filter.resources_list_changed == Some(true),
            Self::Prompts => filter.prompts_list_changed == Some(true),
        }
    }
}

/// Upper bound on tracked legacy session routes; at capacity, an arbitrary
/// existing route is evicted to admit the new one (matches the session store's
/// bounded-memory posture; explicit lifecycle eviction is a Phase 7 item).
pub(crate) const MAX_LEGACY_ROUTES: usize = 4096;

/// A legacy session's delivery route: where its messages go and which
/// resource URIs it subscribed to.
#[derive(Default)]
struct LegacyRoute {
    /// The byte-pipe connection the session was last seen on (stdio delivery
    /// fallback); empty for HTTP-only sessions.
    connection: String,
    uris: HashSet<String>,
}

/// Shared map of live subscriptions; dispatcher clones share it via `Arc`.
#[derive(Default)]
pub(crate) struct SubscriptionRegistry {
    inner: Mutex<HashMap<(String, RequestId), draft::SubscriptionFilter>>,
    /// Legacy (`2025-11-25`) per-session routes, keyed by session id.
    legacy: Mutex<HashMap<String, LegacyRoute>>,
    /// One pending-flush flag per [`ListChangedKind`] slot.
    pending: [AtomicBool; 3],
}

impl SubscriptionRegistry {
    pub(crate) fn insert(
        &self,
        connection: &str,
        id: &RequestId,
        filter: draft::SubscriptionFilter,
    ) {
        self.lock()
            .insert((connection.to_owned(), id.clone()), filter);
    }

    /// Drop the subscription opened by `(connection, id)`, if any. Wired to
    /// `notifications/cancelled` referencing the listen request id.
    pub(crate) fn remove(&self, connection: &str, id: &RequestId) -> bool {
        self.lock()
            .remove(&(connection.to_owned(), id.clone()))
            .is_some()
    }

    // ---- legacy (2025-11-25) session routes -----------------------------------

    /// Record (or refresh) where a legacy session's messages can be delivered.
    /// Called on every legacy dispatch so the stdio fallback stays current.
    pub(crate) fn legacy_touch(&self, session: &str, connection: Option<&str>) {
        let mut routes = self.lock_legacy();
        if !routes.contains_key(session) && routes.len() >= MAX_LEGACY_ROUTES {
            // Bounded memory: evict an arbitrary route to admit the new one.
            if let Some(victim) = routes.keys().next().cloned() {
                routes.remove(&victim);
            }
        }
        let route = routes.entry(session.to_owned()).or_default();
        if let Some(conn) = connection {
            conn.clone_into(&mut route.connection);
        }
    }

    /// Legacy `resources/subscribe`: deliver `notifications/resources/updated`
    /// for `uri` to this session.
    pub(crate) fn legacy_subscribe(&self, session: &str, connection: Option<&str>, uri: String) {
        self.legacy_touch(session, connection);
        self.lock_legacy()
            .get_mut(session)
            .expect("touched above")
            .uris
            .insert(uri);
    }

    /// Legacy `resources/unsubscribe` (idempotent — unknown URIs are a no-op).
    pub(crate) fn legacy_unsubscribe(&self, session: &str, uri: &str) {
        if let Some(route) = self.lock_legacy().get_mut(session) {
            route.uris.remove(uri);
        }
    }

    /// Drop a legacy session's entire delivery route (every subscribed URI and
    /// its connection binding). Called when the session ends — explicit
    /// termination (`DELETE`) or idle eviction — so a gone session stops
    /// receiving `*_list_changed`/`resources/updated`. Returns whether a route
    /// existed.
    pub(crate) fn legacy_remove(&self, session: &str) -> bool {
        self.lock_legacy().remove(session).is_some()
    }

    // ---- publishing ------------------------------------------------------------

    /// Coalesced `*_list_changed`: the first call in a window schedules one
    /// flush; further calls inside the window are absorbed.
    pub(crate) fn schedule_list_changed(self: &Arc<Self>, kind: ListChangedKind) {
        if self.pending[kind.slot()].swap(true, Ordering::AcqRel) {
            return; // a flush is already scheduled
        }
        let registry = Arc::clone(self);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(COALESCE_WINDOW_MS)).await;
            registry.pending[kind.slot()].store(false, Ordering::Release);
            registry
                .publish(kind.method(), None, |f| kind.wants(f))
                .await;
            // Legacy has no opt-in filter: every live session gets it (the
            // advertised `listChanged` capability is the contract).
            registry.publish_legacy(kind.method(), None, |_| true).await;
        });
    }

    /// Immediate `notifications/resources/updated` to every subscription that
    /// listed `uri` (draft filters and legacy `resources/subscribe` alike).
    pub(crate) async fn publish_resource_updated(&self, uri: &str) {
        self.publish(
            methods::notification::RESOURCES_UPDATED,
            Some(("uri", json!(uri))),
            |f| f.resource_subscriptions.iter().any(|u| u == uri),
        )
        .await;
        self.publish_legacy(
            methods::notification::RESOURCES_UPDATED,
            Some(("uri", json!(uri))),
            |route| route.uris.contains(uri),
        )
        .await;
    }

    /// Deliver `method` to every legacy session whose route passes `wants`,
    /// on the legacy wire (no `subscriptionId` — the old protocol has none).
    async fn publish_legacy(
        &self,
        method: &str,
        extra: Option<(&str, Value)>,
        wants: impl Fn(&LegacyRoute) -> bool,
    ) {
        let targets: Vec<(String, String)> = self
            .lock_legacy()
            .iter()
            .filter(|(_, route)| wants(route))
            .map(|(session, route)| (session.clone(), route.connection.clone()))
            .collect();

        for (session, connection) in targets {
            let Some(writer) = legacy_writer(&session, &connection) else {
                continue; // no stream right now; the route stays (HTTP may reconnect)
            };
            let params = extra
                .as_ref()
                .map(|(key, value)| json!({ *key: value.clone() }));
            let note = JsonRpcNotification::new(method, params);
            let _ = writer.send(note.into()).await;
        }
    }

    /// Deliver `method` to every subscription whose filter passes `wants`,
    /// stamping each copy with its subscription id. Subscriptions whose
    /// connection is gone are pruned.
    async fn publish(
        &self,
        method: &str,
        extra: Option<(&str, Value)>,
        wants: impl Fn(&draft::SubscriptionFilter) -> bool,
    ) {
        let targets: Vec<(String, RequestId)> = self
            .lock()
            .iter()
            .filter(|(_, filter)| wants(filter))
            .map(|(key, _)| key.clone())
            .collect();

        for (connection, id) in targets {
            let Some(writer) = outbound::writer(&connection) else {
                self.remove(&connection, &id);
                continue;
            };
            let note = subscription_notification(method, &id, extra.clone());
            if writer.send(note).await.is_err() {
                self.remove(&connection, &id);
            }
        }
    }

    /// Drop every live draft subscription at graceful teardown, answering each
    /// listen request with the `SubscriptionsListenResult` envelope that ends
    /// its stream.
    ///
    /// The 2026-07-28 RC had deleted that envelope, leaving a stream with no
    /// defined ending but the transport closing under it; the frozen spec
    /// restored it for exactly this case — "sent only when the server tears
    /// the subscription down". An *abrupt* close still carries no response, so
    /// delivery is best-effort: a connection whose writer is already gone is
    /// simply dropped.
    pub(crate) async fn close_all(&self) {
        // Take the targets and clear under the lock — it must not be held
        // across an await, and a subscription being torn down must stop
        // receiving notifications either way.
        let targets: Vec<(String, RequestId)> = {
            let mut live = self.lock();
            let targets = live.keys().cloned().collect();
            live.clear();
            targets
        };
        for (connection, id) in targets {
            let Some(writer) = outbound::writer(&connection) else {
                continue;
            };
            let result = json!({
                "resultType": turbomcp_protocol::neutral::result_type::COMPLETE,
                "_meta": { meta::keys::SUBSCRIPTION_ID: subscription_id_value(&id) },
            });
            let _ = writer
                .send(turbomcp_core::JsonRpcResponse::success(id, result).into())
                .await;
        }
    }

    fn lock(
        &self,
    ) -> std::sync::MutexGuard<'_, HashMap<(String, RequestId), draft::SubscriptionFilter>> {
        self.inner.lock().expect("subscription registry poisoned")
    }

    fn lock_legacy(&self) -> std::sync::MutexGuard<'_, HashMap<String, LegacyRoute>> {
        self.legacy.lock().expect("legacy route registry poisoned")
    }
}

/// Resolve a legacy session's server→client writer for *session-scoped*
/// publishes (list_changed, resources/updated): the HTTP `GET` SSE stream
/// first, then the byte-pipe connection the session was last seen on. `None`
/// is not an error — an HTTP client may simply not have its stream open.
pub(crate) fn legacy_writer(
    session: &str,
    connection: &str,
) -> Option<tokio::sync::mpsc::Sender<JsonRpcMessage>> {
    outbound::writer(&outbound::session_stream_id(session)).or_else(|| {
        (!connection.is_empty())
            .then(|| outbound::writer(connection))
            .flatten()
    })
}

/// Resolve the channel for a *request-related* server→client message (inline
/// bidi requests; progress and log notifications): the originating request's
/// own stream first — its POST SSE response on HTTP, the pipe on stdio — per
/// the transports spec's SHOULD; then the session's `GET` stream (legacy MAY).
/// Draft callers pass an empty `session`: the draft forbids delivering
/// request-scoped messages on any stream but the request's own.
pub(crate) fn request_writer(
    connection: &str,
    session: &str,
) -> Option<tokio::sync::mpsc::Sender<JsonRpcMessage>> {
    (!connection.is_empty())
        .then(|| outbound::writer(connection))
        .flatten()
        .or_else(|| {
            (!session.is_empty())
                .then(|| outbound::writer(&outbound::session_stream_id(session)))
                .flatten()
        })
}

/// The `_meta.subscriptionId` value for a listen request id: the JSON-RPC ID
/// **verbatim** (string or number — the spec pins the value to the ID itself,
/// never a stringified copy).
pub(crate) fn subscription_id_value(id: &RequestId) -> Value {
    match id {
        RequestId::Number(n) => json!(n),
        RequestId::String(s) => json!(s),
    }
}

/// Build one stream notification, stamped with its subscription id.
fn subscription_notification(
    method: &str,
    id: &RequestId,
    extra: Option<(&str, Value)>,
) -> JsonRpcMessage {
    let mut params = serde_json::Map::new();
    params.insert(
        "_meta".to_owned(),
        json!({ meta::keys::SUBSCRIPTION_ID: subscription_id_value(id) }),
    );
    if let Some((key, value)) = extra {
        params.insert(key.to_owned(), value);
    }
    JsonRpcNotification::new(method, Some(Value::Object(params))).into()
}

/// Publishes server-side change events to every live subscription. Cheap to
/// clone; obtained from
/// [`VersionDispatcher::notifier`](crate::VersionDispatcher::notifier).
///
/// `*_list_changed` events are coalesced (bursts collapse into one
/// notification); `resource_updated` is delivered immediately, with the
/// writer's backpressure applied.
#[derive(Clone)]
pub struct ServerNotifier {
    subs: Arc<SubscriptionRegistry>,
}

impl ServerNotifier {
    pub(crate) fn new(subs: Arc<SubscriptionRegistry>) -> Self {
        Self { subs }
    }

    /// The tool list changed (`notifications/tools/list_changed`).
    pub fn tools_list_changed(&self) {
        self.subs.schedule_list_changed(ListChangedKind::Tools);
    }

    /// The resource list changed (`notifications/resources/list_changed`).
    pub fn resources_list_changed(&self) {
        self.subs.schedule_list_changed(ListChangedKind::Resources);
    }

    /// The prompt list changed (`notifications/prompts/list_changed`).
    pub fn prompts_list_changed(&self) {
        self.subs.schedule_list_changed(ListChangedKind::Prompts);
    }

    /// `uri`'s content changed (`notifications/resources/updated`), delivered
    /// to every subscription that listed it.
    pub async fn resource_updated(&self, uri: &str) {
        self.subs.publish_resource_updated(uri).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(tools: bool, uris: &[&str]) -> draft::SubscriptionFilter {
        draft::SubscriptionFilter {
            tools_list_changed: tools.then_some(true),
            resources_list_changed: None,
            prompts_list_changed: None,
            resource_subscriptions: uris.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[tokio::test]
    async fn publish_respects_filters_and_stamps_subscription_id() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _guard = outbound::register("sub-test-conn", tx);
        let reg = Arc::new(SubscriptionRegistry::default());
        reg.insert(
            "sub-test-conn",
            &RequestId::from(1i64),
            filter(true, &["file://a"]),
        );
        reg.insert("sub-test-conn", &RequestId::from(2i64), filter(false, &[]));

        reg.publish_resource_updated("file://a").await;
        reg.publish(methods::notification::TOOLS_LIST_CHANGED, None, |f| {
            f.tools_list_changed == Some(true)
        })
        .await;

        let mut methods_seen = Vec::new();
        while let Ok(msg) = rx.try_recv() {
            let JsonRpcMessage::Notification(n) = msg else {
                panic!("expected notification");
            };
            let meta = &n.params.as_ref().unwrap()["_meta"];
            assert_eq!(
                meta[meta::keys::SUBSCRIPTION_ID],
                json!(1),
                "only subscription 1 opted in to anything — and the id rides verbatim (a number, not \"1\")"
            );
            methods_seen.push(n.method);
        }
        assert_eq!(
            methods_seen,
            vec![
                methods::notification::RESOURCES_UPDATED.to_owned(),
                methods::notification::TOOLS_LIST_CHANGED.to_owned(),
            ]
        );
    }

    #[tokio::test]
    async fn close_all_answers_every_subscription_and_clears() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _guard = outbound::register("close-conn", tx);
        let reg = Arc::new(SubscriptionRegistry::default());
        reg.insert("close-conn", &RequestId::from(7i64), filter(true, &[]));
        reg.insert(
            "close-conn",
            &RequestId::String("listen-a".into()),
            filter(true, &[]),
        );

        reg.close_all().await;

        // Each listen request is answered with the frozen `2026-07-28` closing
        // envelope: `resultType: complete` plus the subscription id, carried on
        // a response to the listen request's own id.
        let mut closed = Vec::new();
        while let Ok(JsonRpcMessage::Response(r)) = rx.try_recv() {
            let result = r.result.expect("a result");
            assert_eq!(result["resultType"], "complete");
            assert_eq!(
                result["_meta"]["io.modelcontextprotocol/subscriptionId"],
                subscription_id_value(&r.id)
            );
            closed.push(r.id);
        }
        assert_eq!(closed.len(), 2, "every live subscription is answered");
        assert!(closed.contains(&RequestId::from(7i64)));
        assert!(closed.contains(&RequestId::String("listen-a".into())));

        // The registry is empty: a publish reaches nobody.
        reg.publish(methods::notification::TOOLS_LIST_CHANGED, None, |_| true)
            .await;
        assert!(rx.try_recv().is_err(), "no subscriptions remain");
    }

    #[tokio::test]
    async fn dead_connections_are_pruned_on_publish() {
        let reg = Arc::new(SubscriptionRegistry::default());
        reg.insert(
            "never-registered",
            &RequestId::from(1i64),
            filter(true, &[]),
        );
        reg.publish(methods::notification::TOOLS_LIST_CHANGED, None, |_| true)
            .await;
        assert!(
            !reg.remove("never-registered", &RequestId::from(1i64)),
            "publish should have pruned the dead subscription"
        );
    }

    #[tokio::test]
    async fn list_changed_bursts_coalesce_into_one_notification() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let _guard = outbound::register("coalesce-conn", tx);
        let reg = Arc::new(SubscriptionRegistry::default());
        reg.insert("coalesce-conn", &RequestId::from(1i64), filter(true, &[]));

        let notifier = ServerNotifier::new(Arc::clone(&reg));
        for _ in 0..5 {
            notifier.tools_list_changed();
        }
        tokio::time::sleep(Duration::from_millis(COALESCE_WINDOW_MS * 3)).await;

        let first = rx.try_recv().expect("one coalesced notification");
        assert!(matches!(
            first,
            JsonRpcMessage::Notification(n) if n.method == methods::notification::TOOLS_LIST_CHANGED
        ));
        assert!(rx.try_recv().is_err(), "the burst coalesced into one");
    }
}
