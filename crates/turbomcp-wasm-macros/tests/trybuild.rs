//! Compile-fail snapshots for `turbomcp-wasm-macros`.
//!
//! These tests exercise the syntactic rejection paths of the `#[server]`
//! macro — cases the proc-macro can detect without any `turbomcp-wasm`
//! runtime types in scope. Richer fixtures that exercise the *expanded*
//! output (missing `Clone`, wrong return types, missing trait bounds, etc.)
//! belong in a downstream integration crate that depends on both
//! `turbomcp-wasm` and `turbomcp-wasm-macros` — adding them here would create
//! a dependency cycle.
//!
//! Re-running after a macro change? Use `TRYBUILD=overwrite cargo test
//! --test trybuild` to refresh the `.stderr` snapshots.

#[test]
fn compile_fail_cases() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/compile_fail/server_on_struct.rs");
    t.compile_fail("tests/compile_fail/server_on_fn.rs");
}
