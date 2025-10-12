#!/bin/bash

# TurboMCP Version Consistency Checker
# Validates that all versions are consistent across the workspace

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Expected version (can be overridden)
EXPECTED_VERSION=${VERSION:-""}

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

echo -e "${BLUE}🔍 TurboMCP Version Consistency Check${NC}"
echo -e "${BLUE}=====================================${NC}"
echo ""

version_issues=0

# Step 1: Collect all versions from crate Cargo.toml files
print_section "Step 1: Checking Crate Versions"

for crate in "${CRATES[@]}"; do
    cargo_toml="crates/$crate/Cargo.toml"
    if [ ! -f "$cargo_toml" ]; then
        print_error "Missing: $cargo_toml"
        version_issues=$((version_issues + 1))
        continue
    fi

    version=$(grep '^version = ' "$cargo_toml" | head -1 | sed 's/version = "\(.*\)"/\1/')

    if [ -z "$EXPECTED_VERSION" ]; then
        EXPECTED_VERSION="$version"
    fi

    if [ "$version" = "$EXPECTED_VERSION" ]; then
        echo "  ✓ $crate: $version"
    else
        print_error "  ✗ $crate: $version (expected $EXPECTED_VERSION)"
        version_issues=$((version_issues + 1))
    fi
done

echo ""

# Step 2: Check workspace Cargo.toml
print_section "Step 2: Checking Workspace Dependencies"

workspace_issues=0
for crate in "${CRATES[@]}"; do
    # Check if crate is referenced in workspace Cargo.toml
    if grep -q "^$crate = { version = \"$EXPECTED_VERSION\"" Cargo.toml; then
        echo "  ✓ $crate referenced with version $EXPECTED_VERSION"
    else
        # Try to find what version it has
        workspace_version=$(grep "^$crate = { version =" Cargo.toml | sed 's/.*version = "\([^"]*\)".*/\1/' || echo "NOT FOUND")
        if [ "$workspace_version" != "NOT FOUND" ]; then
            print_error "  ✗ $crate: workspace has $workspace_version, expected $EXPECTED_VERSION"
        else
            print_warning "  ⚠ $crate: not found in workspace dependencies (may be optional)"
        fi
        workspace_issues=$((workspace_issues + 1))
    fi
done

version_issues=$((version_issues + workspace_issues))
echo ""

# Step 3: Check internal dependencies
print_section "Step 3: Checking Internal Dependencies"

internal_dep_issues=0
for crate in "${CRATES[@]}"; do
    cargo_toml="crates/$crate/Cargo.toml"

    # Check if this crate depends on other internal crates
    for dep_crate in "${CRATES[@]}"; do
        if [ "$crate" = "$dep_crate" ]; then
            continue
        fi

        # Check if dep_crate is in dependencies
        if grep -q "^$dep_crate = " "$cargo_toml"; then
            dep_line=$(grep "^$dep_crate = " "$cargo_toml")

            # Check if using workspace = true
            if echo "$dep_line" | grep -q "workspace = true"; then
                echo "  ✓ $crate → $dep_crate: workspace = true"
            else
                dep_version=$(echo "$dep_line" | sed 's/.*version = "\([^"]*\)".*/\1/' || echo "PATH ONLY")

                if [ "$dep_version" = "PATH ONLY" ] || [ "$dep_version" = "$dep_line" ]; then
                    echo "  ℹ $crate → $dep_crate: path dependency only"
                elif [ "$dep_version" = "$EXPECTED_VERSION" ]; then
                    echo "  ✓ $crate → $dep_crate: $dep_version"
                else
                    print_error "  ✗ $crate → $dep_crate: $dep_version (expected $EXPECTED_VERSION)"
                    internal_dep_issues=$((internal_dep_issues + 1))
                fi
            fi
        fi
    done
done

version_issues=$((version_issues + internal_dep_issues))
echo ""

# Step 4: Check for hardcoded versions in source files
print_section "Step 4: Checking for Hardcoded Versions in Tests"

hardcoded_issues=0

# Common patterns to search for
# Look for old version patterns in test files
for test_file in $(find crates/*/src -name "*.rs" -type f | grep -E "(test|spec)\.rs$"); do
    # Search for quoted version strings that don't match expected
    while IFS= read -r line; do
        if echo "$line" | grep -qE '"[0-9]+\.[0-9]+\.[0-9]+(-[a-z0-9.]+)?"' && \
           ! echo "$line" | grep -q "\"$EXPECTED_VERSION\""; then
            found_version=$(echo "$line" | grep -oE '"[0-9]+\.[0-9]+\.[0-9]+(-[a-z0-9.]+)?"' | tr -d '"')
            if [ "$found_version" != "$EXPECTED_VERSION" ]; then
                print_warning "  ⚠ $test_file: Found version $found_version"
                echo "    Line: $(echo "$line" | xargs)"
                hardcoded_issues=$((hardcoded_issues + 1))
            fi
        fi
    done < "$test_file"
done

if [ $hardcoded_issues -eq 0 ]; then
    print_status "No hardcoded version mismatches found in test files"
else
    print_warning "Found $hardcoded_issues potential hardcoded version(s) - review manually"
fi

echo ""

# Step 5: Check git tags
print_section "Step 5: Checking Git Tags"

latest_tag=$(git describe --tags --abbrev=0 2>/dev/null || echo "NONE")
if [ "$latest_tag" = "NONE" ]; then
    print_warning "No git tags found"
else
    echo "  Latest tag: $latest_tag"

    # Remove 'v' prefix if present
    tag_version="${latest_tag#v}"

    if [ "$tag_version" = "$EXPECTED_VERSION" ]; then
        print_status "Git tag matches expected version"
    else
        print_warning "Git tag ($tag_version) differs from crate version ($EXPECTED_VERSION)"
        echo "  This is expected if you haven't tagged the new release yet"
    fi
fi

echo ""

# Final summary
print_section "Summary"
echo "Expected version: $EXPECTED_VERSION"
echo "Crates checked: ${#CRATES[@]}"
echo "Issues found: $version_issues"

echo ""

if [ $version_issues -eq 0 ]; then
    print_status "All version checks passed!"
    exit 0
else
    print_error "Found $version_issues version inconsistencies"
    echo ""
    echo "To fix version issues, run:"
    echo "  # Update all crate versions"
    echo "  for crate in ${CRATES[@]}; do"
    echo "    sed -i '' 's/version = \".*\"/version = \"$EXPECTED_VERSION\"/g' \"crates/\$crate/Cargo.toml\""
    echo "  done"
    echo ""
    echo "  # Update workspace dependencies"
    echo "  sed -i '' 's/version = \".*\"/version = \"$EXPECTED_VERSION\"/g' Cargo.toml"
    exit 1
fi
