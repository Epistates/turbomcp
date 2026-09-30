//! [`Subscription`]: one `subscriptions/listen` stream (`2026-07-28`).
//!
//! A subscription is identified by the JSON-RPC id of the `subscriptions/listen`
//! request that opened it, and every notification on it carries that id in
//! `_meta`. The connection demultiplexes on it, so each handle sees only its own
//! notifications ("on stdio ... clients MUST use this field to correlate
//! notifications with their originating subscription").
//!
//! It ends one of three ways, and the handle says which: the server closes it
//! gracefully (its response to the listen request), the connection drops (no
//! such response: "the client MAY treat [it] as a trigger to reconnect"), or
//! the client cancels it, which dropping the handle does too.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use turbomcp_core::{JsonRpcNotification, RequestId, meta};
use turbomcp_protocol::methods::notification;
use turbomcp_protocol::neutral;

use crate::connection::Connection;

/// How many notifications one subscription may hold unread. Past it one is
/// dropped with a warning; the connection is never closed for it.
const SUBSCRIPTION_QUEUE: usize = 1024;

/// One notification on a [`Subscription`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SubscriptionEvent {
    /// `notifications/tools/list_changed`.
    ToolsListChanged,
    /// `notifications/prompts/list_changed`.
    PromptsListChanged,
    /// `notifications/resources/list_changed`.
    ResourcesListChanged,
    /// `notifications/resources/updated` for a watched resource.
    ResourceUpdated {
        /// The resource that changed.
        uri: String,
    },
    /// Anything else on the stream (an extension's notifications).
    Other {
        /// The notification method.
        method: String,
        /// Its params, as sent.
        params: Option<Value>,
    },
}

impl SubscriptionEvent {
    fn from_notification(n: JsonRpcNotification) -> Self {
        match n.method.as_str() {
            notification::TOOLS_LIST_CHANGED => Self::ToolsListChanged,
            notification::PROMPTS_LIST_CHANGED => Self::PromptsListChanged,
            notification::RESOURCES_LIST_CHANGED => Self::ResourcesListChanged,
            notification::RESOURCES_UPDATED => match n
                .params
                .as_ref()
                .and_then(|p| p.get("uri"))
                .and_then(Value::as_str)
            {
                Some(uri) => Self::ResourceUpdated {
                    uri: uri.to_owned(),
                },
                None => Self::Other {
                    method: n.method,
                    params: n.params,
                },
            },
            _ => Self::Other {
                method: n.method,
                params: n.params,
            },
        }
    }
}

/// Why a [`Subscription`] ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SubscriptionEnd {
    /// The server closed it gracefully (shutting down, typically).
    Closed,
    /// The connection went away without the server closing it. The server
    /// keeps no subscription state across connections, so re-listen on a new
    /// one.
    Lost,
    /// This client cancelled it.
    Cancelled,
}

struct Route {
    events: mpsc::Sender<JsonRpcNotification>,
    end: oneshot::Sender<SubscriptionEnd>,
}

/// The live subscriptions of one connection, by listen request id.
#[derive(Default)]
pub(crate) struct SubscriptionRoutes {
    routes: Mutex<HashMap<RequestId, Route>>,
}

/// A route registered before its listen request goes out, removed on drop.
pub(crate) struct RouteGuard {
    routes: Arc<SubscriptionRoutes>,
    id: RequestId,
}

impl Drop for RouteGuard {
    fn drop(&mut self) {
        self.routes.remove(&self.id);
    }
}

impl SubscriptionRoutes {
    fn remove(&self, id: &RequestId) -> Option<Route> {
        self.routes
            .lock()
            .expect("subscription routes poisoned")
            .remove(id)
    }

    /// Route `id`'s notifications, before the request that opens it is sent:
    /// they may follow its acknowledgement before the caller runs again.
    pub(crate) fn register(
        self: &Arc<Self>,
        id: RequestId,
    ) -> (
        RouteGuard,
        mpsc::Receiver<JsonRpcNotification>,
        oneshot::Receiver<SubscriptionEnd>,
    ) {
        let (events, events_rx) = mpsc::channel(SUBSCRIPTION_QUEUE);
        let (end, end_rx) = oneshot::channel();
        self.routes
            .lock()
            .expect("subscription routes poisoned")
            .insert(id.clone(), Route { events, end });
        (
            RouteGuard {
                routes: Arc::clone(self),
                id,
            },
            events_rx,
            end_rx,
        )
    }

    /// Deliver a notification that names a live subscription to its handle.
    /// The acknowledgement itself is not an event: it completes the listen
    /// request.
    pub(crate) fn deliver(&self, n: &JsonRpcNotification) {
        let Some(id) = n
            .params
            .as_ref()
            .and_then(|p| p.get("_meta"))
            .and_then(|m| m.get(meta::keys::SUBSCRIPTION_ID))
            .and_then(|id| serde_json::from_value::<RequestId>(id.clone()).ok())
        else {
            return;
        };
        let routes = self.routes.lock().expect("subscription routes poisoned");
        let Some(route) = routes.get(&id) else {
            return;
        };
        if n.method != notification::SUBSCRIPTIONS_ACKNOWLEDGED
            && let Err(mpsc::error::TrySendError::Full(_)) = route.events.try_send(n.clone())
        {
            tracing::warn!(
                subscription = ?id,
                "subscription is {SUBSCRIPTION_QUEUE} notifications behind; dropped one"
            );
        }
    }

    /// The server answered listen request `id`: its graceful close. `false` if
    /// no subscription has that id.
    pub(crate) fn close(&self, id: &RequestId) -> bool {
        match self.remove(id) {
            Some(route) => {
                let _ = route.end.send(SubscriptionEnd::Closed);
                true
            }
            None => false,
        }
    }

    /// The connection is gone: every subscription on it is lost.
    pub(crate) fn lose_all(&self) {
        let routes =
            std::mem::take(&mut *self.routes.lock().expect("subscription routes poisoned"));
        for route in routes.into_values() {
            let _ = route.end.send(SubscriptionEnd::Lost);
        }
    }
}

/// A live `subscriptions/listen` subscription, from
/// [`Client::listen`](crate::Client::listen).
///
/// Read its notifications with [`next`](Self::next) (or as a
/// [`Stream`](futures::Stream)); `None` means it ended, and
/// [`end`](Self::end) says how. Dropping the handle cancels the subscription.
pub struct Subscription {
    id: RequestId,
    accepted: neutral::SubscriptionFilter,
    events: mpsc::Receiver<JsonRpcNotification>,
    end_rx: oneshot::Receiver<SubscriptionEnd>,
    ended: Option<SubscriptionEnd>,
    conn: Connection,
    _route: RouteGuard,
}

impl core::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Subscription")
            .field("id", &self.id)
            .field("accepted", &self.accepted)
            .field("ended", &self.ended)
            .finish_non_exhaustive()
    }
}

impl Subscription {
    pub(crate) fn new(
        id: RequestId,
        accepted: neutral::SubscriptionFilter,
        route: (
            RouteGuard,
            mpsc::Receiver<JsonRpcNotification>,
            oneshot::Receiver<SubscriptionEnd>,
        ),
        conn: Connection,
    ) -> Self {
        let (guard, events, end_rx) = route;
        Self {
            id,
            accepted,
            events,
            end_rx,
            ended: None,
            conn,
            _route: guard,
        }
    }

    /// The subscription's id: the JSON-RPC id of its listen request, which
    /// every notification on it carries.
    #[must_use]
    pub fn id(&self) -> &RequestId {
        &self.id
    }

    /// What the server agreed to send, which may be less than was asked for
    /// ("notification types the server does not support are omitted").
    #[must_use]
    pub fn accepted(&self) -> &neutral::SubscriptionFilter {
        &self.accepted
    }

    /// The next notification, or `None` once the subscription has ended.
    pub async fn next(&mut self) -> Option<SubscriptionEvent> {
        std::future::poll_fn(|cx| self.poll_event(cx)).await
    }

    /// Why the subscription ended, once [`next`](Self::next) has returned
    /// `None`; `None` while it is live.
    #[must_use]
    pub fn end(&self) -> Option<SubscriptionEnd> {
        self.ended
    }

    /// End the subscription: the server is told to stop sending.
    pub fn cancel(mut self) {
        self.cancel_on_wire();
    }

    fn cancel_on_wire(&mut self) {
        // Already over (the server closed it, or the connection went): there
        // is nothing to cancel.
        if self.ended.is_none()
            && let Ok(end) = self.end_rx.try_recv()
        {
            self.ended = Some(end);
        }
        if self.ended.is_none() {
            self.ended = Some(SubscriptionEnd::Cancelled);
            self.conn
                .cancel_on_wire(&self.id, "the client ended the subscription");
        }
    }

    fn poll_event(&mut self, cx: &mut Context<'_>) -> Poll<Option<SubscriptionEvent>> {
        if self.ended.is_some() {
            return Poll::Ready(None);
        }
        match self.events.poll_recv(cx) {
            Poll::Ready(Some(n)) => Poll::Ready(Some(SubscriptionEvent::from_notification(n))),
            Poll::Ready(None) => {
                // The route is gone, so the end was already sent (or the
                // connection dropped the sender with the route).
                self.ended = Some(self.end_rx.try_recv().unwrap_or(SubscriptionEnd::Lost));
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

impl futures::Stream for Subscription {
    type Item = SubscriptionEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.get_mut().poll_event(cx)
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.cancel_on_wire();
    }
}
