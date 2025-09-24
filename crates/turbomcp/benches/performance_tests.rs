//! Performance benchmarks for TurboMCP framework
//!
//! Note: All benchmarks are conducted on consumer hardware (MacBook Pro M3, 32GB RAM)
//! and should be used for relative performance comparison rather than absolute metrics.
//! Your results may vary depending on hardware configuration.

use criterion::{Criterion, criterion_group, criterion_main};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use turbomcp::prelude::*;

/// Benchmark server for performance testing
#[derive(Debug)]
struct BenchmarkServer {
    counter: AtomicI32,
    state: std::sync::Arc<tokio::sync::RwLock<HashMap<String, String>>>,
}

impl BenchmarkServer {
    fn new() -> Self {
        Self {
            counter: AtomicI32::new(0),
            state: std::sync::Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    #[allow(dead_code)]
    async fn state_read(&self, key: String) -> McpResult<Option<String>> {
        let state = self.state.read().await;
        Ok(state.get(&key).cloned())
    }

    async fn state_write(&self, key: String, value: String) -> McpResult<()> {
        let mut state = self.state.write().await;
        state.insert(key, value);
        Ok(())
    }

    fn next_counter(&self) -> i32 {
        self.counter.fetch_add(1, Ordering::SeqCst)
    }
}

#[async_trait]
impl HandlerRegistration for BenchmarkServer {
    async fn register_with_builder(&self, _builder: &mut ServerBuilder) -> McpResult<()> {
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
        // Initialize with some test data
        for i in 0..100 {
            self.state_write(format!("key{i}"), format!("value{i}"))
                .await?;
        }
        Ok(())
    }
}

/// Benchmark simple synchronous operations
fn bench_sync_operations(c: &mut Criterion) {
    let server = BenchmarkServer::new();

    c.bench_function("counter_increment", |b| {
        b.iter(|| std::hint::black_box(server.next_counter()))
    });

    c.bench_function("server_creation", |b| {
        b.iter(|| std::hint::black_box(BenchmarkServer::new()))
    });
}

/// Benchmark helper functions performance
fn bench_helper_functions(c: &mut Criterion) {
    c.bench_function("text_helper", |b| {
        b.iter(|| std::hint::black_box(text("Test message")))
    });

    c.bench_function("error_text_helper", |b| {
        b.iter(|| std::hint::black_box(error_text("Error message")))
    });

    c.bench_function("tool_success_helper", |b| {
        b.iter(|| std::hint::black_box(tool_success(vec![text("Success")])))
    });

    c.bench_function("tool_error_helper", |b| {
        b.iter(|| std::hint::black_box(tool_error("Error occurred")))
    });
}

/// Benchmark context operations
fn bench_context_operations(c: &mut Criterion) {
    use turbomcp_core::RequestContext;

    c.bench_function("context_creation", |b| {
        b.iter(|| {
            let request_ctx = RequestContext::new();
            let handler_meta = HandlerMetadata {
                name: "bench_handler".to_string(),
                handler_type: "tool".to_string(),
                description: None,
            };

            std::hint::black_box(Context::new(request_ctx, handler_meta))
        })
    });
}

/// Benchmark actual macro-generated schema performance
#[cfg(feature = "schema-generation")]
fn bench_schema_generation(c: &mut Criterion) {
    use turbomcp::prelude::*;

    // Test actual macro-generated schema performance instead of schemars
    #[derive(Clone)]
    struct BenchServer;

    #[server]
    #[allow(dead_code)]
    impl BenchServer {
        #[tool("Benchmark tool for schema generation performance")]
        async fn benchmark_tool(
            &self,
            name: String,
            count: i32,
            #[allow(unused_variables)] active: bool,
        ) -> McpResult<String> {
            Ok(format!("Benchmark: {} ({})", name, count))
        }
    }

    c.bench_function("macro_schema_generation", |b| {
        b.iter(|| {
            let (_, _, schema) = BenchServer::benchmark_tool_metadata();
            std::hint::black_box(schema)
        })
    });
}

/// Benchmark URI template matching if enabled  
#[cfg(feature = "uri-templates")]
fn bench_uri_templates(c: &mut Criterion) {
    use turbomcp::uri::UriTemplate;

    let template = UriTemplate::new("api://v{version}/{service}/users/{id}").unwrap();
    let test_uri = "api://v1/auth/users/123";

    c.bench_function("uri_template_creation", |b| {
        b.iter(|| std::hint::black_box(UriTemplate::new("api://v{version}/{service}/users/{id}")))
    });

    c.bench_function("uri_template_matching", |b| {
        b.iter(|| std::hint::black_box(template.matches(test_uri)))
    });
}

// Group all benchmarks
#[cfg(all(feature = "schema-generation", feature = "uri-templates"))]
criterion_group!(
    benches,
    bench_sync_operations,
    bench_helper_functions,
    bench_context_operations,
    bench_schema_generation,
    bench_uri_templates
);

#[cfg(all(feature = "schema-generation", not(feature = "uri-templates")))]
criterion_group!(
    benches,
    bench_sync_operations,
    bench_helper_functions,
    bench_context_operations,
    bench_schema_generation
);

#[cfg(all(not(feature = "schema-generation"), feature = "uri-templates"))]
criterion_group!(
    benches,
    bench_sync_operations,
    bench_helper_functions,
    bench_context_operations,
    bench_uri_templates
);

#[cfg(not(any(feature = "schema-generation", feature = "uri-templates")))]
criterion_group!(
    benches,
    bench_sync_operations,
    bench_helper_functions,
    bench_context_operations
);

criterion_main!(benches);
