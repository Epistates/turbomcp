#![no_main]
//! A server's SSE stream as the client reads it: chunks split anywhere,
//! through the per-event size limit and the event parser, to frames.

use libfuzzer_sys::fuzz_target;
use turbomcp_transport_http::fuzzing;

fuzz_target!(|input: (Vec<Vec<u8>>, u16)| {
    let (chunks, limit) = input;
    let _ = futures::executor::block_on(fuzzing::fuzz_sse(chunks, usize::from(limit).max(16)));
});
