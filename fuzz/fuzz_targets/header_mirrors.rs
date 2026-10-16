#![no_main]
//! `Mcp-Param-*` headers against the arguments they mirror, where the tool
//! schema (a gateway's upstream) and the request are both untrusted.

use libfuzzer_sys::fuzz_target;
use serde_json::{Map, Value};

fuzz_target!(|input: (Vec<u8>, Vec<u8>, Vec<(String, Option<String>)>)| {
    let (schema, arguments, headers) = input;
    let object = |bytes: &[u8]| serde_json::from_slice::<Map<String, Value>>(bytes).ok();
    if let (Some(schema), Some(arguments)) = (object(&schema), object(&arguments)) {
        turbomcp_server::fuzzing::header_mirrors(schema, arguments, headers);
    }
});
