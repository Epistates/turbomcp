//! Tests for stdio transport safety validation
//!
//! These tests verify that the #[server] macro correctly rejects servers
//! using printf-like macros (println!, print!) when stdio transport is enabled.
//!
//! Uses trybuild to test compile-fail scenarios.

#[test]
fn stdio_safety_compile_tests() {
    let t = trybuild::TestCases::new();

    // Test that println! is rejected in stdio servers
    t.compile_fail("tests/compile_fail/stdio_println_rejected.rs");

    // Test that print! is rejected in stdio servers
    t.compile_fail("tests/compile_fail/stdio_print_rejected.rs");
}
