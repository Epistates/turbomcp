//! Per-call progress: tokens the client mints, and where each one's
//! `notifications/progress` goes.
//!
//! "The progress token MUST be unique across all active requests." Minting
//! them here makes that true by construction, where a caller-chosen token
//! could collide with another in-flight call, and routing each to its own call
//! spares the caller demultiplexing a global handler by hand.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::watch;
use turbomcp_protocol::neutral;

/// What a call's progress updates are handed to. Called on the connection's
/// reader, in order, so keep it cheap: forward to a channel rather than block.
pub type ProgressCallback = Arc<dyn Fn(neutral::Progress) + Send + Sync>;

struct Route {
    callback: Option<ProgressCallback>,
    /// Ticks on every update, for a call that resets its timeout on progress.
    tick: watch::Sender<()>,
}

/// The live progress routes of one connection.
#[derive(Default)]
pub(crate) struct ProgressRoutes {
    routes: Mutex<HashMap<String, Route>>,
    next: AtomicU64,
}

/// One call's registration: its token, a receiver that ticks on each update,
/// and the route's removal when the call ends.
pub(crate) struct Registration {
    pub(crate) token: String,
    pub(crate) ticks: watch::Receiver<()>,
    routes: Arc<ProgressRoutes>,
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.routes
            .routes
            .lock()
            .expect("progress routes poisoned")
            .remove(&self.token);
    }
}

impl ProgressRoutes {
    /// Mint a token and route its updates to `callback`.
    pub(crate) fn register(self: &Arc<Self>, callback: Option<ProgressCallback>) -> Registration {
        let n = self.next.fetch_add(1, Ordering::Relaxed);
        let token = format!("turbomcp-progress-{n}");
        let (tick, ticks) = watch::channel(());
        self.routes
            .lock()
            .expect("progress routes poisoned")
            .insert(token.clone(), Route { callback, tick });
        Registration {
            token,
            ticks,
            routes: Arc::clone(self),
        }
    }

    /// Deliver a `notifications/progress` whose token this client minted.
    /// `false` if the token is not one of ours (the notification then goes to
    /// the connection's notification handler, as before).
    pub(crate) fn deliver(&self, params: Option<&Value>) -> bool {
        let Some(params) = params else { return false };
        let Some(token) = params.get("progressToken").and_then(Value::as_str) else {
            return false;
        };
        // The user's callback runs outside the lock.
        let callback = {
            let routes = self.routes.lock().expect("progress routes poisoned");
            let Some(route) = routes.get(token) else {
                return false;
            };
            route.tick.send_replace(());
            route.callback.clone()
        };
        if let (Some(callback), Some(progress)) = (callback, neutral::Progress::from_params(params))
        {
            callback(progress);
        }
        true
    }
}
