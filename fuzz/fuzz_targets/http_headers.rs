#![no_main]
//! Every header check a Streamable HTTP request goes through before
//! dispatch: `Accept`, `Content-Type`, `Origin`, `Host`, `Last-Event-ID`
//! and the `MCP-Protocol-Version` / body agreement.

use libfuzzer_sys::fuzz_target;
use turbomcp_transport_http::fuzzing;

fuzz_target!(|input: (Vec<(String, Vec<u8>)>, Vec<u8>, String)| {
    let (headers, body, authority) = input;
    fuzzing::request_headers(&headers, &body, &authority);
});
