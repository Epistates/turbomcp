#!/bin/bash

# Simple TurboMCP Release Check Script
# Quick validation without benchmarks and examples
# World-class implementation with proper tool detection and error handling

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

DRY_RUN=${DRY_RUN:-true}
VERSION="1.1.0-exp.2"

# ============================================================================
# Utility Functions
# ============================================================================

# Function to print status
print_status() {
    echo -e "${GREEN}✅ $1${NC}"
}

print_error() {
    echo -e "${RED}❌ $1${NC}"
}

# ============================================================================
# Tool Detection and Environment Setup
# ============================================================================

# Detect available search tools with fallback priority
detect_search_tool() {
    # Try ripgrep (preferred)
    if command -v rg >/dev/null 2>&1 && rg --version >/dev/null 2>&1; then
        echo "rg"
        return 0
    fi
    
    # Fallback to grep with extended regex
    if command -v grep >/dev/null 2>&1; then
        echo "grep"
        return 0
    fi
    
    print_error "No search tool available (ripgrep or grep required)"
    exit 1
}

# Set search tool globally
SEARCH_TOOL=$(detect_search_tool)
print_status "Using search tool: $SEARCH_TOOL"

# Universal search function with tool-specific implementations
search_todos() {
    local pattern="$1"
    local path="$2"
    
    case "$SEARCH_TOOL" in
        "rg")
            # Use ripgrep with proper flags
            rg "$pattern" --type rust "$path" --count-matches 2>/dev/null | awk -F: '{sum+=$2} END {print (sum ? sum : 0)}'
            ;;
        "grep")
            # Use grep with fallback
            find "$path" -name "*.rs" -exec grep -l "$pattern" {} \; 2>/dev/null | wc -l | tr -d ' '
            ;;
        *)
            echo "0"
            ;;
    esac
}

echo -e "${BLUE}🚀 TurboMCP Simple Release Check${NC}"
echo -e "${BLUE}===================================${NC}"
echo ""

if [ "$DRY_RUN" = "true" ]; then
    echo -e "${YELLOW}⚠️  DRY RUN MODE${NC}"
    echo ""
fi

echo -e "${BLUE}📋 Basic Checks${NC}"
echo "----------------"

# Check compilation
echo "Checking library compilation..."
if cargo check --workspace --lib; then
    print_status "All libraries compile"
else
    print_error "Compilation failed"
    exit 1
fi

echo ""
echo "Running code formatting check..."
if cargo fmt --all -- --check; then
    print_status "Code formatting is correct"
else
    print_error "Code formatting issues found - run 'cargo fmt'"
    exit 1
fi

echo ""
echo "Running clippy lints..."
if cargo clippy --workspace --lib -- -D clippy::correctness -D clippy::suspicious -D clippy::complexity; then
    print_status "Clippy lints pass (excluding pedantic)"
else
    print_error "Clippy issues found"
    exit 1
fi

echo ""
echo "Running tests..."
if cargo test --workspace --lib; then
    print_status "All tests pass"
else
    print_error "Tests failed"
    exit 1
fi

echo ""
echo -e "${BLUE}📦 Version Check${NC}"
echo "-----------------"
version_issues=0
for crate in turbomcp-core turbomcp-macros turbomcp-protocol turbomcp-transport turbomcp-cli turbomcp-client turbomcp-server turbomcp; do
    crate_version=$(grep '^version = ' "crates/$crate/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
    if [ "$crate_version" != "$VERSION" ]; then
        print_error "$crate has version $crate_version, expected $VERSION"
        version_issues=$((version_issues + 1))
    fi
done

if [ $version_issues -eq 0 ]; then
    print_status "All crates have version $VERSION"
else
    exit 1
fi

echo ""
echo -e "${BLUE}🔍 Code Quality Check${NC}"
echo "----------------------"
echo "Checking for critical TODOs and stub implementations..."
todo_count=$(search_todos "TODO.*implement|TODO.*stub|not_implemented|unimplemented" "crates/")
if [ "$todo_count" -gt 5 ]; then
    print_error "Found $todo_count critical TODOs/stubs - too many for 1.0.1 release"
    echo "To inspect: find crates/ -name '*.rs' -exec grep -n \"TODO.*implement\\|TODO.*stub\\|not_implemented\\|unimplemented\" {} +"
    exit 1
else
    print_status "Code quality check passed ($todo_count acceptable TODOs found)"
fi

echo ""
echo -e "${BLUE}📦 Package Check${NC}"
echo "-----------------"
# Check that all crates have the required metadata fields
for crate in turbomcp-core turbomcp-macros turbomcp-protocol turbomcp-transport turbomcp-cli turbomcp-client turbomcp-server turbomcp; do
    echo "Checking $crate metadata..."
    cargo_toml="crates/$crate/Cargo.toml"
    
    # Check for required fields
    required_fields=("description" "license" "repository")
    missing_fields=()
    
    for field in "${required_fields[@]}"; do
        if ! grep -q "^$field = " "$cargo_toml"; then
            missing_fields+=("$field")
        fi
    done
    
    if [ ${#missing_fields[@]} -ne 0 ]; then
        print_error "$crate missing required fields: ${missing_fields[*]}"
        exit 1
    else
        print_status "$crate has all required metadata"
    fi
done

echo ""
echo -e "${GREEN}🎉 Release Ready!${NC}"
echo ""
echo "Publish order:"
echo "1. turbomcp-core"
echo "2. turbomcp-macros"  
echo "3. turbomcp-protocol"
echo "4. turbomcp-transport"
echo "5. turbomcp-cli"
echo "6. turbomcp-client"
echo "7. turbomcp-server"
echo "8. turbomcp"

if [ "$DRY_RUN" = "true" ]; then
    echo ""
    echo "To publish: DRY_RUN=false $0"
fi