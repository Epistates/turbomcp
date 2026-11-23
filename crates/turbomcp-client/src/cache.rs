//! The client-side response cache (SEP-2549).
//!
//! Draft (`2026-07-28`) cacheable results — the four `*/list`s and
//! `resources/read` — carry `ttlMs` (a freshness hint in milliseconds) and
//! `cacheScope`. This cache honors them: a result with `ttlMs > 0` is served
//! from memory until it expires, and the `*_list_changed` /
//! `resources/updated` notifications invalidate the affected entries early.
//! A missing or zero `ttlMs` (including every legacy `2025-11-25` result,
//! whose wire has no cache fields) is never stored, so the cache is inert
//! unless the server opts in. It holds about 64 MiB of parsed results, by an
//! estimate of what each holds, and keeps none longer than an hour.
//!
//! Scope: this cache is per-[`Client`](crate::Client) connection. On a
//! transport that guarantees one principal for the connection, `"private"`
//! and `"public"` results are both its to hold. On one that can't (an HTTP
//! transport whose bearer source may present different principals' tokens),
//! only `cacheScope: "public"` results are kept: a `"private"` one (the
//! default) is valid only "for the same authorization context".

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use serde_json::Value;
use turbomcp_protocol::methods::{notification, request};

/// What the cache may hold, in estimated bytes of parsed results. A result
/// can be up to the frame limit, and a parsed `Value` is several times its
/// wire size, so a count of entries bounded nothing.
const CACHE_BYTES: u64 = 64 * 1024 * 1024;

/// The longest a result is kept, whatever its `ttlMs` says. `ttlMs` is a
/// freshness hint, and a server's `u64::MAX` would otherwise mean forever.
const MAX_TTL: Duration = Duration::from_secs(60 * 60);

/// One cached raw result, how long it stays fresh, and its estimated size.
#[derive(Clone, Debug)]
struct Entry {
    value: Value,
    ttl: Duration,
    weight: u32,
}

/// Each entry expires after its own `ttlMs`, counted again when a newer
/// result for the same key replaces it.
struct PerEntryTtl;

impl moka::Expiry<Key, Entry> for PerEntryTtl {
    fn expire_after_create(&self, _: &Key, entry: &Entry, _: Instant) -> Option<Duration> {
        Some(entry.ttl)
    }

    fn expire_after_update(
        &self,
        _: &Key,
        entry: &Entry,
        _: Instant,
        _: Option<Duration>,
    ) -> Option<Duration> {
        Some(entry.ttl)
    }
}

/// An estimate of the memory a parsed result holds: every value's slot plus
/// its string and key bytes.
fn weight(value: &Value) -> u32 {
    let slot = std::mem::size_of::<Value>();
    let mut bytes = 0usize;
    let mut stack = vec![value];
    while let Some(value) = stack.pop() {
        bytes = bytes.saturating_add(slot);
        match value {
            Value::String(text) => bytes = bytes.saturating_add(text.len()),
            Value::Array(items) => stack.extend(items),
            Value::Object(map) => {
                for (key, value) in map {
                    bytes = bytes.saturating_add(key.len() + slot);
                    stack.push(value);
                }
            }
            _ => {}
        }
    }
    u32::try_from(bytes).unwrap_or(u32::MAX)
}

/// Method + discriminator (pagination cursor for `*/list`, URI for
/// `resources/read`) → cached entry.
type Key = (String, Option<String>);

/// An in-process response cache keyed by method + discriminator. Shared
/// between the [`Client`](crate::Client) (lookups/stores) and the connection
/// actor (notification-driven invalidation).
#[derive(Debug)]
pub(crate) struct ResponseCache {
    entries: moka::sync::Cache<Key, Entry>,
    /// Whether only `cacheScope: "public"` results may be kept (see the
    /// module docs).
    public_only: bool,
    /// Per [`Family`], how many notifications have invalidated it, so a
    /// response requested before one isn't stored after it.
    generations: [AtomicU64; 3],
}

/// What one notification invalidates.
#[derive(Clone, Copy)]
enum Family {
    Tools,
    Prompts,
    /// Lists, templates and reads: `resources/list_changed` drops all three.
    Resources,
}

impl Family {
    fn of_method(method: &str) -> Option<Self> {
        match method {
            request::TOOLS_LIST => Some(Self::Tools),
            request::PROMPTS_LIST => Some(Self::Prompts),
            request::RESOURCES_LIST
            | request::RESOURCES_TEMPLATES_LIST
            | request::RESOURCES_READ => Some(Self::Resources),
            _ => None,
        }
    }

    fn of_notification(method: &str) -> Option<Self> {
        match method {
            notification::TOOLS_LIST_CHANGED => Some(Self::Tools),
            notification::PROMPTS_LIST_CHANGED => Some(Self::Prompts),
            notification::RESOURCES_LIST_CHANGED | notification::RESOURCES_UPDATED => {
                Some(Self::Resources)
            }
            _ => None,
        }
    }
}

/// Where a response came from, for [`ResponseCache::store`]: the generation of
/// its family when it was requested.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Requested(Option<u64>);

impl ResponseCache {
    /// A cache that keeps only `cacheScope: "public"` results when
    /// `public_only`.
    pub(crate) fn new(public_only: bool) -> Self {
        Self::with_budget(public_only, CACHE_BYTES)
    }

    fn with_budget(public_only: bool, bytes: u64) -> Self {
        Self {
            entries: moka::sync::Cache::builder()
                .max_capacity(bytes)
                .weigher(|_: &Key, entry: &Entry| entry.weight)
                .expire_after(PerEntryTtl)
                .support_invalidation_closures()
                .build(),
            public_only,
            generations: Default::default(),
        }
    }

    /// Drop every entry of `method`.
    fn drop_method(&self, method: &'static str) {
        // Only fails when invalidation closures are off; they're on.
        let _ = self
            .entries
            .invalidate_entries_if(move |(m, _), _| m == method);
    }

    /// Forget every page of `method`: the server refused one of its cursors
    /// ("clients SHOULD discard cached pages" for a cursor it no longer
    /// honours), so the rest may be from a list that no longer exists.
    pub(crate) fn forget(&self, method: &str) {
        let method = method.to_owned();
        let _ = self
            .entries
            .invalidate_entries_if(move |(m, _), _| *m == method);
    }

    /// A fresh cached result for `method` + `discriminator`, if any.
    pub(crate) fn get(&self, method: &str, discriminator: Option<&str>) -> Option<Value> {
        let key = (method.to_owned(), discriminator.map(str::to_owned));
        self.entries.get(&key).map(|entry| entry.value)
    }

    /// Note the generation `method`'s family is at, before requesting it.
    pub(crate) fn requesting(&self, method: &str) -> Requested {
        Requested(
            Family::of_method(method).map(|f| self.generations[f as usize].load(Ordering::Acquire)),
        )
    }

    /// Store a raw result if its `ttlMs` declares it cacheable. Per SEP-2549,
    /// `ttlMs: 0` means immediately stale and a missing `ttlMs` behaves like
    /// `0` — neither is stored. Nor is one whose family a notification
    /// invalidated after it was `requested`: "the cached response ... should
    /// be considered immediately stale", and storing it late would make it
    /// fresh for its whole `ttlMs`.
    pub(crate) fn store(
        &self,
        method: &str,
        discriminator: Option<&str>,
        value: &Value,
        requested: Requested,
    ) {
        if let (Some(family), Requested(Some(then))) = (Family::of_method(method), requested)
            && self.generations[family as usize].load(Ordering::Acquire) != then
        {
            return;
        }
        let Some(ttl_ms) = value
            .get("ttlMs")
            .and_then(Value::as_u64)
            .filter(|ms| *ms > 0)
        else {
            return;
        };
        // `"private"` is the default scope.
        if self.public_only && value.get("cacheScope").and_then(Value::as_str) != Some("public") {
            return;
        }
        self.entries.insert(
            (method.to_owned(), discriminator.map(str::to_owned)),
            Entry {
                value: value.clone(),
                ttl: Duration::from_millis(ttl_ms).min(MAX_TTL),
                weight: weight(value),
            },
        );
    }

    /// Invalidate the entries a server notification obsoletes: each
    /// `*_list_changed` drops its capability's entries (`resources` also
    /// drops templates and reads — the resource set changed under them), and
    /// `resources/updated` drops the named URI's read entry.
    pub(crate) fn on_notification(&self, method: &str, params: Option<&Value>) {
        if let Some(family) = Family::of_notification(method) {
            self.generations[family as usize].fetch_add(1, Ordering::AcqRel);
        }
        match method {
            notification::TOOLS_LIST_CHANGED => self.drop_method(request::TOOLS_LIST),
            notification::PROMPTS_LIST_CHANGED => self.drop_method(request::PROMPTS_LIST),
            notification::RESOURCES_LIST_CHANGED => {
                self.drop_method(request::RESOURCES_LIST);
                self.drop_method(request::RESOURCES_TEMPLATES_LIST);
                self.drop_method(request::RESOURCES_READ);
            }
            notification::RESOURCES_UPDATED => {
                if let Some(uri) = params.and_then(|p| p.get("uri")).and_then(Value::as_str) {
                    self.entries
                        .invalidate(&(request::RESOURCES_READ.to_owned(), Some(uri.to_owned())));
                }
            }
            _ => {}
        }
    }

    /// Drop every entry.
    pub(crate) fn clear(&self) {
        self.entries.invalidate_all();
    }
}

impl Default for ResponseCache {
    fn default() -> Self {
        Self::new(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn zero_or_missing_ttl_is_never_stored() {
        let cache = ResponseCache::default();
        cache.store(
            "tools/list",
            None,
            &json!({ "tools": [], "ttlMs": 0 }),
            Requested(None),
        );
        cache.store("tools/list", None, &json!({ "tools": [] }), Requested(None));
        assert!(cache.get("tools/list", None).is_none());
    }

    #[test]
    fn without_one_principal_only_public_results_are_kept() {
        let cache = ResponseCache::new(true);
        let private = json!({ "tools": [], "ttlMs": 60_000 });
        cache.store("tools/list", None, &private, Requested(None));
        assert!(
            cache.get("tools/list", None).is_none(),
            "private is the default"
        );
        let public = json!({ "tools": [], "ttlMs": 60_000, "cacheScope": "public" });
        cache.store("tools/list", None, &public, Requested(None));
        assert!(cache.get("tools/list", None).is_some());
        cache.forget("tools/list");
        assert!(cache.get("tools/list", None).is_none());
    }

    /// A response requested before a `list_changed` and stored after it is
    /// not cached: it predates the change.
    #[test]
    fn a_response_that_predates_an_invalidation_is_not_stored() {
        let cache = ResponseCache::default();
        let requested = cache.requesting("tools/list");
        cache.on_notification(notification::TOOLS_LIST_CHANGED, None);
        cache.store(
            "tools/list",
            None,
            &json!({ "tools": [], "ttlMs": 60_000 }),
            requested,
        );
        assert!(cache.get("tools/list", None).is_none());
        // Another family's notification doesn't spoil it.
        let requested = cache.requesting("tools/list");
        cache.on_notification(notification::PROMPTS_LIST_CHANGED, None);
        cache.store(
            "tools/list",
            None,
            &json!({ "tools": [], "ttlMs": 60_000 }),
            requested,
        );
        assert!(cache.get("tools/list", None).is_some());
    }

    #[test]
    fn fresh_entries_hit_and_cursors_are_distinct_entries() {
        let cache = ResponseCache::default();
        let result = json!({ "tools": [], "ttlMs": 60_000 });
        cache.store("tools/list", None, &result, Requested(None));
        assert_eq!(cache.get("tools/list", None), Some(result));
        // A different discriminator (cursor) is a different entry.
        assert!(cache.get("tools/list", Some("page2")).is_none());
    }

    #[test]
    fn entries_expire_after_their_ttl() {
        let cache = ResponseCache::default();
        cache.store(
            "tools/list",
            None,
            &json!({ "tools": [], "ttlMs": 15 }),
            Requested(None),
        );
        assert!(cache.get("tools/list", None).is_some(), "fresh: a hit");
        std::thread::sleep(Duration::from_millis(40));
        assert!(
            cache.get("tools/list", None).is_none(),
            "past ttlMs: a miss"
        );
    }

    /// The cache holds what fits its byte budget: a result past it isn't
    /// kept, and many results stay within it together.
    #[test]
    fn the_cache_stays_within_its_byte_budget() {
        let cache = ResponseCache::with_budget(false, 20_000);
        let item = |n: usize| json!({ "ttlMs": 60_000, "text": "x".repeat(n) });
        cache.store(
            "resources/read",
            Some("huge"),
            &item(50_000),
            Requested(None),
        );
        cache.entries.run_pending_tasks();
        assert!(cache.get("resources/read", Some("huge")).is_none());
        for i in 0..100 {
            cache.store(
                "resources/read",
                Some(&format!("r{i}")),
                &item(1_000),
                Requested(None),
            );
        }
        cache.entries.run_pending_tasks();
        assert!(cache.entries.weighted_size() <= 20_000);
    }

    /// `ttlMs` is a hint; a server's "forever" is kept for at most an hour.
    #[test]
    fn a_ttl_is_capped() {
        let cache = ResponseCache::default();
        cache.store(
            "tools/list",
            None,
            &json!({ "tools": [], "ttlMs": u64::MAX }),
            Requested(None),
        );
        let entry = cache
            .entries
            .get(&("tools/list".to_owned(), None))
            .expect("stored");
        assert_eq!(entry.ttl, MAX_TTL);
    }

    #[test]
    fn list_changed_invalidates_its_capability() {
        let cache = ResponseCache::default();
        let result = json!({ "ttlMs": 60_000 });
        cache.store(request::TOOLS_LIST, None, &result, Requested(None));
        cache.store(request::RESOURCES_LIST, None, &result, Requested(None));
        cache.store(
            request::RESOURCES_TEMPLATES_LIST,
            None,
            &result,
            Requested(None),
        );
        cache.store(
            request::RESOURCES_READ,
            Some("file:///a"),
            &result,
            Requested(None),
        );
        cache.store(request::PROMPTS_LIST, None, &result, Requested(None));

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
        cache.store(request::TOOLS_LIST, None, &result, Requested(None));
        cache.on_notification(notification::PROMPTS_LIST_CHANGED, None);
        assert!(cache.get(request::PROMPTS_LIST, None).is_none());
        assert!(cache.get(request::TOOLS_LIST, None).is_some());
    }

    #[test]
    fn resources_updated_invalidates_only_the_named_uri() {
        let cache = ResponseCache::default();
        let result = json!({ "ttlMs": 60_000 });
        cache.store(
            request::RESOURCES_READ,
            Some("file:///a"),
            &result,
            Requested(None),
        );
        cache.store(
            request::RESOURCES_READ,
            Some("file:///b"),
            &result,
            Requested(None),
        );
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
