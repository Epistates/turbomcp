#![no_main]
//! The Streamable HTTP draft header codec decodes attacker-controlled
//! `Mcp-Param-*` header values (Base64 sentinel encoding). Decoding arbitrary
//! strings must never panic, and the encode→decode round-trip must be lossless
//! for any value the encoder accepts.

use libfuzzer_sys::fuzz_target;
use turbomcp_service::mcp_headers;

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };

    // Decoding untrusted header values must not panic.
    let _ = mcp_headers::decode_value(s);

    // Header-name and header-safety predicates must be total.
    let _ = mcp_headers::is_valid_header_name(s);
    let _ = mcp_headers::is_header_safe(s);

    // Round-trip: whatever encode produces must decode back to the original.
    let encoded = mcp_headers::encode_value(s);
    let decoded = mcp_headers::decode_value(&encoded);
    assert_eq!(
        decoded.as_deref(),
        Some(s),
        "sentinel round-trip must be lossless"
    );
});
