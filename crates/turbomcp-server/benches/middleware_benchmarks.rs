//! Performance benchmarks for middleware layers
//!
//! To run these benchmarks, enable the required features:
//! ```bash
//! cargo bench --bench middleware_benchmarks --features "middleware,auth"
//! ```

use criterion::{criterion_group, criterion_main};

#[cfg(feature = "middleware")]
use criterion::{Criterion, black_box};

#[cfg(all(feature = "middleware", feature = "auth"))]
use secrecy::Secret;
#[cfg(all(feature = "middleware", feature = "auth"))]
use turbomcp_server::middleware::auth::{AuthConfig, Claims};
#[cfg(feature = "middleware")]
use turbomcp_server::middleware::rate_limit::{RateLimitConfig, RateLimitLayer};

#[cfg(feature = "middleware")]
fn benchmark_rate_limit_config(c: &mut Criterion) {
    c.bench_function("rate_limit/create_strict", |b| {
        b.iter(|| black_box(RateLimitConfig::strict()))
    });

    c.bench_function("rate_limit/calculate_rate", |b| {
        let layer = RateLimitLayer::new(RateLimitConfig::new(120));
        b.iter(|| black_box(layer.requests_per_second()))
    });
}

#[cfg(all(feature = "middleware", feature = "auth"))]
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

#[cfg(all(feature = "middleware", feature = "auth"))]
criterion_group!(benches, benchmark_rate_limit_config, benchmark_auth_config);

#[cfg(all(feature = "middleware", not(feature = "auth")))]
criterion_group!(benches, benchmark_rate_limit_config);

#[cfg(not(feature = "middleware"))]
fn no_benchmarks(_c: &mut criterion::Criterion) {
    // Middleware feature not enabled - run with: cargo bench --features middleware,auth
}

#[cfg(not(feature = "middleware"))]
criterion_group!(benches, no_benchmarks);

criterion_main!(benches);
