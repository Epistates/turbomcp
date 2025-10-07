//! Performance benchmark tests for TurboMCP

use bytes::Bytes;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use serde_json::json;
use std::sync::Arc;
use tokio::runtime::Runtime;
use turbomcp_protocol::{Error, ErrorKind, MessageId, StateManager};
use turbomcp_transport::core::{
    TransportConfig, TransportMessage, TransportMessageMetadata, TransportType,
};
use turbomcp_transport::stdio::StdioTransport;

fn benchmark_state_operations(c: &mut Criterion) {
    let _rt = Runtime::new().unwrap();

    c.bench_function("state_set_single", |b| {
        let state = StateManager::new();
        b.iter(|| {
            state.set(
                format!("key_{}", fastrand::usize(0..1000)),
                json!({"value": fastrand::u32(..)}),
            );
        });
    });

    c.bench_function("state_get_single", |b| {
        let state = StateManager::new();
        // Pre-populate with test data
        for i in 0..1000 {
            state.set(format!("key_{}", i), json!({"value": i}));
        }

        b.iter(|| {
            let key = format!("key_{}", fastrand::usize(0..1000));
            let _ = state.get(&key);
        });
    });

    c.bench_function("state_concurrent_operations", |b| {
        b.iter(|| {
            let _rt = Runtime::new().unwrap();
            let state = Arc::new(StateManager::new());

            // Simplified synchronous version
            for i in 0..10 {
                for j in 0..10 {
                    let key = format!("thread_{}_key_{}", i, j);
                    state.set(key, json!(j));
                }
            }
        });
    });

    let mut group = c.benchmark_group("state_size_scaling");
    for size in [100, 1000, 10000].iter() {
        group.bench_with_input(BenchmarkId::new("operations", size), size, |b, &size| {
            let state = StateManager::new();

            // Pre-populate
            for i in 0..size {
                state.set(
                    format!("key_{}", i),
                    json!({"value": i, "data": "x".repeat(100)}),
                );
            }

            b.iter(|| {
                // Mixed operations
                let key = format!("key_{}", fastrand::usize(0..size));
                match fastrand::u32(0..3) {
                    0 => {
                        state.get(&key);
                    }
                    1 => {
                        state.set(key, json!({"updated": true}));
                    }
                    2 => {
                        state.contains(&key);
                    }
                    _ => unreachable!(),
                }
            });
        });
    }
    group.finish();
}

fn benchmark_error_handling(c: &mut Criterion) {
    c.bench_function("error_creation", |b| {
        b.iter(|| {
            let _err = Error::new(ErrorKind::Transport, "Connection failed");
        });
    });

    c.bench_function("error_with_context", |b| {
        b.iter(|| {
            let _err = Error::new(ErrorKind::Transport, "Connection failed")
                .with_context("operation", "connect")
                .with_context("retry_count", 3);
        });
    });

    c.bench_function("error_conversion", |b| {
        b.iter(|| {
            let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
            let _mcp_err: Error = io_err.into();
        });
    });
}

fn benchmark_message_operations(c: &mut Criterion) {
    c.bench_function("message_id_generation", |b| {
        b.iter(|| {
            let _id = MessageId::from(fastrand::i64(..) as i64);
        });
    });

    c.bench_function("message_serialization", |b| {
        let message = json!({
            "jsonrpc": "2.0",
            "id": "test-123",
            "method": "test_method",
            "params": {
                "key": "value",
                "number": 42,
                "array": [1, 2, 3, 4, 5]
            }
        });

        b.iter(|| {
            let _serialized = serde_json::to_string(&message).unwrap();
        });
    });

    let mut group = c.benchmark_group("message_size_scaling");
    for payload_size in [1024, 10240, 102400].iter() {
        // 1KB, 10KB, 100KB
        group.bench_with_input(
            BenchmarkId::new("transport_message", payload_size),
            payload_size,
            |b, &size| {
                let payload = Bytes::from("x".repeat(size));
                let metadata = TransportMessageMetadata::default();

                b.iter(|| {
                    let _msg = TransportMessage::with_metadata(
                        MessageId::from(fastrand::i64(..) as i64),
                        payload.clone(),
                        metadata.clone(),
                    );
                });
            },
        );
    }
    group.finish();
}

fn benchmark_transport_operations(c: &mut Criterion) {
    let _rt = Runtime::new().unwrap();

    c.bench_function("transport_creation", |b| {
        b.iter(|| {
            let _transport = StdioTransport::new();
        });
    });

    c.bench_function("transport_config", |b| {
        b.iter(|| {
            let _transport = StdioTransport::new();
            let config = TransportConfig {
                transport_type: TransportType::Stdio,
                ..Default::default()
            };
            // Simplified synchronous version - just create the config
            let _ = config;
        });
    });

    c.bench_function("transport_state_check", |b| {
        b.iter(|| {
            let transport = StdioTransport::new();
            // Simplified - just create transport
            let _ = transport;
        });
    });
}

fn benchmark_json_operations(c: &mut Criterion) {
    c.bench_function("json_parse_simple", |b| {
        let json_str = r#"{"key": "value", "number": 42}"#;
        b.iter(|| {
            let _: serde_json::Value = serde_json::from_str(json_str).unwrap();
        });
    });

    c.bench_function("json_parse_complex", |b| {
        let json_str = r#"{
            "jsonrpc": "2.0",
            "id": "test-123",
            "method": "complex_method",
            "params": {
                "nested": {
                    "deep": {
                        "value": "test"
                    }
                },
                "array": [1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
                "metadata": {
                    "timestamp": "2024-01-01T00:00:00Z",
                    "version": "1.0.0",
                    "tags": ["test", "benchmark", "performance"]
                }
            }
        }"#;

        b.iter(|| {
            let _: serde_json::Value = serde_json::from_str(json_str).unwrap();
        });
    });

    let mut group = c.benchmark_group("json_size_scaling");
    for array_size in [10, 100, 1000].iter() {
        group.bench_with_input(
            BenchmarkId::new("parse_array", array_size),
            array_size,
            |b, &size| {
                let large_array: Vec<i32> = (0..size).collect();
                let json_str = serde_json::to_string(&json!({
                    "data": large_array,
                    "metadata": {"size": size}
                }))
                .unwrap();

                b.iter(|| {
                    let _: serde_json::Value = serde_json::from_str(&json_str).unwrap();
                });
            },
        );
    }
    group.finish();
}

fn benchmark_memory_operations(c: &mut Criterion) {
    c.bench_function("state_export_import", |b| {
        let state = StateManager::new();

        // Pre-populate with various data types
        for i in 0..100 {
            state.set(
                format!("key_{}", i),
                json!({
                    "id": i,
                    "name": format!("item_{}", i),
                    "data": "x".repeat(100),
                    "nested": {
                        "value": i * 2,
                        "flag": i % 2 == 0
                    }
                }),
            );
        }

        b.iter(|| {
            let exported = state.export();
            let new_state = StateManager::new();
            let _ = new_state.import(exported).unwrap();
        });
    });

    c.bench_function("large_state_clear", |b| {
        b.iter(|| {
            let state = StateManager::new();

            // Create large state
            for i in 0..1000 {
                state.set(
                    format!("key_{}", i),
                    json!({
                        "data": "x".repeat(1000),
                        "index": i
                    }),
                );
            }

            // Clear it
            state.clear();
        });
    });
}

criterion_group!(
    benches,
    benchmark_state_operations,
    benchmark_error_handling,
    benchmark_message_operations,
    benchmark_transport_operations,
    benchmark_json_operations,
    benchmark_memory_operations
);

criterion_main!(benches);
