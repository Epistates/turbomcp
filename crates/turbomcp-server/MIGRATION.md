> **Note:** This is the v1.x to v2.0.0 migration guide. For v2.x to v3.x migration, see the [top-level MIGRATION.md](../../MIGRATION.md).

# TurboMCP Server 2.0.0 Migration Guide

This guide helps you migrate from turbomcp-server 1.x to 2.0.0.

## 📋 Table of Contents

- [Overview](#overview)
- [Breaking Changes](#breaking-changes)
- [New Features](#new-features)
- [Migration Steps](#migration-steps)

## 🚀 Overview

**TurboMCP Server 2.0.0** has one major breaking change (RBAC removal) but is otherwise backward compatible.

### Key Changes
- **RBAC Removed** - Authorization moved to application layer (breaking)
- **Enhanced Middleware** - Better middleware composition
- **Improved Context** - Enhanced context management
- **Better Errors** - More detailed error messages

### Migration Timeline
- **Major Impact**: If using RBAC feature (see migration below)
- **Minor Impact**: All other users (minimal changes)
- **Full Compatibility**: Core server functionality unchanged

## 💥 Breaking Changes

### 1. RBAC Feature Removed

**What Changed:**
```toml
# v1.x - RBAC included
[dependencies]
turbomcp-server = { version = "1.x", features = ["rbac"] }

# v2.0.0 - RBAC removed
[dependencies]
turbomcp-server = { version = "2.0.0" }
# Feature "rbac" no longer exists
```

**Why:**
- Authorization is an application-layer concern, not protocol-layer
- Eliminates `casbin` dependency and `instant` unmaintained warning
- Reduces attack surface and improves security
- Follows industry best practices (separation of concerns)

**Migration Path:**

```rust
// Before (v1.x) - RBAC middleware
use turbomcp_server::middleware::rbac::RbacMiddleware;

let rbac = RbacMiddleware::new(policy_path)?;
builder.with_middleware(rbac);

// After (v2.0.0) - Implement in your application

// Option 1: Use JWT claims for authorization
use turbomcp_server::middleware::auth::AuthMiddleware;

let auth = AuthMiddleware::new(|claims: &Claims| {
    // Your authorization logic here
    claims.role == "admin" || claims.permissions.contains("tool:execute")
});
builder.with_middleware(auth);

// Option 2: Use external policy engine (Oso, Casbin, etc.)
use oso::Oso;

let oso = Oso::new()?;
oso.load_files(vec!["policy.polar"])?;

let auth = AuthMiddleware::new(move |claims: &Claims, resource: &str| {
    oso.is_allowed(claims.user, resource, "execute")
});
builder.with_middleware(auth);

// Option 3: Custom authorization logic
let auth = AuthMiddleware::new(|claims: &Claims| {
    // Database lookup
    let user_perms = db.get_user_permissions(&claims.user_id)?;
    user_perms.can_execute_tools()
});
builder.with_middleware(auth);
```

**Complete Examples**: See `../../RBAC-REMOVAL-SUMMARY.md` for detailed patterns.

### 2. authz Middleware Removed

Similar to RBAC, the generic `authz` middleware was removed. Use `AuthMiddleware` with custom logic instead.

## ✨ New Features

### 1. Enhanced Middleware System

**IMPROVED:** Better middleware composition and ordering:

```rust
use turbomcp_server::ServerBuilder;
use turbomcp_server::middleware::{auth::*, rate_limit::*, cors::*};

let server = ServerBuilder::new()
    .with_middleware(CorsMiddleware::permissive())  // First
    .with_middleware(AuthMiddleware::new(auth_fn))  // Second
    .with_middleware(RateLimitMiddleware::new(config))  // Third
    .build()?;
```

**Benefits:**
- Clear middleware ordering
- Better error propagation
- Enhanced observability

### 2. Improved Context Management

**ENHANCED:** Better context injection and lifecycle:

```rust
use turbomcp_server::context::RequestContext;

#[tool("Process data")]
async fn process(ctx: RequestContext, data: String) -> McpResult<String> {
    // Enhanced context with better methods
    ctx.info("Processing started").await?;
    ctx.track_metric("process_calls", 1).await?;

    Ok(processed)
}
```

**Benefits:**
- Better async handling
- Enhanced tracking capabilities
- Improved error context

### 3. Graceful Shutdown

**NEW:** Built-in graceful shutdown support:

```rust
use turbomcp_server::ServerBuilder;
use tokio::signal;

let server = ServerBuilder::new()
    .with_graceful_shutdown(async {
        signal::ctrl_c().await.ok();
    })
    .build()?;

// Server will complete in-flight requests before shutting down
```

**Benefits:**
- No lost requests
- Clean resource cleanup
- Better production behavior

### 4. Enhanced Health Checks

**IMPROVED:** Built-in health check support:

```rust
use turbomcp_server::ServerBuilder;

let server = ServerBuilder::new()
    .with_health_check(|server_state| async {
        // Custom health check logic
        Ok(server_state.is_healthy())
    })
    .build()?;
```

## 🔧 Migration Steps

### Step 1: Update Dependencies

```toml
# Before (v1.x)
[dependencies]
turbomcp-server = { version = "1.1.2", features = ["rbac"] }

# After (v2.0.0)
[dependencies]
turbomcp-server = "2.0.0"
# Note: "rbac" feature removed
```

### Step 2: Migrate RBAC Usage

If you were using RBAC:

```rust
// v1.x - RBAC middleware
use turbomcp_server::middleware::rbac::RbacMiddleware;
use turbomcp_server::middleware::authz::AuthzMiddleware;

builder
    .with_middleware(RbacMiddleware::new(policy)?)
    .with_middleware(AuthzMiddleware::new(rules)?);

// v2.0.0 - Application-layer authorization
use turbomcp_server::middleware::auth::AuthMiddleware;

builder.with_middleware(AuthMiddleware::new(|claims| {
    // Implement your authorization logic
    check_permissions(claims)
}));
```

See `../../RBAC-REMOVAL-SUMMARY.md` for complete migration patterns.

### Step 3: Update Middleware Stack

```rust
// v1.x - Old middleware imports
use turbomcp_server::middleware::{auth::*, rate_limit::*, rbac::*};

// v2.0.0 - No RBAC
use turbomcp_server::middleware::{auth::*, rate_limit::*};

// Authorization in AuthMiddleware instead
```

### Step 4: Build and Test

```bash
# Clean build
cargo clean
cargo build --all-features

# Run tests
cargo test
```

## 🎯 Common Migration Patterns

### Pattern 1: Basic Server (No RBAC - No Changes)

```rust
// v1.x and v2.0.0 - IDENTICAL
use turbomcp_server::ServerBuilder;

#[server]
impl MyServer {
    #[tool("Process")]
    async fn process(&self, input: String) -> McpResult<String> {
        Ok(input)
    }
}
```

**No changes needed if you weren't using RBAC!**

### Pattern 2: Server with RBAC → Auth

```rust
// v1.x - RBAC
use turbomcp_server::middleware::rbac::RbacMiddleware;

let server = ServerBuilder::new()
    .with_middleware(RbacMiddleware::new("policy.csv")?)
    .build()?;

// v2.0.0 - Custom authorization
use turbomcp_server::middleware::auth::AuthMiddleware;

let server = ServerBuilder::new()
    .with_middleware(AuthMiddleware::new(|claims: &Claims| {
        // Implement RBAC logic here
        let role = claims.get("role")?;
        matches!(role, "admin" | "editor")
    }))
    .build()?;
```

### Pattern 3: Multiple Middleware

```rust
// v1.x
use turbomcp_server::middleware::{auth::*, rate_limit::*, rbac::*};

builder
    .with_middleware(AuthMiddleware::new(config))
    .with_middleware(RbacMiddleware::new(policy)?)
    .with_middleware(RateLimitMiddleware::new(rate_config));

// v2.0.0 - RBAC logic in AuthMiddleware
use turbomcp_server::middleware::{auth::*, rate_limit::*};

builder
    .with_middleware(AuthMiddleware::new_with_authz(
        auth_config,
        |claims, resource| {
            // Authorization logic (was in RBAC)
            check_permissions(claims, resource)
        }
    ))
    .with_middleware(RateLimitMiddleware::new(rate_config));
```

### Pattern 4: Graceful Shutdown (NEW)

```rust
// v2.0.0 - NEW feature
use turbomcp_server::ServerBuilder;
use tokio::signal;

let server = ServerBuilder::new()
    .with_graceful_shutdown(async {
        signal::ctrl_c().await.ok();
    })
    .build()?;

server.run().await?;
```

## 🐛 Troubleshooting

### Issue: "feature 'rbac' not found"

**Solution:** RBAC removed, implement in application:

```toml
# Remove rbac feature
[dependencies]
turbomcp-server = "2.0.0"  # Not features = ["rbac"]
```

See `RBAC-REMOVAL-SUMMARY.md` for migration guide.

### Issue: "module 'rbac' not found"

**Solution:** Use AuthMiddleware with custom logic:

```rust
// Instead of:
use turbomcp_server::middleware::rbac::RbacMiddleware;

// Use:
use turbomcp_server::middleware::auth::AuthMiddleware;

// Implement authorization in AuthMiddleware
```

### Issue: Build errors after removing RBAC

**Solution:** Clean and rebuild:

```bash
cargo clean
rm Cargo.lock
cargo build
```

## 📊 Feature Comparison

| Feature | v1.x | v2.0.0 |
|---------|------|--------|
| Core server | ✅ | ✅ |
| Tools/Resources | ✅ | ✅ |
| Middleware | ✅ | ✅ Enhanced |
| Rate limiting | ✅ | ✅ |
| CORS | ✅ | ✅ |
| Auth | ✅ | ✅ Enhanced |
| RBAC | ✅ | ❌ Removed |
| Graceful shutdown | ❌ | ✅ NEW |
| Health checks | Basic | ✅ Enhanced |

## 📚 Additional Resources

- **RBAC Migration**: See `../../RBAC-REMOVAL-SUMMARY.md`
- **Main Migration Guide**: See `../../MIGRATION.md`
- **API Documentation**: https://docs.rs/turbomcp-server
- **Examples**: See `../../examples/` for server patterns

## 🎉 Benefits of 2.0.0

- ✅ **Cleaner Architecture** - Authorization in application layer
- ✅ **Better Security** - Removed unmaintained dependencies
- ✅ **Enhanced Middleware** - Better composition and ordering
- ✅ **Graceful Shutdown** - Production-ready lifecycle
- ✅ **Flexible Authorization** - Implement your own logic

## 🤝 Getting Help

- **Issues**: https://github.com/Epistates/turbomcp/issues
- **Discussions**: https://github.com/Epistates/turbomcp/discussions
- **Documentation**: https://docs.rs/turbomcp-server

## 📝 Version Compatibility

| turbomcp-server | Status | Migration |
|-----------------|--------|-----------|
| 2.0.0             | ✅ Current | RBAC removed, see guide |
| 1.1.x           | 🟡 Maintenance | Upgrade for security |
| 1.0.x           | ⚠️ EOL | Upgrade recommended |
