#!/bin/bash

# TurboMCP Release Preparation Script
# Validates that the workspace is ready for release
#
# This script:
# - Verifies compilation and tests
# - Checks version consistency
# - Validates crate metadata
# - Generates documentation
# - Packages crates for verification
#
# Usage:
#   VERSION=2.0.0-rc.1 ./scripts/prepare-release.sh

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
VERSION=${VERSION:-""}

# Crate publish order (dependencies first)
CRATES=(
    "turbomcp-protocol"   # No internal deps
    "turbomcp-dpop"       # No internal deps
    "turbomcp-auth"       # Depends on protocol, dpop
    "turbomcp-transport"  # Depends on protocol, auth (optional but should be available)
    "turbomcp-macros"     # Depends on protocol, transport
    "turbomcp-server"     # Depends on protocol, macros, transport, auth
    "turbomcp-client"     # Depends on protocol, transport
    "turbomcp-cli"        # Depends on client, transport, protocol
    "turbomcp"            # Main SDK - depends on all
    "turbomcp-proxy"      # Consumer-only - depends on protocol, transport, client, server (optional), auth (optional)
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

echo -e "${BLUE}🚀 TurboMCP Release Preparation${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

# Pre-flight checks
print_section "Pre-flight Checks"

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ] || [ ! -d "crates" ]; then
    print_error "Must be run from the turbomcp workspace root"
    exit 1
fi

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    print_error "Cargo is not installed or not in PATH"
    exit 1
fi

print_status "Environment checks passed"
echo ""

# Auto-detect version if not set
if [ -z "$VERSION" ]; then
    VERSION=$(grep '^version = ' "crates/turbomcp-protocol/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
    print_warning "Auto-detected version: $VERSION"
fi

echo "Target version: $VERSION"
echo ""

# Run version consistency check
print_section "Version Consistency Check"
if ./scripts/check-versions.sh; then
    print_status "Version consistency check passed"
else
    print_error "Version consistency check failed"
    echo ""
    echo "Run this to fix versions:"
    echo "  VERSION=$VERSION ./scripts/update-versions.sh"
    exit 1
fi
echo ""

# Check for uncommitted changes
print_section "Git Status Check"
if [ -n "$(git status --porcelain)" ]; then
    print_warning "Uncommitted changes detected:"
    git status --short
    echo ""
    print_warning "Strongly recommend committing changes before release"
    echo ""
    read -p "Continue anyway? (yes/no): " -r
    echo
    if [[ ! $REPLY =~ ^[Yy][Ee][Ss]$ ]]; then
        print_error "Stopped by user"
        exit 1
    fi
else
    print_status "Working directory is clean"
fi
echo ""

# Clean workspace
print_section "Cleaning Workspace"
cargo clean
print_status "Workspace cleaned"
echo ""

# Check compilation
print_section "Compilation Check"
if cargo check --workspace --all-targets; then
    print_status "All crates compile successfully"
else
    print_error "Compilation failed"
    exit 1
fi
echo ""

# Run tests
print_section "Running Tests"
echo "Running library tests..."
if cargo test --workspace --lib --quiet; then
    print_status "All library tests pass"
else
    print_error "Tests failed"
    exit 1
fi
echo ""

# Run clippy
print_section "Linting with Clippy"
if cargo clippy --workspace --all-targets -- -D warnings; then
    print_status "Clippy checks passed"
else
    print_error "Clippy warnings found - must fix before publishing"
    exit 1
fi
echo ""

# Check formatting
print_section "Format Check"
if cargo fmt --all -- --check; then
    print_status "Code formatting is correct"
else
    print_error "Code formatting issues found. Run 'cargo fmt --all'"
    exit 1
fi
echo ""

# Generate documentation with full features (docs.rs uses nightly)
print_section "Documentation Generation"
echo "Building docs with full features (nightly + doc_cfg)..."
if cargo +nightly doc --lib --workspace --no-deps --features full --quiet 2>&1 | tee /tmp/rustdoc.log; then
    print_status "Documentation generated successfully"
else
    print_error "Documentation generation failed"
    echo ""
    echo "Last 30 lines of output:"
    tail -30 /tmp/rustdoc.log
    exit 1
fi

# Check for rustdoc warnings
print_section "Documentation Quality Check"
if grep -i "error\[E" /tmp/rustdoc.log; then
    print_error "Documentation has compilation errors - must fix before publishing"
    exit 1
fi

# Warn about rustdoc warnings (but don't fail)
if grep -i "warning:" /tmp/rustdoc.log; then
    print_warning "Documentation generation completed with warnings:"
    grep -i "warning:" /tmp/rustdoc.log | head -5
    echo ""
    print_warning "Review warnings above and fix if possible"
    echo ""
else
    print_status "Documentation has no warnings"
fi
echo ""

# Check crate metadata
print_section "Crate Metadata Check"

metadata_issues=0

for crate in "${CRATES[@]}"; do
    crate_dir="crates/$crate"
    cargo_toml="$crate_dir/Cargo.toml"

    if [ ! -f "$cargo_toml" ]; then
        print_error "$crate: Cargo.toml not found"
        metadata_issues=$((metadata_issues + 1))
        continue
    fi

    # Check for required metadata fields
    required_fields=("description" "license" "repository" "homepage" "keywords" "categories")
    missing_fields=()

    for field in "${required_fields[@]}"; do
        if ! grep -q "^$field = " "$cargo_toml" && ! grep -q "^$field = \[" "$cargo_toml"; then
            missing_fields+=("$field")
        fi
    done

    if [ ${#missing_fields[@]} -ne 0 ]; then
        print_error "$crate: Missing fields: ${missing_fields[*]}"
        metadata_issues=$((metadata_issues + 1))
    fi

    # Check if README exists
    if [ ! -f "$crate_dir/README.md" ]; then
        print_warning "$crate: Missing README.md (optional but recommended)"
    fi
done

if [ $metadata_issues -eq 0 ]; then
    print_status "All crates have required metadata"
else
    print_error "Found $metadata_issues metadata issues"
    exit 1
fi
echo ""

# Package verification
print_section "Package Verification"

packaging_issues=0

for crate in "${CRATES[@]}"; do
    echo "Packaging $crate..."

    # Package without verifying (verification requires dependencies to be published)
    if cargo package --manifest-path "crates/$crate/Cargo.toml" --no-verify --quiet 2>&1 | grep -v "warning:"; then
        # Get package size
        pkg_size=$(ls -lh "target/package/$crate-$VERSION.crate" 2>/dev/null | awk '{print $5}' || echo "unknown")
        echo "  ✓ $crate packaged successfully ($pkg_size)"
    else
        print_error "  ✗ Failed to package $crate"
        packaging_issues=$((packaging_issues + 1))
    fi
done

if [ $packaging_issues -eq 0 ]; then
    print_status "All crates packaged successfully"
else
    print_error "Found $packaging_issues packaging issues"
    exit 1
fi
echo ""

# Final summary
print_section "Release Readiness Summary"
echo "Version: $VERSION"
echo "Crates: ${#CRATES[@]}"
echo "✅ Compilation: passed"
echo "✅ Tests: passed"
echo "✅ Clippy: passed"
echo "✅ Formatting: passed"
echo "✅ Documentation: generated & validated"
echo "✅ Doc quality: checked"
echo "✅ Metadata: passed"
echo "✅ Packaging: passed"
echo ""

print_status "🎉 All checks passed! Ready for release."
echo ""
echo -e "${GREEN}Next steps:${NC}"
echo "1. Review any warnings above"
echo "2. Commit any final changes: git add -A && git commit -m 'chore: prepare release $VERSION'"
echo "3. Create git tag: git tag v$VERSION"
echo "4. Publish to crates.io: DRY_RUN=false ./scripts/publish.sh"
echo "5. Push changes and tag: git push && git push origin v$VERSION"
echo "6. Create GitHub release"
echo ""
echo "Or run publish in dry-run mode first:"
echo "  ./scripts/publish.sh"
