//! Benchmarks for the server-side SSE hot path.
//!
//! Covers the per-message costs of the Streamable HTTP transport:
//! - `sse_event_bytes` — framing one payload into an SSE `id:`/`event:`/`data:` event
//! - `send_to_session` — routing one payload to a session's live subscriber
//! - `broadcast` — fanning one payload out across N sessions
//!
//! Run with: `cargo bench -p turbomcp-server --features http,internal-bench --bench sse_throughput`

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use tokio::runtime::Runtime;
use tokio::sync::Mutex;
use turbomcp_server::transport::http::{SessionManager, sse_event_bytes};

/// Build a single-line JSON-RPC-shaped payload of roughly `size` bytes.
fn payload(size: usize) -> String {
    let skeleton =
        r#"{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":""}}"#;
    let filler = "x".repeat(size.saturating_sub(skeleton.len()).max(1));
    format!(
        r#"{{"jsonrpc":"2.0","method":"notifications/message","params":{{"level":"info","data":"{filler}"}}}}"#
    )
}

fn rt() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .build()
        .expect("build tokio runtime")
}

fn bench_sse_event_bytes(c: &mut Criterion) {
    let mut group = c.benchmark_group("sse_event_bytes");
    let id = "0192aef3-bench-stream-42";

    for size in [64usize, 4096, 65536] {
        let data = payload(size);
        group.throughput(Throughput::Bytes(data.len() as u64));
        group.bench_with_input(BenchmarkId::new("single_line", size), &data, |b, data| {
            b.iter(|| {
                black_box(sse_event_bytes(
                    black_box(id),
                    Some("message"),
                    black_box(data),
                ))
            });
        });
    }

    // Multi-line data exercises the per-line `data: ` framing loop.
    let multiline = (0..64)
        .map(|i| format!("line {i}: {}", "y".repeat(56)))
        .collect::<Vec<_>>()
        .join("\n");
    group.throughput(Throughput::Bytes(multiline.len() as u64));
    group.bench_function("multi_line_64x64", |b| {
        b.iter(|| {
            black_box(sse_event_bytes(
                black_box(id),
                Some("message"),
                black_box(&multiline),
            ))
        });
    });

    group.finish();
}

fn bench_send_to_session(c: &mut Criterion) {
    let rt = rt();
    let mut group = c.benchmark_group("send_to_session");

    for size in [64usize, 4096, 65536] {
        let data = payload(size);
        let (manager, session_id, receiver) = rt.block_on(async {
            let manager = SessionManager::new();
            let session_id = manager.create_session(None).await;
            let receiver = manager
                .subscribe_session(&session_id)
                .await
                .expect("session exists");
            (manager, session_id, receiver)
        });
        let receiver = Arc::new(Mutex::new(receiver));

        group.throughput(Throughput::Bytes(data.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            b.to_async(&rt).iter(|| {
                let manager = manager.clone();
                let session_id = session_id.clone();
                let receiver = Arc::clone(&receiver);
                async move {
                    assert!(manager.send_to_session(&session_id, black_box(data)).await);
                    let message = receiver
                        .lock()
                        .await
                        .recv()
                        .await
                        .expect("subscriber receives routed message");
                    black_box(message.len());
                }
            });
        });
    }

    group.finish();
}

fn bench_broadcast(c: &mut Criterion) {
    let rt = rt();
    let mut group = c.benchmark_group("broadcast");
    let data = payload(4096);

    for sessions in [1usize, 8, 64] {
        let (manager, receivers) = rt.block_on(async {
            let manager = SessionManager::new();
            let mut receivers = Vec::with_capacity(sessions);
            for _ in 0..sessions {
                let session_id = manager.create_session(None).await;
                receivers.push(
                    manager
                        .subscribe_session(&session_id)
                        .await
                        .expect("session exists"),
                );
            }
            (manager, receivers)
        });
        let receivers = Arc::new(Mutex::new(receivers));

        group.throughput(Throughput::Bytes((data.len() * sessions) as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(sessions),
            &sessions,
            |b, _sessions| {
                b.to_async(&rt).iter(|| {
                    let manager = manager.clone();
                    let receivers = Arc::clone(&receivers);
                    let data = &data;
                    async move {
                        manager.broadcast(black_box(data)).await;
                        // Drain every subscriber so unbounded queues stay flat
                        // across iterations; drain cost is identical before and
                        // after the Arc<str> change.
                        let mut receivers = receivers.lock().await;
                        for receiver in receivers.iter_mut() {
                            let message = receiver
                                .recv()
                                .await
                                .expect("each session receives the broadcast");
                            black_box(message.len());
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_sse_event_bytes,
    bench_send_to_session,
    bench_broadcast
);
criterion_main!(benches);
