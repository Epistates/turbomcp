#![no_main]
//! Differential fuzz of the SIMD codec: `sonic-rs` parses the same untrusted
//! bytes the `serde_json` baseline does, so the two backends must never
//! panic, and whenever BOTH accept an input they must agree on the decoded
//! message (a divergence would make wire behavior depend on the `simd`
//! feature). Grammar-acceptance differences on malformed input are tolerated
//! — the codec contract is `Err`, not equivalence of error taxonomies.

use libfuzzer_sys::fuzz_target;
use turbomcp_core::JsonRpcMessage;
use turbomcp_core::codec::{Codec, SerdeJsonCodec, SonicRsCodec};

fuzz_target!(|data: &[u8]| {
    let sonic = SonicRsCodec;
    let serde = SerdeJsonCodec;

    let sonic_msg = sonic.decode::<JsonRpcMessage>(data);
    if let Ok(msg) = &sonic_msg {
        // Whatever sonic decoded must re-encode and re-decode identically.
        let bytes = sonic.encode(msg).expect("re-encode a decoded message");
        let round: JsonRpcMessage = sonic.decode(&bytes).expect("re-decode is stable");
        assert_eq!(&round, msg, "sonic round-trip drifted");
        // …and the serde backend must read sonic's encoding the same way.
        let cross: JsonRpcMessage = serde.decode(&bytes).expect("serde reads sonic output");
        assert_eq!(&cross, msg, "backends disagree on sonic's encoding");
    }

    if let (Ok(a), Ok(b)) = (sonic_msg, serde.decode::<JsonRpcMessage>(data)) {
        assert_eq!(a, b, "backends decoded the same bytes differently");
    }
});
