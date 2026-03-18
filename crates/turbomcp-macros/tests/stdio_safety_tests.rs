//! Tests for stdio transport safety validation
//!
//! ## Current Status
//!
//! The two compile-fail tests below (`stdio_println_rejected.rs` and
//! `stdio_print_rejected.rs`) were written against the v2 `#[server]` macro
//! which accepted a `transports = ["stdio"]` attribute and used that to
//! enable compile-time `print!`/`println!` detection.
//!
//! In v3, the `transports` attribute is deprecated and emits a hard compile
//! error ("transports attribute is deprecated") before any stdio-safety
//! analysis can run. The compile-fail `.stderr` snapshots describe the old
//! stdio-safety error, so they no longer match actual compiler output. The
//! test is therefore kept `#[ignore]` until the compile-fail files are updated
//! to reflect v3 architecture.
//!
//! ## What Compile-Fail Tests Should Exist (TODOs)
//!
//! The following scenarios represent the intended trybuild coverage for the
//! v3 `#[server]` macro and should be added as the macro evolves:
//!
//! 1. **`server_requires_name.rs`** - `#[server]` without `name = "..."` should
//!    produce a clear error: "missing required attribute `name`".
//!
//! 2. **`tool_on_non_method.rs`** - `#[tool]` applied to a struct field or
//!    free function (outside `#[server]` impl block) should produce a clear
//!    error.
//!
//! 3. **`resource_missing_uri.rs`** - `#[resource]` without a URI argument
//!    should produce a clear error: "resource requires a URI pattern, e.g.
//!    #[resource(\"config://app\")]".
//!
//! 4. **`deprecated_transports_attr.rs`** - `#[server(transports = ["stdio"])]`
//!    should produce the deprecation error with the migration guidance message.
//!    This is the v3-appropriate successor to the stdio_print_rejected tests.
//!
//! 5. **`tool_unsupported_return_type.rs`** - A `#[tool]` returning a type
//!    that does not implement `IntoToolResponse` should produce a clear
//!    compile error (not a wall of trait-bound diagnostics).
//!
//! See: https://github.com/dtolnay/trybuild for trybuild usage.

#[test]
#[ignore = "Compile-fail snapshots describe v2 stdio-safety errors triggered by the \
    deprecated `transports` attribute. In v3, the `transports` attribute itself \
    fails to compile with a deprecation error before reaching stdio-safety analysis, \
    so the .stderr snapshots no longer match. TODO: replace these tests with \
    v3-appropriate compile-fail scenarios listed in the module doc above."]
fn stdio_safety_compile_tests() {
    let t = trybuild::TestCases::new();

    // These test files use `#[server(transports = ["stdio"])]` which in v3
    // triggers: "transports attribute is deprecated. Enable features in Cargo.toml..."
    // The .stderr snapshots instead expect stdio-safety errors about print!/println!.
    // The snapshots must be updated before these tests can be re-enabled.
    t.compile_fail("tests/compile_fail/stdio_println_rejected.rs");
    t.compile_fail("tests/compile_fail/stdio_print_rejected.rs");
}
