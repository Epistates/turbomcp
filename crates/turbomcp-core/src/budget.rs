//! What a peer's message may cost: the length of the names and URIs it sends,
//! and the size of a JSON Schema before anything compiles it.
//!
//! A name or URI arrives inside a body of up to a megabyte, and the SDK keeps
//! some (a subscription's URIs), matches some against templates (a resource
//! URI), echoes some (a progress token on every notification) and records
//! some in telemetry. [`MAX_NAME_BYTES`] and [`MAX_URI_BYTES`] bound them, far
//! above any real one.
//!
//! A tool's `inputSchema` and `outputSchema` and an elicitation's
//! `requestedSchema` can come from the other side of the connection, or from
//! a gateway's upstream, and compiling one costs time and memory with its
//! size and nesting. `2026-07-28` asks for a bound (basic/index.mdx,
//! "Composition-Keyword Resource Use": implementations "SHOULD apply
//! reasonable bounds, such as a maximum schema depth, a cap on the total
//! number of subschemas"). The server, the client and the gateway check every
//! schema against this one budget before they compile it.

use alloc::{borrow::Cow, collections::BTreeMap, format, vec, vec::Vec};
use serde_json::Value;

/// The longest id the SDK keeps or echoes for a peer: a URL elicitation's
/// `elicitationId`, a string `progressToken`. Telemetry cuts a tool or prompt
/// name to it.
pub const MAX_NAME_BYTES: usize = 1024;

/// The longest resource URI a peer may name, to read, subscribe to, watch or
/// complete against. Telemetry cuts a URI to it.
pub const MAX_URI_BYTES: usize = 8 * 1024;

/// `text` cut to at most `max` bytes, on a character boundary, with `…`
/// appended when anything was cut: how a peer's string is quoted in an
/// error, a log line or a span.
#[must_use]
pub fn shortened(text: &str, max: usize) -> Cow<'_, str> {
    if text.len() <= max {
        return Cow::Borrowed(text);
    }
    Cow::Owned(format!("{}…", &text[..text.floor_char_boundary(max)]))
}

/// The deepest schema compiled (nesting of objects and arrays).
pub const MAX_SCHEMA_DEPTH: usize = 64;

/// The most JSON values a compiled schema may contain.
pub const MAX_SCHEMA_NODES: usize = 20_000;

/// The most schema values one instance value may be checked against, with
/// every `$ref` expanded. A few dozen definitions, each referring twice to
/// the next, are a small document whose expansion doubles per level:
/// validating one string against thirty levels took seconds.
pub const MAX_SCHEMA_EXPANSION: u64 = 100_000;

/// The most regular expressions a compiled schema may hold (`pattern`
/// values and `patternProperties` keys). Each compiles to up to
/// [`MAX_PATTERN_BYTES`], so the node budget alone allowed thousands.
pub const MAX_SCHEMA_PATTERNS: usize = 128;

/// The most a schema's regular expression may compile to, in bytes (the
/// engine's own default is 10 MiB).
pub const MAX_PATTERN_BYTES: usize = 1024 * 1024;

/// The most a schema's regular expression may grow its lazy DFA cache to.
pub const MAX_PATTERN_DFA_BYTES: usize = 1024 * 1024;

/// The most backtracking steps a schema's regular expression may take per
/// match, for the look-around and back-references only a backtracking engine
/// runs.
pub const MAX_PATTERN_BACKTRACK: usize = 1_000_000;

/// The schema budget, as messages quote it.
#[derive(Clone, Copy, Debug)]
pub struct SchemaBudget;

impl core::fmt::Display for SchemaBudget {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "depth {MAX_SCHEMA_DEPTH}, {MAX_SCHEMA_NODES} values, {MAX_SCHEMA_PATTERNS} patterns, \
             {MAX_SCHEMA_EXPANSION} values with references expanded"
        )
    }
}

/// Whether `schema` stays within [`MAX_SCHEMA_DEPTH`], [`MAX_SCHEMA_NODES`],
/// [`MAX_SCHEMA_PATTERNS`] and [`MAX_SCHEMA_EXPANSION`]. It walks at most
/// that many values (and each `$ref` target once), so checking costs no more
/// than the budget allows.
#[must_use]
pub fn schema_within_budget(schema: &Value) -> bool {
    within_structure(schema) && expansion(schema) <= MAX_SCHEMA_EXPANSION
}

/// The depth, size and pattern count of the document itself.
fn within_structure(schema: &Value) -> bool {
    let mut nodes = 0usize;
    let mut patterns = 0usize;
    let mut stack = vec![(schema, 1usize)];
    while let Some((value, depth)) = stack.pop() {
        nodes += 1;
        if nodes > MAX_SCHEMA_NODES || depth > MAX_SCHEMA_DEPTH {
            return false;
        }
        match value {
            Value::Array(items) => stack.extend(items.iter().map(|v| (v, depth + 1))),
            Value::Object(map) => {
                if map.get("pattern").is_some_and(Value::is_string) {
                    patterns += 1;
                }
                if let Some(Value::Object(by_pattern)) = map.get("patternProperties") {
                    patterns += by_pattern.len();
                }
                if patterns > MAX_SCHEMA_PATTERNS {
                    return false;
                }
                stack.extend(map.values().map(|v| (v, depth + 1)));
            }
            _ => {}
        }
    }
    true
}

/// How many values validating one instance value could visit, each `$ref`
/// counted as the size of what it names. A target's size is worked out once
/// and reused, so this is linear in the document. A `$ref` back into a
/// definition still being sized (a recursive schema, a tree say) counts one:
/// validation follows it only as deep as the instance goes. A `$ref` this
/// can't resolve locally (a JSON pointer from the root, an `$anchor`, or an
/// `$id` spelled exactly as the `$ref`) counts one too; the validator
/// resolves only local references.
fn expansion(schema: &Value) -> u64 {
    let mut named = BTreeMap::new();
    let mut stack = vec![schema];
    while let Some(value) = stack.pop() {
        match value {
            Value::Object(map) => {
                for key in ["$anchor", "$dynamicAnchor", "$id"] {
                    if let Some(Value::String(name)) = map.get(key) {
                        named.insert(name.as_str(), value);
                    }
                }
                stack.extend(map.values());
            }
            Value::Array(items) => stack.extend(items),
            _ => {}
        }
    }
    let mut sizer = Sizer {
        root: schema,
        named,
        sized: BTreeMap::new(),
        sizing: Vec::new(),
    };
    sizer.size(schema, 0)
}

/// Works out [`expansion`], remembering each target's size by address.
struct Sizer<'a> {
    root: &'a Value,
    named: BTreeMap<&'a str, &'a Value>,
    sized: BTreeMap<usize, u64>,
    sizing: Vec<usize>,
}

impl<'a> Sizer<'a> {
    /// Deeper than any document the structural check passes, plus a chain of
    /// references; past it the schema is over budget.
    const MAX_NESTING: usize = 4 * MAX_SCHEMA_DEPTH;

    fn size(&mut self, value: &'a Value, nesting: usize) -> u64 {
        if nesting > Self::MAX_NESTING {
            return u64::MAX;
        }
        let address = core::ptr::from_ref(value) as usize;
        if let Some(&known) = self.sized.get(&address) {
            return known;
        }
        if self.sizing.contains(&address) {
            return 1;
        }
        self.sizing.push(address);
        let mut total: u64 = 1;
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    total = total.saturating_add(self.size(child, nesting + 1));
                    if key == "$ref"
                        && let Value::String(reference) = child
                        && let Some(target) = self.resolve(reference)
                    {
                        total = total.saturating_add(self.size(target, nesting + 1));
                    }
                }
            }
            Value::Array(items) => {
                for child in items {
                    total = total.saturating_add(self.size(child, nesting + 1));
                }
            }
            _ => {}
        }
        self.sizing.pop();
        self.sized.insert(address, total);
        total
    }

    /// The value `reference` names in this document, if it names one.
    fn resolve(&self, reference: &str) -> Option<&'a Value> {
        let Some(fragment) = reference.strip_prefix('#') else {
            return self.named.get(reference).copied();
        };
        if !fragment.is_empty() && !fragment.starts_with('/') {
            return self.named.get(fragment).copied();
        }
        let mut at = self.root;
        for segment in fragment.split('/').skip(1) {
            let segment = segment.replace("~1", "/").replace("~0", "~");
            at = match at {
                Value::Object(map) => map.get(&segment)?,
                Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
                _ => return None,
            };
        }
        Some(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::String;
    use serde_json::{Map, json};

    #[test]
    fn a_long_string_is_cut_on_a_character_boundary() {
        assert_eq!(shortened("short", 8), "short");
        assert_eq!(shortened("abcdefgh", 4), "abcd…");
        // "é" is two bytes: a cut inside it backs up to before it.
        assert_eq!(shortened("aé", 2), "a…");
    }

    /// A chain of definitions each naming the next twice is a small
    /// document with an exponential expansion; a recursive one is fine.
    #[test]
    fn a_reference_diamond_is_over_budget_and_recursion_is_not() {
        let diamond = |depth: usize| {
            let mut defs = Map::new();
            for i in 0..depth {
                let next = format!("#/$defs/d{}", i + 1);
                defs.insert(
                    format!("d{i}"),
                    json!({ "allOf": [ { "$ref": next }, { "$ref": next } ] }),
                );
            }
            defs.insert(format!("d{depth}"), json!({ "type": "string" }));
            json!({ "$defs": defs, "$ref": "#/$defs/d0" })
        };
        assert!(schema_within_budget(&diamond(8)));
        assert!(!schema_within_budget(&diamond(30)));

        let tree = json!({
            "$defs": { "node": {
                "type": "object",
                "properties": { "children": { "type": "array", "items": { "$ref": "#/$defs/node" } } }
            } },
            "$ref": "#/$defs/node"
        });
        assert!(schema_within_budget(&tree));

        // An `$anchor` names a target as well as a pointer does.
        let mut anchored = Map::new();
        for i in 0..30 {
            let mut def = json!({ "allOf": [ { "$ref": format!("#a{}", i + 1) }, { "$ref": format!("#a{}", i + 1) } ] });
            def["$anchor"] = json!(format!("a{i}"));
            anchored.insert(format!("d{i}"), def);
        }
        anchored.insert("d30".into(), json!({ "$anchor": "a30", "type": "string" }));
        assert!(!schema_within_budget(
            &json!({ "$defs": anchored, "$ref": "#a0" })
        ));
    }

    #[test]
    fn the_patterns_a_schema_holds_are_bounded() {
        let many = |n: usize| {
            let properties: Map<String, Value> = (0..n)
                .map(|i| {
                    (
                        alloc::format!("p{i}"),
                        json!({ "type": "string", "pattern": "^a+$" }),
                    )
                })
                .collect();
            json!({ "type": "object", "properties": properties })
        };
        assert!(schema_within_budget(&many(MAX_SCHEMA_PATTERNS)));
        assert!(!schema_within_budget(&many(MAX_SCHEMA_PATTERNS + 1)));
        // A property that happens to be named `pattern` is not one.
        assert!(schema_within_budget(
            &json!({ "properties": { "pattern": { "type": "string" } } })
        ));
    }

    #[test]
    fn depth_and_size_are_both_bounded() {
        assert!(schema_within_budget(
            &json!({ "type": "object", "properties": { "a": { "type": "string" } } })
        ));

        let mut deep = json!({ "type": "object" });
        for _ in 0..MAX_SCHEMA_DEPTH {
            deep = json!({ "type": "object", "properties": { "x": deep } });
        }
        assert!(!schema_within_budget(&deep));

        let wide: Map<String, Value> = (0..MAX_SCHEMA_NODES)
            .map(|i| (alloc::format!("p{i}"), json!({ "type": "string" })))
            .collect();
        assert!(!schema_within_budget(
            &json!({ "type": "object", "properties": wide })
        ));
    }
}
