//! The client-side response cache (SEP-2549).
//!
//! Draft (`2026-07-28`) cacheable results — the four `*/list`s and
//! `resources/read` — carry `ttlMs` (a freshness hint in milliseconds) and
//! `cacheScope`. This cache honors them: a result with `ttlMs > 0` is served
//! from memory until it expires, and the `*_list_changed` /
//! `resources/updated` notifications invalidate the affected entries early.
//! A missing or zero `ttlMs` (including every legacy `2025-11-25` result,
//! whose wire has no cache fields) is never stored, so the cache is inert
//! unless the server opts in.
//!
//! Scope note: this cache is per-[`Client`](crate::Client) connection — one
//! principal — so both `"private"` and `"public"` responses are safe to hold.
//! `cacheScope` distinguishes *shared intermediaries*, which this is not.

use parking_lot::Mutex;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde_json::Value;
use turbomcp_protocol::methods::{notification, request};

/// Cap on stored entries (each distinct cursor/URI is one entry). At the cap,
/// the soonest-to-expire entry is evicted to admit the new one.
const MAX_ENTRIES: usize = 1024;

/// One cached raw result and its expiry deadline.
#[derive(Debug)]
struct Entry {
    value: Value,
    expires_at: Instant,
}

/// Method + discriminator (pagination cursor for `*/list`, URI for
/// `resources/read`) → cached entry.
type Key = (String, Option<String>);

/// An in-process response cache keyed by method + discriminator. Shared
/// between the [`Client`](crate::Client) (lookups/stores) and the connection
/// actor (notification-driven invalidation).
#[derive(Debug, Default)]
pub(crate) struct ResponseCache {
    entries: Mutex<HashMap<Key, Entry>>,
}

impl ResponseCache {
    /// A fresh cached result for `method` + `discriminator`, if any. Expired
    /// entries are dropped on the way out.
    pub(crate) fn get(&self, method: &str, discriminator: Option<&str>) -> Option<Value> {
        let key = (method.to_owned(), discriminator.map(str::to_owned));
        let mut entries = self.entries.lock();
        match entries.get(&key) {
            Some(entry) if entry.expires_at > Instant::now() => Some(entry.value.clone()),
            Some(_) => {
                entries.remove(&key);
                None
            }
            None => None,
        }
    }

    /// Store a raw result if its `ttlMs` declares it cacheable. Per SEP-2549,
    /// `ttlMs: 0` means immediately stale and a missing `ttlMs` behaves like
    /// `0` — neither is stored.
    pub(crate) fn store(&self, method: &str, discriminator: Option<&str>, value: &Value) {
        let Some(ttl_ms) = value
            .get("ttlMs")
            .and_then(Value::as_u64)
            .filter(|ms| *ms > 0)
        else {
            return;
        };
        let mut entries = self.entries.lock();
        if entries.len() >= MAX_ENTRIES {
            entries.retain(|_, e| e.expires_at > Instant::now());
        }
        if entries.len() >= MAX_ENTRIES
            && let Some(evict) = entries
                .iter()
                .min_by_key(|(_, e)| e.expires_at)
                .map(|(k, _)| k.clone())
        {
            entries.remove(&evict);
        }
        entries.insert(
            (method.to_owned(), discriminator.map(str::to_owned)),
            Entry {
                value: value.clone(),
                expires_at: Instant::now() + Duration::from_millis(ttl_ms),
            },
        );
    }

    /// Invalidate the entries a server notification obsoletes: each
    /// `*_list_changed` drops its capability's entries (`resources` also
    /// drops templates and reads — the resource set changed under them), and
    /// `resources/updated` drops the named URI's read entry.
    pub(crate) fn on_notification(&self, method: &str, params: Option<&Value>) {
        let mut entries = self.entries.lock();
        match method {
            notification::TOOLS_LIST_CHANGED => {
                entries.retain(|(m, _), _| m != request::TOOLS_LIST);
            }
            notification::PROMPTS_LIST_CHANGED => {
                entries.retain(|(m, _), _| m != request::PROMPTS_LIST);
            }
            notification::RESOURCES_LIST_CHANGED => {
                entries.retain(|(m, _), _| {
                    m != request::RESOURCES_LIST
                        && m != request::RESOURCES_TEMPLATES_LIST
                        && m != request::RESOURCES_READ
                });
            }
            notification::RESOURCES_UPDATED => {
                if let Some(uri) = params.and_then(|p| p.get("uri")).and_then(Value::as_str) {
                    entries.remove(&(request::RESOURCES_READ.to_owned(), Some(uri.to_owned())));
                }
            }
            _ => {}
        }
    }

    /// Drop every entry.
    pub(crate) fn clear(&self) {
        self.entries.lock().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn zero_or_missing_ttl_is_never_stored() {
        let cache = ResponseCache::default();
        cache.store("tools/list", None, &json!({ "tools": [], "ttlMs": 0 }));
        cache.store("tools/list", None, &json!({ "tools": [] }));
        assert!(cache.get("tools/list", None).is_none());
    }

    #[test]
    fn fresh_entries_hit_and_cursors_are_distinct_entries() {
        let cache = ResponseCache::default();
        let result = json!({ "tools": [], "ttlMs": 60_000 });
        cache.store("tools/list", None, &result);
        assert_eq!(cache.get("tools/list", None), Some(result));
        // A different discriminator (cursor) is a different entry.
        assert!(cache.get("tools/list", Some("page2")).is_none());
    }

    #[test]
    fn entries_expire_after_their_ttl() {
        let cache = ResponseCache::default();
        cache.store("tools/list", None, &json!({ "tools": [], "ttlMs": 15 }));
        assert!(cache.get("tools/list", None).is_some(), "fresh: a hit");
        std::thread::sleep(Duration::from_millis(40));
        assert!(
            cache.get("tools/list", None).is_none(),
            "past ttlMs: a miss"
        );
    }

    #[test]
    fn at_capacity_the_soonest_to_expire_entry_is_evicted() {
        let cache = ResponseCache::default();
        // Entry 0 expires first; fill the rest of the capacity with later ones.
        cache.store("tools/list", Some("cursor-0"), &json!({ "ttlMs": 10_000 }));
        for i in 1..MAX_ENTRIES {
            cache.store(
                "tools/list",
                Some(&format!("cursor-{i}")),
                &json!({ "ttlMs": 100_000 }),
            );
        }
        // Admitting one more evicts the soonest-to-expire (entry 0), not the new one.
        cache.store("tools/list", Some("overflow"), &json!({ "ttlMs": 100_000 }));
        assert!(cache.get("tools/list", Some("cursor-0")).is_none());
        assert!(cache.get("tools/list", Some("cursor-1")).is_some());
        assert!(cache.get("tools/list", Some("overflow")).is_some());
    }

    #[test]
    fn list_changed_invalidates_its_capability() {
        let cache = ResponseCache::default();
        let result = json!({ "ttlMs": 60_000 });
        cache.store(request::TOOLS_LIST, None, &result);
        cache.store(request::RESOURCES_LIST, None, &result);
        cache.store(request::RESOURCES_TEMPLATES_LIST, None, &result);
        cache.store(request::RESOURCES_READ, Some("file:///a"), &result);
        cache.store(request::PROMPTS_LIST, None, &result);

        cache.on_notification(notification::TOOLS_LIST_CHANGED, None);
        assert!(cache.get(request::TOOLS_LIST, None).is_none());
        assert!(cache.get(request::PROMPTS_LIST, None).is_some());

        cache.on_notification(notification::RESOURCES_LIST_CHANGED, None);
        assert!(cache.get(request::RESOURCES_LIST, None).is_none());
        assert!(cache.get(request::RESOURCES_TEMPLATES_LIST, None).is_none());
        assert!(
            cache
                .get(request::RESOURCES_READ, Some("file:///a"))
                .is_none()
        );
        assert!(cache.get(request::PROMPTS_LIST, None).is_some());

        // The prompts branch fires too (each capability invalidates only its own).
        cache.store(request::TOOLS_LIST, None, &result);
        cache.on_notification(notification::PROMPTS_LIST_CHANGED, None);
        assert!(cache.get(request::PROMPTS_LIST, None).is_none());
        assert!(cache.get(request::TOOLS_LIST, None).is_some());
    }

    #[test]
    fn resources_updated_invalidates_only_the_named_uri() {
        let cache = ResponseCache::default();
        let result = json!({ "ttlMs": 60_000 });
        cache.store(request::RESOURCES_READ, Some("file:///a"), &result);
        cache.store(request::RESOURCES_READ, Some("file:///b"), &result);
        cache.on_notification(
            notification::RESOURCES_UPDATED,
            Some(&json!({ "uri": "file:///a" })),
        );
        assert!(
            cache
                .get(request::RESOURCES_READ, Some("file:///a"))
                .is_none()
        );
        assert!(
            cache
                .get(request::RESOURCES_READ, Some("file:///b"))
                .is_some()
        );
    }
}
