#![no_main]
//! A client's answers (elicitation, sampling, roots) are peer input to a
//! server, and a gateway re-sends them. Parsing never panics, and each
//! revision's rendering parses back to the same value: nothing is lost,
//! nothing an extra carries can shadow a field.

use libfuzzer_sys::fuzz_target;
use turbomcp_core::ProtocolVersion;
use turbomcp_protocol::neutral::{CreateMessageResult, ElicitOutcome, ListRootsResult};

const VERSIONS: [ProtocolVersion; 3] = [
    ProtocolVersion::V2025_06_18,
    ProtocolVersion::V2025_11_25,
    ProtocolVersion::V2026_07_28,
];

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) else {
        return;
    };
    if let Ok(outcome) = ElicitOutcome::from_wire(&value) {
        for version in &VERSIONS {
            let again = ElicitOutcome::from_wire(&outcome.to_wire(version));
            assert_eq!(again.as_ref(), Ok(&outcome), "{version}");
        }
    }
    if let Ok(result) = CreateMessageResult::from_wire(&value) {
        for version in &VERSIONS {
            // An older revision may refuse what it can't carry; what it
            // renders must come back whole.
            if let Ok(wire) = result.to_wire(version) {
                let again =
                    CreateMessageResult::from_wire(&wire).expect("its own rendering parses");
                assert_eq!(again, result, "{version}");
            }
        }
    }
    let roots = ListRootsResult::from_wire(&value);
    for version in &VERSIONS {
        assert_eq!(
            ListRootsResult::from_wire(&roots.to_wire(version)),
            roots,
            "{version}"
        );
    }
});
