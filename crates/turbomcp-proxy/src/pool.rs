//! Which upstream connection serves a call: [`UpstreamKey`], and the pool
//! of connections it keys.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::OnceCell;
use turbomcp_core::{McpError, McpResult, RequestContext, SessionId};

use crate::link::{Lease, Link, Linker, Subject};

/// Which upstream connection serves a call.
///
/// The choice matters for an upstream that asks callers for input
/// (elicitation, sampling, roots) and for an upstream that keeps state per
/// connection. A request for input has to reach the caller whose call caused
/// it: on `2026-07-28` it comes back in the call's own result, and on
/// Streamable HTTP on the call's own stream, so one shared connection
/// attributes every request. On a `2025-11-25` stdio or WebSocket upstream
/// nothing says whose a request is, and with two calls in flight on one
/// connection the proxy refuses it rather than guess. Keying gives each
/// caller (or session) its own connection, so the question is never open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UpstreamKey {
    /// One connection for every caller.
    Global,
    /// One connection per authenticated caller (issuer and subject);
    /// anonymous callers share one. Isolates per-user upstream state, and
    /// one child process per user for a stdio upstream.
    Principal,
    /// One connection per downstream session (`2025-*`). A `2026-07-28`
    /// request has no session and is keyed by its principal.
    Session,
}

impl UpstreamKey {
    /// The pool key for a request.
    fn of(self, base: &RequestContext) -> String {
        let principal = || {
            base.identity
                .principal_key()
                .map_or_else(|| "anonymous".to_owned(), |p| format!("principal:{p}"))
        };
        match self {
            Self::Global => "global".to_owned(),
            Self::Principal => principal(),
            Self::Session => base
                .extensions
                .get::<SessionId>()
                .map_or_else(principal, |s| format!("session:{s}")),
        }
    }
}

/// How long an unused keyed connection stays open, by default.
pub(crate) const DEFAULT_IDLE: Duration = Duration::from_secs(10 * 60);

/// How many keyed connections stay open at most, by default.
pub(crate) const DEFAULT_MAX: u64 = 1_000;

/// How often idle connections are looked for.
const SWEEP_EVERY: Duration = Duration::from_secs(30);

/// One key's connection, and the caller it acts for.
#[derive(Default)]
struct Entry {
    subject: Subject,
    link: OnceCell<Arc<Link>>,
}

type Cell = Arc<Entry>;

/// The connections to one upstream, by key.
pub(crate) struct Pool {
    key: UpstreamKey,
    linker: Linker,
    /// The connection made at startup to learn what the upstream serves,
    /// adopted by the first key that needs one.
    spare: Mutex<Option<Arc<Link>>>,
    links: moka::sync::Cache<String, Cell>,
    sweeper: tokio::task::JoinHandle<()>,
    /// Every connection acts for its caller, whose token it needs.
    for_callers: bool,
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.sweeper.abort();
    }
}

impl Pool {
    pub(crate) fn new(
        key: UpstreamKey,
        linker: Linker,
        spare: Option<Arc<Link>>,
        (idle, max): (Duration, u64),
        for_callers: bool,
    ) -> Self {
        let runtime = tokio::runtime::Handle::current();
        let links: moka::sync::Cache<String, Cell> = moka::sync::Cache::builder()
            .max_capacity(max)
            .time_to_idle(idle)
            .eviction_listener(move |_key, cell: Cell, _cause| {
                if let Some(link) = cell.link.get().cloned() {
                    runtime.spawn(retire(link));
                }
            })
            .build();
        let sweeper = {
            let links = links.clone();
            tokio::spawn(async move {
                let mut every = tokio::time::interval(SWEEP_EVERY);
                loop {
                    every.tick().await;
                    links.run_pending_tasks();
                }
            })
        };
        Self {
            key,
            linker,
            spare: Mutex::new(spare),
            links,
            sweeper,
            for_callers,
        }
    }

    /// The connection that serves `base`, opened (or reopened, if the last
    /// one died) as needed.
    pub(crate) async fn get(&self, base: &RequestContext) -> McpResult<Lease> {
        let key = self.key.of(base);
        let token = base.extensions.get::<turbomcp_service::SubjectToken>();
        if self.for_callers && token.is_none() {
            tracing::warn!(
                upstream = %self.linker.label,
                "no caller token to exchange: retain tokens on the gateway's authenticator"
            );
            return Err(McpError::internal(format!(
                "upstream {} acts for its caller, and the gateway kept no token to act with",
                self.linker.label
            )));
        }
        for _ in 0..2 {
            let cell = self.links.get_with(key.clone(), Cell::default);
            if self.for_callers {
                *cell.subject.lock().expect("subject slot") = token.cloned();
            }
            let link = cell
                .link
                .get_or_try_init(|| async {
                    let spare = self.spare.lock().expect("pool spare").take();
                    match spare {
                        Some(link) if link.is_alive() => Ok(link),
                        _ => {
                            let subject = self.for_callers.then(|| Arc::clone(&cell.subject));
                            self.linker.link(subject).await
                        }
                    }
                })
                .await
                .map_err(|e| {
                    tracing::warn!(
                        upstream = %self.linker.label,
                        error = %crate::error::chain(&e),
                        "upstream unavailable"
                    );
                    McpError::transport(format!("upstream {} unavailable", self.linker.label))
                })?;
            if link.is_alive() {
                return Ok(Lease::new(Arc::clone(link)));
            }
            // It died (the child exited, the stream closed for good): open
            // another, once.
            self.links.invalidate(&key);
        }
        Err(McpError::transport(format!(
            "upstream {} closed the connection",
            self.linker.label
        )))
    }

    /// End the connection of session `id`, if it has one.
    pub(crate) fn end_session(&self, id: &str) {
        if self.key == UpstreamKey::Session {
            self.links.invalidate(&format!("session:{id}"));
        }
    }

    /// End every connection.
    pub(crate) async fn shutdown(&self) {
        let links: Vec<Arc<Link>> = self
            .links
            .iter()
            .filter_map(|(_, cell)| cell.link.get().cloned())
            .chain(self.spare.lock().expect("pool spare").take())
            .collect();
        futures::future::join_all(links.iter().map(|link| link.shutdown())).await;
        self.links.invalidate_all();
    }
}

/// How long an evicted connection waits for the calls using it.
const RETIRE_AFTER_AT_MOST: Duration = Duration::from_secs(5 * 60);

/// End an evicted connection once no call is still using it.
async fn retire(link: Arc<Link>) {
    link.released(RETIRE_AFTER_AT_MOST).await;
    link.shutdown().await;
}
