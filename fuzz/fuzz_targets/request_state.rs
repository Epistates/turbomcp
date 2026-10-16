#![no_main]
//! A `requestState` comes back from the client, so `open` reads attacker
//! bytes: any token is the data a sealer produced or the uniform rejection.
//! And what a sealer seals opens again, for its own method and subject only.

use libfuzzer_sys::fuzz_target;
use turbomcp_server::fuzzing;

fuzz_target!(|input: (String, Vec<u8>, Option<String>)| {
    let (token, data, subject) = input;
    fuzzing::open_request_state(&token);
    if let Ok(data) = serde_json::from_slice(&data) {
        fuzzing::request_state_round_trip(&data, subject.as_deref());
    }
});
