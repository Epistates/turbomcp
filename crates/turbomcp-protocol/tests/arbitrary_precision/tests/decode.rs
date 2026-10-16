//! With `arbitrary_precision`, serde replays a decimal it buffered (for an
//! untagged enum or a `flatten`) as serde_json's private number map, which a
//! plain `f64` field rejects. Each case decodes from text, which is where the
//! map comes from, directly and through a buffer.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use turbomcp_protocol::{
    v2025_06_18::types as v0618, v2025_11_25::types as v1125, v2026_07_28::types as v0728,
};

/// Untagged, so serde buffers the input before `T` reads it.
#[derive(Deserialize)]
#[serde(untagged)]
enum Buffered<T> {
    Inner(T),
}

fn decode<T: DeserializeOwned>(text: &str) -> T {
    let Buffered::Inner(_) = serde_json::from_str::<Buffered<T>>(text)
        .unwrap_or_else(|e| panic!("buffered {}: {e}", core::any::type_name::<T>()));
    serde_json::from_str(text).unwrap_or_else(|e| panic!("{}: {e}", core::any::type_name::<T>()))
}

#[test]
fn the_feature_is_on() {
    // Only the feature keeps a number as written; without it this is `0.6`.
    let value: serde_json::Value = serde_json::from_str("0.60").unwrap();
    assert_eq!(value.to_string(), "0.60");
}

const BLOCK: &str = r#"{"type":"text","text":"x","annotations":{"priority":0.5}}"#;

#[test]
fn a_content_block_with_a_fractional_priority() {
    decode::<v0618::ContentBlock>(BLOCK);
    decode::<v1125::ContentBlock>(BLOCK);
    let v0728::ContentBlock::TextContent(text) = decode::<v0728::ContentBlock>(BLOCK) else {
        panic!("not a text block");
    };
    assert_eq!(text.annotations.unwrap().priority, Some(0.5));
}

#[test]
fn a_tool_result_carrying_one() {
    let result = format!(r#"{{"content":[{BLOCK}],"resultType":"complete"}}"#);
    decode::<v0618::CallToolResult>(&result);
    decode::<v1125::CallToolResult>(&result);
    decode::<v0728::CallToolResult>(&result);
}

#[test]
fn a_fractional_elicited_value() {
    let result = r#"{"action":"accept","content":{"ratio":3.5,"count":3}}"#;
    decode::<v0618::ElicitResult>(result);
    decode::<v1125::ElicitResult>(result);
    let content = decode::<v0728::ElicitResult>(result).content;
    assert_eq!(
        serde_json::to_string(&content).unwrap(),
        r#"{"count":3,"ratio":3.5}"#
    );
}

#[test]
fn progress_sampling_and_number_schemas() {
    let progress = r#"{"progressToken":"t","progress":0.5,"total":1.5}"#;
    decode::<v0618::ProgressNotificationParams>(progress);
    decode::<v1125::ProgressNotificationParams>(progress);
    decode::<v0728::ProgressNotificationParams>(progress);

    let preferences = r#"{"costPriority":0.25,"speedPriority":0.5,"intelligencePriority":0.75}"#;
    decode::<v0618::ModelPreferences>(preferences);
    decode::<v1125::ModelPreferences>(preferences);
    decode::<v0728::ModelPreferences>(preferences);

    let schema = r#"{"type":"number","minimum":0.5,"maximum":9.5}"#;
    decode::<v0618::NumberSchema>(schema);
    decode::<v1125::NumberSchema>(schema);
    decode::<v0728::NumberSchema>(schema);
}

#[test]
fn a_float_spelled_cache_hint() {
    let result = r#"{"tools":[],"ttlMs":5000.0,"cacheScope":"public","resultType":"complete"}"#;
    assert_eq!(decode::<v0728::ListToolsResult>(result).ttl_ms, 5000);
}
