#![no_main]
//! The digest a `requestState` is bound to canonicalizes peer params
//! (RFC 8785). It must not panic on anything serde_json parses, and a
//! re-encoded retry (keys reordered, numbers respelled) binds the same.

use libfuzzer_sys::fuzz_target;
use turbomcp_server::fuzzing;

fuzz_target!(|input: (String, Vec<u8>)| {
    let (method, params) = input;
    let Ok(params) = serde_json::from_slice::<serde_json::Value>(&params) else {
        return;
    };
    let first = fuzzing::request_binding(&method, Some(params.clone()));
    let reencoded: serde_json::Value =
        serde_json::from_str(&params.to_string()).expect("serde_json re-reads its own output");
    assert_eq!(first, fuzzing::request_binding(&method, Some(reencoded)));
});
