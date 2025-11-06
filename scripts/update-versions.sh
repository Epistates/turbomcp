#!/bin/bash

# TurboMCP Version Update Script (Improved)
# Updates all version numbers across the workspace
#
# Improvements:
# - Cross-platform sed compatibility (macOS + Linux)
# - Removed test file scanning (tests use env!("CARGO_PKG_VERSION"))
# - Better error handling and validation
# - Crate order aligned with dependencies
#
# Usage:
#   VERSION=2.2.1 ./scripts/update-versions-improved.sh

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
NEW_VERSION=${VERSION:-""}

# Crate list (in dependency order for consistency)
CRATES=(
    "turbomcp-protocol"   # No internal deps
    "turbomcp-dpop"       # No internal deps
    "turbomcp-auth"       # Depends on protocol, dpop
    "turbomcp-transport"  # Depends on protocol, auth
    "turbomcp-macros"     # Depends on protocol, transport
    "turbomcp-server"     # Depends on protocol, macros, transport, auth
    "turbomcp-client"     # Depends on protocol, transport
    "turbomcp-cli"        # Depends on client, transport, protocol
    "turbomcp"            # Main SDK - depends on all
)

print_section() {
    echo -e "${BLUE}📋 $1${NC}"
    echo "----------------------------------------"
}

print_status() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

# Cross-platform sed in-place editing
# Usage: sed_inplace "s/pattern/replacement/" file
sed_inplace() {
    local pattern="$1"
    local file="$2"

    if [[ "$OSTYPE" == "darwin"* ]]; then
        # macOS requires empty string after -i
        sed -i '' "$pattern" "$file"
    else
        # Linux/Unix standard syntax
        sed -i "$pattern" "$file"
    fi
}

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ] || [ ! -d "crates" ]; then
    print_error "Must be run from the turbomcp workspace root"
    exit 1
fi

echo -e "${BLUE}🔄 TurboMCP Version Update (Improved)${NC}"
echo -e "${BLUE}=====================================${NC}"
echo ""

# Get current version BEFORE any modifications
CURRENT_VERSION=$(grep '^version = ' "crates/turbomcp-protocol/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')

if [ -z "$CURRENT_VERSION" ]; then
    print_error "Could not detect current version from turbomcp-protocol/Cargo.toml"
    exit 1
fi

echo "Current version: $CURRENT_VERSION"

# Get new version if not specified
if [ -z "$NEW_VERSION" ]; then
    print_error "No version specified. Current version is: $CURRENT_VERSION"
    echo ""
    echo "Usage: VERSION=2.0.0-rc.2 $0"
    exit 1
fi

# Validate version format (basic check)
if ! echo "$NEW_VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[a-z0-9.]+)?$'; then
    print_error "Invalid version format: $NEW_VERSION"
    echo "Expected format: X.Y.Z or X.Y.Z-prerelease"
    exit 1
fi

# Check if version is changing
if [ "$CURRENT_VERSION" = "$NEW_VERSION" ]; then
    print_warning "New version ($NEW_VERSION) is same as current version ($CURRENT_VERSION)"
    read -p "Continue anyway? (yes/no): " -r
    echo
    if [[ ! $REPLY =~ ^[Yy][Ee][Ss]$ ]]; then
        print_warning "Update cancelled"
        exit 0
    fi
fi

echo "New version: $NEW_VERSION"
echo ""

# Confirm before proceeding
read -p "Update all crates from $CURRENT_VERSION to $NEW_VERSION? (yes/no): " -r
echo

if [[ ! $REPLY =~ ^[Yy][Ee][Ss]$ ]]; then
    print_warning "Update cancelled"
    exit 0
fi

# Step 1: Update crate Cargo.toml files
print_section "Step 1: Updating Crate Versions"

crates_updated=0
crates_failed=0

for crate in "${CRATES[@]}"; do
    cargo_toml="crates/$crate/Cargo.toml"

    if [ ! -f "$cargo_toml" ]; then
        print_error "Missing: $cargo_toml"
        crates_failed=$((crates_failed + 1))
        continue
    fi

    # Update the version line
    sed_inplace "s/^version = \".*\"/version = \"$NEW_VERSION\"/" "$cargo_toml"

    # Update internal dependencies (handles both "{ version = ..." and "{ path = ..., version = ...")
    for dep_crate in "${CRATES[@]}"; do
        if [ "$crate" != "$dep_crate" ]; then
            # Pattern 1: { version = "..." } format
            sed_inplace "s/^$dep_crate = { version = \"[^\"]*\"/$dep_crate = { version = \"$NEW_VERSION\"/" "$cargo_toml" || true
            # Pattern 2: { path = "...", version = "..." } format
            sed_inplace "s/\(^$dep_crate = { .*\)version = \"[^\"]*\"/\1version = \"$NEW_VERSION\"/" "$cargo_toml" || true
        fi
    done

    print_status "Updated $crate"
    crates_updated=$((crates_updated + 1))
done

echo ""
echo "Updated $crates_updated crates"

if [ $crates_failed -gt 0 ]; then
    print_error "Failed to update $crates_failed crates"
    exit 1
fi

echo ""

# Step 2: Update workspace Cargo.toml
print_section "Step 2: Updating Workspace Dependencies"

workspace_toml="Cargo.toml"

# Update all internal crate references in workspace dependencies
for crate in "${CRATES[@]}"; do
    sed_inplace "s/^$crate = { version = \"[^\"]*\"/$crate = { version = \"$NEW_VERSION\"/" "$workspace_toml"
done

print_status "Updated workspace Cargo.toml"
echo ""

# Step 3: Verify changes
print_section "Step 3: Verifying Changes"

verification_failed=0

for crate in "${CRATES[@]}"; do
    cargo_toml="crates/$crate/Cargo.toml"
    actual_version=$(grep '^version = ' "$cargo_toml" | head -1 | sed 's/version = "\(.*\)"/\1/')

    if [ "$actual_version" != "$NEW_VERSION" ]; then
        print_error "$crate: Expected version $NEW_VERSION, got $actual_version"
        verification_failed=$((verification_failed + 1))
    fi
done

if [ $verification_failed -gt 0 ]; then
    print_error "Verification failed for $verification_failed crates"
    exit 1
fi

print_status "All crate versions verified"
echo ""

# Step 4: Update Cargo.lock
print_section "Step 4: Updating Cargo.lock"

if cargo update --workspace --quiet; then
    print_status "Cargo.lock updated"
else
    print_error "Failed to update Cargo.lock"
    echo "You may need to run 'cargo update --workspace' manually"
    exit 1
fi

echo ""

# Step 5: Run version consistency check
print_section "Step 5: Running Version Consistency Check"

if [ -f "./scripts/check-versions.sh" ]; then
    if VERSION="$NEW_VERSION" ./scripts/check-versions.sh; then
        print_status "Version consistency check passed"
    else
        print_error "Version consistency check failed"
        echo ""
        echo "Please review the errors above and fix manually"
        exit 1
    fi
else
    print_warning "check-versions.sh not found - skipping consistency check"
fi

echo ""

# Step 6: Quick compilation check
print_section "Step 6: Quick Compilation Check"

echo "Running 'cargo check' to verify changes..."
if cargo check --workspace --quiet 2>&1 | head -20; then
    print_status "Workspace compiles successfully"
else
    print_error "Compilation failed after version update"
    echo ""
    echo "Please review errors and fix manually"
    exit 1
fi

echo ""

# Final summary
print_section "✅ Version Update Complete"
echo "Updated: $CURRENT_VERSION → $NEW_VERSION"
echo "Crates: ${#CRATES[@]} crates updated"
echo ""
print_status "🎉 All versions updated to $NEW_VERSION"
echo ""
echo -e "${BLUE}Next steps:${NC}"
echo "1. Review changes: git diff"
echo "2. Run tests: cargo test --workspace --lib"
echo "3. Run full checks: ./scripts/prepare-release.sh"
echo "4. Commit changes: git add -A && git commit -m 'chore: bump version to $NEW_VERSION'"
echo ""
echo -e "${YELLOW}Note:${NC} Tests use env!(\"CARGO_PKG_VERSION\") - no manual test updates needed"
