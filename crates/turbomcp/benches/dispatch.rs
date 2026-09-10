//! Self-baseline microbenchmarks for the v4 hot path — a regression harness and
//! the evidence behind "performance as a feature".
//!
//! - `dispatch/tools_call`: one `tools/call` driven through the built
//!   `VersionDispatcher` (the modern draft path): argument deserialization +
//!   schema-validated invocation + neutral→wire conversion + response assembly.
//!   This is the per-request server cost, minus transport framing.
//! - `codec/encode` + `codec/decode`: `SerdeJsonCodec` throughput on a
//!   representative `tools/call` frame — the framing cost the dispatch bench
//!   excludes.
//!
//! Run with `cargo bench -p turbomcp --bench dispatch`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use serde_json::json;
use tower::{Service, ServiceExt};
use turbomcp::prelude::*;
use turbomcp::{Codec, JsonRpcMessage, JsonRpcRequest, SerdeJsonCodec};

#[derive(Clone)]
struct BenchServer;

#[server(name = "bench", version = "1.0.0")]
impl BenchServer {
    /// Add two numbers.
    #[tool(description = "Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> String {
        format!("{}", a + b)
    }
}

/// A `tools/call` for `add`, stamped with the draft protocol version so it takes
/// the modern dispatch path (mirrors the macro server tests).
fn tools_call() -> JsonRpcRequest {
    JsonRpcRequest::new(
        1,
        "tools/call",
        Some(json!({
            "name": "add",
            "arguments": { "a": 2.0, "b": 40.0 },
            "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                "io.modelcontextprotocol/clientCapabilities": {} },
        })),
    )
}

fn bench_dispatch(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let svc = BenchServer.into_server().build();

    let fixture = rt.block_on(async {
        svc.clone()
            .oneshot(tools_call().into())
            .await
            .expect("fixture dispatch")
    });
    assert!(matches!(fixture, Some(JsonRpcMessage::Response(ref r))
        if r.error.is_none() && r.result.as_ref().is_some_and(|v| v["content"][0]["text"] == "42")));

    c.bench_function("dispatch/tools_call", |b| {
        b.to_async(&rt).iter(|| {
            let mut svc = svc.clone();
            async move {
                let resp = svc
                    .ready()
                    .await
                    .expect("ready")
                    .call(black_box(tools_call()).into())
                    .await
                    .expect("dispatch");
                black_box(resp);
            }
        });
    });
}

fn bench_codec(c: &mut Criterion) {
    let codec = SerdeJsonCodec;
    let frame: JsonRpcMessage = tools_call().into();
    let bytes = codec.encode(&frame).expect("encode");

    c.bench_function("codec/encode", |b| {
        b.iter(|| black_box(codec.encode(black_box(&frame)).expect("encode")));
    });
    c.bench_function("codec/decode", |b| {
        b.iter(|| {
            let msg: JsonRpcMessage = codec.decode(black_box(&bytes)).expect("decode");
            black_box(msg);
        });
    });
}

criterion_group!(benches, bench_dispatch, bench_codec);
criterion_main!(benches);
