//! Subscription registry + [`ServerNotifier`] — both protocol versions.
//!
//! **Draft (`subscriptions/listen`):** a subscription is `(connection,
//! listen-request id)` plus the filter subset the server agreed to honor
//! (subscriptions spec: the server **MUST NOT** send notification types the
//! client didn't opt in to), delivered through the connection's [`Peer`]. A
//! closed peer means the connection went, and the subscription is pruned on
//! the spot (on stdio the server holds no subscription state across
//! reconnections, per spec). Pruning also runs when a *new* subscription is
//! recorded, because a server whose data never changes never publishes and
//! would otherwise keep every subscription any departed client ever opened.
//!
//! **Legacy (`2025-11-25`):** subscriptions are per *session* —
//! `resources/subscribe` adds a URI; `*_list_changed` goes to every live
//! legacy session unconditionally (the old protocol has no opt-in filter; the
//! capability advertisement is the contract). Delivery prefers the session's
//! HTTP `GET` SSE stream (from the transport's [`SessionStreams`]) and falls
//! back to the byte-pipe connection the session was last seen on (stdio).
//! Routes without a reachable stream are kept (an HTTP client may open its
//! `GET` stream later), bounded by [`MAX_LEGACY_ROUTES`].
//!
//! `*_list_changed` publishes are coalesced: bursts inside
//! [`COALESCE_WINDOW_MS`] collapse into one notification per kind.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use turbomcp_core::{Extensions, JsonRpcMessage, JsonRpcNotification, RequestId, SessionId, meta};
use turbomcp_protocol::methods;
use turbomcp_protocol::v2026_07_28::types as v0728;
use turbomcp_service::{Delivery, Peer, SessionStreams};

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

    fn wants(self, filter: &v0728::SubscriptionFilter) -> bool {
        match self {
            Self::Tools => filter.tools_list_changed == Some(true),
            Self::Resources => filter.resources_list_changed == Some(true),
            Self::Prompts => filter.prompts_list_changed == Some(true),
        }
    }
}

/// Upper bound on tracked legacy session routes. At capacity a route with no
/// reachable stream goes first, then the least recently seen one.
pub(crate) const MAX_LEGACY_ROUTES: usize = 4096;

/// A legacy session's delivery route: where its messages go and which
/// resource URIs it subscribed to.
struct LegacyRoute {
    /// The connection the session was last seen on: the stdio delivery path
    /// (on HTTP, a finished request's stream, closed and so never used).
    connection: Option<Peer>,
    /// The HTTP transport's registry of session `GET` streams, when the
    /// session rides Streamable HTTP.
    streams: Option<SessionStreams>,
    uris: HashSet<String>,
    /// When the session last sent anything.
    last_seen: Instant,
}

impl LegacyRoute {
    fn new() -> Self {
        Self {
            connection: None,
            streams: None,
            uris: HashSet::new(),
            last_seen: Instant::now(),
        }
    }

    /// Where a session-scoped publish (list_changed, resources/updated) goes:
    /// the HTTP `GET` stream first, then the byte-pipe connection. `None` is
    /// not an error; an HTTP client may simply not have its stream open.
    fn peer(&self, session: &str) -> Option<Peer> {
        self.streams
            .as_ref()
            .and_then(|streams| streams.get(session))
            .or_else(|| self.connection.clone().filter(Peer::is_open))
    }
}

/// A draft subscription: where it is delivered and what it asked for.
struct Subscription {
    peer: Peer,
    filter: v0728::SubscriptionFilter,
}

/// Shared map of live subscriptions; dispatcher clones share it via `Arc`.
#[derive(Default)]
pub(crate) struct SubscriptionRegistry {
    inner: Mutex<HashMap<(String, RequestId), Subscription>>,
    /// Legacy (`2025-11-25`) per-session routes, keyed by session id.
    legacy: Mutex<HashMap<String, LegacyRoute>>,
    /// One pending-flush flag per [`ListChangedKind`] slot.
    pending: [AtomicBool; 3],
}

impl SubscriptionRegistry {
    pub(crate) fn insert(&self, peer: &Peer, id: &RequestId, filter: v0728::SubscriptionFilter) {
        let mut live = self.lock();
        // Reclaim subscriptions whose connection has since closed. `publish`
        // does this too, but a server whose data never changes never publishes,
        // and would otherwise accumulate one entry per client that ever
        // subscribed and went away. Subscribing is where the map grows, so it
        // is also where it shrinks; a closed peer *is* what dead means, so
        // this can never disturb a live subscription. O(n) under a lock, but n
        // is the number of open subscriptions and a listen is a rare event.
        live.retain(|_, sub| sub.peer.is_open());
        live.insert(
            (peer.id().as_str().to_owned(), id.clone()),
            Subscription {
                peer: peer.clone(),
                filter,
            },
        );
    }

    /// Drop the subscription opened by `(connection, id)`, if any. Wired to
    /// `notifications/cancelled` referencing the listen request id.
    pub(crate) fn remove(&self, connection: &str, id: &RequestId) -> bool {
        self.lock()
            .remove(&(connection.to_owned(), id.clone()))
            .is_some()
    }

    // ---- legacy (2025-11-25) session routes -----------------------------------

    /// Record (or refresh) where a legacy session's messages can be delivered,
    /// from what the request's transport attached. Called on every legacy
    /// dispatch so the stdio fallback stays current.
    pub(crate) fn legacy_touch(&self, session: &str, ext: &Extensions) {
        let peer = ext.get::<Peer>();
        let mut routes = self.lock_legacy();
        if !routes.contains_key(session) {
            // A byte pipe carries one session at a time. A new one on the same
            // connection (the client initialized again) replaces the old, whose
            // route would otherwise deliver every notification twice and keep
            // sending updates for URIs the new session never subscribed to.
            if let Some(peer) = peer {
                routes
                    .retain(|_, route| route.connection.as_ref().map(Peer::id) != Some(peer.id()));
            }
            if routes.len() >= MAX_LEGACY_ROUTES {
                // A route nothing can be delivered to is dead weight; after
                // that, the one idle longest. Evicting at random threw away
                // live sessions' subscriptions.
                let victim = routes
                    .iter()
                    .filter(|(id, route)| route.peer(id).is_none())
                    .min_by_key(|(_, route)| route.last_seen)
                    .or_else(|| routes.iter().min_by_key(|(_, route)| route.last_seen))
                    .map(|(id, _)| id.clone());
                if let Some(victim) = victim {
                    routes.remove(&victim);
                }
            }
        }
        let route = routes
            .entry(session.to_owned())
            .or_insert_with(LegacyRoute::new);
        route.last_seen = Instant::now();
        if let Some(peer) = peer {
            route.connection = Some(peer.clone());
        }
        if let Some(streams) = ext.get::<SessionStreams>() {
            route.streams = Some(streams.clone());
        }
    }

    /// Legacy `resources/subscribe`: deliver `notifications/resources/updated`
    /// for `uri` to this session.
    pub(crate) fn legacy_subscribe(&self, session: &str, ext: &Extensions, uri: String) {
        self.legacy_touch(session, ext);
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
        // No stream right now is not a reason to drop the route: an HTTP
        // client may reconnect its `GET`.
        let targets: Vec<Peer> = self
            .lock_legacy()
            .iter()
            .filter(|(_, route)| wants(route))
            .filter_map(|(session, route)| route.peer(session))
            .collect();

        for peer in targets {
            let params = extra
                .as_ref()
                .map(|(key, value)| json!({ *key: value.clone() }));
            let note = JsonRpcNotification::new(method, params);
            // Never waits on one session: see `Peer::offer`.
            peer.offer(note.into());
        }
    }

    /// Deliver `method` to every subscription whose filter passes `wants`,
    /// stamping each copy with its subscription id. Subscriptions whose
    /// connection is gone are pruned.
    async fn publish(
        &self,
        method: &str,
        extra: Option<(&str, Value)>,
        wants: impl Fn(&v0728::SubscriptionFilter) -> bool,
    ) {
        let targets: Vec<(String, RequestId, Peer)> = self
            .lock()
            .iter()
            .filter(|(_, sub)| wants(&sub.filter))
            .map(|((conn, id), sub)| (conn.clone(), id.clone(), sub.peer.clone()))
            .collect();

        for (connection, id, peer) in targets {
            let note = subscription_notification(method, &id, extra.clone());
            // Never waits on one subscriber: see `Peer::offer`.
            if peer.offer(note) == Delivery::Closed {
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
        let targets: Vec<(RequestId, Peer)> = {
            let mut live = self.lock();
            live.drain().map(|((_, id), sub)| (id, sub.peer)).collect()
        };
        for (id, peer) in targets {
            let result = json!({
                "resultType": turbomcp_protocol::neutral::result_type::COMPLETE,
                "_meta": { meta::keys::SUBSCRIPTION_ID: subscription_id_value(&id) },
            });
            let _ = peer
                .send(turbomcp_core::JsonRpcResponse::success(id, result).into())
                .await;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<(String, RequestId), Subscription>> {
        self.inner.lock().expect("subscription registry poisoned")
    }

    fn lock_legacy(&self) -> std::sync::MutexGuard<'_, HashMap<String, LegacyRoute>> {
        self.legacy.lock().expect("legacy route registry poisoned")
    }
}

/// Where a request's server→client messages go (inline requests, progress,
/// log lines): the originating request's own stream first (its POST SSE
/// response on HTTP, the pipe on stdio), per the transports spec's SHOULD; then,
/// on the legacy wires only, the session's `GET` stream (a MAY). The draft
/// forbids delivering request-scoped messages on any stream but the request's
/// own, so a draft route has no fallback.
#[derive(Clone, Default, Debug)]
pub(crate) struct Route {
    peer: Option<Peer>,
    session: Option<(SessionStreams, SessionId)>,
}

impl Route {
    /// The route for a request, from what its transport attached.
    pub(crate) fn for_request(ext: &Extensions, session_fallback: bool) -> Self {
        let session = session_fallback
            .then(|| {
                ext.get::<SessionStreams>()
                    .cloned()
                    .zip(ext.get::<SessionId>().cloned())
            })
            .flatten();
        Self {
            peer: ext.get::<Peer>().cloned(),
            session,
        }
    }

    /// A route to exactly `peer` (tests).
    #[cfg(test)]
    pub(crate) fn to(peer: Peer) -> Self {
        Self {
            peer: Some(peer),
            session: None,
        }
    }

    /// The live stream to write to right now, if any.
    pub(crate) fn peer(&self) -> Option<Peer> {
        self.peer.clone().filter(Peer::is_open).or_else(|| {
            self.session
                .as_ref()
                .and_then(|(streams, session)| streams.get(session.as_str()))
        })
    }
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
    /// Which list capabilities the server advertises, by [`ListChangedKind`]
    /// slot. Announcing a change to one it never advertised would send a
    /// legacy session a notification for a capability it was told does not
    /// exist.
    advertised: [bool; 3],
}

impl core::fmt::Debug for ServerNotifier {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ServerNotifier")
            .field("subscriptions", &self.subs.lock().len())
            .finish_non_exhaustive()
    }
}

impl ServerNotifier {
    pub(crate) fn new(subs: Arc<SubscriptionRegistry>, advertised: [bool; 3]) -> Self {
        Self { subs, advertised }
    }

    fn list_changed(&self, kind: ListChangedKind) {
        if self.advertised[kind.slot()] {
            self.subs.schedule_list_changed(kind);
        } else {
            tracing::debug!(
                ?kind,
                "list changed for a capability this server does not have"
            );
        }
    }

    /// The tool list changed (`notifications/tools/list_changed`). A no-op on
    /// a server without tools.
    pub fn tools_list_changed(&self) {
        self.list_changed(ListChangedKind::Tools);
    }

    /// The resource list changed (`notifications/resources/list_changed`). A
    /// no-op on a server without resources.
    pub fn resources_list_changed(&self) {
        self.list_changed(ListChangedKind::Resources);
    }

    /// The prompt list changed (`notifications/prompts/list_changed`). A no-op
    /// on a server without prompts.
    pub fn prompts_list_changed(&self) {
        self.list_changed(ListChangedKind::Prompts);
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

    fn filter(tools: bool, uris: &[&str]) -> v0728::SubscriptionFilter {
        v0728::SubscriptionFilter {
            tools_list_changed: tools.then_some(true),
            resources_list_changed: None,
            prompts_list_changed: None,
            resource_subscriptions: uris.iter().map(|s| (*s).to_owned()).collect(),
        }
    }

    #[tokio::test]
    async fn publish_respects_filters_and_stamps_subscription_id() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let peer = Peer::new("sub-test-conn", &tx);
        let reg = Arc::new(SubscriptionRegistry::default());
        reg.insert(&peer, &RequestId::from(1i64), filter(true, &["file://a"]));
        reg.insert(&peer, &RequestId::from(2i64), filter(false, &[]));

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
        let peer = Peer::new("close-conn", &tx);
        let reg = Arc::new(SubscriptionRegistry::default());
        reg.insert(&peer, &RequestId::from(7i64), filter(true, &[]));
        reg.insert(
            &peer,
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
            let id = r.id.expect("correlated to the listen request");
            assert_eq!(result["resultType"], "complete");
            assert_eq!(
                result["_meta"]["io.modelcontextprotocol/subscriptionId"],
                subscription_id_value(&id)
            );
            closed.push(id);
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
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let gone = Peer::new("never-registered", &tx);
        drop(tx);
        reg.insert(&gone, &RequestId::from(1i64), filter(true, &[]));
        reg.publish(methods::notification::TOOLS_LIST_CHANGED, None, |_| true)
            .await;
        assert!(
            !reg.remove("never-registered", &RequestId::from(1i64)),
            "publish should have pruned the dead subscription"
        );
    }

    /// Pruning only on publish leaves a server whose data never changes with
    /// nothing to reclaim on: clients that subscribe and vanish accumulate.
    /// Subscribing is itself the moment the map grows, so it is also where the
    /// dead entries go — no live subscription is ever disturbed, because a
    /// closed peer is what "dead" means here.
    #[tokio::test]
    async fn subscribing_reclaims_the_connections_that_have_since_gone() {
        let reg = Arc::new(SubscriptionRegistry::default());
        for i in 0..50 {
            let (tx, _rx) = tokio::sync::mpsc::channel(1);
            let gone = Peer::new(format!("gone-{i}"), &tx);
            reg.insert(&gone, &RequestId::from(i64::from(i)), filter(true, &[]));
        }

        // One live subscriber, arriving after the others have gone.
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let peer = Peer::new("still-here", &tx);
        reg.insert(&peer, &RequestId::from(99i64), filter(true, &[]));

        assert_eq!(
            reg.lock().len(),
            1,
            "50 subscriptions whose writers are gone must not outlive them"
        );
        assert!(
            reg.remove("still-here", &RequestId::from(99i64)),
            "the live subscription survives the sweep"
        );
    }

    #[tokio::test]
    async fn list_changed_bursts_coalesce_into_one_notification() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let peer = Peer::new("coalesce-conn", &tx);
        let reg = Arc::new(SubscriptionRegistry::default());
        reg.insert(&peer, &RequestId::from(1i64), filter(true, &[]));

        let notifier = ServerNotifier::new(Arc::clone(&reg), [true; 3]);
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

    /// One subscriber that stops reading used to stall delivery to every other
    /// one, and park the task that published, because each send waited for
    /// room in that subscriber's queue.
    #[tokio::test]
    async fn a_subscriber_that_stops_reading_does_not_stall_the_rest() {
        let (stuck_tx, _stuck_rx) = tokio::sync::mpsc::channel(1);
        let (live_tx, mut live_rx) = tokio::sync::mpsc::channel(8);
        let stuck = Peer::new("stalled-reader", &stuck_tx);
        let live = Peer::new("live-reader", &live_tx);
        let reg = Arc::new(SubscriptionRegistry::default());
        reg.insert(&stuck, &RequestId::from(1i64), filter(false, &["x"]));
        reg.insert(&live, &RequestId::from(2i64), filter(false, &["x"]));

        let notifier = ServerNotifier::new(Arc::clone(&reg), [true; 3]);
        tokio::time::timeout(Duration::from_secs(1), async {
            for _ in 0..3 {
                notifier.resource_updated("x").await;
            }
        })
        .await
        .expect("publishing must not wait on the stalled reader");
        for _ in 0..3 {
            assert!(
                live_rx.try_recv().is_ok(),
                "the live reader got every update"
            );
        }
        assert_eq!(reg.lock().len(), 2, "a slow reader is not unsubscribed");
    }

    #[tokio::test]
    async fn announcing_an_unadvertised_capability_sends_nothing() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let peer = Peer::new("tools-only-conn", &tx);
        let reg = Arc::new(SubscriptionRegistry::default());
        reg.legacy_touch("sess", &Extensions::new().with(peer.clone()));

        let notifier = ServerNotifier::new(Arc::clone(&reg), [true, false, false]);
        notifier.resources_list_changed();
        notifier.prompts_list_changed();
        tokio::time::sleep(Duration::from_millis(COALESCE_WINDOW_MS * 3)).await;
        assert!(
            rx.try_recv().is_err(),
            "nothing for capabilities not advertised"
        );

        notifier.tools_list_changed();
        tokio::time::sleep(Duration::from_millis(COALESCE_WINDOW_MS * 3)).await;
        assert!(rx.try_recv().is_ok());
    }
}
