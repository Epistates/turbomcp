//! Performance benchmarks for middleware layers

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use secrecy::Secret;
use turbomcp_server::middleware::*;

fn benchmark_rate_limit_config(c: &mut Criterion) {
    c.bench_function("rate_limit/create_strict", |b| {
        b.iter(|| black_box(RateLimitConfig::strict()))
    });

    c.bench_function("rate_limit/calculate_rate", |b| {
        let layer = RateLimitLayer::new(RateLimitConfig::new(120));
        b.iter(|| black_box(layer.requests_per_second()))
    });
}

fn benchmark_auth_config(c: &mut Criterion) {
    c.bench_function("auth/create_config", |b| {
        b.iter(|| black_box(AuthConfig::new(Secret::new("test".to_string()))))
    });

    c.bench_function("auth/claims_validation", |b| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let claims = Claims {
            sub: "user123".to_string(),
            roles: vec!["user".to_string()],
            exp: now + 3600,
            iat: now,
            iss: None,
            aud: None,
        };

        b.iter(|| {
            black_box(claims.is_expired());
            black_box(claims.has_role("user"));
        })
    });
}

fn benchmark_middleware_stack(c: &mut Criterion) {
    c.bench_function("stack/build_minimal", |b| {
        b.iter(|| {
            let stack = MiddlewareStack::new();
            black_box(stack.build::<()>())
        })
    });

    c.bench_function("stack/build_complete", |b| {
        b.iter(|| {
            let stack = MiddlewareStack::new()
                .with_auth(AuthConfig::new(Secret::new("secret".to_string())))
                .with_rate_limit(RateLimitConfig::strict())
                .with_timeout(TimeoutConfig::strict());
            black_box(stack.build::<()>())
        })
    });
}

criterion_group!(
    benches,
    benchmark_rate_limit_config,
    benchmark_auth_config,
    benchmark_middleware_stack
);
criterion_main!(benches);
