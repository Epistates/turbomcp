#![no_main]
//! A peer's result decoded on one wire, converted to the neutral types and
//! out to every wire. Each conversion is total, and what it produces
//! serializes.

use libfuzzer_sys::fuzz_target;
use turbomcp_protocol::neutral;
use turbomcp_protocol::v2025_06_18::types as v0618;
use turbomcp_protocol::v2025_11_25::types as v1125;
use turbomcp_protocol::v2026_07_28::types as v0728;

macro_rules! through {
    ($data:expr, $($wire:ident => $neutral:ty),* $(,)?) => {$(
        if let Ok(wire) = serde_json::from_slice::<v0728::$wire>($data) {
            let value: $neutral = wire.into();
            let _ = serde_json::to_vec(&v1125::$wire::from(value.clone())).unwrap();
            let _ = serde_json::to_vec(&v0728::$wire::from(value)).unwrap();
        }
        if let Ok(wire) = serde_json::from_slice::<v1125::$wire>($data) {
            let value: $neutral = wire.into();
            let legacy = v1125::$wire::from(value.clone());
            let _ = serde_json::to_vec(&v0618::$wire::from(legacy)).unwrap();
            let _ = serde_json::to_vec(&v0728::$wire::from(value)).unwrap();
        }
        if let Ok(wire) = serde_json::from_slice::<v0618::$wire>($data) {
            let value: $neutral = v1125::$wire::from(wire).into();
            let _ = serde_json::to_vec(&v0728::$wire::from(value)).unwrap();
        }
    )*};
}

fuzz_target!(|data: &[u8]| {
    through!(
        data,
        CallToolResult => neutral::CallToolResult,
        ListToolsResult => neutral::ListToolsResult,
        Tool => neutral::Tool,
        ReadResourceResult => neutral::ReadResourceResult,
        GetPromptResult => neutral::GetPromptResult,
        ListResourcesResult => neutral::ListResourcesResult,
        ListPromptsResult => neutral::ListPromptsResult,
        CompleteResult => neutral::CompleteResult,
    );
});
