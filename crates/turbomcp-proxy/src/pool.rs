//! Which upstream connection serves a call: [`UpstreamKey`], and the pool
//! of connections it keys.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
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
    /// One connection for every caller: one upstream session, and the
    /// gateway's own identity there. Whatever the upstream authorizes or
    /// keeps per session is then the same for every caller; use
    /// [`Principal`](Self::Principal) (and token exchange, for the
    /// upstream to see each caller as themselves) where it must not be.
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
    /// The pool key for a request, and the caller it counts against.
    fn of(self, base: &RequestContext) -> (String, String) {
        let principal = base
            .identity
            .principal_key()
            .map_or_else(|| "anonymous".to_owned(), |p| format!("principal:{p}"));
        let key = match self {
            Self::Global => "global".to_owned(),
            Self::Principal => principal.clone(),
            Self::Session => base
                .extensions
                .get::<SessionId>()
                .map_or_else(|| principal.clone(), |s| format!("session:{s}")),
        };
        (key, principal)
    }
}

/// How long an unused keyed connection stays open, by default.
pub(crate) const DEFAULT_IDLE: Duration = Duration::from_secs(10 * 60);

/// How many keyed connections stay open at most, by default.
pub(crate) const DEFAULT_MAX: u64 = 1_000;

/// How many keyed connections one caller may hold at most, by default (one
/// per session, under [`UpstreamKey::Session`]).
pub(crate) const DEFAULT_MAX_PER_CALLER: u64 = 32;

/// How often idle connections are looked for.
const SWEEP_EVERY: Duration = Duration::from_secs(30);

/// One key's connection, and the caller it acts for.
struct Entry {
    /// The caller it counts against.
    caller: String,
    subject: Subject,
    link: OnceCell<Arc<Link>>,
}

type Cell = Arc<Entry>;

/// The connections held, in all and by caller: the admission budget.
#[derive(Default)]
struct Held {
    all: u64,
    by_caller: HashMap<String, u64>,
}

/// Admission to the pool. A full pool refuses a new key rather than evict
/// a live connection: with eviction, whoever opens sessions fastest (and
/// sessions are cheap) churns every other caller's upstream, and for a
/// stdio upstream spawns a process per key doing it.
struct Budget {
    max: u64,
    per_caller: u64,
    held: Mutex<Held>,
}

impl Budget {
    fn admit(&self, caller: &str, label: &str) -> McpResult<()> {
        let mut held = self.held.lock();
        if held.all >= self.max {
            return Err(McpError::transport(format!(
                "upstream {label} is at capacity ({} connections)",
                self.max
            )));
        }
        let mine = held.by_caller.entry(caller.to_owned()).or_insert(0);
        if *mine >= self.per_caller {
            return Err(McpError::transport(format!(
                "this caller holds the most upstream {label} connections allowed ({})",
                self.per_caller
            )));
        }
        *mine += 1;
        held.all += 1;
        Ok(())
    }

    fn release(&self, caller: &str) {
        let mut held = self.held.lock();
        held.all = held.all.saturating_sub(1);
        if let Some(mine) = held.by_caller.get_mut(caller) {
            *mine -= 1;
            if *mine == 0 {
                held.by_caller.remove(caller);
            }
        }
    }
}

/// The connections to one upstream, by key.
pub(crate) struct Pool {
    key: UpstreamKey,
    linker: Linker,
    /// The connection made at startup to learn what the upstream serves,
    /// adopted by the first key that needs one.
    spare: Mutex<Option<Arc<Link>>>,
    links: moka::sync::Cache<String, Cell>,
    budget: Arc<Budget>,
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
        (idle, max, per_caller): (Duration, u64, u64),
        for_callers: bool,
    ) -> Self {
        let runtime = tokio::runtime::Handle::current();
        let budget = Arc::new(Budget {
            max,
            per_caller,
            held: Mutex::default(),
        });
        let released = Arc::clone(&budget);
        // No capacity on the cache itself: admission is the budget's, and
        // the cache only ever drops a connection that went idle or died.
        let links: moka::sync::Cache<String, Cell> = moka::sync::Cache::builder()
            .time_to_idle(idle)
            .eviction_listener(move |_key, cell: Cell, _cause| {
                released.release(&cell.caller);
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
                    // Idleness is counted in lookups, not calls: a call
                    // outlasting the idle time would otherwise have its
                    // connection retired under it. A connection in use is
                    // touched instead.
                    for (key, cell) in &links {
                        if cell.link.get().is_some_and(|link| link.in_use()) {
                            let _ = links.get(&*key);
                        }
                    }
                    links.run_pending_tasks();
                }
            })
        };
        Self {
            key,
            linker,
            spare: Mutex::new(spare),
            links,
            budget,
            sweeper,
            for_callers,
        }
    }

    /// The connection that serves `base`, opened (or reopened, if the last
    /// one died) as needed.
    pub(crate) async fn get(&self, base: &RequestContext) -> McpResult<Lease> {
        let (key, caller) = self.key.of(base);
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
            let cell = self
                .links
                .try_get_with(key.clone(), || {
                    self.budget.admit(&caller, &self.linker.label)?;
                    Ok::<_, McpError>(Arc::new(Entry {
                        caller: caller.clone(),
                        subject: Subject::default(),
                        link: OnceCell::new(),
                    }))
                })
                .map_err(|e| (*e).clone())?;
            if self.for_callers {
                *cell.subject.lock() = token.cloned();
            }
            let link = cell
                .link
                .get_or_try_init(|| async {
                    let spare = self.spare.lock().take();
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
                    // A key that never connected holds no place in the pool.
                    self.remove(&key, &cell);
                    McpError::transport(format!("upstream {} unavailable", self.linker.label))
                })?;
            if link.is_alive() {
                return Ok(Lease::new(Arc::clone(link)));
            }
            // It died (the child exited, the stream closed for good): open
            // another, once.
            self.remove(&key, &cell);
        }
        Err(McpError::transport(format!(
            "upstream {} closed the connection",
            self.linker.label
        )))
    }

    /// Drop `key`'s entry if it is still `cell`: another call may already
    /// have replaced it.
    fn remove(&self, key: &str, cell: &Cell) {
        self.links
            .entry(key.to_owned())
            .and_compute_with(|entry| match entry {
                Some(entry) if Arc::ptr_eq(entry.value(), cell) => moka::ops::compute::Op::Remove,
                _ => moka::ops::compute::Op::Nop,
            });
    }

    /// Every open connection, the spare included.
    pub(crate) fn live_links(&self) -> Vec<Arc<Link>> {
        self.links
            .iter()
            .filter_map(|(_, cell)| cell.link.get().cloned())
            .chain(self.spare.lock().clone())
            .filter(|link| link.is_alive())
            .collect()
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
            .chain(self.spare.lock().take())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_pool_refuses_rather_than_evicts() {
        let budget = Budget {
            max: 3,
            per_caller: 2,
            held: Mutex::default(),
        };
        budget.admit("ada", "up").unwrap();
        budget.admit("ada", "up").unwrap();
        let mine = budget.admit("ada", "up").unwrap_err();
        assert!(mine.to_string().contains("most upstream"), "{mine}");
        assert!(mine.is_retryable());
        budget.admit("bob", "up").unwrap();
        let full = budget.admit("cy", "up").unwrap_err();
        assert!(full.to_string().contains("at capacity"), "{full}");

        budget.release("ada");
        budget.admit("cy", "up").unwrap();
        budget.release("bob");
        assert!(!budget.held.lock().by_caller.contains_key("bob"));
    }
}
