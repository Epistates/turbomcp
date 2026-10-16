#![no_main]
//! A composed list resumes at a cursor the client hands back.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|cursor: &str| {
    turbomcp_server::fuzzing::composite_cursor(cursor);
});
