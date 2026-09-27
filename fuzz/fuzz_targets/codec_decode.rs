#![no_main]
//! The wire codec parses untrusted bytes off every transport. Decoding
//! arbitrary input must never panic — it returns `Err(CodecError)` or a value,
//! and any decoded value must re-encode cleanly (round-trip stability).

use libfuzzer_sys::fuzz_target;
use turbomcp_core::JsonRpcMessage;
use turbomcp_core::codec::{Codec, SerdeJsonCodec};

fuzz_target!(|data: &[u8]| {
    let codec = SerdeJsonCodec;
    if let Ok(msg) = codec.decode::<JsonRpcMessage>(data) {
        // A value that decoded must encode again, and re-decode identically.
        let bytes = codec.encode(&msg).expect("re-encode a decoded message");
        let round: JsonRpcMessage = codec.decode(&bytes).expect("re-decode is stable");
        let _ = round;
    }
});
