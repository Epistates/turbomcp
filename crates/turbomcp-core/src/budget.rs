//! What a JSON Schema from a peer may cost before anything compiles it.
//!
//! A tool's `inputSchema` and `outputSchema` and an elicitation's
//! `requestedSchema` can come from the other side of the connection, or from
//! a gateway's upstream, and compiling one costs time and memory with its
//! size and nesting. `2026-07-28` asks for a bound (basic/index.mdx,
//! "Composition-Keyword Resource Use": implementations "SHOULD apply
//! reasonable bounds, such as a maximum schema depth, a cap on the total
//! number of subschemas"). The server, the client and the gateway check every
//! schema against this one budget before they compile it.

use alloc::{borrow::Cow, format, vec};
use serde_json::Value;

/// `text` cut to at most `max` bytes, on a character boundary, with `…`
/// appended when anything was cut: how a peer's string is quoted in an
/// error, a log line or a span.
#[must_use]
pub fn shortened(text: &str, max: usize) -> Cow<'_, str> {
    if text.len() <= max {
        return Cow::Borrowed(text);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    Cow::Owned(format!("{}…", &text[..end]))
}

/// The deepest schema compiled (nesting of objects and arrays).
pub const MAX_SCHEMA_DEPTH: usize = 64;

/// The most JSON values a compiled schema may contain.
pub const MAX_SCHEMA_NODES: usize = 20_000;

/// Whether `schema` stays within [`MAX_SCHEMA_DEPTH`] and
/// [`MAX_SCHEMA_NODES`]. It walks at most that many values, so checking costs
/// no more than the budget allows.
#[must_use]
pub fn schema_within_budget(schema: &Value) -> bool {
    let mut nodes = 0usize;
    let mut stack = vec![(schema, 1usize)];
    while let Some((value, depth)) = stack.pop() {
        nodes += 1;
        if nodes > MAX_SCHEMA_NODES || depth > MAX_SCHEMA_DEPTH {
            return false;
        }
        match value {
            Value::Array(items) => stack.extend(items.iter().map(|v| (v, depth + 1))),
            Value::Object(map) => stack.extend(map.values().map(|v| (v, depth + 1))),
            _ => {}
        }
    }
    true
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
