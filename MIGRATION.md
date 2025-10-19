# TurboMCP 2.0.0 Migration Guide

This guide helps you migrate from TurboMCP 1.x to 2.0.0. The 2.0.0 release represents a major architectural overhaul focused on **clean minimal core + progressive enhancement**.

## 📋 Table of Contents

- [Quick Start](#quick-start)
- [Breaking Changes Summary](#breaking-changes-summary)
- [Crate-by-Crate Migration](#crate-by-crate-migration)
- [Feature Flag Changes](#feature-flag-changes)
- [Common Migration Patterns](#common-migration-patterns)
- [Troubleshooting](#troubleshooting)

## 🚀 Quick Start

### Minimal Migration (Recommended)

If you want to keep existing behavior:

```toml
# In your Cargo.toml

# Before (1.x)
[dependencies]
turbomcp = "1.x"

# After (2.0)
[dependencies]
turbomcp = { version = "2.0.0", features = ["full"] }
```

The `full` feature set restores 1.x behavior with all transports enabled.

### Progressive Migration

For new projects or to embrace the minimal-by-default philosophy:

```toml
# Start minimal
turbomcp = { version = "2.0.0", features = ["stdio"] }  # or just "2.0" (stdio is default)

# Add features as needed
turbomcp = { version = "2.0.0", features = ["stdio", "http", "tcp"] }
```

## 💥 Breaking Changes Summary

### 1. RBAC Removal (Major)

**What Changed:**
- The `rbac` feature has been completely removed
- Authorization functionality is no longer in the protocol layer

**Why:**
- Authorization is an application-layer concern, not protocol-layer
- Eliminates `casbin` dependency and `instant` unmaintained warning
- Reduces attack surface and improves security

**Migration Path:**

```rust
// Before (1.x)
use turbomcp::middleware::rbac::RbacMiddleware;

let rbac = RbacMiddleware::new(policy_path)?;
builder.with_middleware(rbac);

// After (2.0) - Implement in your application
use turbomcp::middleware::auth::AuthMiddleware;

// Option 1: Use JWT claims for authorization
let auth = AuthMiddleware::new(|claims: &Claims| {
    // Your authorization logic here
    claims.role == "admin" || claims.permissions.contains("tool:execute")
});

// Option 2: Use external policy engine (Oso, Casbin, etc.)
use oso::Oso;
let oso = Oso::new()?;
oso.load_files(vec!["policy.polar"])?;

let auth = AuthMiddleware::new(move |claims: &Claims, resource: &str| {
    oso.is_allowed(claims.user, resource, "execute")
});

builder.with_middleware(auth);
```

**Complete Examples:** See `RBAC-REMOVAL-SUMMARY.md` for detailed migration patterns.

### 2. Default Features Changed

**What Changed:**
```toml
# 1.x default
default = ["full"]  # All features enabled

# 2.0 default
default = ["stdio"]  # Minimal by default
```

**Why:**
- Progressive enhancement philosophy
- Smaller binaries by default
- Users opt-in to features they need
- Reduces compilation time for simple servers

**Migration:**

```toml
# If you need all features (1.x behavior)
turbomcp = { version = "2.0.0", features = ["full"] }

# Or selectively enable features
turbomcp = { version = "2.0.0", features = ["stdio", "http", "tcp"] }
```

### 3. New Crate Architecture

**What Changed:**
- Authentication extracted to `turbomcp-auth` crate
- DPoP extracted to `turbomcp-dpop` crate
- Both are optional dependencies

**Why:**
- Cleaner separation of concerns
- Optional security features
- Smaller core for minimal use cases

**Migration:**

```toml
# Before (1.x)
[dependencies]
turbomcp = { version = "1.x", features = ["auth", "dpop"] }

# After (2.0) - Automatically included via features
[dependencies]
turbomcp = { version = "2.0.0", features = ["auth", "dpop"] }

# Or use crates directly
turbomcp-auth = "2.0"
turbomcp-dpop = "2.0"
```

```rust
// Before (1.x)
use turbomcp::auth::*;
use turbomcp::dpop::*;

// After (2.0) - Same API
use turbomcp_auth::*;
use turbomcp_dpop::*;

// Or re-exported from main crate when features enabled
use turbomcp::auth::*;  // Still works with "auth" feature
use turbomcp::dpop::*;  // Still works with "dpop" feature
```

### 4. Feature Gate Renames

**What Changed:**
```toml
# Old names (1.x)
dpop-redis → redis-storage
dpop-test-utils → test-utils
```

**Migration:**
```toml
# Before
turbomcp = { version = "1.x", features = ["dpop-redis"] }

# After
turbomcp = { version = "2.0.0", features = ["dpop", "redis-storage"] }
# Note: "dpop" feature now required for DPoP features
```

**Backward Compatibility:**
- Old names still work but are deprecated
- Will be removed in 3.0

### 5. Module Reorganization

**What Changed:**
- Core context module split into focused submodules
- Protocol types module split into domain-specific modules

**Why:**
- Better maintainability
- Clearer organization
- Easier to navigate

**Migration:**

Most imports are re-exported, so this should be transparent:

```rust
// Before and After (no change for most users)
use turbomcp::prelude::*;

// If you used internal modules directly:
// Before (1.x)
use turbomcp_core::context::*;  // 2000+ line file

// After (2.0) - More specific
use turbomcp_core::context::request::RequestContext;
use turbomcp_core::context::capabilities::CapabilitiesContext;
// etc.

// Or use the re-exports
use turbomcp_core::context::*;  // Still works
```

## 📦 Crate-by-Crate Migration

### turbomcp-core

**Breaking Changes:**
- None for public API
- Internal modules reorganized (transparent via re-exports)

**New Features:**
- `ZeroCopyMessage` type for ultra-high throughput
- Security validation utilities (`validate_path`, etc.)
- Enhanced SIMD support

**Migration:**
```rust
// No changes needed - public API unchanged
```

### turbomcp-protocol

**Breaking Changes:**
- None for public API
- Internal types module reorganized

**Migration:**
```rust
// No changes needed - public API unchanged
```

### turbomcp-transport

**Breaking Changes:**
- None for public API

**New Features:**
- Enhanced resilience with circuit breaker metrics
- Better timeout handling

**Migration:**
```rust
// No changes needed
```

### turbomcp-server

**Breaking Changes:**
- RBAC middleware removed (see [RBAC Removal](#1-rbac-removal-major))
- `authz` middleware removed (application-layer concern)

**Migration:**
```rust
// Before (1.x)
use turbomcp_server::middleware::rbac::RbacMiddleware;
use turbomcp_server::middleware::authz::AuthzMiddleware;

builder
    .with_middleware(RbacMiddleware::new(policy)?)
    .with_middleware(AuthzMiddleware::new(rules)?);

// After (2.0)
use turbomcp_server::middleware::auth::AuthMiddleware;

// Implement authorization in AuthMiddleware
builder.with_middleware(AuthMiddleware::new(|claims| {
    // Your authorization logic
    check_permissions(claims)
}));
```

### turbomcp-client

**Breaking Changes:**
- None

**Migration:**
```rust
// No changes needed
```

### turbomcp-macros

**Breaking Changes:**
- None

**Migration:**
```rust
// No changes needed
```

### turbomcp-cli

**Breaking Changes:**
- Complete rewrite (see `crates/turbomcp-cli/MIGRATION.md`)
- New command structure
- New output formats

**Quick Migration:**
```bash
# Before (1.x)
turbomcp-cli tools-list --url URL
turbomcp-cli tools-call --url URL --name NAME --arguments ARGS

# After (2.0)
turbomcp-cli tools list --url URL
turbomcp-cli tools call NAME --arguments ARGS
```

**Full Details:** See `crates/turbomcp-cli/MIGRATION.md`

### turbomcp (root)

**Breaking Changes:**
- Default features: `["full"]` → `["stdio"]`
- RBAC feature removed
- Auth/DPoP now in separate crates

**Migration:**
```toml
# Restore 1.x behavior
turbomcp = { version = "2.0.0", features = ["full"] }

# Or be selective
turbomcp = { version = "2.0.0", features = ["stdio", "http", "auth"] }
```

### turbomcp-auth (NEW)

**This is a new crate in 2.0.0** containing all OAuth 2.1 and authentication functionality.

**Usage:**
```toml
# Via main crate (recommended)
turbomcp = { version = "2.0.0", features = ["auth"] }

# Or directly
turbomcp-auth = "2.0"
```

```rust
use turbomcp_auth::{
    oauth2::OAuth2Provider,
    providers::ApiKeyProvider,
};
```

### turbomcp-dpop (NEW)

**This is a new crate in 2.0.0** containing RFC 9449 DPoP implementation.

**Usage:**
```toml
# Via main crate (recommended)
turbomcp = { version = "2.0.0", features = ["dpop"] }

# Or directly
turbomcp-dpop = "2.0"
```

```rust
use turbomcp_dpop::{
    DPopProofBuilder,
    storage::RedisNonceStore,
};
```

## 🎯 Feature Flag Changes

### Recommended Feature Sets

```toml
# Minimal (default) - Basic tool servers
turbomcp = { version = "2.0.0", features = ["stdio"] }

# Full (1.x behavior) - All features
turbomcp = { version = "2.0.0", features = ["full"] }

# Network - STDIO + TCP
turbomcp = { version = "2.0.0", features = ["network"] }

# Server-only - TCP + Unix (no STDIO)
turbomcp = { version = "2.0.0", features = ["server-only"] }
```

### Individual Features

```toml
# Core functionality
"schema-generation"    # JSON Schema generation
"context-injection"    # Enhanced Context API
"uri-templates"        # URI template matching

# Authentication & Security
"auth"                 # OAuth 2.1, API key auth
"dpop"                 # DPoP (RFC 9449)
"redis-storage"        # Redis-based DPoP nonce tracking
"dpop-hsm-pkcs11"      # PKCS#11 HSM support
"dpop-hsm-yubico"      # YubiHSM support

# Transport protocols
"stdio"                # Standard MCP transport (default)
"http"                 # HTTP/SSE for web apps
"websocket"            # WebSocket bidirectional
"tcp"                  # Raw TCP sockets
"unix"                 # Unix domain sockets

# Performance
"simd"                 # SIMD JSON acceleration
```

### Deprecated Features

```toml
# These still work but will be removed in 3.0
"dpop-redis"           # Use "redis-storage" instead
"dpop-test-utils"      # Use "test-utils" instead
```

## 🔄 Common Migration Patterns

### Pattern 1: Basic Server (No Changes Needed)

```rust
// Works in both 1.x and 2.0
use turbomcp::prelude::*;

#[server]
struct MyServer;

#[tool("my_tool", "Description")]
async fn my_tool(&self, arg: String) -> McpResult<String> {
    Ok(format!("Hello {}", arg))
}

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer.run_stdio().await
}
```

### Pattern 2: HTTP Server

```toml
# Before (1.x) - HTTP enabled by default
turbomcp = "1.x"

# After (2.0) - Enable HTTP explicitly
turbomcp = { version = "2.0.0", features = ["http"] }
```

```rust
// Code unchanged
#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer.run_http("0.0.0.0:3000").await
}
```

### Pattern 3: Authentication

```toml
# Before (1.x)
turbomcp = { version = "1.x", features = ["auth"] }

# After (2.0) - Same feature name
turbomcp = { version = "2.0.0", features = ["auth"] }
```

```rust
// Before (1.x)
use turbomcp::auth::*;

// After (2.0) - Import from new crate location
use turbomcp_auth::*;
// Or use re-export (when "auth" feature enabled)
use turbomcp::auth::*;
```

### Pattern 4: DPoP with Redis

```toml
# Before (1.x)
turbomcp = { version = "1.x", features = ["dpop", "dpop-redis"] }

# After (2.0)
turbomcp = { version = "2.0.0", features = ["dpop", "redis-storage"] }
```

```rust
// Before (1.x)
use turbomcp::dpop::*;

// After (2.0)
use turbomcp_dpop::*;
// Or use re-export
use turbomcp::dpop::*;
```

### Pattern 5: Middleware Stack

```rust
// Before (1.x) - RBAC included
use turbomcp::middleware::{auth::*, rate_limit::*, rbac::*};

builder
    .with_middleware(AuthMiddleware::new(auth_config))
    .with_middleware(RbacMiddleware::new(policy)?)
    .with_middleware(RateLimitMiddleware::new(rate_config));

// After (2.0) - RBAC removed, implement in auth
use turbomcp::middleware::{auth::*, rate_limit::*};

builder
    .with_middleware(AuthMiddleware::new_with_authz(
        auth_config,
        |claims, resource| {
            // Authorization logic here (was in RBAC)
            check_permissions(claims, resource)
        }
    ))
    .with_middleware(RateLimitMiddleware::new(rate_config));
```

## 🐛 Troubleshooting

### Issue: "feature 'rbac' not found"

```toml
# Solution: RBAC removed, implement in application
# See: RBAC-REMOVAL-SUMMARY.md

# Remove rbac feature
turbomcp = { version = "2.0.0", features = ["auth"] }  # not "rbac"
```

### Issue: "HTTP server not working"

```toml
# Solution: Enable HTTP feature (not default in 2.0)
turbomcp = { version = "2.0.0", features = ["http"] }
```

### Issue: "module 'dpop' not found"

```toml
# Solution: Enable dpop feature
turbomcp = { version = "2.0.0", features = ["dpop"] }

# Or use crate directly
[dependencies]
turbomcp-dpop = "2.0"
```

### Issue: "dpop-redis feature not found"

```toml
# Solution: Renamed to redis-storage
turbomcp = { version = "2.0.0", features = ["dpop", "redis-storage"] }
```

### Issue: Import errors after upgrade

```rust
// Solution: Use prelude or updated imports

// Option 1: Use prelude (recommended)
use turbomcp::prelude::*;

// Option 2: Update imports to new crate structure
use turbomcp_auth::*;  // was: turbomcp::auth::*
use turbomcp_dpop::*;  // was: turbomcp::dpop::*
```

### Issue: Compilation errors in tests

```rust
// Solution: Update feature flags in dev-dependencies

[dev-dependencies]
turbomcp = { version = "2.0.0", features = ["full", "test-utils"] }
```

### Issue: Larger binary size than expected

```toml
# Solution: Use minimal features, not "full"
# Remove unused features to reduce binary size

# Instead of:
turbomcp = { version = "2.0.0", features = ["full"] }

# Use only what you need:
turbomcp = { version = "2.0.0", features = ["stdio", "tcp"] }
```

## 📚 Additional Resources

- **RBAC Migration:** See `RBAC-REMOVAL-SUMMARY.md` for detailed authorization patterns
- **CLI Migration:** See `crates/turbomcp-cli/MIGRATION.md` for CLI changes
- **Architecture Overview:** See `2.0.0-CLEAN-ARCHITECTURE.md` for design rationale
- **Security:** See `SECURITY-AUDIT-2.0.0.md` for security improvements
- **Examples:** See `examples/` directory for updated 2.0 examples

## 🎉 Benefits of 2.0

After migration, you'll enjoy:

- ✅ **Smaller binaries** - Pay only for what you use
- ✅ **Faster compilation** - Minimal dependencies by default
- ✅ **Better security** - Removed unmaintained dependencies
- ✅ **Cleaner architecture** - Clear separation of concerns
- ✅ **Production-ready** - Zero warnings, zero TODOs, zero tech debt
- ✅ **Latest toolchain** - Rust 1.90.0 + updated dependencies

## 🤝 Getting Help

- **Issues:** https://github.com/Epistates/turbomcp/issues
- **Discussions:** https://github.com/Epistates/turbomcp/discussions
- **Examples:** See `examples/` directory
- **Documentation:** https://docs.rs/turbomcp

## 📝 Version Compatibility

| TurboMCP Version | Rust Version | MCP Spec | Status |
|-----------------|--------------|----------|--------|
| 2.0.x           | 1.89.0+      | 2024-11-05 | ✅ Current |
| 1.1.x           | 1.89.0+      | 2025-06-18 | 🟡 Maintenance |
| 1.0.x           | 1.89.0+      | 2025-06-18 | ⚠️ EOL |
