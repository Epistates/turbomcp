# TurboMCP Release Scripts

This directory contains scripts for managing TurboMCP releases to crates.io.

## Overview

The release process is split into separate, focused scripts:

1. **check-versions.sh** - Validates version consistency
2. **update-versions.sh** - Updates all version numbers
3. **prepare-release.sh** - Validates release readiness
4. **publish.sh** - Publishes to crates.io

## Quick Start

For a new release:

```bash
# 1. Update version numbers
VERSION=2.0.0-rc.2 ./scripts/update-versions.sh

# 2. Verify and prepare
VERSION=2.0.0-rc.2 ./scripts/prepare-release.sh

# 3. Test publish (dry-run)
./scripts/publish.sh

# 4. Actual publish
DRY_RUN=false ./scripts/publish.sh

# 5. Tag and push
git tag v2.0.0-rc.2
git push && git push origin v2.0.0-rc.2
```

## Scripts

### check-versions.sh

**Purpose:** Validates version consistency across the workspace.

**Usage:**
```bash
# Check current version consistency
./scripts/check-versions.sh

# Check against specific version
VERSION=2.0.0-rc.2 ./scripts/check-versions.sh
```

**What it checks:**
- ✅ All crate Cargo.toml versions match
- ✅ Workspace Cargo.toml dependencies match
- ✅ Internal dependencies are consistent
- ⚠️  Hardcoded versions in test files
- ℹ️  Git tag comparison

**Exit codes:**
- `0` - All checks passed
- `1` - Version inconsistencies found

---

### update-versions.sh

**Purpose:** Updates all version numbers across the workspace.

**Usage:**
```bash
VERSION=2.0.0-rc.2 ./scripts/update-versions.sh
```

**What it updates:**
- ✅ All crate `Cargo.toml` files
- ✅ Workspace `Cargo.toml` dependencies
- ✅ Internal dependency references
- ⚠️  Hardcoded versions in test files (with warning)
- ✅ Cargo.lock

**Safety:**
- Requires manual confirmation
- Validates version format
- Runs consistency check after update

---

### prepare-release.sh

**Purpose:** Validates that the workspace is ready for release.

**Usage:**
```bash
# Auto-detect version from workspace
./scripts/prepare-release.sh

# Specify version explicitly
VERSION=2.0.0-rc.2 ./scripts/prepare-release.sh
```

**What it validates:**
- ✅ Version consistency (calls `check-versions.sh`)
- ✅ Clean git status (with override option)
- ✅ Compilation (`cargo check`)
- ✅ Tests (`cargo test`)
- ✅ Linting (`cargo clippy`)
- ✅ Formatting (`cargo fmt`)
- ✅ Documentation generation
- ✅ Crate metadata (description, license, etc.)
- ✅ Package creation

**Exit codes:**
- `0` - Ready for release
- `1` - Failed validation

**Notes:**
- Does NOT publish (use `publish.sh` for that)
- macOS compatible (no `timeout` dependency)
- Shows package sizes

---

### publish.sh

**Purpose:** Publishes all crates to crates.io in dependency order.

**Usage:**
```bash
# Dry-run mode (default - safe)
./scripts/publish.sh

# Actual publish (requires confirmation)
DRY_RUN=false ./scripts/publish.sh

# Custom wait time between publishes
DRY_RUN=false WAIT_TIME=60 ./scripts/publish.sh
```

**Publish order:**
1. `turbomcp-protocol` (no deps)
2. `turbomcp-dpop` (no deps)
3. `turbomcp-macros` (→ protocol)
4. `turbomcp-auth` (→ protocol, dpop)
5. `turbomcp-transport` (→ protocol)
6. `turbomcp-server` (→ protocol, macros, transport)
7. `turbomcp-client` (→ protocol, transport)
8. `turbomcp-cli` (→ client, transport, protocol)
9. `turbomcp` (→ all above)

**Features:**
- ✅ Dry-run mode by default
- ✅ Interactive confirmation
- ✅ Waits between publishes for indexing (30s default)
- ✅ Logs all output to `/tmp/turbomcp-publish-*.log`
- ✅ Continue-on-error prompt
- ✅ Final summary with failed crates

**Requirements:**
- Must be logged into crates.io: `cargo login`
- All crates must pass `prepare-release.sh` first

---

## Release Workflow

### Standard Release Process

```bash
# Step 1: Create feature branch
git checkout -b release/2.0.0-rc.2

# Step 2: Update versions
VERSION=2.0.0-rc.2 ./scripts/update-versions.sh

# Step 3: Review changes
git diff

# Step 4: Prepare release (validation)
VERSION=2.0.0-rc.2 ./scripts/prepare-release.sh

# Step 5: Commit version bump
git add -A
git commit -m "chore: bump version to 2.0.0-rc.2"

# Step 6: Test publish (dry-run)
./scripts/publish.sh

# Step 7: Actual publish
DRY_RUN=false ./scripts/publish.sh

# Step 8: Tag release
git tag v2.0.0-rc.2

# Step 9: Push everything
git push origin release/2.0.0-rc.2
git push origin v2.0.0-rc.2

# Step 10: Create GitHub release
# Go to GitHub and create release from tag
```

### Emergency Patch Process

If you need to quickly publish a single crate:

```bash
# Verify crate is ready
cargo package --manifest-path crates/turbomcp-server/Cargo.toml

# Publish
cargo publish --manifest-path crates/turbomcp-server/Cargo.toml
```

---

## Troubleshooting

### Version mismatch errors

```bash
# Check what's wrong
./scripts/check-versions.sh

# Auto-fix versions
VERSION=2.0.0-rc.2 ./scripts/update-versions.sh
```

### Test failures with hardcoded versions

```bash
# Find hardcoded versions
grep -r '"[0-9]\+\.[0-9]\+\.[0-9]\+"' crates/*/src/**/*test*.rs

# Update manually or re-run update-versions.sh
VERSION=2.0.0-rc.2 ./scripts/update-versions.sh
```

### Publish failures

```bash
# Check logs
ls -lh /tmp/turbomcp-publish-*.log
tail -50 /tmp/turbomcp-publish-<crate>.log

# Common issues:
# 1. Not logged in: cargo login
# 2. Version already published: bump version
# 3. Dependency not indexed yet: wait 30s and retry
# 4. Size limit: check package size, exclude large files
```

### crates.io indexing delays

If a publish fails because a dependency isn't indexed yet:

```bash
# Wait and check
sleep 60
curl -s https://crates.io/api/v1/crates/turbomcp-protocol | jq '.versions[0].num'

# Retry failed crate
cargo publish --manifest-path crates/turbomcp-macros/Cargo.toml
```

---

## Script Design Principles

These scripts follow these principles (learned from experience):

1. **Separation of concerns** - Each script does one thing well
2. **macOS compatible** - No GNU-specific tools (`timeout`, etc.)
3. **Safe by default** - Dry-run mode, confirmations
4. **Clear feedback** - Colored output, progress indicators
5. **Fail fast** - Exit on errors with clear messages
6. **Version-aware** - Auto-detection, validation
7. **Idempotent** - Safe to re-run
8. **Logged** - All publish output saved to /tmp

---

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `VERSION` | auto-detect | Target version for release |
| `DRY_RUN` | `true` | Set to `false` for actual publish |
| `WAIT_TIME` | `30` | Seconds between publishes |

---

## Contributing

When modifying these scripts:

1. Test in dry-run mode first
2. Maintain macOS compatibility
3. Keep colored output for clarity
4. Update this README
5. Verify all exit codes are correct
6. Test error handling paths

---

## Related Documentation

- [Cargo Publishing](https://doc.rust-lang.org/cargo/reference/publishing.html)
- [Semantic Versioning](https://semver.org/)
- [crates.io Policies](https://crates.io/policies)
