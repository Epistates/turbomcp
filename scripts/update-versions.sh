#!/bin/bash

# TurboMCP Version Update Script
# Updates all version numbers across the workspace
#
# Usage:
#   VERSION=2.0.0-rc.2 ./scripts/update-versions.sh

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
NEW_VERSION=${VERSION:-""}

# Crate list
CRATES=(
    "turbomcp-protocol"
    "turbomcp-dpop"
    "turbomcp-macros"
    "turbomcp-auth"
    "turbomcp-transport"
    "turbomcp-server"
    "turbomcp-client"
    "turbomcp-cli"
    "turbomcp"
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

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ] || [ ! -d "crates" ]; then
    print_error "Must be run from the turbomcp workspace root"
    exit 1
fi

echo -e "${BLUE}🔄 TurboMCP Version Update${NC}"
echo -e "${BLUE}==========================${NC}"
echo ""

# Get current version if new version not specified
if [ -z "$NEW_VERSION" ]; then
    CURRENT_VERSION=$(grep '^version = ' "crates/turbomcp-protocol/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
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

echo "New version: $NEW_VERSION"
echo ""

# Confirm before proceeding
read -p "Update all crates to version $NEW_VERSION? (yes/no): " -r
echo

if [[ ! $REPLY =~ ^[Yy][Ee][Ss]$ ]]; then
    print_warning "Update cancelled"
    exit 0
fi

# Step 1: Update crate Cargo.toml files
print_section "Step 1: Updating Crate Versions"

for crate in "${CRATES[@]}"; do
    cargo_toml="crates/$crate/Cargo.toml"

    if [ ! -f "$cargo_toml" ]; then
        print_error "Missing: $cargo_toml"
        continue
    fi

    # Update the version line
    sed -i '' "s/^version = \".*\"/version = \"$NEW_VERSION\"/" "$cargo_toml"

    # Update internal dependencies
    for dep_crate in "${CRATES[@]}"; do
        if [ "$crate" != "$dep_crate" ]; then
            sed -i '' "s/^$dep_crate = { version = \"[^\"]*\"/$dep_crate = { version = \"$NEW_VERSION\"/" "$cargo_toml" || true
        fi
    done

    print_status "Updated $crate"
done

echo ""

# Step 2: Update workspace Cargo.toml
print_section "Step 2: Updating Workspace Dependencies"

sed -i '' "s/turbomcp = { version = \"[^\"]*\"/turbomcp = { version = \"$NEW_VERSION\"/" Cargo.toml
sed -i '' "s/turbomcp-protocol = { version = \"[^\"]*\"/turbomcp-protocol = { version = \"$NEW_VERSION\"/" Cargo.toml
sed -i '' "s/turbomcp-transport = { version = \"[^\"]*\"/turbomcp-transport = { version = \"$NEW_VERSION\"/" Cargo.toml
sed -i '' "s/turbomcp-client = { version = \"[^\"]*\"/turbomcp-client = { version = \"$NEW_VERSION\"/" Cargo.toml
sed -i '' "s/turbomcp-server = { version = \"[^\"]*\"/turbomcp-server = { version = \"$NEW_VERSION\"/" Cargo.toml
sed -i '' "s/turbomcp-macros = { version = \"[^\"]*\"/turbomcp-macros = { version = \"$NEW_VERSION\"/" Cargo.toml
sed -i '' "s/turbomcp-cli = { version = \"[^\"]*\"/turbomcp-cli = { version = \"$NEW_VERSION\"/" Cargo.toml

print_status "Updated workspace Cargo.toml"
echo ""

# Step 3: Find and update test files with hardcoded versions
print_section "Step 3: Checking for Hardcoded Versions in Tests"

test_files_updated=0

for test_file in $(find crates/*/src -name "*.rs" -type f 2>/dev/null | grep -E "(test|config)\.rs$"); do
    # Check if file contains version strings
    if grep -q '"[0-9]\+\.[0-9]\+\.[0-9]\+\(-[a-z0-9.]\+\)\?"' "$test_file"; then
        # Update version strings in assertions and constants
        sed -i '' "s/\"[0-9]\+\.[0-9]\+\.[0-9]\+\(-[a-z0-9.]\+\)\?\"/\"$NEW_VERSION\"/g" "$test_file"
        print_warning "Updated hardcoded versions in $test_file"
        test_files_updated=$((test_files_updated + 1))
    fi
done

if [ $test_files_updated -eq 0 ]; then
    print_status "No hardcoded versions found in test files"
else
    print_warning "Updated $test_files_updated test file(s) - please review changes!"
fi

echo ""

# Step 4: Update Cargo.lock
print_section "Step 4: Updating Cargo.lock"

if cargo update --workspace --quiet; then
    print_status "Cargo.lock updated"
else
    print_warning "Failed to update Cargo.lock - you may need to run 'cargo update' manually"
fi

echo ""

# Final verification
print_section "Verification"

echo "Running version consistency check..."
if ./scripts/check-versions.sh; then
    print_status "Version update successful!"
else
    print_error "Version consistency check failed - manual review required"
    exit 1
fi

echo ""
print_status "🎉 All versions updated to $NEW_VERSION"
echo ""
echo "Next steps:"
echo "1. Review changes: git diff"
echo "2. Test build: cargo check --workspace"
echo "3. Run tests: cargo test --workspace --lib"
echo "4. Commit changes: git add -A && git commit -m 'chore: bump version to $NEW_VERSION'"
