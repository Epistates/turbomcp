//! Component tags: reading back what `#[tool(tags(…))]` wrote.
//!
//! A tag categorizes a component — a tool, resource, resource template, or
//! prompt — for catalog policy: which components a given caller or deployment
//! should be offered. The markers write them into the component's own `_meta`
//! under [`meta::keys::TAGS`] as an array of strings.
//!
//! **Why `_meta` and not a side table.** A server's components do not all come
//! from one `#[server]` impl: a mounted sub-server or a proxied upstream
//! contributes [`neutral::Tool`](turbomcp_protocol::neutral::Tool) values that
//! no compile-time table of this process could know about. Carrying the tags on
//! the value itself is what makes one policy cover declared and acquired
//! components alike — and it is what `_meta` is for.

use serde_json::{Map, Value};
use turbomcp_core::meta;

/// The tags on a component, from its `_meta`.
///
/// Anything that is not an array of strings at [`meta::keys::TAGS`] reads as
/// untagged: `_meta` is an open map that other parties write into, so a
/// malformed value there is someone else's data, not an error to raise here.
///
/// ```
/// # use serde_json::json;
/// # use turbomcp_server::tags;
/// # use turbomcp_protocol::neutral::Tool;
/// let tool = Tool::new("delete", json!({"type": "object"}))
///     .with_meta_entry("io.turbomcp/tags", json!(["admin", "dangerous"]));
/// assert_eq!(tags::of(&tool.meta).collect::<Vec<_>>(), ["admin", "dangerous"]);
/// ```
pub fn of(component_meta: &Map<String, Value>) -> impl Iterator<Item = &str> {
    component_meta
        .get(meta::keys::TAGS)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
}

/// Whether a component carries `tag`.
pub fn has(component_meta: &Map<String, Value>, tag: &str) -> bool {
    of(component_meta).any(|t| t == tag)
}

/// Whether a component carries any of `tags`. An empty `tags` is `false` —
/// "any of nothing" is nothing.
pub fn has_any(component_meta: &Map<String, Value>, tags: &[&str]) -> bool {
    of(component_meta).any(|t| tags.contains(&t))
}

/// Whether a component carries all of `tags`. An empty `tags` is `true`.
pub fn has_all(component_meta: &Map<String, Value>, tags: &[&str]) -> bool {
    tags.iter().all(|want| has(component_meta, want))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(value: Value) -> Map<String, Value> {
        let mut m = Map::new();
        m.insert(meta::keys::TAGS.to_string(), value);
        m
    }

    #[test]
    fn reads_the_tags_the_macro_writes() {
        let m = meta(json!(["admin", "dangerous"]));
        assert_eq!(of(&m).collect::<Vec<_>>(), ["admin", "dangerous"]);
        assert!(has(&m, "admin"));
        assert!(!has(&m, "readonly"));
        assert!(has_any(&m, &["readonly", "dangerous"]));
        assert!(has_all(&m, &["admin", "dangerous"]));
        assert!(!has_all(&m, &["admin", "readonly"]));
    }

    #[test]
    fn an_untagged_component_matches_nothing_and_everything() {
        let m = Map::new();
        assert_eq!(of(&m).count(), 0);
        assert!(!has(&m, "admin"));
        // Vacuous truth, deliberately: "all of nothing" holds, so a filter
        // requiring no tags admits every component rather than none.
        assert!(has_all(&m, &[]));
        assert!(!has_any(&m, &[]));
    }

    #[test]
    fn a_malformed_value_reads_as_untagged() {
        // `_meta` is open — another party may write anything at this key. That
        // is their data being wrong, not a condition this layer reports.
        for bad in [json!("admin"), json!(42), json!({"a": 1}), json!(null)] {
            assert_eq!(of(&meta(bad)).count(), 0);
        }
        // A mixed array keeps the strings and drops the rest.
        let m = meta(json!(["admin", 7, null, "safe"]));
        assert_eq!(of(&m).collect::<Vec<_>>(), ["admin", "safe"]);
    }
}
