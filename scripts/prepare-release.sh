#!/bin/bash

# TurboMCP Release Preparation Script
# This script prepares all crates for publishing to crates.io

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
DRY_RUN=${DRY_RUN:-true}
VERSION=${VERSION:-"1.1.0-exp.5"}

# Crate publish order (dependencies first)
CRATES=(
    "turbomcp-core"
    "turbomcp-protocol"
    "turbomcp-transport" 
    "turbomcp-macros"
    "turbomcp-dpop"
    "turbomcp-server"
    "turbomcp-client"
    "turbomcp-cli"
    "turbomcp"
)

echo -e "${BLUE}🚀 TurboMCP Release Preparation${NC}"
echo -e "${BLUE}================================${NC}"
echo ""

if [ "$DRY_RUN" = "true" ]; then
    echo -e "${YELLOW}⚠️  DRY RUN MODE - No actual publishing will occur${NC}"
    echo -e "${YELLOW}   Set DRY_RUN=false to perform actual release${NC}"
    echo ""
fi

# Function to print section headers
print_section() {
    echo -e "${BLUE}📋 $1${NC}"
    echo "----------------------------------------"
}

# Function to print status
print_status() {
    echo -e "${GREEN}✅ $1${NC}"
}

# Function to print warnings
print_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

# Function to print errors
print_error() {
    echo -e "${RED}❌ $1${NC}"
}

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

# Check if we're logged into crates.io
if [ "$DRY_RUN" = "false" ]; then
    if [ ! -f ~/.cargo/credentials.toml ]; then
        print_error "Not logged into crates.io. Run 'cargo login' first"
        exit 1
    fi
fi

print_status "Environment checks passed"

# Clean workspace
print_section "Cleaning Workspace"
cargo clean
print_status "Workspace cleaned"

# Check compilation
print_section "Compilation Check"
if cargo check --workspace --all-targets; then
    print_status "All crates compile successfully"
else
    print_error "Compilation failed"
    exit 1
fi

# Run tests
print_section "Running Tests"
if cargo test --workspace --lib; then
    print_status "All tests pass"
else
    print_error "Tests failed"
    exit 1
fi

# Run clippy
print_section "Linting with Clippy"
if cargo clippy --workspace --all-targets -- -D warnings; then
    print_status "Clippy checks passed"
else
    print_warning "Clippy warnings found - review before publishing"
fi

# Check formatting
print_section "Format Check"
if cargo fmt --all -- --check; then
    print_status "Code formatting is correct"
else
    print_error "Code formatting issues found. Run 'cargo fmt --all'"
    exit 1
fi

# Version consistency check
print_section "Version Consistency Check"
version_issues=0

for crate in "${CRATES[@]}"; do
    crate_version=$(grep '^version = ' "crates/$crate/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
    # CLI is at 1.0.5, which is fine for 1.0.4 release
    if [ "$crate" = "turbomcp-cli" ] && [ "$crate_version" = "1.0.5" ] && [ "$VERSION" = "1.0.4" ]; then
        print_status "$crate has version $crate_version (ahead, OK)"
    elif [ "$crate_version" != "$VERSION" ]; then
        print_error "$crate has version $crate_version, expected $VERSION"
        version_issues=$((version_issues + 1))
    fi
done

if [ $version_issues -eq 0 ]; then
    print_status "All crates have consistent version $VERSION"
else
    print_error "Version inconsistencies found"
    exit 1
fi

# Check for uncommitted changes
print_section "Git Status Check"
if [ -n "$(git status --porcelain)" ]; then
    print_warning "Uncommitted changes detected:"
    git status --short
    echo ""
    print_warning "Consider committing changes before release"
else
    print_status "Working directory is clean"
fi

# Generate documentation
print_section "Documentation Generation"
if cargo doc --workspace --no-deps; then
    print_status "Documentation generated successfully"
else
    print_warning "Documentation generation had issues"
fi

# Check crate readiness
print_section "Crate Readiness Check"

for crate in "${CRATES[@]}"; do
    echo "Checking $crate..."
    
    # Check required fields in Cargo.toml
    crate_dir="crates/$crate"
    cargo_toml="$crate_dir/Cargo.toml"
    
    if [ ! -f "$cargo_toml" ]; then
        print_error "$cargo_toml not found"
        exit 1
    fi
    
    # Check for required metadata
    required_fields=("description" "license" "repository" "homepage")
    missing_fields=()
    
    for field in "${required_fields[@]}"; do
        if ! grep -q "^$field = " "$cargo_toml"; then
            missing_fields+=("$field")
        fi
    done
    
    if [ ${#missing_fields[@]} -ne 0 ]; then
        print_error "$crate missing required fields: ${missing_fields[*]}"
        exit 1
    fi
    
    # Check if README exists
    if [ ! -f "$crate_dir/README.md" ]; then
        print_warning "$crate missing README.md"
    fi
    
    print_status "$crate is ready for publishing"
done

# Dry run package for each crate
print_section "Package Verification"

if [ "$DRY_RUN" = "true" ]; then
    for crate in "${CRATES[@]}"; do
        echo "Packaging $crate..."
        if cargo package --manifest-path "crates/$crate/Cargo.toml" --no-verify; then
            print_status "$crate packaged successfully"
        else
            print_error "Failed to package $crate"
            exit 1
        fi
    done
else
    print_status "Skipping package verification for production release (cargo publish will handle packaging)"
fi

# Publishing section
print_section "Publishing Crates"

if [ "$DRY_RUN" = "true" ]; then
    echo "DRY RUN: Would publish crates in the following order:"
    for i in "${!CRATES[@]}"; do
        echo "$((i+1)). ${CRATES[$i]}"
    done
    echo ""
    echo "To perform actual publishing, run:"
    echo "DRY_RUN=false $0"
else
    echo "Publishing crates to crates.io..."
    
    for i in "${!CRATES[@]}"; do
        crate="${CRATES[$i]}"
        echo ""
        echo "Publishing $((i+1))/${#CRATES[@]}: $crate"
        
        # Publish with timeout
        if timeout 300 cargo publish --manifest-path "crates/$crate/Cargo.toml"; then
            print_status "$crate published successfully"
        else
            print_error "Failed to publish $crate"
            exit 1
        fi
        
        # Wait between publishes to allow crates.io to process
        if [ $i -lt $((${#CRATES[@]} - 1)) ]; then
            echo "Waiting 30 seconds for crates.io to process..."
            sleep 30
        fi
    done
    
    print_status "All crates published successfully!"
fi

# Summary
print_section "Release Summary"
echo "Version: $VERSION"
echo "Crates: ${#CRATES[@]}"
echo "Publish order: ${CRATES[*]}"

if [ "$DRY_RUN" = "true" ]; then
    echo ""
    echo -e "${YELLOW}Next steps:${NC}"
    echo "1. Review any warnings above"
    echo "2. Commit any final changes"
    echo "3. Create a git tag: git tag v$VERSION"
    echo "4. Run: DRY_RUN=false $0"
    echo "5. Push tag: git push origin v$VERSION"
else
    echo ""
    echo -e "${GREEN}🎉 Release completed successfully!${NC}"
    echo ""
    echo "Next steps:"
    echo "1. Create and push git tag: git tag v$VERSION && git push origin v$VERSION"
    echo "2. Create GitHub release with changelog"
    echo "3. Update social media and blog posts"
    echo "4. Monitor crates.io for successful indexing"
fi