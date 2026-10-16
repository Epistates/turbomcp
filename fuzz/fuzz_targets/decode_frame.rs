#![no_main]
//! Every transport decodes a frame and, when it can't, answers with the
//! error the frame is owed. Classification must be total, and the error
//! response it builds must itself be a frame that decodes.

use libfuzzer_sys::fuzz_target;
use turbomcp_core::JsonRpcMessage;
use turbomcp_core::codec::{Codec, SerdeJsonCodec, decode_message};

fuzz_target!(|data: &[u8]| {
    let codec = SerdeJsonCodec;
    if let Err(invalid) = decode_message(&codec, data)
        && let Some(response) = invalid.response()
    {
        let bytes = codec
            .encode(&JsonRpcMessage::Response(response))
            .expect("encode");
        decode_message(&codec, &bytes).expect("an error response is a valid frame");
    }
});
