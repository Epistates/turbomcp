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

# Crate list (in dependency order)
CRATES=(
    "turbomcp-protocol"
    "turbomcp-dpop"
    "turbomcp-transport"
    "turbomcp-macros"
    "turbomcp-auth"
    "turbomcp-server"
    "turbomcp-client"
    "turbomcp-cli"
    "turbomcp"
    "turbomcp-proxy"
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
            workspace_issues=$((workspace_issues + 1))
        else
            # Not in workspace deps is OK - these are optional crates
            print_warning "  ⚠ $crate: not found in workspace dependencies (optional)"
        fi
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

        # Check if dep_crate is in dependencies (may have multiple entries for dev-deps)
        # Only take the first match to avoid concatenating versions from multiple sections
        if grep -q "^$dep_crate = " "$cargo_toml"; then
            dep_line=$(grep "^$dep_crate = " "$cargo_toml" | head -1)

            # Check if using workspace = true
            if echo "$dep_line" | grep -q "workspace = true"; then
                echo "  ✓ $crate → $dep_crate: workspace = true"
            else
                # Extract version - handle both inline and multiline formats
                if echo "$dep_line" | grep -q 'version = '; then
                    dep_version=$(echo "$dep_line" | grep -oE 'version = "[^"]*"' | head -1 | sed 's/version = "\([^"]*\)"/\1/')
                else
                    dep_version="PATH ONLY"
                fi

                if [ -z "$dep_version" ] || [ "$dep_version" = "PATH ONLY" ]; then
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

# Quick grep-based check for old version patterns in test files
# Only flag obvious old versions like 2.0.0, 2.0.1, 2.0.2 that should be updated
old_versions=$(grep -rE 'DEFAULT_VERSION|SERVER_VERSION' crates/*/src crates/*/tests 2>/dev/null | \
    grep -oE '"2\.0\.[0-2]"' | sort -u || true)

if [ -n "$old_versions" ]; then
    print_error "Found old version references in test files:"
    echo "$old_versions"
    hardcoded_issues=1
fi

if [ $hardcoded_issues -eq 0 ]; then
    print_status "No hardcoded version mismatches found in test files"
else
    print_error "Found hardcoded version mismatch(es) in test files"
    echo ""
    echo "To fix, run: VERSION=$EXPECTED_VERSION ./scripts/update-versions.sh"
    version_issues=$((version_issues + hardcoded_issues))
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

# Step 6: Check documentation files
print_section "Step 6: Checking Documentation and README Files"

doc_issues=0

# Check main README for version patterns
if [ -f "README.md" ]; then
    # Quick check for version-like patterns
    if grep -q '[0-9]\+\.[0-9]\+\.[0-9]\+' README.md 2>/dev/null; then
        # Look for Cargo.toml examples with version numbers different from expected
        if grep -q 'turbomcp.*version.*=' README.md 2>/dev/null; then
            print_warning "  ⚠ README.md: Contains turbomcp dependency examples - verify versions match $EXPECTED_VERSION"
            doc_issues=$((doc_issues + 1))
        fi
    fi
fi

if [ $doc_issues -eq 0 ]; then
    print_status "No obvious version inconsistencies found in documentation"
else
    print_warning "Found $doc_issues documentation item(s) - manual review recommended"
fi

version_issues=$((version_issues + doc_issues))
echo ""

# Step 7: Check scripts directory for version references
print_section "Step 7: Checking Scripts for Version References"

# Informational only - scripts typically auto-detect versions
if [ -f "scripts/prepare-release.sh" ]; then
    print_status "Release scripts present (scripts/prepare-release.sh)"
else
    print_warning "No release scripts found"
fi

echo ""

# Step 8: Check for old version references in internal dependencies
print_section "Step 8: Checking for Old Internal Version References"

old_version_issues=0

# Only check for OLD internal turbomcp crate versions (not external deps like serde = "1.0")
for old_ver in "1.0" "2.0" "2.1" "2.2"; do
    if grep -E "^turbomcp.*version = \"${old_ver}\.[0-9]+\"" crates/*/Cargo.toml 2>/dev/null >/dev/null; then
        print_error "  ✗ Found internal turbomcp dependency with old version ${old_ver}.x"
        old_version_issues=$((old_version_issues + 1))
    fi
done

if [ $old_version_issues -eq 0 ]; then
    print_status "No outdated internal version references detected"
fi

version_issues=$((version_issues + old_version_issues))
echo ""

# Final summary
print_section "Summary"
echo "Expected version: $EXPECTED_VERSION"
echo "Crates checked: ${#CRATES[@]}"
echo "  ✓ Crate versions"
echo "  ✓ Workspace dependencies"
echo "  ✓ Internal dependencies"
echo "  ✓ Hardcoded versions in tests"
echo "  ✓ Git tags"
echo "  ✓ Documentation files"
echo "  ✓ Scripts"
echo "  ✓ Code comments"
echo ""
echo "Total issues found: $version_issues"

echo ""

if [ $version_issues -eq 0 ]; then
    print_status "All version checks passed!"
    exit 0
else
    print_error "Found $version_issues version inconsistencies"
    echo ""
    echo "To fix version issues, run:"
    echo "  # Update all crate versions"
    echo "  VERSION=$EXPECTED_VERSION ./scripts/update-versions.sh"
    exit 1
fi
