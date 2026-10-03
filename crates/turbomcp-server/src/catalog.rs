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
        Self(moka::sync::Cache::new(256))
    }
}
impl Validators {
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
            self.validate(schema, value)
                .map_err(|_| McpError::internal("tool structuredContent violates outputSchema"))?;
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
                let v = Arc::new(
                    jsonschema::validator_for(schema)
                        .map_err(|e| McpError::internal(format!("invalid tool schema: {e}")))?,
                );
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
