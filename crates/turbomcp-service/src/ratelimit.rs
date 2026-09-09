//! The HTTP rate-limiting seam.
//!
//! Like [auth](crate::HttpAuthenticator), rate limiting is **HTTP-transport-level**
//! in this design: the `429 Too Many Requests` + `Retry-After` response and the
//! per-source-IP fallback key both live at the HTTP boundary, and stdio (a
//! single trusted local connection) is never limited. So the transport holds a
//! limiter behind `Arc<dyn RateLimiter>` and charges one request against a
//! [`RateKey`] before dispatch.
//!
//! The key is identity-derived: an authenticated request is charged per
//! authenticated subject (so one principal can't exhaust another's budget),
//! and an anonymous request per source IP. The shipped [`GovernorRateLimiter`]
//! is an in-process [GCRA](https://en.wikipedia.org/wiki/Generic_cell_rate_algorithm)
//! limiter ([`governor`]); the trait is the seam a distributed (e.g. Redis)
//! limiter would implement for multi-instance deployments.

use std::net::IpAddr;
use std::num::NonZeroU32;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use governor::clock::{Clock, DefaultClock};
use governor::{DefaultKeyedRateLimiter, Quota};

/// The bucket a request is charged against.
///
/// Authenticated requests key on the subject; anonymous requests key on the
/// source IP; [`RateKey::Global`] is the fallback when neither is available
/// (e.g. a test harness with no peer address) — a single shared bucket.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum RateKey {
    /// Per authenticated subject (`Identity::Bearer`/`Dpop`/`Custom` subject).
    Subject(String),
    /// Per source IP (anonymous requests).
    Ip(IpAddr),
    /// One shared bucket — used when no subject and no peer address is known.
    Global,
}

/// A pluggable rate limiter (HTTP transport-level; mirrors
/// [`HttpAuthenticator`](crate::HttpAuthenticator)).
pub trait RateLimiter: Send + Sync {
    /// Charge one request against `key`. `Ok(())` admits it; `Err(retry_after)`
    /// rejects it, carrying the soonest a retry could succeed — the HTTP
    /// transport renders that as `429` + `Retry-After`.
    fn check(&self, key: &RateKey) -> Result<(), Duration>;
}

/// How often (in checks) [`GovernorRateLimiter`] opportunistically shrinks its
/// per-key state map. `governor` never evicts on its own, so without this the
/// map grows with every distinct subject/IP ever seen; `retain_recent` drops
/// keys whose buckets have fully replenished (i.e. idle clients).
const SHRINK_EVERY: usize = 1024;

/// An in-process [GCRA](governor) rate limiter, keyed by [`RateKey`].
///
/// Each key gets an independent token bucket: `quota` sets the steady-state
/// rate and the burst allowance. Memory is bounded by opportunistic
/// [`retain_recent`](DefaultKeyedRateLimiter::retain_recent) — idle keys are
/// reclaimed once their bucket refills.
pub struct GovernorRateLimiter {
    inner: DefaultKeyedRateLimiter<RateKey>,
    clock: DefaultClock,
    checks: AtomicUsize,
}

impl core::fmt::Debug for GovernorRateLimiter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GovernorRateLimiter")
            .field("tracked_keys", &self.inner.len())
            .field(
                "checks",
                &self.checks.load(core::sync::atomic::Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl GovernorRateLimiter {
    /// A limiter from an explicit [`Quota`] (full control over rate, burst, and
    /// replenish period).
    #[must_use]
    pub fn new(quota: Quota) -> Self {
        Self {
            inner: DefaultKeyedRateLimiter::keyed(quota),
            clock: DefaultClock::default(),
            checks: AtomicUsize::new(0),
        }
    }

    /// A limiter allowing `rate` requests per second per key, with a burst
    /// allowance equal to `rate` (the `governor` default for `per_second`).
    #[must_use]
    pub fn per_second(rate: NonZeroU32) -> Self {
        Self::new(Quota::per_second(rate))
    }

    /// A limiter allowing `rate` requests per second per key with a distinct
    /// `burst` ceiling (the maximum a key may spend in one instant).
    #[must_use]
    pub fn per_second_burst(rate: NonZeroU32, burst: NonZeroU32) -> Self {
        Self::new(Quota::per_second(rate).allow_burst(burst))
    }
}

impl RateLimiter for GovernorRateLimiter {
    fn check(&self, key: &RateKey) -> Result<(), Duration> {
        // Bound the keyed state map: `governor` doesn't evict, so periodically
        // reclaim keys whose buckets have refilled (idle clients).
        if (self.checks.fetch_add(1, Ordering::Relaxed)).is_multiple_of(SHRINK_EVERY) {
            self.inner.retain_recent();
        }
        match self.inner.check_key(key) {
            Ok(()) => Ok(()),
            Err(not_until) => Err(not_until.wait_time_from(self.clock.now())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nz(n: u32) -> NonZeroU32 {
        NonZeroU32::new(n).unwrap()
    }

    #[test]
    fn admits_within_budget_then_rejects() {
        // burst of 2: two immediate checks pass, the third is over budget.
        let limiter = GovernorRateLimiter::per_second_burst(nz(1), nz(2));
        let key = RateKey::Ip("127.0.0.1".parse().unwrap());
        assert!(limiter.check(&key).is_ok());
        assert!(limiter.check(&key).is_ok());
        let retry = limiter.check(&key).expect_err("third over budget");
        assert!(retry > Duration::ZERO);
    }

    #[test]
    fn keys_have_independent_buckets() {
        let limiter = GovernorRateLimiter::per_second_burst(nz(1), nz(1));
        let a = RateKey::Subject("alice".into());
        let b = RateKey::Subject("bob".into());
        assert!(limiter.check(&a).is_ok());
        // alice is now exhausted, but bob has his own bucket.
        assert!(limiter.check(&a).is_err());
        assert!(limiter.check(&b).is_ok());
    }
}
