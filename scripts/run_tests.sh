#!/bin/bash
set -e

# ⚠️  DEPRECATED: Use 'make test' instead
# This script is maintained for backwards compatibility but is no longer the primary test runner.
# Use the Makefile target 'make test' for the standard test suite.
#
# TurboMCP Test Runner Script
# This script runs the complete test suite for the TurboMCP framework

echo "🚀 TurboMCP Test Suite Runner"
echo "============================="

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Function to print colored output
print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Default options
RUN_FORMAT_CHECK=true
RUN_CLIPPY=true
RUN_UNIT_TESTS=true
RUN_INTEGRATION_TESTS=true
RUN_DOC_TESTS=true
RUN_PROPERTY_TESTS=true
RUN_BENCHMARKS=false
RUN_COVERAGE=false
VERBOSE=false

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --no-format)
            RUN_FORMAT_CHECK=false
            shift
            ;;
        --no-clippy)
            RUN_CLIPPY=false
            shift
            ;;
        --no-unit)
            RUN_UNIT_TESTS=false
            shift
            ;;
        --no-integration)
            RUN_INTEGRATION_TESTS=false
            shift
            ;;
        --no-doc)
            RUN_DOC_TESTS=false
            shift
            ;;
        --no-property)
            RUN_PROPERTY_TESTS=false
            shift
            ;;
        --benchmarks)
            RUN_BENCHMARKS=true
            shift
            ;;
        --coverage)
            RUN_COVERAGE=true
            shift
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        --help)
            echo "TurboMCP Test Runner"
            echo ""
            echo "Usage: $0 [options]"
            echo ""
            echo "Options:"
            echo "  --no-format       Skip code formatting check"
            echo "  --no-clippy       Skip clippy linting"
            echo "  --no-unit         Skip unit tests"
            echo "  --no-integration  Skip integration tests"
            echo "  --no-doc          Skip documentation tests"
            echo "  --no-property     Skip property tests"
            echo "  --benchmarks      Run benchmark tests"
            echo "  --coverage        Generate coverage report"
            echo "  --verbose         Enable verbose output"
            echo "  --help            Show this help message"
            exit 0
            ;;
        *)
            print_error "Unknown option: $1"
            exit 1
            ;;
    esac
done

# Set verbose flag for cargo
if [ "$VERBOSE" = true ]; then
    CARGO_VERBOSE="--verbose"
else
    CARGO_VERBOSE=""
fi

# Track test results
FAILED_TESTS=()

# Function to run a test and track results
run_test() {
    local test_name="$1"
    local test_command="$2"
    
    print_status "Running $test_name..."
    
    if eval "$test_command"; then
        print_success "$test_name passed"
    else
        print_error "$test_name failed"
        FAILED_TESTS+=("$test_name")
    fi
    
    echo ""
}

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    print_error "Please run this script from the project root directory"
    exit 1
fi

# Check for required tools
print_status "Checking for required tools..."

if ! command -v cargo &> /dev/null; then
    print_error "cargo is not installed or not in PATH"
    exit 1
fi

if [ "$RUN_CLIPPY" = true ] && ! cargo clippy --version &> /dev/null; then
    print_warning "clippy is not available, skipping clippy check"
    RUN_CLIPPY=false
fi

if [ "$RUN_COVERAGE" = true ] && ! command -v cargo-tarpaulin &> /dev/null; then
    print_warning "cargo-tarpaulin is not available, skipping coverage"
    RUN_COVERAGE=false
fi

print_success "Tool check completed"
echo ""

# Clean previous builds
print_status "Cleaning previous builds..."
cargo clean
print_success "Clean completed"
echo ""

# Check code formatting
if [ "$RUN_FORMAT_CHECK" = true ]; then
    run_test "Code Formatting Check" "cargo fmt --all -- --check"
fi

# Run clippy linting
if [ "$RUN_CLIPPY" = true ]; then
    run_test "Clippy Linting" "cargo clippy --all-targets --all-features $CARGO_VERBOSE -- -D warnings"
fi

# Build the project
run_test "Project Build" "cargo build --all-features $CARGO_VERBOSE"

# Build examples
print_status "Building examples..."
if cargo build --example turbomcp_demo --features turbomcp $CARGO_VERBOSE; then
    print_success "TurboMCP demo example built"
else
    print_error "Failed to build TurboMCP demo example"
    FAILED_TESTS+=("TurboMCP Demo Example Build")
fi

if cargo build --example simple_server $CARGO_VERBOSE; then
    print_success "Simple server example built"
else
    print_error "Failed to build simple server example"
    FAILED_TESTS+=("Simple Server Example Build")
fi
echo ""

# Run unit tests
if [ "$RUN_UNIT_TESTS" = true ]; then
    print_status "Running unit tests by crate..."
    
    # Test each crate individually for better error reporting
    run_test "Core Unit Tests" "cargo test --lib --package mcp-core $CARGO_VERBOSE"
    run_test "Protocol Unit Tests" "cargo test --lib --package mcp-protocol $CARGO_VERBOSE"
    run_test "Transport Unit Tests" "cargo test --lib --package mcp-transport $CARGO_VERBOSE"
    run_test "Server Unit Tests" "cargo test --lib --package mcp-server $CARGO_VERBOSE"
    run_test "TurboMCP Unit Tests" "cargo test --lib --package turbomcp --all-features $CARGO_VERBOSE"
    run_test "TurboMCP Macros Unit Tests" "cargo test --lib --package turbomcp-macros $CARGO_VERBOSE"
fi

# Run integration tests
if [ "$RUN_INTEGRATION_TESTS" = true ]; then
    print_status "Running integration tests..."
    
    # Run specific integration test files
    run_test "Unit Tests Integration" "cargo test --test unit_tests --all-features $CARGO_VERBOSE"
    run_test "Integration Tests" "cargo test --test integration_tests --all-features $CARGO_VERBOSE"
    run_test "Error Handling Tests" "cargo test --test error_handling_tests --all-features $CARGO_VERBOSE"
    run_test "Context Injection Tests" "cargo test --test context_injection_tests --all-features $CARGO_VERBOSE"
    run_test "Mock Client Tests" "cargo test --test mock_client_tests --all-features $CARGO_VERBOSE"
    run_test "Concurrency Tests" "cargo test --test concurrency_tests --all-features $CARGO_VERBOSE"
    run_test "Documentation Tests" "cargo test --test doc_tests --all-features $CARGO_VERBOSE"
fi

# Run property tests
if [ "$RUN_PROPERTY_TESTS" = true ]; then
    run_test "Property-Based Tests" "cargo test --test property_tests --all-features $CARGO_VERBOSE"
fi

# Run documentation tests
if [ "$RUN_DOC_TESTS" = true ]; then
    run_test "Documentation Tests" "cargo test --doc --all-features $CARGO_VERBOSE"
fi

# Run benchmarks
if [ "$RUN_BENCHMARKS" = true ]; then
    if [ -d "benches" ]; then
        run_test "Benchmark Tests" "cargo bench --all-features $CARGO_VERBOSE"
    else
        print_warning "No benchmark directory found, skipping benchmarks"
    fi
fi

# Generate coverage report
if [ "$RUN_COVERAGE" = true ]; then
    print_status "Generating coverage report..."
    if cargo tarpaulin --all-features --workspace --timeout 120 --out Html --output-dir coverage/; then
        print_success "Coverage report generated in coverage/ directory"
    else
        print_error "Failed to generate coverage report"
        FAILED_TESTS+=("Coverage Report")
    fi
    echo ""
fi

# Test MCP protocol compatibility if inspector is available
if command -v npx &> /dev/null && npm list -g @modelcontextprotocol/inspector &> /dev/null; then
    print_status "Testing MCP protocol compatibility..."
    if timeout 10s npx @modelcontextprotocol/inspector ./target/debug/examples/turbomcp_demo 2>/dev/null; then
        print_success "MCP protocol compatibility test passed"
    else
        print_warning "MCP protocol compatibility test timed out or failed"
    fi
    echo ""
fi

# Summary
echo "🏁 Test Suite Summary"
echo "===================="

if [ ${#FAILED_TESTS[@]} -eq 0 ]; then
    print_success "All tests passed! 🎉"
    
    echo ""
    print_status "Test Statistics:"
    echo "  - Unit tests: $([ "$RUN_UNIT_TESTS" = true ] && echo "✅ Passed" || echo "⏭️ Skipped")"
    echo "  - Integration tests: $([ "$RUN_INTEGRATION_TESTS" = true ] && echo "✅ Passed" || echo "⏭️ Skipped")"
    echo "  - Documentation tests: $([ "$RUN_DOC_TESTS" = true ] && echo "✅ Passed" || echo "⏭️ Skipped")"
    echo "  - Property tests: $([ "$RUN_PROPERTY_TESTS" = true ] && echo "✅ Passed" || echo "⏭️ Skipped")"
    echo "  - Code formatting: $([ "$RUN_FORMAT_CHECK" = true ] && echo "✅ Passed" || echo "⏭️ Skipped")"
    echo "  - Clippy linting: $([ "$RUN_CLIPPY" = true ] && echo "✅ Passed" || echo "⏭️ Skipped")"
    echo "  - Benchmarks: $([ "$RUN_BENCHMARKS" = true ] && echo "✅ Passed" || echo "⏭️ Skipped")"
    echo "  - Coverage: $([ "$RUN_COVERAGE" = true ] && echo "✅ Generated" || echo "⏭️ Skipped")"
    
    echo ""
    print_success "🎯 TurboMCP is ready for production!"
    exit 0
else
    print_error "The following tests failed:"
    for test in "${FAILED_TESTS[@]}"; do
        echo "  ❌ $test"
    done
    
    echo ""
    print_error "Please fix the failing tests before proceeding"
    exit 1
fi