//! One live connection to an upstream, and what makes them.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::broadcast;
use turbomcp_client::{Client, ClientBuilder, NotificationHandler};
use turbomcp_core::McpResult;
use turbomcp_protocol::{methods, neutral};
use turbomcp_server::bus::Change;

use crate::ProxyError;
use crate::bridge::{Completions, UpstreamHandlers};
use crate::process::ChildProcess;

/// How old a catalogue must be before a miss refetches it.
const MISS_REFETCH_AFTER: Duration = Duration::from_secs(1);

/// How many changes one connection's tap may hold before skipping.
const CHANGE_BUFFER: usize = 64;

/// What opening a connection yields: the handshaken client, and the child
/// process behind it when the upstream is a command.
pub(crate) type Opened = (Client, Option<ChildProcess>);

/// Opens a connection with the client a [`ClientBuilder`] describes.
pub(crate) type Dial = Arc<
    dyn Fn(ClientBuilder) -> Pin<Box<dyn Future<Output = Result<Opened, ProxyError>> + Send>>
        + Send
        + Sync,
>;

/// Makes [`Link`]s to one upstream, all configured alike.
pub(crate) struct Linker {
    pub(crate) label: String,
    pub(crate) dial: Dial,
    pub(crate) client: ClientBuilder,
    pub(crate) catalog_ttl: Duration,
    pub(crate) forward_input: bool,
    pub(crate) serialize: bool,
    /// Where every link's change notifications go.
    pub(crate) changes: broadcast::Sender<Change>,
}

impl Linker {
    /// Open a link and run the handshake.
    pub(crate) async fn link(&self) -> Result<Arc<Link>, ProxyError> {
        let (tap, _) = broadcast::channel(CHANGE_BUFFER);
        let completions = crate::bridge::completions();
        let mut client = self
            .client
            .clone()
            .with_notifications(ChangeTap(tap.clone()));
        if self.forward_input {
            let handlers = UpstreamHandlers {
                completions: completions.clone(),
            };
            client = client
                .with_elicitation(handlers.clone())
                .with_sampling(handlers.clone())
                .with_roots(handlers);
        }
        let (client, child) = (self.dial)(client).await?;
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
            client,
        });
        link.listen(&tap, &self.label).await;
        link.relay_changes(tap.subscribe(), self.changes.clone());
        Ok(link)
    }
}

/// One live connection to an upstream, with what is cached from it.
pub(crate) struct Link {
    pub(crate) client: Client,
    pub(crate) tools: Catalog<neutral::Tool>,
    pub(crate) prompts: Catalog<neutral::Prompt>,
    pub(crate) resources: Catalog<neutral::Resource>,
    pub(crate) templates: Catalog<neutral::ResourceTemplate>,
    /// Where this upstream's URL elicitations finish (its ids are its own).
    pub(crate) completions: Completions,
    /// Held across a call that may ask for input, when calls are serialized.
    serial: Option<tokio::sync::Mutex<()>>,
    child: tokio::sync::Mutex<Option<ChildProcess>>,
    closed: AtomicBool,
    /// Calls using the connection now.
    leases: AtomicUsize,
    /// Told when the last call using it ends.
    released: tokio::sync::Notify,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
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
        if let Ok(tasks) = self.tasks.get_mut() {
            for task in tasks.drain(..) {
                task.abort();
            }
        }
    }
}

impl Link {
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

    /// On `2026-07-28`, change notifications come only to a
    /// `subscriptions/listen` stream; open one for whatever list changes the
    /// upstream says it announces. (Older revisions send them unasked, and
    /// [`ChangeTap`] already hears them.)
    async fn listen(&self, tap: &broadcast::Sender<Change>, label: &str) {
        if self.client.protocol_version().is_stateful() {
            return;
        }
        let caps = self.client.server_capabilities();
        let mut filter = neutral::SubscriptionFilter::new();
        filter.tools_list_changed = caps.tools.as_ref().is_some_and(|c| c.list_changed);
        filter.prompts_list_changed = caps.prompts.as_ref().is_some_and(|c| c.list_changed);
        filter.resources_list_changed = caps.resources.as_ref().is_some_and(|c| c.list_changed);
        if !(filter.tools_list_changed
            || filter.prompts_list_changed
            || filter.resources_list_changed)
        {
            return;
        }
        match self.client.listen(filter).await {
            Ok(mut subscription) => {
                let tap = tap.clone();
                let task = tokio::spawn(async move {
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
                });
                self.tasks.lock().expect("link tasks").push(task);
            }
            Err(e) => tracing::warn!(
                upstream = %label,
                error = %e,
                "could not subscribe to the upstream's change notifications"
            ),
        }
    }

    /// Drop a catalogue the moment the upstream says its list changed, and
    /// pass the change on.
    fn relay_changes(
        self: &Arc<Self>,
        mut rx: broadcast::Receiver<Change>,
        out: broadcast::Sender<Change>,
    ) {
        let weak = Arc::downgrade(self);
        let task = tokio::spawn(async move {
            loop {
                let change = match rx.recv().await {
                    Ok(change) => Some(change),
                    Err(broadcast::error::RecvError::Lagged(_)) => None,
                    Err(broadcast::error::RecvError::Closed) => return,
                };
                let Some(link) = weak.upgrade() else { return };
                match &change {
                    Some(Change::ToolsListChanged) => link.tools.invalidate(),
                    Some(Change::PromptsListChanged) => link.prompts.invalidate(),
                    Some(Change::ResourcesListChanged) => {
                        link.resources.invalidate();
                        link.templates.invalidate();
                    }
                    Some(_) => {}
                    // Missed some: anything may have changed.
                    None => {
                        link.tools.invalidate();
                        link.prompts.invalidate();
                        link.resources.invalidate();
                        link.templates.invalidate();
                    }
                }
                match change {
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
        self.tasks.lock().expect("link tasks").push(task);
    }
}

/// Hears the upstream's change notifications on the stateful revisions.
struct ChangeTap(broadcast::Sender<Change>);

#[async_trait::async_trait]
impl NotificationHandler for ChangeTap {
    async fn on_notification(&self, method: String, params: Option<Value>) {
        use methods::notification as n;
        let change = match method.as_str() {
            n::TOOLS_LIST_CHANGED => Change::ToolsListChanged,
            n::PROMPTS_LIST_CHANGED => Change::PromptsListChanged,
            n::RESOURCES_LIST_CHANGED => Change::ResourcesListChanged,
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
        let _ = self.0.send(change);
    }
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
        *self.cached.lock().expect("catalog lock") = None;
    }

    fn fresh(&self) -> Option<Arc<HashMap<String, T>>> {
        self.cached
            .lock()
            .expect("catalog lock")
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
        *self.cached.lock().expect("catalog lock") = Some((Instant::now(), Arc::clone(&map)));
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
            .expect("catalog lock")
            .as_ref()
            .is_some_and(|(at, _)| at.elapsed() < MISS_REFETCH_AFTER);
        if recent {
            return Ok(None);
        }
        self.invalidate();
        Ok(self.all(&fetch).await?.get(key).cloned())
    }
}
