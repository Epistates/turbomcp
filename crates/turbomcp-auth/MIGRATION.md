> **Note:** This is the v1.x to v2.0.0 migration guide. For v2.x to v3.x migration, see the [top-level MIGRATION.md](../../MIGRATION.md).

# TurboMCP Auth 2.0.0 Migration Guide

This guide helps you migrate from turbomcp (with inline auth) to the standalone turbomcp-auth crate in 2.0.0.

## 📋 Table of Contents

- [Overview](#overview)
- [Migration Paths](#migration-paths)
- [Breaking Changes](#breaking-changes)
- [New Features](#new-features)
- [Migration Steps](#migration-steps)

## 🚀 Overview

**TurboMCP Auth 2.0.0** is a **NEW standalone crate** extracted from the main turbomcp crate.

### Key Changes
- **Extracted Crate**: OAuth 2.1 and authentication moved to dedicated `turbomcp-auth` crate
- **Cleaner Dependencies**: Optional auth features don't bloat minimal builds
- **Same API**: Functionality unchanged, only import paths different
- **Progressive Enhancement**: Only include auth when you need it

### Who Needs This Guide?
- ✅ Users of OAuth 2.1 or authentication features in turbomcp 1.x
- ✅ Users migrating from inline auth to standalone crate
- ❌ New users (just use `turbomcp-auth` directly)
- ❌ Users not using auth features (no action needed)

## 💥 Breaking Changes

### Authentication Functionality Extracted

**What Changed:**
```toml
# v1.x - Auth integrated in main crate
[dependencies]
turbomcp = { version = "1.x", features = ["auth"] }

# v2.0.0 - Auth in separate crate
[dependencies]
turbomcp = { version = "2.0.0", features = ["auth"] }
# OR use directly:
turbomcp-auth = "2.0.0"
```

**Why:**
- Cleaner separation of concerns
- Minimal builds don't include OAuth dependencies
- Better modularity and independent versioning
- Easier to maintain and test

**Impact:** Import paths changed, functionality unchanged

## 🔄 Migration Paths

### Path 1: Via Main Crate (Recommended)

**Best for:** Existing turbomcp users who want minimal changes

```toml
[dependencies]
turbomcp = { version = "2.0.0", features = ["auth"] }
```

```rust
// Imports work via re-exports
use turbomcp::auth::*;
```

**Migration:** Just update your `Cargo.toml` - imports still work!

### Path 2: Direct Crate Usage

**Best for:** New projects or those wanting explicit dependencies

```toml
[dependencies]
turbomcp-auth = "2.0.0"
turbomcp-protocol = "2.0.0"  # Required for MCP types
```

```rust
// Direct imports
use turbomcp_auth::{
    oauth2::OAuth2Provider,
    providers::ApiKeyProvider,
};
```

**Migration:** Update imports from `turbomcp::auth::*` to `turbomcp_auth::*`

## 📦 Feature Flags

### Main Crate Features

```toml
# Auth features available via main crate
turbomcp = { version = "2.0.0", features = [
    "auth",     # OAuth 2.1 and API key authentication
    "dpop",     # DPoP support (requires auth)
]}
```

### Direct Crate Features

```toml
turbomcp-auth = { version = "2.0.0", features = [
    "dpop",     # DPoP (RFC 9449) support
]}
```

## ✨ New Features in 2.0.0

### 1. Standalone Crate

- Independent versioning
- Cleaner dependency graph
- Better documentation
- Focused testing

### 2. Enhanced OAuth 2.1 Support

- Complete RFC 8707 Resource Indicators
- RFC 9728 Protected Resource Metadata
- RFC 7591 Dynamic Client Registration
- PKCE support
- Multi-provider support (Google, GitHub, Microsoft)

### 3. Improved Security

- Redirect URI validation
- Domain whitelisting
- Attack vector prevention
- Production security levels

### 4. MCP Protocol Integration

- Resource registry with RFC 9728 compliance
- Bearer token methods
- Auto resource indicators
- Security level configuration

## 🔧 Migration Steps

### Step 1: Update Dependencies

**Option A - Via Main Crate:**
```toml
# Before
[dependencies]
turbomcp = { version = "1.1.2", features = ["auth"] }

# After
[dependencies]
turbomcp = { version = "2.0.0", features = ["auth"] }
```

**Option B - Direct Crate:**
```toml
# Before
[dependencies]
turbomcp = { version = "1.1.2", features = ["auth"] }

# After
[dependencies]
turbomcp-auth = "2.0.0"
turbomcp-protocol = "2.0.0"
```

### Step 2: Update Imports

**Option A - Via Main Crate (No Changes):**
```rust
// Still works!
use turbomcp::auth::*;
```

**Option B - Direct Crate:**
```rust
// Before (v1.x)
use turbomcp::auth::OAuth2Provider;
use turbomcp::auth::providers::ApiKeyProvider;

// After (v2.0.0)
use turbomcp_auth::oauth2::OAuth2Provider;
use turbomcp_auth::providers::ApiKeyProvider;
```

### Step 3: Build and Test

```bash
# Clean build
cargo clean
cargo build --all-features

# Run tests
cargo test --features auth
```

## 🎯 Common Migration Patterns

### Pattern 1: OAuth 2.1 Provider

```rust
// v1.x and v2.0.0 - IDENTICAL code
use turbomcp::auth::oauth2::{OAuth2Provider, OAuth2Config};

let config = OAuth2Config {
    client_id: "your-client-id".to_string(),
    client_secret: "your-secret".to_string(),
    auth_url: "https://provider.com/oauth/authorize".to_string(),
    token_url: "https://provider.com/oauth/token".to_string(),
    redirect_uri: "http://localhost:8080/callback".to_string(),
};

let provider = OAuth2Provider::new(config);
```

**No changes needed!**

### Pattern 2: API Key Authentication

```rust
// Code unchanged
use turbomcp::auth::providers::ApiKeyProvider;

let provider = ApiKeyProvider::new(vec![
    "api-key-1".to_string(),
    "api-key-2".to_string(),
]);
```

### Pattern 3: DPoP Integration

```toml
# v2.0.0 - Both features needed
turbomcp = { version = "2.0.0", features = ["auth", "dpop"] }
```

```rust
use turbomcp::auth::oauth2::OAuth2Provider;
use turbomcp::dpop::DPopProofBuilder;

// Use together for enhanced security
let provider = OAuth2Provider::new(config)
    .with_dpop_support(true);
```

### Pattern 4: Direct Crate Usage

```rust
// v2.0.0 - NEW option
use turbomcp_auth::{
    oauth2::OAuth2Provider,
    providers::{ApiKeyProvider, JwtProvider},
};

// All functionality available directly
let provider = OAuth2Provider::new(config);
```

## 🐛 Troubleshooting

### Issue: "crate `turbomcp::auth` not found"

**Solution:** Enable the `auth` feature:

```toml
[dependencies]
turbomcp = { version = "2.0.0", features = ["auth"] }
```

### Issue: Import errors with direct crate

**Solution:** Update import paths:

```rust
// Before
use turbomcp::auth::OAuth2Provider;

// After
use turbomcp_auth::oauth2::OAuth2Provider;
```

### Issue: DPoP features not available

**Solution:** Enable both `auth` and `dpop` features:

```toml
[dependencies]
turbomcp = { version = "2.0.0", features = ["auth", "dpop"] }
```

## 📚 Additional Resources

- **Main Migration Guide**: See `../../MIGRATION.md` for workspace-level changes
- **API Documentation**: https://docs.rs/turbomcp-auth
- **Examples**: See `../../examples/` for OAuth 2.1 usage examples
- **OAuth 2.1 Spec**: https://datatracker.ietf.org/doc/html/draft-ietf-oauth-v2-1

## 🎉 Benefits of 2.0.0

- ✅ **Cleaner Dependencies** - Only include auth when needed
- ✅ **Better Modularity** - Independent crate versioning
- ✅ **Same Functionality** - All features preserved
- ✅ **Flexible Usage** - Use via main crate OR directly
- ✅ **Future Proof** - Easier to maintain and extend

## 🤝 Getting Help

- **Issues**: https://github.com/Epistates/turbomcp/issues
- **Discussions**: https://github.com/Epistates/turbomcp/discussions
- **Documentation**: https://docs.rs/turbomcp-auth

## 📝 Version Compatibility

| turbomcp-auth | Status | Notes |
|---------------|--------|-------|
| 2.0.0           | ✅ Current | Standalone crate |
| 1.x (inline)  | ⚠️ Deprecated | Use 2.0 standalone |
