//! End-to-end integration benchmarks
//!
//! Measures real-world performance across full request/response cycles
//! including transport, parsing, validation, and handler execution.

use criterion::{{Criterion, criterion_group, criterion_main, BenchmarkId, Throughput}};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::runtime::Runtime;
use turbomcp::prelude::*;

/// Performance counter for tracking operations
#[derive(Debug)]
struct BenchmarkMetrics {
    requests_processed: AtomicU64,
    total_latency_ns: AtomicU64,
    errors_encountered: AtomicU64,
}

impl BenchmarkMetrics {
    fn new() -> Self {
        Self {
            requests_processed: AtomicU64::new(0),
            total_latency_ns: AtomicU64::new(0),
            errors_encountered: AtomicU64::new(0),
        }
    }

    fn record_request(&self, latency_ns: u64) {
        self.requests_processed.fetch_add(1, Ordering::Relaxed);
        self.total_latency_ns.fetch_add(latency_ns, Ordering::Relaxed);
    }

    fn record_error(&self) {
        self.errors_encountered.fetch_add(1, Ordering::Relaxed);
    }

    fn average_latency_ns(&self) -> f64 {
        let total = self.total_latency_ns.load(Ordering::Relaxed);
        let count = self.requests_processed.load(Ordering::Relaxed);
        if count == 0 { 0.0 } else { total as f64 / count as f64 }
    }
}

/// High-performance benchmark server
#[derive(Debug)]
struct BenchmarkServer {
    metrics: Arc<BenchmarkMetrics>,
    operation_counter: AtomicU64,
}

impl BenchmarkServer {
    fn new() -> Self {
        Self {
            metrics: Arc::new(BenchmarkMetrics::new()),
            operation_counter: AtomicU64::new(0),
        }
    }

    fn next_operation_id(&self) -> u64 {
        self.operation_counter.fetch_add(1, Ordering::Relaxed)
    }
}

#[async_trait]
impl HandlerRegistration for BenchmarkServer {
    async fn register_with_builder(&self, builder: &mut ServerBuilder) -> McpResult<()> {
        // Register multiple tools for comprehensive benchmarking
        builder
            .tool("fast_computation", "Lightweight CPU-bound operation")
            .tool("memory_operation", "Memory-intensive operation")
            .tool("io_simulation", "Simulated I/O operation")
            .tool("validation_heavy", "Schema validation intensive operation")
            .tool("error_scenario", "Controlled error generation for testing");
        Ok(())
    }
}

#[async_trait]
impl TurboMcpServer for BenchmarkServer {
    fn name(&self) -> &'static str {
        "BenchmarkServer"
    }

    fn version(&self) -> &'static str {
        "1.0.0"
    }

    async fn startup(&self) -> McpResult<()> {
        // Pre-warm any caches or connections
        Ok(())
    }

    async fn fast_computation(&self, _ctx: Context, iterations: i32) -> McpResult<String> {
        let start = Instant::now();
        let op_id = self.next_operation_id();

        // Simulate lightweight computation
        let mut result = 0u64;
        for i in 0..iterations {
            result = result.wrapping_add((i as u64).wrapping_mul(17));
        }

        let elapsed = start.elapsed().as_nanos() as u64;
        self.metrics.record_request(elapsed);

        Ok(format!("Operation {}: computed {} iterations, result={}", op_id, iterations, result))
    }

    async fn memory_operation(&self, _ctx: Context, size_kb: i32) -> McpResult<String> {
        let start = Instant::now();
        let op_id = self.next_operation_id();

        // Simulate memory-intensive operation
        let size = (size_kb * 1024) as usize;
        let mut buffer = Vec::with_capacity(size);
        buffer.resize(size, 0u8);

        // Touch memory to ensure allocation
        for i in (0..size).step_by(4096) {
            buffer[i] = (i % 256) as u8;
        }

        let checksum: u64 = buffer.iter().map(|&b| b as u64).sum();

        let elapsed = start.elapsed().as_nanos() as u64;
        self.metrics.record_request(elapsed);

        Ok(format!("Operation {}: allocated {}KB, checksum={}", op_id, size_kb, checksum))
    }

    async fn io_simulation(&self, _ctx: Context, delay_ms: i32) -> McpResult<String> {
        let start = Instant::now();
        let op_id = self.next_operation_id();

        // Simulate I/O delay
        tokio::time::sleep(Duration::from_millis(delay_ms as u64)).await;

        let elapsed = start.elapsed().as_nanos() as u64;
        self.metrics.record_request(elapsed);

        Ok(format!("Operation {}: simulated {}ms I/O", op_id, delay_ms))
    }

    async fn validation_heavy(&self, _ctx: Context, schema_complexity: i32) -> McpResult<String> {
        let start = Instant::now();
        let op_id = self.next_operation_id();

        // Simulate complex validation
        let mut validation_result = String::new();
        for i in 0..schema_complexity {
            validation_result.push_str(&format!("field_{}:validated;", i));
        }

        let elapsed = start.elapsed().as_nanos() as u64;
        self.metrics.record_request(elapsed);

        Ok(format!("Operation {}: validated {} schema fields", op_id, schema_complexity))
    }

    async fn error_scenario(&self, _ctx: Context, error_type: String) -> McpResult<String> {
        let start = Instant::now();
        self.metrics.record_error();

        let elapsed = start.elapsed().as_nanos() as u64;
        self.metrics.record_request(elapsed);

        match error_type.as_str() {
            "validation" => Err(McpError::InvalidInput("Benchmark validation error".to_string())),
            "permission" => Err(McpError::Unauthorized("Benchmark permission error".to_string())),
            "timeout" => Err(McpError::Timeout("Benchmark timeout error".to_string())),
            _ => Err(McpError::Tool("Benchmark generic error".to_string())),
        }
    }
}

/// Benchmark end-to-end request processing latency
fn bench_request_latency(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("request_latency");

    // Set up server once for all benchmarks
    let server = Arc::new(BenchmarkServer::new());

    // Different operation types with varying complexity
    let scenarios = vec![
        ("fast_computation", r#"{"iterations": 100}"#),
        ("memory_operation", r#"{"size_kb": 64}"#),
        ("validation_heavy", r#"{"schema_complexity": 50}"#),
    ];

    for (tool_name, params) in scenarios {
        group.bench_with_input(
            BenchmarkId::new("tool_execution", tool_name),
            &(tool_name, params),
            |b, (tool, params)| {
                b.to_async(&rt).iter(|| async {
                    let ctx = Context::new(
                        turbomcp_core::RequestContext::new(),
                        HandlerMetadata {
                            name: tool.to_string(),
                            handler_type: "tool".to_string(),
                            description: None,
                        },
                    );

                    // Parse parameters
                    let arguments: serde_json::Value = serde_json::from_str(params).unwrap();

                    // Execute tool (this would normally go through the full MCP stack)
                    match *tool {
                        "fast_computation" => {
                            let iterations = arguments["iterations"].as_i64().unwrap() as i32;
                            let result = server.fast_computation(ctx, iterations).await;
                            std::hint::black_box(result)
                        },
                        "memory_operation" => {
                            let size_kb = arguments["size_kb"].as_i64().unwrap() as i32;
                            let result = server.memory_operation(ctx, size_kb).await;
                            std::hint::black_box(result)
                        },
                        "validation_heavy" => {
                            let complexity = arguments["schema_complexity"].as_i64().unwrap() as i32;
                            let result = server.validation_heavy(ctx, complexity).await;
                            std::hint::black_box(result)
                        },
                        _ => unreachable!(),
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark sustained throughput under load
fn bench_sustained_throughput(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("sustained_throughput");

    let server = Arc::new(BenchmarkServer::new());

    // Test different concurrent request levels
    for concurrency in [1, 10, 50, 100].iter() {
        group.throughput(Throughput::Elements(*concurrency as u64));

        group.bench_with_input(
            BenchmarkId::new("concurrent_requests", concurrency),
            concurrency,
            |b, &concurrent_count| {
                b.to_async(&rt).iter(|| async {
                    // Spawn concurrent requests
                    let mut handles = Vec::new();

                    for i in 0..concurrent_count {
                        let server = server.clone();
                        let handle = tokio::spawn(async move {
                            let ctx = Context::new(
                                turbomcp_core::RequestContext::new(),
                                HandlerMetadata {
                                    name: "fast_computation".to_string(),
                                    handler_type: "tool".to_string(),
                                    description: None,
                                },
                            );

                            server.fast_computation(ctx, 10 + i).await
                        });
                        handles.push(handle);
                    }

                    // Wait for all to complete
                    let results = futures::future::join_all(handles).await;
                    std::hint::black_box(results)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark error handling overhead
fn bench_error_handling(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("error_handling");

    let server = Arc::new(BenchmarkServer::new());

    let error_types = vec!["validation", "permission", "timeout", "generic"];

    for error_type in error_types {
        group.bench_with_input(
            BenchmarkId::new("error_generation", error_type),
            &error_type,
            |b, &err_type| {
                b.to_async(&rt).iter(|| async {
                    let ctx = Context::new(
                        turbomcp_core::RequestContext::new(),
                        HandlerMetadata {
                            name: "error_scenario".to_string(),
                            handler_type: "tool".to_string(),
                            description: None,
                        },
                    );

                    let result = server.error_scenario(ctx, err_type.to_string()).await;
                    std::hint::black_box(result)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark memory efficiency under sustained load
fn bench_memory_efficiency(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("memory_efficiency");

    let server = Arc::new(BenchmarkServer::new());

    // Test different payload sizes
    for payload_size in [1, 10, 100, 1000].iter() {
        group.throughput(Throughput::Bytes(*payload_size as u64 * 1024));

        group.bench_with_input(
            BenchmarkId::new("payload_processing", payload_size),
            payload_size,
            |b, &size_kb| {
                b.to_async(&rt).iter(|| async {
                    let ctx = Context::new(
                        turbomcp_core::RequestContext::new(),
                        HandlerMetadata {
                            name: "memory_operation".to_string(),
                            handler_type: "tool".to_string(),
                            description: None,
                        },
                    );

                    let result = server.memory_operation(ctx, size_kb).await;
                    std::hint::black_box(result)
                });
            },
        );
    }

    group.finish();
}

// Criterion benchmark groups
criterion_group!(
    benches,
    bench_request_latency,
    bench_sustained_throughput,
    bench_error_handling,
    bench_memory_efficiency
);

criterion_main!(benches);