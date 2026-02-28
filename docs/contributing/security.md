# Security & Robustness

TurboMCP is built with production-grade security and robustness as core design goals. The architecture follows a Zero-Trust model, utilizing secure serialization, active sanitization, and continuous fuzzing to prevent vulnerabilities.

## Threat Model

The Model Context Protocol connects LLMs (potentially executing arbitrary generated outputs) with local system resources or databases. TurboMCP treats all incoming protocol messages as fundamentally untrusted.

## Security Features

1.  **Fuzzing (Continuous Protocol Hardening)**
    *   TurboMCP uses `cargo-fuzz` on protocol and payload parsing boundaries (JSON-RPC, tool deserialization, message validation, capability parsing).
    *   Fuzz targets are run continuously via GitHub Actions to surface panics, out-of-memory errors, or algorithmic complexity (DoS) vulnerabilities.

2.  **Error Sanitization**
    *   Errors returned from the Server to the Client pass through a built-in sanitization layer (`sanitize_error_message`).
    *   The following patterns are actively redacted from `McpError` messages before transmission to prevent information leakage to LLMs:
        *   Database connection strings (PostgreSQL, MySQL, MongoDB, Redis, etc.)
        *   URLs containing embedded credentials
        *   Secrets in key=value or key:value format (`api_key`, `password`, `token`, `secret`, `Bearer`)
        *   IPv4 addresses
        *   File paths (Unix and Windows)
        *   Email addresses

3.  **Strict Resource Bounds**
    *   All incoming payloads are strictly size-limited (default: 1 MB request, 10 MB response; configurable via `LimitsConfig` with `default()`, `strict()`, and `unlimited()` presets).
    *   Recursion/nesting depth limits are enforced during JSON deserialization to prevent stack overflow attacks.

4.  **Zero-Copy Message Processing (Optional)**
    *   When the `zero-copy` feature flag is enabled, internal routing can use `rkyv` zero-copy serialization to reduce allocation pressure under heavy load.
    *   The `Bytes` type is used throughout the transport layer to minimize copies on the hot path regardless of feature flags.

## Reporting a Vulnerability

If you discover a security vulnerability in TurboMCP, please **DO NOT** open a public issue. Instead, email security@turbomcp.org.
