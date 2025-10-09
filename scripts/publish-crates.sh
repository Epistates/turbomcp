#!/bin/bash

# TurboMCP Manual Publishing Script
# Quick script to publish all crates in the correct order
#
# Environment variables:
#   DELAY_BETWEEN_PUBLISHES - Delay between publishes (default: 60 seconds)
#   VERIFICATION_RETRIES - Number of verification attempts (default: 3)
#
# Usage:
#   ./scripts/publish-crates.sh
#   DELAY_BETWEEN_PUBLISHES=120 ./scripts/publish-crates.sh  # 2 minute delays

set -euo pipefail

# Configuration
DELAY_BETWEEN_PUBLISHES=${DELAY_BETWEEN_PUBLISHES:-60}  # 60 seconds between publishes
VERIFICATION_RETRIES=${VERIFICATION_RETRIES:-3}  # Number of verification attempts

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

# Crate publish order (dependencies first)
# Note: turbomcp-core was merged into turbomcp-protocol in v2.0.0
CRATES=(
    "turbomcp-protocol"
    "turbomcp-transport"
    "turbomcp-macros"
    "turbomcp-dpop"
    "turbomcp-server"
    "turbomcp-client"
    "turbomcp-cli"
    "turbomcp"
)

echo -e "${BLUE}🚀 Publishing TurboMCP Crates${NC}"
echo "=============================="
echo ""
echo "Configuration:"
echo "  Delay between publishes: ${DELAY_BETWEEN_PUBLISHES}s" 
echo "  Verification retries: ${VERIFICATION_RETRIES}"
echo ""

# Check if we're logged in by testing credentials file
if ! test -f ~/.cargo/credentials.toml; then
    echo -e "${YELLOW}⚠️  Not logged into crates.io. Please run 'cargo login' first${NC}"
    exit 1
fi

echo "Publishing order:"
for i in "${!CRATES[@]}"; do
    echo "$((i+1)). ${CRATES[$i]}"
done
echo ""

read -p "Continue with publishing? (y/N): " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 1
fi

for i in "${!CRATES[@]}"; do
    crate="${CRATES[$i]}"
    echo ""
    echo -e "${BLUE}Publishing $((i+1))/${#CRATES[@]}: $crate${NC}"
    
    # Publish with extended delay handling
    if cargo publish --manifest-path "crates/$crate/Cargo.toml"; then
        echo -e "${GREEN}✅ $crate published successfully${NC}"
        
        # Verify publication by checking crates.io (with retries)
        echo "Verifying publication on crates.io..."
        for attempt in $(seq 1 $VERIFICATION_RETRIES); do
            if cargo search --limit 1 "$crate" | grep -q "$crate"; then
                echo -e "${GREEN}✅ $crate verified on crates.io${NC}"
                break
            else
                if [ $attempt -lt $VERIFICATION_RETRIES ]; then
                    echo "Verification attempt $attempt failed, retrying in 10s..."
                    sleep 10
                else
                    echo -e "${YELLOW}⚠️  $crate published but verification failed (this is usually fine)${NC}"
                fi
            fi
        done
    else
        echo -e "${RED}❌ Failed to publish $crate${NC}"
        echo "This could be due to network issues, rate limiting, or dependency problems."
        echo "You may want to try again with a longer delay: DELAY_BETWEEN_PUBLISHES=120 $0"
        exit 1
    fi
    
    # Wait between publishes (except for the last one)
    if [ $i -lt $((${#CRATES[@]} - 1)) ]; then
        echo "Waiting $DELAY_BETWEEN_PUBLISHES seconds for crates.io to process and avoid timeout..."
        echo "(This prevents rate limiting and dependency resolution issues)"
        sleep $DELAY_BETWEEN_PUBLISHES
    fi
done

echo ""
echo -e "${GREEN}🎉 All crates published successfully!${NC}"
echo ""
echo "Next steps:"
echo "1. Create git tag: git tag v1.1.0"
echo "2. Push tag: git push origin v1.1.0"
echo "3. Create GitHub release"
echo "4. Launch social media campaign!"