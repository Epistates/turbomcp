#!/bin/bash
# Test all feature combinations to verify no cfg warnings

set -e

echo "=== Testing Feature Combinations ==="
echo ""

# Test 1: Minimal (stdio only)
echo "Test 1: stdio only"
cargo check --no-default-features --features stdio 2>&1 | tee /tmp/test1.log
if grep -q "unexpected.*cfg" /tmp/test1.log; then
    echo "❌ FAIL: Unexpected cfg warnings with stdio only"
    exit 1
fi
echo "✅ PASS: stdio only"
echo ""

# Test 2: HTTP only
echo "Test 2: http only"
cargo check --no-default-features --features http 2>&1 | tee /tmp/test2.log
if grep -q "unexpected.*cfg" /tmp/test2.log; then
    echo "❌ FAIL: Unexpected cfg warnings with http only"
    exit 1
fi
echo "✅ PASS: http only"
echo ""

# Test 3: Partial features (http + websocket)
echo "Test 3: http + websocket"
cargo check --no-default-features --features http,websocket 2>&1 | tee /tmp/test3.log
if grep -q "unexpected.*cfg.*tcp" /tmp/test3.log || grep -q "unexpected.*cfg.*unix" /tmp/test3.log; then
    echo "❌ FAIL: Unexpected cfg warnings about tcp/unix"
    exit 1
fi
echo "✅ PASS: http + websocket (no tcp/unix warnings)"
echo ""

# Test 4: All transports
echo "Test 4: all transports"
cargo check --features stdio,http,websocket,tcp,unix 2>&1 | tee /tmp/test4.log
if grep -q "unexpected.*cfg" /tmp/test4.log; then
    echo "❌ FAIL: Unexpected cfg warnings with all features"
    exit 1
fi
echo "✅ PASS: all transports"
echo ""

# Test 5: Default (should be stdio)
echo "Test 5: default features"
cargo check 2>&1 | tee /tmp/test5.log
if grep -q "unexpected.*cfg" /tmp/test5.log; then
    echo "❌ FAIL: Unexpected cfg warnings with default features"
    exit 1
fi
echo "✅ PASS: default features"
echo ""

echo "🎉 All feature combination tests passed!"
