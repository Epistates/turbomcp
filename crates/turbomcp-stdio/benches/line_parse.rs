//! Benchmarks stdin line → `TransportMessage` parsing for the stdio transport.
//!
//! The reader task receives one owned `String` per newline-delimited message
//! and converts it into a `TransportMessage`. This measures that conversion
//! (JSON validation, id extraction, payload `Bytes` construction) across
//! payload sizes.
//!
//! Run with: `cargo bench -p turbomcp-stdio --features internal-bench --bench line_parse`

use std::hint::black_box;

use criterion::{BatchSize, BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use turbomcp_stdio::StdioTransport;

/// Build a JSON-RPC request line of roughly `size` bytes.
fn request_line(size: usize) -> String {
    let skeleton = r#"{"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"bench","arguments":{"data":""}}}"#;
    let filler = "z".repeat(size.saturating_sub(skeleton.len()).max(1));
    format!(
        r#"{{"jsonrpc":"2.0","id":42,"method":"tools/call","params":{{"name":"bench","arguments":{{"data":"{filler}"}}}}}}"#
    )
}

fn bench_line_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("stdio_parse_message");

    for size in [128usize, 1024, 8192, 65536] {
        let line = request_line(size);
        group.throughput(Throughput::Bytes(line.len() as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &line, |b, line| {
            b.iter_batched(
                || line.clone(),
                |line| {
                    black_box(
                        StdioTransport::bench_parse_message(line).expect("valid JSON-RPC line"),
                    )
                },
                BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

criterion_group!(benches, bench_line_parse);
criterion_main!(benches);
