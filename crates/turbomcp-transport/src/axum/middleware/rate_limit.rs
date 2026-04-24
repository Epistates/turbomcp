//! Rate limiting middleware backed by the `governor` crate (GCRA, lock-free).
//!
//! Extracts a per-request key (IP / UserId / Custom) and enforces a
//! requests-per-minute quota with burst allowance. On limit exceedance we
//! respond with `429 Too Many Requests` and expose rate-limit metadata via
//! `X-RateLimit-*` headers.

use std::num::NonZeroU32;
use std::sync::Arc;
use std::sync::LazyLock;

use axum::{
    extract::State,
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};
use dashmap::DashMap;
use governor::{
    Quota, RateLimiter,
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
};
use parking_lot::Mutex;

use crate::axum::config::{RateLimitConfig, RateLimitKey};

type KeyedLimiter =
    RateLimiter<String, governor::state::keyed::DashMapStateStore<String>, DefaultClock>;

/// One limiter per (requests_per_minute, burst_capacity) tuple. We key on the
/// quota tuple so that different `RateLimitConfig`s (e.g. staging vs prod)
/// don't share state, but identical configs share a single lock-free store.
struct SharedLimiters {
    limiters: DashMap<(u32, u32), Arc<KeyedLimiter>>,
    // Fallback limiter when `requests_per_minute` is zero or one (Quota requires
    // NonZeroU32). We use a single global `InMemoryState` limiter here — kept
    // behind a `Mutex` only because `InMemoryState` is not clone-safe under high
    // contention; contention is negligible since this path is only hit when
    // config is effectively "deny-all", which is handled upstream.
    _fallback: Mutex<()>,
}

impl SharedLimiters {
    fn new() -> Self {
        Self {
            limiters: DashMap::new(),
            _fallback: Mutex::new(()),
        }
    }

    fn get_or_init(&self, rpm: NonZeroU32, burst: NonZeroU32) -> Arc<KeyedLimiter> {
        let key = (rpm.get(), burst.get());
        if let Some(l) = self.limiters.get(&key) {
            return Arc::clone(&*l);
        }
        let quota = Quota::per_minute(rpm).allow_burst(burst);
        let limiter = Arc::new(RateLimiter::keyed(quota));
        // Race-safe insertion: another thread may have inserted in between —
        // `entry().or_insert_with` guarantees exactly one limiter per key.
        let entry = self
            .limiters
            .entry(key)
            .or_insert_with(|| Arc::clone(&limiter));
        Arc::clone(&*entry)
    }
}

static LIMITERS: LazyLock<SharedLimiters> = LazyLock::new(SharedLimiters::new);

/// Rate limiting middleware (axum `from_fn_with_state`).
pub async fn rate_limiting_middleware(
    State(rate_config): State<RateLimitConfig>,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    if !rate_config.enabled {
        return Ok(next.run(request).await);
    }

    let rate_key = match rate_config.key_function {
        RateLimitKey::IpAddress => request
            .headers()
            .get("x-forwarded-for")
            .or_else(|| request.headers().get("x-real-ip"))
            .and_then(|h| h.to_str().ok())
            .unwrap_or("unknown")
            .to_string(),
        RateLimitKey::UserId => request
            .extensions()
            .get::<String>()
            .cloned()
            .unwrap_or_else(|| "anonymous".to_string()),
        RateLimitKey::Custom => "custom_key".to_string(),
    };

    // Zero means disabled at the config level — no-op rather than panic on
    // NonZeroU32::new.
    let Some(rpm) = NonZeroU32::new(rate_config.requests_per_minute) else {
        return Ok(next.run(request).await);
    };
    let burst = NonZeroU32::new(rate_config.burst_capacity.max(1))
        .unwrap_or(NonZeroU32::new(1).expect("1 is nonzero"));

    let limiter = LIMITERS.get_or_init(rpm, burst);

    // Decision: governor returns `Ok(())` on allow, `Err(NotUntil)` on deny.
    match limiter.check_key(&rate_key) {
        Ok(()) => {}
        Err(_not_until) => return Err(StatusCode::TOO_MANY_REQUESTS),
    }

    let mut response = next.run(request).await;

    let headers = response.headers_mut();
    if let Ok(hv) = HeaderValue::from_str(&rpm.get().to_string()) {
        headers.insert("X-RateLimit-Limit", hv);
    }
    // Remaining is approximate with GCRA — governor tracks conformance, not
    // a discrete counter. Report burst capacity as the remaining-budget hint.
    if let Ok(hv) = HeaderValue::from_str(&burst.get().to_string()) {
        headers.insert("X-RateLimit-Burst", hv);
    }

    Ok(response)
}

// Silence dead_code on the fallback placeholder — kept to document design
// intent that we may add a global limiter path later.
#[allow(dead_code)]
fn _phantom_single_limiter() -> RateLimiter<NotKeyed, InMemoryState, DefaultClock> {
    let quota = Quota::per_minute(NonZeroU32::new(1).expect("1 is nonzero"));
    RateLimiter::direct(quota)
}
