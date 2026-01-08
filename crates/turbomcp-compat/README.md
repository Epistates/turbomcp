# turbomcp-compat

Backward compatibility layer for migrating from TurboMCP v2.x to v3.x.

## Overview

This crate provides type aliases and compatibility shims to help users migrate their code from TurboMCP v2.x to v3.x with minimal changes. All types are deprecated and will guide users to the correct v3.x alternatives.

## Usage

Add to your `Cargo.toml`:

```toml
[dependencies]
turbomcp = "3.0"
turbomcp-compat = "3.0"
```

Then use the compatibility types:

```rust
// v2.x imports (deprecated, with migration guidance)
use turbomcp_compat::v2::{
    ServerError,      // -> McpError
    ServerResult,     // -> McpResult
    Error,            // -> McpError
};

// Deprecation warnings will guide you to v3.x types
```

## Migration Guide

See [MIGRATION.md](https://github.com/Epistates/turbomcp/blob/main/MIGRATION.md) for a complete migration guide.

### Quick Reference

| v2.x Type | v3.x Type | Notes |
|-----------|-----------|-------|
| `ServerError` | `McpError` | Unified error type |
| `ServerResult<T>` | `McpResult<T>` | Unified result type |
| `Error` | `McpError` | Protocol errors consolidated |
| `Claims` | `AuthContext` | Use turbomcp-auth crate |

## Features

- `server`: Enable server-related compatibility types (Claims, AuthConfig)
- `full`: Enable all compatibility features

## Deprecation Timeline

- **v3.0.0**: All compat types marked deprecated
- **v3.1.0**: Deprecation warnings become errors with `#[deny(deprecated)]`
- **v4.0.0**: This crate will be removed

## License

MIT
