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

/// A tool input the size real servers' run to: thirty described fields.
#[derive(serde::Deserialize, schemars::JsonSchema)]
#[allow(dead_code)]
struct Wide {
    /// The first field, described the way real tools describe theirs.
    f00: Option<String>,
    /// A field. Its description is a sentence long, like most.
    f01: Option<String>,
    /// A field. Its description is a sentence long, like most.
    f02: Option<String>,
    /// A field. Its description is a sentence long, like most.
    f03: Option<String>,
    /// A field. Its description is a sentence long, like most.
    f04: Option<String>,
    /// A field. Its description is a sentence long, like most.
    f05: Option<String>,
    /// A field. Its description is a sentence long, like most.
    f06: Option<String>,
    /// A field. Its description is a sentence long, like most.
    f07: Option<String>,
    /// A field. Its description is a sentence long, like most.
    f08: Option<String>,
    /// A field. Its description is a sentence long, like most.
    f09: Option<String>,
    /// A field. Its description is a sentence long, like most.
    f10: Option<u32>,
    /// A field. Its description is a sentence long, like most.
    f11: Option<u32>,
    /// A field. Its description is a sentence long, like most.
    f12: Option<u32>,
    /// A field. Its description is a sentence long, like most.
    f13: Option<u32>,
    /// A field. Its description is a sentence long, like most.
    f14: Option<u32>,
    /// A field. Its description is a sentence long, like most.
    f15: Option<bool>,
    /// A field. Its description is a sentence long, like most.
    f16: Option<bool>,
    /// A field. Its description is a sentence long, like most.
    f17: Option<bool>,
    /// A field. Its description is a sentence long, like most.
    f18: Option<bool>,
    /// A field. Its description is a sentence long, like most.
    f19: Option<bool>,
    /// A field. Its description is a sentence long, like most.
    f20: Option<Vec<String>>,
    /// A field. Its description is a sentence long, like most.
    f21: Option<Vec<String>>,
    /// A field. Its description is a sentence long, like most.
    f22: Option<Vec<String>>,
    /// A field. Its description is a sentence long, like most.
    f23: Option<Vec<String>>,
    /// A field. Its description is a sentence long, like most.
    f24: Option<Vec<String>>,
    /// A field. Its description is a sentence long, like most.
    f25: Option<f64>,
    /// A field. Its description is a sentence long, like most.
    f26: Option<f64>,
    /// A field. Its description is a sentence long, like most.
    f27: Option<f64>,
    /// A field. Its description is a sentence long, like most.
    f28: Option<f64>,
    /// A field. Its description is a sentence long, like most.
    f29: Option<f64>,
    /// The one required field.
    id: String,
}

#[derive(Clone)]
struct BenchServer;

#[server(name = "bench", version = "1.0.0")]
impl BenchServer {
    /// Add two numbers.
    #[tool(description = "Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> String {
        format!("{}", a + b)
    }

    /// Take a wide input.
    #[tool]
    async fn wide(&self, Params(w): Params<Wide>) -> String {
        w.id
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

/// The same call from an authenticated caller: a bearer identity with a
/// realistic claim set (a few KB of JWT body), which every per-request
/// context clone used to copy.
fn bench_dispatch_authenticated(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let svc = BenchServer.into_server().build();
    let mut claims = serde_json::Map::new();
    claims.insert("iss".into(), json!("https://auth.example.com"));
    claims.insert("sub".into(), json!("user-1234"));
    claims.insert(
        "scope".into(),
        json!("tools:read tools:write resources:read"),
    );
    for i in 0..40 {
        claims.insert(
            format!("claim_{i}"),
            json!(format!("value-{i}-{}", "x".repeat(40))),
        );
    }
    let identity = turbomcp::Identity::Bearer {
        sub: "user-1234".into(),
        claims: claims.into(),
    };
    c.bench_function("dispatch/tools_call_authenticated", |b| {
        b.to_async(&rt).iter(|| {
            let mut svc = svc.clone();
            let identity = identity.clone();
            async move {
                let mut request = turbomcp::McpRequest::from(black_box(tools_call()));
                request.extensions.insert(identity);
                let resp = svc
                    .ready()
                    .await
                    .expect("ready")
                    .call(request)
                    .await
                    .expect("dispatch");
                black_box(resp);
            }
        });
    });
}

/// A call to a tool with a large input schema: what the per-call lookup,
/// clone and validation cost grows with.
fn bench_dispatch_wide(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().expect("runtime");
    let svc = BenchServer.into_server().build();
    let call = || {
        JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({
                "name": "wide",
                "arguments": { "id": "x", "f00": "a", "f10": 3, "f20": ["b"] },
                "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": {} },
            })),
        )
    };
    c.bench_function("dispatch/tools_call_wide_schema", |b| {
        b.to_async(&rt).iter(|| {
            let mut svc = svc.clone();
            async move {
                let resp = svc
                    .ready()
                    .await
                    .expect("ready")
                    .call(black_box(call()).into())
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

criterion_group!(
    benches,
    bench_dispatch,
    bench_dispatch_authenticated,
    bench_dispatch_wide,
    bench_codec
);
criterion_main!(benches);
