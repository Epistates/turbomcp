#!/bin/bash
# ⚠️  DEPRECATED: Use 'make test' and 'make coverage' instead
# This script is maintained for backwards compatibility but is no longer the primary test runner.
# Use Makefile targets: 'make test' for tests and 'make coverage' for coverage reporting.
#
# Comprehensive test runner for TurboMCP
# Runs all test suites with proper coverage reporting

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
COVERAGE_DIR="coverage"
REPORTS_DIR="test-reports"
CARGO_FLAGS=${CARGO_FLAGS:-""}
RUST_TEST_THREADS=${RUST_TEST_THREADS:-1}

# Create directories
mkdir -p "$COVERAGE_DIR" "$REPORTS_DIR"

echo -e "${BLUE}🧪 Starting TurboMCP Comprehensive Test Suite${NC}"
echo "========================================"

# Function to print section headers
print_section() {
    echo -e "\n${BLUE}📋 $1${NC}"
    echo "----------------------------------------"
}

# Function to run tests with timeout and retry
run_test_with_retry() {
    local test_name="$1"
    local test_command="$2"
    local timeout_seconds="${3:-300}"
    local max_retries="${4:-2}"
    
    echo -e "${YELLOW}Running: $test_name${NC}"
    
    for attempt in $(seq 1 $max_retries); do
        if timeout "$timeout_seconds" bash -c "$test_command"; then
            echo -e "${GREEN}✅ $test_name passed${NC}"
            return 0
        else
            echo -e "${RED}❌ $test_name failed (attempt $attempt/$max_retries)${NC}"
            if [ "$attempt" -eq "$max_retries" ]; then
                echo -e "${RED}🚨 $test_name failed after $max_retries attempts${NC}"
                return 1
            fi
            echo "Retrying in 5 seconds..."
            sleep 5
        fi
    done
}

# Function to generate coverage report
generate_coverage() {
    print_section "Generating Coverage Report"
    
    echo "Running cargo tarpaulin..."
    cargo tarpaulin \
        --config tarpaulin.toml \
        --timeout 300 \
        --out Html \
        --out Xml \
        --out Json \
        --output-dir "$COVERAGE_DIR" \
        --skip-clean \
        --verbose \
        || {
            echo -e "${YELLOW}⚠️  Tarpaulin failed, trying alternative coverage method${NC}"
            
            # Fallback: Use LLVM coverage
            export RUSTFLAGS="-C instrument-coverage"
            export LLVM_PROFILE_FILE="$COVERAGE_DIR/%p-%m.profraw"
            
            cargo test --all-features
            
            # Generate coverage report
            if command -v llvm-profdata >/dev/null 2>&1 && command -v llvm-cov >/dev/null 2>&1; then
                llvm-profdata merge -sparse "$COVERAGE_DIR"/*.profraw -o "$COVERAGE_DIR/coverage.profdata"
                llvm-cov export \
                    --format=lcov \
                    --instr-profile="$COVERAGE_DIR/coverage.profdata" \
                    target/debug/deps/* \
                    > "$COVERAGE_DIR/lcov.info"
                echo -e "${GREEN}✅ LLVM coverage report generated${NC}"
            else
                echo -e "${YELLOW}⚠️  LLVM coverage tools not available${NC}"
            fi
        }
}

# 1. Unit Tests
print_section "Unit Tests"
run_test_with_retry "Unit tests" "cargo test --lib --all-features $CARGO_FLAGS" 120

# 2. Integration Tests
print_section "Integration Tests"
run_test_with_retry "Authentication flow tests" "cargo test --test auth_flows_test $CARGO_FLAGS" 180
run_test_with_retry "Transport robustness tests" "cargo test --test transport_robustness_test $CARGO_FLAGS" 180
run_test_with_retry "Middleware and routing tests" "cargo test --test server_middleware_routing_test $CARGO_FLAGS" 180
run_test_with_retry "Protocol validation tests" "cargo test --test protocol_validation_test $CARGO_FLAGS" 120
run_test_with_retry "Macro system tests" "cargo test --test macro_system_test $CARGO_FLAGS" 120
run_test_with_retry "Transport implementations tests" "cargo test --test transport_implementations_test $CARGO_FLAGS" 180
run_test_with_retry "Error propagation tests" "cargo test --test error_propagation_fault_injection_test $CARGO_FLAGS" 300

# 3. Performance Tests
print_section "Performance Tests"
export RUST_TEST_THREADS=1  # Performance tests need exclusive access
run_test_with_retry "Performance and concurrency tests" "cargo test --test performance_concurrency_test --release $CARGO_FLAGS" 600

# 4. Property-Based Tests
print_section "Property-Based Tests"
run_test_with_retry "Property-based tests" "cargo test --test property_based_tests $CARGO_FLAGS" 300

# 5. Documentation Tests
print_section "Documentation Tests"
run_test_with_retry "Doc tests" "cargo test --doc --all-features $CARGO_FLAGS" 120

# 6. Benchmark Tests (if criterion is available)
print_section "Benchmark Tests"
if cargo bench --help >/dev/null 2>&1; then
    echo "Running benchmark compilation check..."
    run_test_with_retry "Benchmark compilation" "cargo bench --no-run --all-features $CARGO_FLAGS" 180
else
    echo -e "${YELLOW}⚠️  Benchmark tests skipped (criterion not available)${NC}"
fi

# 7. Examples Compilation
print_section "Examples Compilation"
run_test_with_retry "Examples compilation" "cargo build --examples --all-features $CARGO_FLAGS" 120

# 8. Clippy Lints
print_section "Clippy Lints"
run_test_with_retry "Clippy lints" "cargo clippy --all-features --all-targets -- -D warnings $CARGO_FLAGS" 120

# 9. Format Check
print_section "Format Check"
run_test_with_retry "Format check" "cargo fmt --all -- --check" 30

# 10. Security Audit
print_section "Security Audit"
if command -v cargo-audit >/dev/null 2>&1; then
    run_test_with_retry "Security audit" "cargo audit" 60
else
    echo -e "${YELLOW}⚠️  cargo-audit not installed, skipping security audit${NC}"
fi

# 11. Dependency Check
print_section "Dependency Check"
if command -v cargo-outdated >/dev/null 2>&1; then
    echo "Checking for outdated dependencies..."
    cargo outdated --exit-code 1 || echo -e "${YELLOW}⚠️  Some dependencies are outdated${NC}"
else
    echo -e "${YELLOW}⚠️  cargo-outdated not installed, skipping dependency check${NC}"
fi

# 12. Coverage Report Generation
if [ "${SKIP_COVERAGE:-false}" != "true" ]; then
    generate_coverage
else
    echo -e "${YELLOW}⚠️  Coverage generation skipped${NC}"
fi

# 13. Test Report Generation
print_section "Generating Test Reports"

# Create a comprehensive test report
cat > "$REPORTS_DIR/test-summary.md" << EOF
# TurboMCP Test Report

Generated on: $(date)

## Test Results

$(if [ -f "$COVERAGE_DIR/tarpaulin-report.html" ]; then
    echo "✅ Coverage report generated: $COVERAGE_DIR/tarpaulin-report.html"
else
    echo "⚠️  Coverage report not available"
fi)

## Test Categories

- ✅ Unit Tests
- ✅ Integration Tests  
- ✅ Performance Tests
- ✅ Property-Based Tests
- ✅ Documentation Tests
- ✅ Code Quality (Clippy/Format)

## Key Metrics

- Test execution time: $(date)
- Rust version: $(rustc --version)
- Target: $(rustc -vV | grep host | cut -d' ' -f2)

## Coverage Details

See the HTML coverage report in the coverage/ directory for detailed line-by-line coverage information.

## Performance Benchmarks

Performance test results are included in the test output above.

## Recommendations

1. Maintain test coverage above 80%
2. Address any failing performance benchmarks
3. Fix all clippy warnings
4. Keep dependencies up to date
5. Regular security audits

EOF

echo -e "${GREEN}📊 Test report generated: $REPORTS_DIR/test-summary.md${NC}"

# 14. Final Summary
print_section "Test Suite Summary"

echo -e "${GREEN}🎉 TurboMCP Comprehensive Test Suite Completed!${NC}"
echo ""
echo "📁 Reports available in:"
echo "  - Coverage: $COVERAGE_DIR/"
echo "  - Reports: $REPORTS_DIR/"
echo ""
echo "📊 Key Files:"
echo "  - Coverage HTML: $COVERAGE_DIR/tarpaulin-report.html"
echo "  - Test Summary: $REPORTS_DIR/test-summary.md"
echo ""
echo "🔍 Next Steps:"
echo "  1. Review coverage report for areas needing more tests"
echo "  2. Address any performance issues identified"
echo "  3. Fix any remaining clippy warnings"
echo "  4. Consider adding more edge case tests"
echo ""

# Check if we should open the coverage report
if [ "${OPEN_COVERAGE:-false}" = "true" ] && [ -f "$COVERAGE_DIR/tarpaulin-report.html" ]; then
    if command -v open >/dev/null 2>&1; then
        open "$COVERAGE_DIR/tarpaulin-report.html"
    elif command -v xdg-open >/dev/null 2>&1; then
        xdg-open "$COVERAGE_DIR/tarpaulin-report.html"
    fi
fi

echo -e "${BLUE}Test suite execution completed successfully! 🚀${NC}"