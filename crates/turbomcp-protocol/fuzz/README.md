# TurboMCP Protocol Fuzz Testing

This directory contains fuzz testing targets for the turbomcp-protocol crate using
[cargo-fuzz](https://github.com/rust-fuzz/cargo-fuzz) and libFuzzer.

## Prerequisites

Install cargo-fuzz (requires nightly Rust):

```bash
cargo install cargo-fuzz
```

## Available Fuzz Targets

### 1. `fuzz_jsonrpc_parsing`

Tests JSON-RPC 2.0 message parsing against arbitrary byte sequences. Validates:
- UTF-8 decoding robustness
- Request/response/notification parsing
- Batch message handling
- Protocol type deserialization

### 2. `fuzz_tool_deserialization`

Tests Tool type deserialization using both:
- Raw JSON parsing from arbitrary bytes
- Structured fuzzing with `Arbitrary` trait for systematic coverage

Covers: `Tool`, `ToolExecution`, `ToolInputSchema`, `ToolAnnotations`, `TaskSupportMode`

### 3. `fuzz_message_validation`

Tests MCP message validation against malformed, edge-case, and adversarial inputs:
- JSON-RPC field variations (version, id, method)
- Params type validation
- Result/error combinations
- Nested structure handling

### 4. `fuzz_capability_parsing`

Tests capability negotiation parsing:
- `ServerCapabilities` and `ClientCapabilities`
- Initialize request/result roundtrips
- Experimental capability handling

## Running Fuzz Tests

```bash
# Navigate to the fuzz directory
cd crates/turbomcp-protocol/fuzz

# Run a specific fuzz target
cargo +nightly fuzz run fuzz_jsonrpc_parsing

# Run with a time limit (e.g., 60 seconds)
cargo +nightly fuzz run fuzz_jsonrpc_parsing -- -max_total_time=60

# Run with parallel jobs
cargo +nightly fuzz run fuzz_jsonrpc_parsing -- -jobs=4

# List all available targets
cargo +nightly fuzz list
```

## Corpus Management

Fuzz corpus is stored in `corpus/<target_name>/`. To seed the corpus with
interesting inputs:

```bash
mkdir -p corpus/fuzz_jsonrpc_parsing
echo '{"jsonrpc":"2.0","id":1,"method":"test"}' > corpus/fuzz_jsonrpc_parsing/valid_request
```

## Crash Analysis

When a crash is found, the offending input is saved to `artifacts/<target_name>/`.
To reproduce:

```bash
cargo +nightly fuzz run fuzz_jsonrpc_parsing artifacts/fuzz_jsonrpc_parsing/crash-xxx
```

## Coverage

To generate coverage reports:

```bash
cargo +nightly fuzz coverage fuzz_jsonrpc_parsing
```

## CI Integration

For CI, run with limited time:

```bash
# Quick smoke test (10 seconds per target)
for target in $(cargo +nightly fuzz list); do
    cargo +nightly fuzz run "$target" -- -max_total_time=10
done
```
