# TurboMCP Performance Benchmarking Infrastructure

## 🚀 Comprehensive Benchmarking Suite

This directory contains comprehensive performance benchmarks for TurboMCP, designed to ensure comprehensive performance across all components.

## 📊 Benchmark Categories

### 1. Core Performance (`turbomcp-core/benches/`)
- **Zero-copy operations**: Message creation, cloning, batching
- **Memory management**: Buffer pools, allocation patterns
- **Parsing optimization**: Lazy JSON parsing, UTF-8 validation
- **Stream processing**: JSON boundary detection, message boundaries

### 2. Framework Performance (`turbomcp/benches/`)
- **Macro-generated code**: Schema generation, handler dispatch
- **Context operations**: Request handling, metadata management
- **Helper functions**: Utility performance, error handling
- **URI template matching**: Pattern matching, parameter extraction

### 3. End-to-End Workflows (`benches/integration/`)
- **Full request cycles**: Client → Server → Response
- **Transport performance**: STDIO, TCP, WebSocket, HTTP
- **Authentication flows**: OAuth2, token validation
- **Real-world scenarios**: Complex tool chains, elicitation workflows

## 🎯 Performance Targets

### Latency Goals
- **Tool execution**: < 1ms overhead
- **Message parsing**: < 100μs for typical payloads
- **Schema validation**: < 50μs per validation
- **Transport overhead**: < 200μs per message

### Throughput Goals
- **Message processing**: > 100k messages/second
- **JSON parsing**: > 500MB/s sustained
- **Memory efficiency**: < 1KB overhead per request
- **Zero-copy efficiency**: > 95% allocation savings

## 🔧 Running Benchmarks

### Quick Performance Check
```bash
# Run all benchmarks with default configuration
cargo bench

# Generate HTML reports
cargo bench --features html_reports
```

### Detailed Analysis
```bash
# Run specific benchmark suites
cargo bench --bench zero_copy_bench
cargo bench --bench performance_tests
cargo bench --bench integration_benchmarks

# Profile memory usage
cargo bench --features profiling
```

### CI/CD Integration
```bash
# Regression testing (exits with error on >5% regression)
cargo bench --bench regression_detector

# Historical tracking
cargo bench --features historical-tracking
```

## 📈 Performance Monitoring

### Automated Regression Detection
- **Threshold**: 5% performance regression triggers CI failure
- **Baseline**: Last 10 stable release averages
- **Metrics**: P50, P95, P99 latencies + throughput

### Historical Tracking
- **Storage**: Performance data stored in `benches/results/`
- **Trending**: Automatic performance trend analysis
- **Alerts**: Slack/email notifications for significant changes

### Comparative Analysis
- **Performance baselines**: Track improvements over time
- **Hardware normalization**: Results adjusted for different hardware
- **Confidence intervals**: Statistical significance testing

## 🎯 Benchmark Design Principles

### 1. Real-World Scenarios
- Benchmarks reflect actual usage patterns
- Realistic payload sizes and complexity
- Production-like concurrency levels

### 2. Statistical Rigor
- Multiple iterations for statistical significance
- Outlier detection and removal
- Confidence interval reporting

### 3. Hardware Independence
- Results normalized for different hardware configurations
- Documented reference hardware specifications
- Scaling factors for different CPU/memory configurations

### 4. Reproducibility
- Deterministic benchmark setup
- Isolated test environments
- Version-locked dependencies

## 📋 Adding New Benchmarks

### 1. Core Component Benchmarks
Add to `crates/*/benches/` following the pattern:
```rust
use criterion::{Criterion, black_box, criterion_group, criterion_main};

fn bench_your_feature(c: &mut Criterion) {
    c.bench_function("your_feature", |b| {
        b.iter(|| black_box(your_function()))
    });
}

criterion_group!(benches, bench_your_feature);
criterion_main!(benches);
```

### 2. Integration Benchmarks
Add to `benches/integration/` for end-to-end workflows:
```rust
#[tokio::main]
async fn main() {
    // Full workflow benchmarks here
}
```

### 3. Regression Tests
Update `benches/regression/` with performance assertions:
```rust
assert!(result.mean() < baseline * 1.05, "Performance regression detected");
```

## 🔍 Interpreting Results

### Key Metrics
- **Latency**: Time per operation (lower is better)
- **Throughput**: Operations per second (higher is better)
- **Memory**: Peak and average allocation (lower is better)
- **CPU**: Utilization during sustained load (efficient is better)

### Performance Indicators
- **Fast**: Meets or exceeds performance targets
- **Acceptable**: Within functional requirements
- **Needs attention**: Below performance targets

### Benchmark Quality Indicators
- **Precision**: Low coefficient of variation (< 5%)
- **Stability**: Consistent results across runs
- **Coverage**: Comprehensive scenario testing
- **Relevance**: Real-world applicability

## 🛠️ Optimization Workflow

### 1. Identify Bottlenecks
```bash
# Profile hot paths
cargo bench --features flame-graph
perf record -g cargo bench
```

### 2. Implement Optimizations
- Zero-copy where possible
- Minimize allocations
- Optimize hot paths
- Cache expensive computations

### 3. Validate Improvements
```bash
# Before/after comparison
cargo bench --save-baseline before
# ... make changes ...
cargo bench --baseline before
```

### 4. Regression Protection
- Update performance targets
- Add specific regression tests
- Document optimization techniques

## 📚 References

- [Criterion.rs Documentation](https://bheisler.github.io/criterion.rs/book/)
- [Rust Performance Book](https://nnethercote.github.io/perf-book/)
- [Zero-Copy Optimization Techniques](https://docs.rs/bytes/)
- [MCP Protocol Performance Guidelines](https://spec.modelcontextprotocol.io/performance)

---

**Comprehensive performance is not just about speed—it's about consistency, predictability, and continuous improvement.**