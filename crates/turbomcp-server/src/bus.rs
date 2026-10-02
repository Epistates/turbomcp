//! Carrying change notifications across replicas.
//!
//! A [`ServerNotifier`](crate::ServerNotifier) reaches the subscriptions its
//! own process holds: a `subscriptions/listen` stream, a session's `GET`
//! stream, a `resources/subscribe`. Behind a load balancer the client that
//! should hear about a change may be holding its stream on another replica.
//! A [`NotificationBus`] closes that gap: with one installed
//! ([`ServerBuilder::with_notification_bus`](crate::ServerBuilder::with_notification_bus)),
//! the notifier publishes each change to the bus instead, and every replica
//! delivers what the bus carries (its own changes included) to the
//! subscriptions it holds.
//!
//! The trait is the seam; the transport is yours: Redis pub/sub, NATS,
//! Postgres `LISTEN`/`NOTIFY`, a Kafka topic. [`Change`] is serde, so a
//! bus serializes it as it likes. [`LocalBus`] is the in-process one, for
//! several servers in one process and for tests.
//!
//! Delivery is at-most-once, as list-changed and resource-updated
//! notifications are: a missed one costs a client a stale cache until the
//! next, not correctness.

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use serde::{Deserialize, Serialize};

/// A change subscribers hear about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
#[non_exhaustive]
pub enum Change {
    /// `notifications/tools/list_changed`.
    ToolsListChanged,
    /// `notifications/resources/list_changed`.
    ResourcesListChanged,
    /// `notifications/prompts/list_changed`.
    PromptsListChanged,
    /// `notifications/resources/updated` for `uri`.
    ResourceUpdated {
        /// The resource that changed.
        uri: String,
    },
}

/// A [`NotificationBus`] could not publish.
#[derive(Debug, Clone, thiserror::Error)]
#[error("notification bus: {0}")]
pub struct BusError(pub String);

/// Carries [`Change`]s between replicas.
#[async_trait]
pub trait NotificationBus: Send + Sync + 'static {
    /// Tell every replica, this one included, about `change`. An `Err` is
    /// logged, and the change is delivered to this replica's subscriptions
    /// alone.
    async fn publish(&self, change: Change) -> Result<(), BusError>;

    /// Every change published from now on, by any replica (this one
    /// included). Called once per server; the stream should end only when the
    /// bus does.
    fn subscribe(&self) -> BoxStream<'static, Change>;
}

/// An in-process [`NotificationBus`] over a Tokio broadcast channel: the
/// servers sharing one see each other's changes. A subscriber that falls
/// more than `capacity` changes behind skips the ones it missed.
#[derive(Clone, Debug)]
pub struct LocalBus {
    tx: tokio::sync::broadcast::Sender<Change>,
}

impl LocalBus {
    /// A bus buffering up to `capacity` changes per subscriber.
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            tx: tokio::sync::broadcast::channel(capacity.max(1)).0,
        }
    }
}

impl Default for LocalBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

#[async_trait]
impl NotificationBus for LocalBus {
    async fn publish(&self, change: Change) -> Result<(), BusError> {
        // No subscriber yet is not a failure: nothing is listening to miss it.
        let _ = self.tx.send(change);
        Ok(())
    }

    fn subscribe(&self) -> BoxStream<'static, Change> {
        let rx = self.tx.subscribe();
        Box::pin(futures::stream::unfold(rx, |mut rx| async move {
            loop {
                match rx.recv().await {
                    Ok(change) => return Some((change, rx)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(missed)) => {
                        tracing::warn!(missed, "a notification bus subscriber fell behind");
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
                }
            }
        }))
    }
}

/// A bus shared by every clone of a dispatcher, and whether its changes are
/// being delivered yet.
#[derive(Clone)]
pub(crate) struct Installed {
    pub(crate) bus: Arc<dyn NotificationBus>,
    started: Arc<std::sync::atomic::AtomicBool>,
}

impl Installed {
    pub(crate) fn new(bus: Arc<dyn NotificationBus>) -> Self {
        Self {
            bus,
            started: Arc::default(),
        }
    }

    /// Start delivering the bus's changes to `subs`, once, if there is a
    /// runtime to do it on. The task ends with the bus's stream, or once the
    /// registry is gone.
    pub(crate) fn start(
        &self,
        subs: &Arc<crate::subscriptions::SubscriptionRegistry>,
        advertised: [bool; 3],
    ) {
        use std::sync::atomic::Ordering;
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        if self.started.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut changes = self.bus.subscribe();
        let subs = Arc::downgrade(subs);
        runtime.spawn(async move {
            use futures::StreamExt;
            while let Some(change) = changes.next().await {
                let Some(subs) = subs.upgrade() else { break };
                crate::subscriptions::deliver(&subs, advertised, change).await;
            }
        });
    }
}
