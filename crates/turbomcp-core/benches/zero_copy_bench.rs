//! Zero-copy performance benchmarks
//!
//! Demonstrates the performance improvements from zero-copy message processing.
//!
//! Run with:
//! ```bash
//! cargo bench --bench zero_copy_bench
//! ```

use bytes::{Bytes, BytesMut};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use serde_json::json;
use turbomcp_core::message::Message as OldMessage;
use turbomcp_core::zero_copy::{BufferPool, MessageBatch, MessageId, ZeroCopyMessage};

/// Benchmark message creation with zero-copy vs traditional
fn bench_message_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_creation");

    // Test different payload sizes
    for size in [100, 1_000, 10_000, 100_000].iter() {
        let payload = json!({
            "data": "x".repeat(*size),
            "timestamp": 1234567890,
            "metadata": {
                "version": "1.0.0",
                "type": "test"
            }
        });

        group.throughput(Throughput::Bytes(*size as u64));

        // Zero-copy implementation
        group.bench_with_input(BenchmarkId::new("zero_copy", size), size, |b, _| {
            b.iter(|| {
                let msg = ZeroCopyMessage::from_json(MessageId::from("test"), &payload).unwrap();
                black_box(msg);
            });
        });

        // Traditional implementation (if available)
        group.bench_with_input(BenchmarkId::new("traditional", size), size, |b, _| {
            b.iter(|| {
                let msg = OldMessage::json(turbomcp_core::MessageId::from("test"), payload.clone())
                    .unwrap();
                black_box(msg);
            });
        });
    }

    group.finish();
}

/// Benchmark message cloning (Arc vs deep copy)
fn bench_message_cloning(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_cloning");

    for size in [100, 1_000, 10_000].iter() {
        let payload = Bytes::from(vec![0u8; *size]);
        let zero_copy_msg = ZeroCopyMessage::from_bytes(MessageId::from("test"), payload.clone());

        group.throughput(Throughput::Bytes(*size as u64));

        // Zero-copy clone (Arc increment)
        group.bench_with_input(BenchmarkId::new("cheap_clone", size), size, |b, _| {
            b.iter(|| {
                let cloned = zero_copy_msg.cheap_clone();
                black_box(cloned);
            });
        });

        // Deep clone for comparison
        group.bench_with_input(BenchmarkId::new("deep_clone", size), size, |b, _| {
            b.iter(|| {
                let cloned = zero_copy_msg.clone();
                black_box(cloned);
            });
        });
    }

    group.finish();
}

/// Benchmark lazy JSON parsing
fn bench_lazy_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_parsing");

    for size in [10, 100, 1_000].iter() {
        let mut json_obj = json!({});
        for i in 0..*size {
            json_obj[format!("field_{}", i)] = json!(format!("value_{}", i));
        }

        let payload = Bytes::from(serde_json::to_vec(&json_obj).unwrap());

        group.throughput(Throughput::Elements(*size as u64));

        // Lazy parsing (first access only)
        group.bench_with_input(BenchmarkId::new("lazy_parse", size), size, |b, _| {
            b.iter(|| {
                let mut msg = ZeroCopyMessage::from_bytes(MessageId::from("test"), payload.clone());
                let raw = msg.parse_json_lazy().unwrap();
                black_box(raw);
            });
        });

        // Full deserialization for comparison
        group.bench_with_input(BenchmarkId::new("full_deserialize", size), size, |b, _| {
            b.iter(|| {
                let msg = ZeroCopyMessage::from_bytes(MessageId::from("test"), payload.clone());
                let value: serde_json::Value = msg.deserialize().unwrap();
                black_box(value);
            });
        });
    }

    group.finish();
}

/// Benchmark buffer pool performance
fn bench_buffer_pool(c: &mut Criterion) {
    let mut group = c.benchmark_group("buffer_pool");

    let pool = BufferPool::new(100, 4096);

    // Benchmark buffer acquisition from pool
    group.bench_function("pool_acquire", |b| {
        b.iter(|| {
            let buffer = pool.acquire();
            black_box(buffer);
            // Buffer is dropped and could be returned to pool
        });
    });

    // Benchmark buffer allocation without pool
    group.bench_function("direct_alloc", |b| {
        b.iter(|| {
            let buffer = BytesMut::with_capacity(4096);
            black_box(buffer);
        });
    });

    // Benchmark acquire-release cycle
    group.bench_function("acquire_release_cycle", |b| {
        b.iter(|| {
            let buffer = pool.acquire();
            pool.release(buffer);
        });
    });

    group.finish();
}

/// Benchmark message batching
fn bench_message_batching(c: &mut Criterion) {
    let mut group = c.benchmark_group("message_batching");

    let messages: Vec<(MessageId, Bytes)> = (0..100)
        .map(|i| {
            let id = MessageId::from(format!("msg_{}", i));
            let payload = Bytes::from(format!("payload_{}", i));
            (id, payload)
        })
        .collect();

    // Benchmark batch creation
    group.bench_function("batch_create", |b| {
        b.iter(|| {
            let mut batch = MessageBatch::new(100);
            for (id, payload) in &messages {
                batch.add(id.clone(), payload.clone());
            }
            black_box(batch);
        });
    });

    // Benchmark batch iteration
    let mut batch = MessageBatch::new(100);
    for (id, payload) in &messages {
        batch.add(id.clone(), payload.clone());
    }

    group.bench_function("batch_iterate", |b| {
        b.iter(|| {
            let mut count = 0;
            for (id, payload) in batch.iter() {
                black_box(id);
                black_box(payload);
                count += 1;
            }
            black_box(count);
        });
    });

    // Benchmark individual message access
    group.bench_function("batch_random_access", |b| {
        let mut rng = 0usize;
        b.iter(|| {
            rng = (rng * 1103515245 + 12345) % 100; // Simple LCG
            let msg = batch.get(rng);
            black_box(msg);
        });
    });

    group.finish();
}

/// Benchmark UTF-8 validation (fast path)
fn bench_utf8_validation(c: &mut Criterion) {
    use turbomcp_core::zero_copy::fast;

    let mut group = c.benchmark_group("utf8_validation");

    for size in [100, 1_000, 10_000].iter() {
        let valid_utf8 = vec![b'a'; *size];
        let invalid_utf8 = {
            let mut v = vec![b'a'; *size];
            v[*size / 2] = 0xFF; // Invalid UTF-8 byte
            v
        };

        group.throughput(Throughput::Bytes(*size as u64));

        // Valid UTF-8
        group.bench_with_input(BenchmarkId::new("valid", size), size, |b, _| {
            b.iter(|| {
                let is_valid = fast::validate_utf8_fast(&valid_utf8);
                black_box(is_valid);
            });
        });

        // Invalid UTF-8
        group.bench_with_input(BenchmarkId::new("invalid", size), size, |b, _| {
            b.iter(|| {
                let is_valid = fast::validate_utf8_fast(&invalid_utf8);
                black_box(is_valid);
            });
        });
    }

    group.finish();
}

/// Benchmark JSON boundary detection
fn bench_json_boundaries(c: &mut Criterion) {
    use turbomcp_core::zero_copy::fast;

    let mut group = c.benchmark_group("json_boundaries");

    // Create JSON with multiple objects
    let json_stream = r#"{"a":1}{"b":2}{"c":3}{"d":4}{"e":5}"#;
    let nested_json = r#"{"a":{"b":{"c":1}}}{"d":{"e":{"f":2}}}"#;

    group.bench_function("simple_objects", |b| {
        b.iter(|| {
            let boundaries = fast::find_json_boundaries(json_stream.as_bytes());
            black_box(boundaries);
        });
    });

    group.bench_function("nested_objects", |b| {
        b.iter(|| {
            let boundaries = fast::find_json_boundaries(nested_json.as_bytes());
            black_box(boundaries);
        });
    });

    group.finish();
}

// Criterion benchmark groups
criterion_group!(
    benches,
    bench_message_creation,
    bench_message_cloning,
    bench_lazy_parsing,
    bench_buffer_pool,
    bench_message_batching,
    bench_utf8_validation,
    bench_json_boundaries
);

criterion_main!(benches);
