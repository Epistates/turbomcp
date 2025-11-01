#!/bin/bash

# TurboMCP Publish Script
# Publishes all crates to crates.io in the correct dependency order
#
# Usage:
#   DRY_RUN=true ./scripts/publish.sh     # Test run (default)
#   DRY_RUN=false ./scripts/publish.sh    # Actual publish

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
DRY_RUN=${DRY_RUN:-true}
WAIT_TIME=${WAIT_TIME:-30}  # Seconds to wait between publishes
VERSION=${VERSION:-""}       # Will be auto-detected if not set

# Crate publish order (dependencies first)
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

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ] || [ ! -d "crates" ]; then
    print_error "Must be run from the turbomcp workspace root"
    exit 1
fi

# Auto-detect version if not set
if [ -z "$VERSION" ]; then
    VERSION=$(grep '^version = ' "crates/turbomcp-protocol/Cargo.toml" | head -1 | sed 's/version = "\(.*\)"/\1/')
    print_warning "Auto-detected version: $VERSION"
fi

echo -e "${BLUE}🚀 TurboMCP Publish to crates.io${NC}"
echo -e "${BLUE}================================${NC}"
echo ""
echo "Version: $VERSION"
echo "Crates to publish: ${#CRATES[@]}"
echo ""

if [ "$DRY_RUN" = "true" ]; then
    echo -e "${YELLOW}⚠️  DRY RUN MODE - No actual publishing will occur${NC}"
    echo -e "${YELLOW}   Set DRY_RUN=false to perform actual publish${NC}"
    echo ""
fi

# Pre-flight checks
print_section "Pre-flight Checks"

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
    print_status "Logged into crates.io"
fi

# Verify all crates exist
for crate in "${CRATES[@]}"; do
    if [ ! -f "crates/$crate/Cargo.toml" ]; then
        print_error "Crate not found: $crate"
        exit 1
    fi
done

print_status "All ${#CRATES[@]} crates found"
echo ""

# Show publish order
print_section "Publish Order"
for i in "${!CRATES[@]}"; do
    echo "$((i+1)). ${CRATES[$i]}"
done
echo ""

if [ "$DRY_RUN" = "true" ]; then
    print_warning "This is a DRY RUN - no actual publishing will occur"
    print_warning "To publish for real, run: DRY_RUN=false $0"
    exit 0
fi

# Confirm before proceeding
print_section "Confirmation Required"
echo -e "${RED}WARNING: This will publish ${#CRATES[@]} crates to crates.io!${NC}"
echo -e "${RED}This action CANNOT be undone!${NC}"
echo ""
read -p "Are you sure you want to continue? (yes/no): " -r
echo

if [[ ! $REPLY =~ ^[Yy][Ee][Ss]$ ]]; then
    print_warning "Publish cancelled by user"
    exit 1
fi

# Publishing
print_section "Publishing Crates"

published_count=0
failed_crates=()

for i in "${!CRATES[@]}"; do
    crate="${CRATES[$i]}"
    echo ""
    echo -e "${BLUE}Publishing $((i+1))/${#CRATES[@]}: $crate${NC}"
    echo "----------------------------------------"

    # Try to publish
    if cargo publish --manifest-path "crates/$crate/Cargo.toml" 2>&1 | tee "/tmp/turbomcp-publish-$crate.log"; then
        print_status "$crate published successfully"
        published_count=$((published_count + 1))

        # Wait between publishes (except for the last one)
        if [ $i -lt $((${#CRATES[@]} - 1)) ]; then
            echo ""
            echo "⏳ Waiting $WAIT_TIME seconds for crates.io to index..."
            sleep "$WAIT_TIME"
        fi
    else
        print_error "$crate failed to publish"
        failed_crates+=("$crate")

        # Show last 20 lines of error
        echo ""
        echo "Last 20 lines of error:"
        tail -20 "/tmp/turbomcp-publish-$crate.log"

        # Ask if we should continue
        echo ""
        read -p "Continue with remaining crates? (yes/no): " -r
        echo

        if [[ ! $REPLY =~ ^[Yy][Ee][Ss]$ ]]; then
            print_error "Publish stopped by user after $published_count/${#CRATES[@]} crates"
            exit 1
        fi
    fi
done

echo ""
print_section "Publish Summary"
echo "Version: $VERSION"
echo "Total crates: ${#CRATES[@]}"
echo "Successfully published: $published_count"
echo "Failed: ${#failed_crates[@]}"

if [ ${#failed_crates[@]} -gt 0 ]; then
    echo ""
    print_error "Failed crates:"
    for crate in "${failed_crates[@]}"; do
        echo "  - $crate"
    done
    echo ""
    print_error "Some crates failed to publish. Check logs in /tmp/turbomcp-publish-*.log"
    exit 1
fi

echo ""
print_status "🎉 All crates published successfully!"
echo ""
echo "Next steps:"
echo "1. Verify crates are live: https://crates.io/crates/turbomcp"
echo "2. Create git tag: git tag v$VERSION && git push origin v$VERSION"
echo "3. Create GitHub release with changelog"
echo "4. Announce on social media"
