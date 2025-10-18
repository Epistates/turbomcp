#!/bin/bash

# ⚠️  DEPRECATED: Use 'make coverage' instead
# This script is maintained for backwards compatibility.
# Use the Makefile target 'make coverage' for coverage reporting.
#
# Coverage reporting script for TurboMCP
set -euo pipefail

echo "🧪 Running test coverage analysis for TurboMCP"

# Clean up previous coverage data
echo "Cleaning up previous coverage data..."
cargo llvm-cov clean

# Run tests with coverage
echo "Running tests with coverage collection..."
cargo llvm-cov --all-features --workspace --lcov --output-path coverage/lcov.info

# Generate HTML report
echo "Generating HTML coverage report..."
cargo llvm-cov --all-features --workspace --html --output-dir coverage/html

# Generate summary
echo "Generating coverage summary..."
cargo llvm-cov --all-features --workspace --summary-only

echo "✅ Coverage analysis complete!"
echo "📊 HTML report: coverage/html/index.html"
echo "📄 LCOV data: coverage/lcov.info"