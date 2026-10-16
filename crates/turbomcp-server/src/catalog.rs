//! Authoritative component lookup and bounded schema compilation.
use serde_json::Value;
use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use turbomcp_core::{McpError, McpResult};
use turbomcp_protocol::neutral;

/// Feed `value`'s structure into `hasher`, objects in key order. Two equal
/// values with keys in different orders hash differently, which costs a
/// cache miss and nothing else.
fn hash_value(value: &Value, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash as _;
    match value {
        Value::Null => 0u8.hash(hasher),
        Value::Bool(b) => (1u8, b).hash(hasher),
        Value::Number(n) => (2u8, n.to_string()).hash(hasher),
        Value::String(s) => (3u8, s).hash(hasher),
        Value::Array(items) => {
            (4u8, items.len()).hash(hasher);
            for item in items {
                hash_value(item, hasher);
            }
        }
        Value::Object(map) => {
            (5u8, map.len()).hash(hasher);
            for (k, v) in map {
                k.hash(hasher);
                hash_value(v, hasher);
            }
        }
    }
}

/// Compatibility lookup for dynamic providers. Providers with a direct index
/// should override the capability's lookup method to avoid enumeration.
pub(crate) async fn find<T, F, Fut>(
    mut fetch: F,
    matches: impl Fn(&T) -> bool,
) -> McpResult<Option<T>>
where
    F: FnMut(neutral::ListParams) -> Fut,
    Fut: Future<Output = McpResult<(Vec<T>, Option<String>)>>,
{
    let mut params = neutral::ListParams::default();
    let mut seen = HashSet::new();
    for _ in 0..10_000 {
        let (items, next) = fetch(params).await?;
        if let Some(item) = items.into_iter().find(&matches) {
            return Ok(Some(item));
        }
        let Some(cursor) = next else { return Ok(None) };
        if !seen.insert(cursor.clone()) {
            return Err(McpError::internal("catalog cursor did not advance"));
        }
        params = neutral::ListParams::with_cursor(cursor);
    }
    Err(McpError::internal("catalog page limit exceeded"))
}

/// The deepest schema compiled (nesting of objects and arrays).
const MAX_SCHEMA_DEPTH: usize = 64;
/// The most JSON values a compiled schema may contain.
const MAX_SCHEMA_NODES: usize = 20_000;
/// Compiled validators kept by default; see
/// [`ServerBuilder::schema_cache_capacity`](crate::ServerBuilder::schema_cache_capacity).
pub(crate) const DEFAULT_SCHEMA_CACHE: u64 = 256;

/// Whether `schema` stays within the compile budget (basic/index.mdx,
/// "Composition-Keyword Resource Use": bound schema depth and subschema count).
/// A gateway compiles schemas an upstream wrote, so this runs before compile.
fn within_budget(schema: &Value) -> bool {
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

/// Cache keyed by the complete schema, not a tool name or a first caller's
/// catalog. Changed definitions acquire a new validator; errors aren't cached.
///
/// Looked up on every call, so the key is a structural hash of the schema
/// (no serialization, no allocation) and a hit is confirmed by comparing the
/// schema itself: a hash collision costs a recompile, never the wrong
/// validator.
pub(crate) struct Validators(moka::sync::Cache<u64, Arc<(Value, Arc<jsonschema::Validator>)>>);
impl Default for Validators {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_SCHEMA_CACHE)
    }
}
impl Validators {
    pub(crate) fn with_capacity(capacity: u64) -> Self {
        Self(moka::sync::Cache::new(capacity))
    }

    /// Validate a `tools/call`'s arguments against the tool's `inputSchema`.
    ///
    /// A root that can't describe an object is the server's bug (the spec
    /// types `inputSchema` as `type: "object"`): every call would fail, so it
    /// is refused as one, naming the tool, rather than read by the model as
    /// bad arguments.
    pub(crate) fn input(&self, tool: &str, schema: &Value, arguments: &Value) -> McpResult<()> {
        if !neutral::is_object_root(schema) {
            return Err(McpError::internal(format!(
                "tool `{tool}` has an inputSchema whose root is not `type: \"object\"`"
            )));
        }
        self.validate(schema, arguments)
    }

    pub(crate) fn output(
        &self,
        schema: Option<&Value>,
        result: neutral::CallToolResult,
    ) -> McpResult<neutral::CallToolResult> {
        if !result.is_error
            && let Some(schema) = schema
        {
            let value = result.structured_content.as_ref().ok_or_else(|| {
                McpError::internal("tool outputSchema requires structuredContent")
            })?;
            self.validate(schema, value).map_err(|e| match e {
                McpError::InvalidParams(_) => {
                    McpError::internal("tool structuredContent violates outputSchema")
                }
                other => other,
            })?;
        }
        Ok(result)
    }

    pub(crate) fn validate(&self, schema: &Value, value: &Value) -> McpResult<()> {
        let key = {
            use std::hash::Hasher as _;
            let mut hasher = std::hash::DefaultHasher::new();
            hash_value(schema, &mut hasher);
            hasher.finish()
        };
        let validator = match self.0.get(&key).filter(|entry| entry.0 == *schema) {
            Some(entry) => Arc::clone(&entry.1),
            None => {
                let v = Arc::new(compile(schema)?);
                self.0
                    .insert(key, Arc::new((schema.clone(), Arc::clone(&v))));
                v
            }
        };
        validator
            .validate(value)
            .map_err(|e| McpError::invalid_params(format!("invalid arguments (inputSchema): {e}")))
    }
}

/// Compile `schema` within the budget. An unknown `$schema` is reported as an
/// unsupported dialect (basic/index.mdx: "return an error indicating the
/// dialect is not supported"), not as a malformed schema.
fn compile(schema: &Value) -> McpResult<jsonschema::Validator> {
    if !within_budget(schema) {
        return Err(McpError::internal(format!(
            "tool schema exceeds the validation budget \
             (depth {MAX_SCHEMA_DEPTH}, {MAX_SCHEMA_NODES} values)"
        )));
    }
    jsonschema::validator_for(schema).map_err(|e| match e.kind() {
        jsonschema::error::ValidationErrorKind::Referencing(
            jsonschema::ReferencingError::UnknownSpecification { specification },
        ) => McpError::internal(format!(
            "unsupported JSON Schema dialect `{specification}` in tool schema"
        )),
        _ => McpError::internal(format!("invalid tool schema: {e}")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, json};

    #[test]
    fn an_untyped_object_root_validates_its_properties() {
        let v = Validators::default();
        let schema = json!({ "properties": { "q": { "type": "string" } }, "required": ["q"] });
        v.input("search", &schema, &json!({ "q": "x" })).unwrap();
        assert!(matches!(
            v.input("search", &schema, &json!({})),
            Err(McpError::InvalidParams(_))
        ));
    }

    #[test]
    fn a_non_object_root_is_the_servers_error() {
        let err = Validators::default()
            .input("bad", &json!({ "type": "string" }), &json!({}))
            .unwrap_err();
        assert!(matches!(err, McpError::Internal(_)), "{err:?}");
        assert!(err.to_string().contains("`bad`"), "{err}");
    }

    #[test]
    fn an_unknown_dialect_says_so() {
        let schema = json!({ "$schema": "https://example.com/my-dialect", "type": "object" });
        let err = Validators::default()
            .input("t", &schema, &json!({}))
            .unwrap_err();
        assert!(matches!(err, McpError::Internal(_)), "{err:?}");
        assert!(
            err.to_string().contains("unsupported JSON Schema dialect"),
            "{err}"
        );
    }

    #[test]
    fn a_schema_over_budget_is_refused_before_compiling() {
        let mut deep = json!({ "type": "object" });
        for _ in 0..MAX_SCHEMA_DEPTH {
            deep = json!({ "type": "object", "properties": { "x": deep } });
        }
        let err = Validators::default()
            .input("deep", &deep, &json!({}))
            .unwrap_err();
        assert!(err.to_string().contains("validation budget"), "{err}");

        let wide: Map<String, Value> = (0..MAX_SCHEMA_NODES)
            .map(|i| (format!("p{i}"), json!({ "type": "string" })))
            .collect();
        let wide = json!({ "type": "object", "properties": wide });
        let err = Validators::default()
            .input("wide", &wide, &json!({}))
            .unwrap_err();
        assert!(err.to_string().contains("validation budget"), "{err}");
    }

    #[test]
    fn an_output_schema_that_does_not_compile_is_not_reported_as_a_violation() {
        let mut result = neutral::CallToolResult::text("{\"n\":1}");
        result.structured_content = Some(json!({ "n": 1 }));
        let schema = json!({ "$schema": "https://example.com/x", "type": "object" });
        let err = Validators::default()
            .output(Some(&schema), result)
            .unwrap_err();
        assert!(err.to_string().contains("dialect"), "{err}");
    }
}
