//! Entry points for `fuzz/` into the parsers a peer reaches. Compiled only
//! under `cfg(fuzzing)`, which `cargo fuzz` sets; not API.

use serde_json::{Map, Value};
use turbomcp_core::{JsonRpcRequest, ObservedHeaders};
use turbomcp_protocol::neutral;

use crate::mrtr::StateSealer;

/// A sealer with a fixed key, so a fuzzer's corpus stays meaningful across
/// runs.
fn sealer() -> StateSealer {
    StateSealer::from_keys([7; 32], [[9; 32]])
}

/// Open a client-supplied `requestState`: any input is either the handler
/// data a sealer produced or the uniform rejection, never a panic.
pub fn open_request_state(token: &str) {
    let _ = sealer().open("tools/call", None, token);
    let _ = sealer().open("tools/call", Some("alice"), token);
}

/// What a sealer seals, it opens, for the same method and subject only.
///
/// # Panics
/// When a round trip loses or leaks the data.
pub fn request_state_round_trip(data: &Value, subject: Option<&str>) {
    let sealer = sealer();
    let Ok(token) = sealer.seal("tools/call", subject, data) else {
        return; // over the size limit
    };
    let opened = sealer
        .open("tools/call", subject, &token)
        .expect("a sealed state opens");
    assert_eq!(&opened, data, "the round trip changed the data");
    assert!(
        sealer.open("prompts/get", subject, &token).is_err(),
        "another method opened it"
    );
    let other = if subject == Some("mallory") {
        "eve"
    } else {
        "mallory"
    };
    assert!(
        sealer.open("tools/call", Some(other), &token).is_err(),
        "another subject opened it"
    );
}

/// The request digest a `requestState` is bound to, over peer params.
pub fn request_binding(method: &str, params: Option<Value>) -> String {
    crate::dispatcher::request_binding(&JsonRpcRequest::new(1, method, params))
}

/// Resume a composed list at a peer-supplied cursor.
pub fn composite_cursor(cursor: &str) {
    let mounts = ["weather", "#1", "files", "a.b"];
    if let Ok((at, own)) = crate::composite::resume_at(mounts, Some(cursor)) {
        assert!(at < mounts.len());
        assert!(own.as_deref().is_none_or(|own| cursor.ends_with(own)));
    }
}

/// Compare `Mcp-Param-*` headers with the arguments they mirror, for a tool
/// whose schema and arguments came from the peer (a gateway's upstream).
pub fn header_mirrors(
    schema: Map<String, Value>,
    arguments: Map<String, Value>,
    headers: Vec<(String, Option<String>)>,
) {
    let tool = neutral::Tool::new("t", Value::Object(schema));
    let params = neutral::CallToolParams::new("t", arguments);
    let observed = ObservedHeaders(headers.into_iter().collect());
    let _ = crate::dispatcher::check_header_mirrors(Some(&observed), &params, &tool);
}
