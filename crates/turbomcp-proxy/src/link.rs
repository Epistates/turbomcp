//! One live connection to an upstream, and what makes them.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use parking_lot::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::broadcast;
use turbomcp_client::{Client, ClientBuilder, NotificationHandler, ServerProcess};
use turbomcp_core::McpResult;
use turbomcp_protocol::{methods, neutral};
use turbomcp_server::bus::Change;

use crate::ProxyError;
use crate::bridge::{Completions, UpstreamHandlers};

/// How old a catalogue must be before a miss refetches it.
const MISS_REFETCH_AFTER: Duration = Duration::from_secs(1);

/// How many changes one connection's tap may hold before skipping.
const CHANGE_BUFFER: usize = 64;

/// What opening a connection yields: the handshaken client, and the child
/// process behind it when the upstream is a command.
pub(crate) type Opened = (Client, Option<ServerProcess>);

/// The caller a connection acts for, when the upstream sees callers (token
/// exchange): the latest token they presented, refreshed by each call.
pub(crate) type Subject = Arc<Mutex<Option<turbomcp_service::SubjectToken>>>;

/// Opens a connection with the client a [`ClientBuilder`] describes, for
/// the caller in `Subject` (`None`: the gateway itself, as at startup).
pub(crate) type Dial = Arc<
    dyn Fn(
            ClientBuilder,
            Option<Subject>,
        ) -> Pin<Box<dyn Future<Output = Result<Opened, ProxyError>> + Send>>
        + Send
        + Sync,
>;

/// Makes [`Link`]s to one upstream, all configured alike.
pub(crate) struct Linker {
    pub(crate) label: String,
    pub(crate) dial: Dial,
    pub(crate) client: ClientBuilder,
    pub(crate) catalog_ttl: Duration,
    pub(crate) inputs: Inputs,
    pub(crate) serialize: bool,
    /// Where every link's change notifications go.
    pub(crate) changes: broadcast::Sender<Change>,
    /// The resource URIs downstream clients watch, which every link
    /// subscribes to upstream (see [`Link::sync_watches`]).
    pub(crate) watched: Watched,
    /// What the upstream said about itself at the first handshake.
    pub(crate) profile: Arc<Mutex<Option<Profile>>>,
    /// Every connection opened, for a change one reports to reach the
    /// catalogues of all.
    pub(crate) siblings: Siblings,
    /// Where a connection's `resources/updated` goes to be checked against
    /// what that upstream serves before anyone hears it.
    pub(crate) updates: Updates,
}

/// A `resources/updated` an upstream connection reported, to be checked.
pub(crate) type Updates = tokio::sync::mpsc::Sender<(std::sync::Weak<Link>, String)>;

/// How many reported updates may wait to be checked; past it they drop.
pub(crate) const UPDATES_BUFFER: usize = 1024;

/// The connections to one upstream.
pub(crate) type Siblings = Arc<Mutex<Vec<std::sync::Weak<Link>>>>;

/// Drop the catalogues `change` makes stale, on every live connection in
/// `siblings` (`None`: changes were missed, so all of them). What one
/// connection says changed changed for every caller: a per-caller
/// connection otherwise served its own stale list until it expired.
fn invalidate(siblings: &Siblings, change: Option<&Change>) {
    let mut siblings = siblings.lock();
    siblings.retain(|link| link.strong_count() > 0);
    for link in siblings.iter().filter_map(std::sync::Weak::upgrade) {
        match change {
            Some(Change::ToolsListChanged) => link.tools.invalidate(),
            Some(Change::PromptsListChanged) => link.prompts.invalidate(),
            Some(Change::ResourcesListChanged) => {
                link.resources.invalidate();
                link.templates.invalidate();
            }
            Some(_) => {}
            None => {
                link.tools.invalidate();
                link.prompts.invalidate();
                link.resources.invalidate();
                link.templates.invalidate();
            }
        }
    }
}

/// What an upstream says about itself in its handshake. The gateway serves
/// the first one it saw; a later connection (after a restart, or for
/// another caller) can find the upstream changed.
#[derive(Clone, PartialEq)]
pub(crate) struct Profile {
    version: turbomcp_core::ProtocolVersion,
    capabilities: neutral::ServerCapabilities,
    server: Option<(String, String)>,
    instructions: Option<String>,
}

impl Profile {
    fn of(client: &Client) -> Self {
        Self {
            version: client.protocol_version().clone(),
            capabilities: client.server_capabilities().clone(),
            server: client
                .server_info()
                .map(|info| (info.name.clone(), info.version.clone())),
            instructions: client.instructions().map(str::to_owned),
        }
    }
}

/// Which of the upstream's requests for input reach downstream callers;
/// the rest are never declared upstream, so the upstream knows not to ask.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Inputs {
    pub(crate) elicitation: bool,
    pub(crate) sampling: bool,
    pub(crate) roots: bool,
}

impl Inputs {
    pub(crate) fn any(self) -> bool {
        self.elicitation || self.sampling || self.roots
    }
}

/// The resource URIs downstream clients watch.
pub(crate) type Watched = Arc<Mutex<std::collections::BTreeSet<String>>>;

impl Linker {
    /// Hold a new connection's handshake against the first one's. A
    /// different answer means the upstream changed under the gateway (a
    /// redeploy while it was down): downstream clients are told to list
    /// again, and what the gateway can't follow (a capability it did not
    /// serve at startup, another revision) is logged for the operator.
    fn compare(&self, client: &Client) {
        let now = Profile::of(client);
        let mut first = self.profile.lock();
        let Some(first) = first.as_ref() else {
            *first = Some(now);
            return;
        };
        if *first == now {
            return;
        }
        let caps = |p: &Profile| {
            [
                p.capabilities.tools.is_some(),
                p.capabilities.prompts.is_some(),
                p.capabilities.resources.is_some(),
                p.capabilities.completions,
            ]
        };
        if first.version != now.version || caps(first) != caps(&now) {
            tracing::warn!(
                upstream = %self.label,
                was = %first.version,
                now = %now.version,
                "the upstream changed revision or capabilities since the gateway started; \
                 restart the gateway to serve what it serves now"
            );
        } else {
            tracing::info!(upstream = %self.label, "the upstream changed since the gateway started");
        }
        invalidate(&self.siblings, None);
        for change in [
            Change::ToolsListChanged,
            Change::PromptsListChanged,
            Change::ResourcesListChanged,
        ] {
            let _ = self.changes.send(change);
        }
    }

    /// Open a link, for `subject` when it acts for a caller, and run the
    /// handshake.
    pub(crate) async fn link(&self, subject: Option<Subject>) -> Result<Arc<Link>, ProxyError> {
        let (tap, _) = broadcast::channel(CHANGE_BUFFER);
        let completions = crate::bridge::completions();
        let mut client = self.client.clone().with_notifications(ChangeTap {
            changes: tap.clone(),
            upstream: self.label.clone(),
        });
        let handlers = UpstreamHandlers {
            completions: completions.clone(),
        };
        if self.inputs.elicitation {
            client = client.with_elicitation(handlers.clone());
        }
        if self.inputs.sampling {
            client = client.with_sampling(handlers.clone());
        }
        if self.inputs.roots {
            client = client.with_roots(handlers);
        }
        let (client, child) = (self.dial)(client, subject).await?;
        let link = Arc::new(Link {
            tools: Catalog::new(self.catalog_ttl),
            prompts: Catalog::new(self.catalog_ttl),
            resources: Catalog::new(self.catalog_ttl),
            templates: Catalog::new(self.catalog_ttl),
            completions,
            serial: self.serialize.then(|| tokio::sync::Mutex::new(())),
            child: tokio::sync::Mutex::new(child),
            closed: AtomicBool::new(false),
            leases: AtomicUsize::new(0),
            released: tokio::sync::Notify::new(),
            tasks: Mutex::new(Vec::new()),
            watching: tokio::sync::Mutex::new(Watching::default()),
            tap: tap.clone(),
            label: self.label.clone(),
            client,
        });
        self.siblings.lock().push(Arc::downgrade(&link));
        link.relay_changes(
            tap.subscribe(),
            self.changes.clone(),
            Arc::clone(&self.siblings),
            self.updates.clone(),
        );
        self.compare(&link.client);
        // What downstream clients already watch, this link watches too.
        let watched = self.watched.lock().clone();
        link.sync_watches(watched, true).await;
        Ok(link)
    }
}

/// One live connection to an upstream, with what is cached from it.
pub(crate) struct Link {
    pub(crate) client: Client,
    /// Shared: a call through the gateway gets the catalogue's own `Arc`,
    /// whose compiled input schema the dispatcher keeps.
    pub(crate) tools: Catalog<Arc<neutral::Tool>>,
    pub(crate) prompts: Catalog<neutral::Prompt>,
    pub(crate) resources: Catalog<neutral::Resource>,
    pub(crate) templates: Catalog<neutral::ResourceTemplate>,
    /// Where this upstream's URL elicitations finish (its ids are its own).
    pub(crate) completions: Completions,
    /// Held across a call that may ask for input, when calls are serialized.
    serial: Option<tokio::sync::Mutex<()>>,
    child: tokio::sync::Mutex<Option<ServerProcess>>,
    closed: AtomicBool,
    /// Calls using the connection now.
    leases: AtomicUsize,
    /// Told when the last call using it ends.
    released: tokio::sync::Notify,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    /// What this connection is subscribed to upstream.
    watching: tokio::sync::Mutex<Watching>,
    /// Where its change notifications go first.
    tap: broadcast::Sender<Change>,
    label: String,
}

/// One connection's upstream subscriptions.
#[derive(Default)]
struct Watching {
    /// The resource URIs subscribed to.
    uris: std::collections::BTreeSet<String>,
    /// On `2026-07-28`, the `subscriptions/listen` stream carrying them (and
    /// the list changes).
    listener: Option<tokio::task::JoinHandle<()>>,
}

/// One call's use of a [`Link`]: a connection is not closed under a call.
pub(crate) struct Lease(Arc<Link>);

impl Lease {
    pub(crate) fn new(link: Arc<Link>) -> Self {
        link.leases.fetch_add(1, Ordering::AcqRel);
        Self(link)
    }
}

impl core::ops::Deref for Lease {
    type Target = Link;

    fn deref(&self) -> &Link {
        &self.0
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.0.leases.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.released.notify_waiters();
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        for task in self.tasks.get_mut().drain(..) {
            task.abort();
        }
        if let Some(listener) = self.watching.get_mut().listener.take() {
            listener.abort();
        }
    }
}

impl Link {
    /// Whether a call is using the connection.
    pub(crate) fn in_use(&self) -> bool {
        self.leases.load(Ordering::Acquire) > 0
    }

    /// Whether the connection is still up.
    pub(crate) fn is_alive(&self) -> bool {
        !self.closed.load(Ordering::Acquire) && !self.client.is_closed()
    }

    /// This link's turn for a call that may ask for input: with calls
    /// serialized, one at a time, so a request for input names its call.
    pub(crate) async fn turn(&self) -> Option<tokio::sync::MutexGuard<'_, ()>> {
        match &self.serial {
            Some(serial) => Some(serial.lock().await),
            None => None,
        }
    }

    /// Resolves once no call is using the connection, or after `limit`.
    pub(crate) async fn released(&self, limit: Duration) {
        let _ = tokio::time::timeout(limit, async {
            loop {
                let released = self.released.notified();
                if self.leases.load(Ordering::Acquire) == 0 {
                    return;
                }
                released.await;
            }
        })
        .await;
    }

    /// End the connection. A stdio upstream gets the spec's shutdown: its
    /// stdin closes, then `SIGTERM` if it hasn't exited within the grace
    /// period, then `SIGKILL`, to its whole process group.
    pub(crate) async fn shutdown(&self) {
        if self.closed.swap(true, Ordering::AcqRel) {
            return;
        }
        self.client.close().await;
        if let Some(child) = self.child.lock().await.take() {
            child.shutdown().await;
        }
    }

    /// Subscribe upstream to exactly `target` (the URIs downstream clients
    /// watch), changing only what differs; `opening` for a new connection.
    ///
    /// On the stateful revisions that is `resources/subscribe` and
    /// `resources/unsubscribe`. On `2026-07-28` change notifications come
    /// only to a `subscriptions/listen` stream, whose filter names the URIs
    /// with the list changes the upstream announces: a changed set opens a
    /// new stream before the old one ends, so nothing is missed between.
    /// Nothing is asked of an upstream that doesn't offer subscriptions.
    pub(crate) async fn sync_watches(
        &self,
        target: std::collections::BTreeSet<String>,
        opening: bool,
    ) {
        let caps = self.client.server_capabilities().clone();
        let subscribe = caps.resources.as_ref().is_some_and(|r| r.subscribe);
        let target = if subscribe {
            target
        } else {
            Default::default()
        };
        let mut watching = self.watching.lock().await;
        if self.client.protocol_version().is_stateful() {
            for uri in target.difference(&watching.uris) {
                if let Err(e) = self.client.subscribe_resource(uri.clone()).await {
                    tracing::warn!(upstream = %self.label, uri, error = %e, "upstream subscribe failed");
                }
            }
            for uri in watching.uris.difference(&target) {
                if let Err(e) = self.client.unsubscribe_resource(uri.clone()).await {
                    tracing::debug!(upstream = %self.label, uri, error = %e, "upstream unsubscribe failed");
                }
            }
            watching.uris = target;
            return;
        }
        if !opening && watching.uris == target {
            return;
        }
        let mut filter = neutral::SubscriptionFilter::new();
        filter.tools_list_changed = caps.tools.as_ref().is_some_and(|c| c.list_changed);
        filter.prompts_list_changed = caps.prompts.as_ref().is_some_and(|c| c.list_changed);
        filter.resources_list_changed = caps.resources.as_ref().is_some_and(|c| c.list_changed);
        filter.resource_subscriptions = target.iter().cloned().collect();
        let listener = if filter.tools_list_changed
            || filter.prompts_list_changed
            || filter.resources_list_changed
            || !filter.resource_subscriptions.is_empty()
        {
            match self.client.listen(filter).await {
                Ok(mut subscription) => {
                    let tap = self.tap.clone();
                    Some(tokio::spawn(async move {
                        use turbomcp_client::SubscriptionEvent as Event;
                        while let Some(event) = subscription.next().await {
                            let change = match event {
                                Event::ToolsListChanged => Change::ToolsListChanged,
                                Event::PromptsListChanged => Change::PromptsListChanged,
                                Event::ResourcesListChanged => Change::ResourcesListChanged,
                                Event::ResourceUpdated { uri } => Change::ResourceUpdated { uri },
                                _ => continue,
                            };
                            let _ = tap.send(change);
                        }
                    }))
                }
                Err(e) => {
                    tracing::warn!(
                        upstream = %self.label,
                        error = %e,
                        "could not subscribe to the upstream's change notifications"
                    );
                    return;
                }
            }
        } else {
            None
        };
        if let Some(old) = std::mem::replace(&mut watching.listener, listener) {
            old.abort();
        }
        watching.uris = target;
    }

    /// Drop the catalogues the moment the upstream says its list changed
    /// (on every connection to it), and pass the change on. A resource
    /// update goes to `updates` instead, to be relayed only for a resource
    /// this upstream serves: URIs aren't prefixed in a composite, and one
    /// upstream must not announce changes to another's resources.
    fn relay_changes(
        self: &Arc<Self>,
        mut rx: broadcast::Receiver<Change>,
        out: broadcast::Sender<Change>,
        siblings: Siblings,
        updates: Updates,
    ) {
        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            loop {
                let change = match rx.recv().await {
                    Ok(change) => Some(change),
                    Err(broadcast::error::RecvError::Lagged(_)) => None,
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                if weak.strong_count() == 0 {
                    return;
                }
                invalidate(&siblings, change.as_ref());
                match change {
                    Some(Change::ResourceUpdated { uri }) => {
                        if updates.try_send((weak.clone(), uri)).is_err() {
                            tracing::debug!(
                                "resource updates arriving faster than checked; dropped"
                            );
                        }
                    }
                    Some(change) => {
                        let _ = out.send(change);
                    }
                    None => {
                        let _ = out.send(Change::ToolsListChanged);
                        let _ = out.send(Change::PromptsListChanged);
                        let _ = out.send(Change::ResourcesListChanged);
                    }
                }
            }
        });
        self.tasks.lock().push(task);
    }
}

/// What an upstream connection hears: its changes, for the gateway's
/// callers, and its log messages, for the gateway's operator.
///
/// Log messages stay with the operator. Nothing in one says which call it
/// is about, and on a connection several callers share, relaying them to
/// the gateway's callers would hand one caller what another's call logged.
struct ChangeTap {
    changes: broadcast::Sender<Change>,
    upstream: String,
}

#[async_trait::async_trait]
impl NotificationHandler for ChangeTap {
    async fn on_notification(&self, method: String, params: Option<Value>) {
        use methods::notification as n;
        let change = match method.as_str() {
            n::TOOLS_LIST_CHANGED => Change::ToolsListChanged,
            n::PROMPTS_LIST_CHANGED => Change::PromptsListChanged,
            n::RESOURCES_LIST_CHANGED => Change::ResourcesListChanged,
            n::MESSAGE => {
                log_upstream(&self.upstream, params.as_ref());
                return;
            }
            n::RESOURCES_UPDATED => match params
                .as_ref()
                .and_then(|p| p.get("uri"))
                .and_then(Value::as_str)
            {
                Some(uri) => Change::ResourceUpdated {
                    uri: uri.to_owned(),
                },
                None => return,
            },
            _ => return,
        };
        let _ = self.changes.send(change);
    }
}

/// The most of one upstream log message logged.
const MAX_LOGGED: usize = 4096;

/// An upstream's `notifications/message`, into `tracing` on target
/// `turbomcp::upstream_log` at its severity. The data is logged as JSON,
/// whose string escaping leaves no control character for a terminal to act
/// on, and bounded: the upstream is not the gateway's to trust.
fn log_upstream(upstream: &str, params: Option<&Value>) {
    let Some(params) = params else { return };
    let level = params
        .get("level")
        .cloned()
        .and_then(|l| serde_json::from_value::<turbomcp_core::LogLevel>(l).ok())
        .unwrap_or(turbomcp_core::LogLevel::Info);
    let logger = params.get("logger").and_then(Value::as_str).map(bounded);
    let data = bounded(&params.get("data").map(Value::to_string).unwrap_or_default());
    macro_rules! at {
        ($level:ident) => {
            tracing::$level!(
                target: "turbomcp::upstream_log",
                upstream = %upstream,
                logger = ?logger,
                "{data}"
            )
        };
    }
    use turbomcp_core::LogLevel as L;
    match level {
        L::Debug => at!(debug),
        L::Info | L::Notice => at!(info),
        L::Warning => at!(warn),
        _ => at!(error),
    }
}

/// `text` cut at [`MAX_LOGGED`] bytes, on a character boundary.
fn bounded(text: &str) -> String {
    if text.len() <= MAX_LOGGED {
        return text.to_owned();
    }
    let mut end = MAX_LOGGED;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &text[..end])
}

/// A catalogue as fetched, and when.
type Snapshot<T> = (Instant, Arc<HashMap<String, T>>);

/// A looked-up catalogue: what resolves a call to its component without an
/// upstream round trip per call. Refreshed when stale, when the upstream
/// says it changed, and once on a miss (a component added upstream without
/// a notification is found on first use).
pub(crate) struct Catalog<T> {
    ttl: Duration,
    cached: Mutex<Option<Snapshot<T>>>,
    refresh: tokio::sync::Mutex<()>,
}

impl<T: Clone> Catalog<T> {
    fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            cached: Mutex::new(None),
            refresh: tokio::sync::Mutex::new(()),
        }
    }

    fn invalidate(&self) {
        *self.cached.lock() = None;
    }

    fn fresh(&self) -> Option<Arc<HashMap<String, T>>> {
        self.cached
            .lock()
            .as_ref()
            .filter(|(at, _)| at.elapsed() < self.ttl)
            .map(|(_, map)| Arc::clone(map))
    }

    /// The whole catalogue, fetched with `fetch` when it isn't fresh.
    pub(crate) async fn all<F, Fut>(&self, fetch: F) -> McpResult<Arc<HashMap<String, T>>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = McpResult<HashMap<String, T>>>,
    {
        if let Some(map) = self.fresh() {
            return Ok(map);
        }
        // One fetch for a stampede of lookups.
        let _refreshing = self.refresh.lock().await;
        if let Some(map) = self.fresh() {
            return Ok(map);
        }
        let map = Arc::new(fetch().await?);
        *self.cached.lock() = Some((Instant::now(), Arc::clone(&map)));
        Ok(map)
    }

    /// The entry under `key`, refetching once if a fresh catalogue lacks it.
    pub(crate) async fn find<F, Fut>(&self, key: &str, fetch: F) -> McpResult<Option<T>>
    where
        F: Fn() -> Fut,
        Fut: Future<Output = McpResult<HashMap<String, T>>>,
    {
        let map = self.all(&fetch).await?;
        if let Some(found) = map.get(key) {
            return Ok(Some(found.clone()));
        }
        // A miss refetches only a catalogue older than a second, so a caller
        // naming components that don't exist can't turn each request into an
        // upstream listing.
        let recent = self
            .cached
            .lock()
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < MISS_REFETCH_AFTER);
        if recent {
            return Ok(None);
        }
        self.invalidate();
        Ok(self.all(&fetch).await?.get(key).cloned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upstream_log_text_is_bounded_on_a_char_boundary() {
        assert_eq!(bounded("short"), "short");
        let long = "é".repeat(MAX_LOGGED);
        let cut = bounded(&long);
        assert!(cut.len() <= MAX_LOGGED + '…'.len_utf8());
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn upstream_log_data_carries_no_raw_control_characters() {
        // JSON string escaping is what makes the data safe to log.
        let data = Value::String("\u{1b}[31mred".into()).to_string();
        assert!(!data.contains('\u{1b}'), "{data}");
    }
}
