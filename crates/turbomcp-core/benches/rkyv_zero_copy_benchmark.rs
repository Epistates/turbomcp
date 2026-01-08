//! Benchmark for rkyv zero-copy serialization
//!
//! Run with:
//! ```bash
//! cargo bench -p turbomcp-core --features zero-copy
//! ```

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::hint::black_box;
use turbomcp_core::rkyv_types::{
    ArchivedInternalMessage, InternalId, InternalMessage, InternalResponse, RoutingHints,
};

/// Generate a sample tool call message
fn create_tool_call_message() -> InternalMessage {
    InternalMessage::new()
        .with_id(InternalId::Number(42))
        .with_method("tools/call")
        .with_params_json(
            br#"{"name":"calculator","arguments":{"operation":"add","a":123,"b":456}}"#,
        )
        .with_session_id("session-12345")
        .with_correlation_id("corr-67890")
}

/// Generate a sample list response
fn create_list_response() -> InternalResponse {
    let result = br#"{"tools":[{"name":"calculator","description":"Performs math operations"},{"name":"weather","description":"Gets weather info"},{"name":"search","description":"Searches the web"}]}"#;
    InternalResponse::success(InternalId::Number(1), result.to_vec())
        .with_correlation_id("corr-12345")
}

/// Benchmark rkyv serialization of InternalMessage
fn bench_rkyv_serialize_message(c: &mut Criterion) {
    let msg = create_tool_call_message();

    c.bench_function("rkyv_serialize_internal_message", |b| {
        b.iter(|| {
            let bytes = rkyv::to_bytes::<rancor::Error>(black_box(&msg)).expect("serialize failed");
            black_box(bytes)
        })
    });
}

/// Benchmark rkyv serialization of InternalResponse
fn bench_rkyv_serialize_response(c: &mut Criterion) {
    let resp = create_list_response();

    c.bench_function("rkyv_serialize_internal_response", |b| {
        b.iter(|| {
            let bytes =
                rkyv::to_bytes::<rancor::Error>(black_box(&resp)).expect("serialize failed");
            black_box(bytes)
        })
    });
}

/// Benchmark rkyv zero-copy access (no deserialization)
fn bench_rkyv_zero_copy_access(c: &mut Criterion) {
    let msg = create_tool_call_message();
    let bytes = rkyv::to_bytes::<rancor::Error>(&msg).expect("serialize failed");

    c.bench_function("rkyv_zero_copy_access", |b| {
        b.iter(|| {
            let archived =
                rkyv::access::<ArchivedInternalMessage, rancor::Error>(black_box(&bytes))
                    .expect("access failed");
            black_box((archived.method_str(), archived.is_request()))
        })
    });
}

/// Benchmark full rkyv deserialization
fn bench_rkyv_full_deserialize(c: &mut Criterion) {
    let msg = create_tool_call_message();
    let bytes = rkyv::to_bytes::<rancor::Error>(&msg).expect("serialize failed");

    c.bench_function("rkyv_full_deserialize", |b| {
        b.iter(|| {
            let archived =
                rkyv::access::<ArchivedInternalMessage, rancor::Error>(black_box(&bytes))
                    .expect("access failed");
            let deserialized: InternalMessage =
                rkyv::deserialize::<InternalMessage, rancor::Error>(archived)
                    .expect("deserialize failed");
            black_box(deserialized)
        })
    });
}

/// Benchmark various message sizes with rkyv
fn bench_message_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_sizes");

    // Small message (simple list request)
    let small_msg = InternalMessage::new()
        .with_id(InternalId::Number(1))
        .with_method("tools/list");

    // Medium message (tool call with params)
    let medium_msg = create_tool_call_message();

    // Large message (tool call with large params)
    let data_items: Vec<&str> = vec!["item"; 100];
    let transform_items: Vec<&str> = vec!["normalize"; 30];
    let large_params = serde_json::json!({
        "name": "data_processor",
        "arguments": {
            "data": data_items,
            "options": {
                "format": "json",
                "compress": true,
                "validate": true,
                "transform": transform_items
            }
        }
    });
    let large_msg = InternalMessage::new()
        .with_id(InternalId::Number(1))
        .with_method("tools/call")
        .with_params_raw(serde_json::to_vec(&large_params).unwrap());

    for (name, msg) in [
        ("small", &small_msg),
        ("medium", &medium_msg),
        ("large", &large_msg),
    ] {
        let bytes = rkyv::to_bytes::<rancor::Error>(msg).expect("serialize failed");

        group.bench_with_input(BenchmarkId::new("rkyv_serialize", name), msg, |b, msg| {
            b.iter(|| {
                let bytes = rkyv::to_bytes::<rancor::Error>(black_box(msg)).expect("serialize");
                black_box(bytes)
            })
        });

        group.bench_with_input(
            BenchmarkId::new("rkyv_zero_copy_access", name),
            &bytes,
            |b, bytes| {
                b.iter(|| {
                    let archived =
                        rkyv::access::<ArchivedInternalMessage, rancor::Error>(black_box(bytes))
                            .expect("access");
                    black_box(archived.method_str())
                })
            },
        );

        group.bench_with_input(
            BenchmarkId::new("rkyv_full_deserialize", name),
            &bytes,
            |b, bytes| {
                b.iter(|| {
                    let archived =
                        rkyv::access::<ArchivedInternalMessage, rancor::Error>(black_box(bytes))
                            .expect("access");
                    let deserialized: InternalMessage =
                        rkyv::deserialize::<InternalMessage, rancor::Error>(archived)
                            .expect("deserialize");
                    black_box(deserialized)
                })
            },
        );
    }

    group.finish();
}

/// Benchmark routing hints builder
fn bench_routing_hints(c: &mut Criterion) {
    c.bench_function("routing_hints_builder", |b| {
        b.iter(|| {
            let hints = RoutingHints::new()
                .with_tool_name("calculator")
                .with_resource_uri("file:///test.txt");
            black_box(hints)
        })
    });
}

/// Benchmark message builder
fn bench_message_builder(c: &mut Criterion) {
    c.bench_function("internal_message_builder", |b| {
        b.iter(|| {
            let msg = InternalMessage::new()
                .with_id(InternalId::Number(42))
                .with_method("tools/call")
                .with_params_json(br#"{"name":"test"}"#)
                .with_session_id("sess-123")
                .with_correlation_id("corr-456");
            black_box(msg)
        })
    });
}

criterion_group!(
    benches,
    bench_rkyv_serialize_message,
    bench_rkyv_serialize_response,
    bench_rkyv_zero_copy_access,
    bench_rkyv_full_deserialize,
    bench_message_sizes,
    bench_routing_hints,
    bench_message_builder,
);

criterion_main!(benches);
