//! Benchmarks the per-message metrics record path of the HTTP client transport.
//!
//! Every message sent or received bumps two counters in `TransportMetrics`.
//! The cost under measurement is the synchronization guarding those counters;
//! the win from lock-free counters is contention-dependent, so the bench
//! sweeps the number of concurrently recording tasks (1/4/16).
//!
//! Run with: `cargo bench -p turbomcp-http --bench metrics_recording`

use std::hint::black_box;
use std::sync::Arc;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use turbomcp_http::{StreamableHttpClientConfig, StreamableHttpClientTransport};

const CALLS_PER_TASK: u64 = 1_000;

fn bench_metrics_recording(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(8)
        .enable_all()
        .build()
        .expect("build tokio runtime");

    let transport = Arc::new(
        StreamableHttpClientTransport::new(StreamableHttpClientConfig::default())
            .expect("default config builds"),
    );

    let mut group = c.benchmark_group("metrics_record_received");
    for tasks in [1usize, 4, 16] {
        group.throughput(Throughput::Elements(tasks as u64 * CALLS_PER_TASK));
        group.bench_with_input(BenchmarkId::from_parameter(tasks), &tasks, |b, &tasks| {
            b.to_async(&rt).iter(|| {
                let transport = Arc::clone(&transport);
                async move {
                    let handles: Vec<_> = (0..tasks)
                        .map(|_| {
                            let transport = Arc::clone(&transport);
                            tokio::spawn(async move {
                                for _ in 0..CALLS_PER_TASK {
                                    transport.record_message_received(black_box(256)).await;
                                }
                            })
                        })
                        .collect();
                    for handle in handles {
                        handle.await.expect("recording task completes");
                    }
                }
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_metrics_recording);
criterion_main!(benches);
