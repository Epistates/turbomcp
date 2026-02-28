> **Note:** This is the v1.x to v2.0.0 migration guide. For v2.x to v3.x migration, see the [top-level MIGRATION.md](../../MIGRATION.md).

# TurboMCP DPoP 2.0.0 Migration Guide

This guide helps you migrate from turbomcp (with inline DPoP) to the standalone turbomcp-dpop crate in 2.0.0.

## 📋 Table of Contents

- [Overview](#overview)
- [Migration Paths](#migration-paths)
- [Breaking Changes](#breaking-changes)
- [New Features](#new-features)
- [Migration Steps](#migration-steps)

## 🚀 Overview

**TurboMCP DPoP 2.0.0** is a **NEW standalone crate** extracted from the main turbomcp crate.

### Key Changes
- **Extracted Crate**: DPoP functionality moved to dedicated `turbomcp-dpop` crate
- **Cleaner Dependencies**: Optional security features don't bloat minimal builds
- **Same API**: Functionality unchanged, only import paths different
- **Progressive Enhancement**: Only include DPoP when you need it

### Who Needs This Guide?
- ✅ Users of DPoP (RFC 9449) features in turbomcp 1.x
- ✅ Users migrating from inline DPoP to standalone crate
- ❌ New users (just use `turbomcp-dpop` directly)
- ❌ Users not using DPoP features (no action needed)

## 💥 Breaking Changes

### DPoP Functionality Extracted

**What Changed:**
```toml
# v1.x - DPoP integrated in main crate
[dependencies]
turbomcp = { version = "1.x", features = ["dpop"] }

# v2.0.0 - DPoP in separate crate
[dependencies]
turbomcp = { version = "2.0.0", features = ["dpop"] }
# OR use directly:
turbomcp-dpop = "2.0.0"
```

**Why:**
- Cleaner separation of concerns
- Minimal builds don't include cryptographic dependencies
- Better modularity and independent versioning
- Easier to maintain and test

**Impact:** Import paths changed, functionality unchanged

## 🔄 Migration Paths

### Path 1: Via Main Crate (Recommended)

**Best for:** Existing turbomcp users who want minimal changes

```toml
# Enable DPoP feature in main crate
[dependencies]
turbomcp = { version = "2.0.0", features = ["dpop"] }
```

```rust
// Imports work via re-exports
use turbomcp::dpop::*;
```

**Migration:** Just update your `Cargo.toml` - imports still work!

### Path 2: Direct Crate Usage

**Best for:** New projects or those wanting explicit dependencies

```toml
[dependencies]
turbomcp-dpop = "2.0.0"
```

```rust
// Direct imports
use turbomcp_dpop::{
    DPopProofBuilder,
    storage::RedisNonceStore,
    hsm::YubiHsmKeyStore,
};
```

**Migration:** Update imports from `turbomcp::dpop::*` to `turbomcp_dpop::*`

## 📦 Feature Flags

### Main Crate Features

```toml
# All DPoP features available via main crate
turbomcp = { version = "2.0.0", features = [
    "dpop",              # Basic DPoP support
    "redis-storage",     # Redis nonce storage (renamed from dpop-redis)
    "dpop-hsm-pkcs11",   # PKCS#11 HSM support
    "dpop-hsm-yubico",   # YubiHSM support
    "dpop-hsm",          # All HSM backends
]}
```

### Direct Crate Features

```toml
turbomcp-dpop = { version = "2.0.0", features = [
    "redis-storage",  # Redis nonce tracking
    "hsm-pkcs11",     # PKCS#11 HSM
    "hsm-yubico",     # YubiHSM
    "hsm",            # All HSM backends
    "test-utils",     # Testing utilities
]}
```

### Feature Name Changes

| v1.x Feature | v2.0.0 Feature | Notes |
|--------------|----------------|-------|
| `dpop-redis` | `redis-storage` | Renamed for clarity |
| `dpop-test-utils` | `test-utils` | Simplified name |
| (others) | Unchanged | No changes |

**Backward Compatibility:** Old names still work but are deprecated

## ✨ New Features in 2.0.0

### 1. Standalone Crate

- Independent versioning
- Cleaner dependency graph
- Better documentation
- Focused testing

### 2. Enhanced HSM Support

- Improved YubiHSM integration
- Better PKCS#11 support
- Enhanced key rotation
- Production-ready configuration

### 3. Improved Storage Backends

- Optimized Redis integration
- Better memory storage
- Enhanced nonce tracking
- Configurable TTLs

## 🔧 Migration Steps

### Step 1: Update Dependencies

**Option A - Via Main Crate:**
```toml
# Before
[dependencies]
turbomcp = { version = "1.1.2", features = ["dpop", "dpop-redis"] }

# After
[dependencies]
turbomcp = { version = "2.0.0", features = ["dpop", "redis-storage"] }
```

**Option B - Direct Crate:**
```toml
# Before
[dependencies]
turbomcp = { version = "1.1.2", features = ["dpop"] }

# After
[dependencies]
turbomcp-dpop = "2.0.0"
```

### Step 2: Update Imports

**Option A - Via Main Crate (No Changes):**
```rust
// Still works!
use turbomcp::dpop::*;
```

**Option B - Direct Crate:**
```rust
// Before (v1.x)
use turbomcp::dpop::DPopProofBuilder;
use turbomcp::dpop::storage::RedisNonceStore;

// After (v2.0.0)
use turbomcp_dpop::DPopProofBuilder;
use turbomcp_dpop::storage::RedisNonceStore;
```

### Step 3: Update Feature Flags

```toml
# Update renamed features
# Before: dpop-redis
# After: redis-storage

[dependencies]
turbomcp = { version = "2.0.0", features = ["dpop", "redis-storage"] }
```

### Step 4: Build and Test

```bash
# Clean build
cargo clean
cargo build --all-features

# Run tests
cargo test --features dpop
```

## 🎯 Common Migration Patterns

### Pattern 1: Basic DPoP Usage

```rust
// v1.x and v2.0.0 - IDENTICAL code
use turbomcp::dpop::DPopProofBuilder;

let proof = DPopProofBuilder::new(
    method,
    url,
    access_token,
)
.with_nonce(nonce)
.build()?;
```

**No changes needed!**

### Pattern 2: Redis Storage

```toml
# v1.x
turbomcp = { version = "1.x", features = ["dpop", "dpop-redis"] }

# v2.0.0
turbomcp = { version = "2.0.0", features = ["dpop", "redis-storage"] }
```

```rust
// Code unchanged
use turbomcp::dpop::storage::RedisNonceStore;

let store = RedisNonceStore::new("redis://localhost")?;
```

### Pattern 3: HSM Integration

```toml
# v1.x
turbomcp = { version = "1.x", features = ["dpop", "dpop-hsm-yubico"] }

# v2.0.0
turbomcp = { version = "2.0.0", features = ["dpop", "dpop-hsm-yubico"] }
```

```rust
// Code unchanged
use turbomcp::dpop::hsm::YubiHsmKeyStore;

let key_store = YubiHsmKeyStore::new(config)?;
```

### Pattern 4: Direct Crate Usage

```rust
// v2.0.0 - NEW option
use turbomcp_dpop::{
    DPopProofBuilder,
    storage::RedisNonceStore,
    hsm::YubiHsmKeyStore,
};

// All functionality available directly
let proof = DPopProofBuilder::new(method, url, token)
    .build()?;
```

## 🐛 Troubleshooting

### Issue: "crate `turbomcp::dpop` not found"

**Solution:** Enable the `dpop` feature:

```toml
[dependencies]
turbomcp = { version = "2.0.0", features = ["dpop"] }
```

### Issue: "feature `dpop-redis` not found"

**Solution:** Feature was renamed to `redis-storage`:

```toml
# Before
features = ["dpop", "dpop-redis"]

# After
features = ["dpop", "redis-storage"]
```

### Issue: Import errors with direct crate

**Solution:** Update import paths:

```rust
// Before
use turbomcp::dpop::DPopProofBuilder;

// After
use turbomcp_dpop::DPopProofBuilder;
```

## 📚 Additional Resources

- **Main Migration Guide**: See `../../MIGRATION.md` for workspace-level changes
- **API Documentation**: https://docs.rs/turbomcp-dpop
- **Examples**: See `../../examples/` for DPoP usage examples
- **RFC 9449 Spec**: https://datatracker.ietf.org/doc/html/rfc9449

## 🎉 Benefits of 2.0.0

- ✅ **Cleaner Dependencies** - Only include DPoP when needed
- ✅ **Better Modularity** - Independent crate versioning
- ✅ **Same Functionality** - All features preserved
- ✅ **Flexible Usage** - Use via main crate OR directly
- ✅ **Future Proof** - Easier to maintain and extend

## 🤝 Getting Help

- **Issues**: https://github.com/Epistates/turbomcp/issues
- **Discussions**: https://github.com/Epistates/turbomcp/discussions
- **Documentation**: https://docs.rs/turbomcp-dpop

## 📝 Version Compatibility

| turbomcp-dpop | Status | Notes |
|---------------|--------|-------|
| 2.0.0           | ✅ Current | Standalone crate |
| 1.x (inline)  | ⚠️ Deprecated | Use 2.0 standalone |
