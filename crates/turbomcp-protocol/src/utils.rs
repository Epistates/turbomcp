//! Utility functions and helper macros.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use pin_project_lite::pin_project;
use tokio::time::{Sleep, sleep};

pin_project! {
    /// Timeout wrapper for futures
    pub struct Timeout<F> {
        #[pin]
        future: F,
        #[pin]
        delay: Sleep,
    }
}

impl<F> Timeout<F> {
    /// Create a new timeout wrapper
    pub fn new(future: F, duration: Duration) -> Self {
        Self {
            future,
            delay: sleep(duration),
        }
    }
}

impl<F> Future for Timeout<F>
where
    F: Future,
{
    type Output = Result<F::Output, TimeoutError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();

        // First check if the future is ready
        if let Poll::Ready(output) = this.future.poll(cx) {
            return Poll::Ready(Ok(output));
        }

        // Then check if the timeout has expired
        match this.delay.poll(cx) {
            Poll::Ready(()) => Poll::Ready(Err(TimeoutError)),
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Timeout error
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutError;

impl std::fmt::Display for TimeoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Operation timed out")
    }
}

impl std::error::Error for TimeoutError {}

/// Utility function to create a timeout future
pub fn timeout<F>(duration: Duration, future: F) -> Timeout<F>
where
    F: Future,
{
    Timeout::new(future, duration)
}

/// Retry configuration
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of attempts
    pub max_attempts: usize,
    /// Base delay between attempts
    pub base_delay: Duration,
    /// Maximum delay between attempts
    pub max_delay: Duration,
    /// Backoff multiplier
    pub backoff_multiplier: f64,
    /// Whether to add jitter
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

impl RetryConfig {
    /// Create a new retry configuration
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum attempts
    #[must_use]
    pub const fn with_max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts = max_attempts;
        self
    }

    /// Set base delay
    #[must_use]
    pub const fn with_base_delay(mut self, delay: Duration) -> Self {
        self.base_delay = delay;
        self
    }

    /// Set maximum delay
    #[must_use]
    pub const fn with_max_delay(mut self, delay: Duration) -> Self {
        self.max_delay = delay;
        self
    }

    /// Set backoff multiplier
    #[must_use]
    pub const fn with_backoff_multiplier(mut self, multiplier: f64) -> Self {
        self.backoff_multiplier = multiplier;
        self
    }

    /// Enable or disable jitter
    #[must_use]
    pub const fn with_jitter(mut self, jitter: bool) -> Self {
        self.jitter = jitter;
        self
    }

    /// Calculate delay for the given attempt number
    #[must_use]
    pub fn delay_for_attempt(&self, attempt: usize) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }

        let base_delay_ms = self.base_delay.as_millis() as f64;
        let multiplier = self.backoff_multiplier.powi((attempt - 1) as i32);
        let delay_ms = base_delay_ms * multiplier;

        let delay = Duration::from_millis(delay_ms as u64).min(self.max_delay);

        if self.jitter {
            let jitter_factor = (rand::random::<f64>() - 0.5).mul_add(0.1, 1.0); // ±5% jitter
            let jittered_delay = delay.mul_f64(jitter_factor);
            jittered_delay.min(self.max_delay)
        } else {
            delay
        }
    }
}

/// Retry a future with exponential backoff
///
/// # Panics
///
/// Panics if no retry attempts are made and no error is captured
pub async fn retry_with_backoff<F, Fut, T, E>(
    mut operation: F,
    config: RetryConfig,
    should_retry: impl Fn(&E) -> bool,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, E>>,
{
    let mut last_error = None;

    for attempt in 0..config.max_attempts {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(error) => {
                if !should_retry(&error) || attempt + 1 >= config.max_attempts {
                    return Err(error);
                }

                let delay = config.delay_for_attempt(attempt + 1);
                sleep(delay).await;
                last_error = Some(error);
            }
        }
    }

    // This should never happen since we always set last_error before breaking
    // But if it does, we need to return some error. Use expect to catch this bug.
    Err(last_error.expect("Retry loop ended without attempts - this is a bug in retry logic"))
}

/// Circuit breaker state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Circuit is closed (normal operation)
    Closed,
    /// Circuit is open (failing fast)
    Open,
    /// Circuit is half-open (testing recovery)
    HalfOpen,
}

/// Simple circuit breaker implementation
#[derive(Debug)]
pub struct CircuitBreaker {
    state: parking_lot::Mutex<CircuitBreakerState>,
    failure_threshold: usize,
    recovery_timeout: Duration,
    success_threshold: usize,
}

#[derive(Debug)]
struct CircuitBreakerState {
    state: CircuitState,
    failure_count: usize,
    success_count: usize,
    last_failure_time: Option<std::time::Instant>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    #[must_use]
    pub const fn new(failure_threshold: usize, recovery_timeout: Duration) -> Self {
        Self {
            state: parking_lot::Mutex::new(CircuitBreakerState {
                state: CircuitState::Closed,
                failure_count: 0,
                success_count: 0,
                last_failure_time: None,
            }),
            failure_threshold,
            recovery_timeout,
            success_threshold: 3,
        }
    }

    /// Execute an operation through the circuit breaker
    pub async fn call<F, Fut, T, E>(&self, operation: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        // Check if circuit is open
        if self.is_open() {
            return Err(CircuitBreakerError::Open);
        }

        // Execute the operation
        match operation().await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(error) => {
                self.record_failure();
                Err(CircuitBreakerError::Operation(error))
            }
        }
    }

    /// Get current circuit state
    pub fn state(&self) -> CircuitState {
        self.state.lock().state
    }

    fn is_open(&self) -> bool {
        let mut state = self.state.lock();

        match state.state {
            CircuitState::Open => {
                // Check if recovery timeout has passed
                state.last_failure_time.is_none_or(|last_failure| {
                    if last_failure.elapsed() >= self.recovery_timeout {
                        state.state = CircuitState::HalfOpen;
                        state.success_count = 0;
                        false
                    } else {
                        true
                    }
                })
            }
            _ => false,
        }
    }

    fn record_success(&self) {
        let mut state = self.state.lock();

        match state.state {
            CircuitState::Closed => {
                state.failure_count = 0;
            }
            CircuitState::HalfOpen => {
                state.success_count += 1;
                if state.success_count >= self.success_threshold {
                    state.state = CircuitState::Closed;
                    state.failure_count = 0;
                    state.success_count = 0;
                }
            }
            CircuitState::Open => {
                // Should not reach here
            }
        }
    }

    fn record_failure(&self) {
        let mut state = self.state.lock();

        state.failure_count += 1;
        state.last_failure_time = Some(std::time::Instant::now());

        match state.state {
            CircuitState::Closed => {
                if state.failure_count >= self.failure_threshold {
                    state.state = CircuitState::Open;
                }
            }
            CircuitState::HalfOpen => {
                state.state = CircuitState::Open;
                state.success_count = 0;
            }
            CircuitState::Open => {
                // Already open
            }
        }
    }
}

/// Circuit breaker error
#[derive(Debug)]
pub enum CircuitBreakerError<E> {
    /// Circuit is open
    Open,
    /// Operation failed
    Operation(E),
}

impl<E> std::fmt::Display for CircuitBreakerError<E>
where
    E: std::fmt::Display,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "Circuit breaker is open"),
            Self::Operation(e) => write!(f, "Operation failed: {e}"),
        }
    }
}

impl<E> std::error::Error for CircuitBreakerError<E>
where
    E: std::error::Error + 'static,
{
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Open => None,
            Self::Operation(e) => Some(e),
        }
    }
}

/// Utility macro for measuring execution time
#[macro_export]
macro_rules! measure_time {
    ($name:expr, $block:block) => {{
        let _start = std::time::Instant::now();
        let result = $block;
        let _elapsed = _start.elapsed();

        #[cfg(feature = "tracing")]
        tracing::debug!("{} took {:?}", $name, _elapsed);

        result
    }};
}

/// Utility macro for conditional compilation based on features
#[macro_export]
macro_rules! feature_gate {
    ($feature:expr, $block:block) => {
        #[cfg(feature = $feature)]
        $block
    };
    ($feature:expr, $if_block:block, $else_block:block) => {
        #[cfg(feature = $feature)]
        $if_block
        #[cfg(not(feature = $feature))]
        $else_block
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_timeout() {
        // Test successful operation within timeout
        let result = timeout(Duration::from_millis(100), async { 42 }).await;
        assert_eq!(result.unwrap(), 42);

        // Test timeout
        let result = timeout(Duration::from_millis(10), async {
            sleep(Duration::from_millis(50)).await;
            42
        })
        .await;
        assert!(result.is_err());
    }

    #[test]
    fn test_retry_config() {
        let config = RetryConfig::new()
            .with_max_attempts(5)
            .with_base_delay(Duration::from_millis(50))
            .with_jitter(false);

        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.base_delay, Duration::from_millis(50));
        assert!(!config.jitter);

        // Test delay calculation
        assert_eq!(config.delay_for_attempt(0), Duration::ZERO);
        assert_eq!(config.delay_for_attempt(1), Duration::from_millis(50));
        assert_eq!(config.delay_for_attempt(2), Duration::from_millis(100));
    }

    #[tokio::test]
    async fn test_retry_with_backoff() {
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = counter.clone();

        let config = RetryConfig::new()
            .with_max_attempts(3)
            .with_base_delay(Duration::from_millis(1))
            .with_jitter(false);

        let result = retry_with_backoff(
            move || {
                let count = counter_clone.fetch_add(1, Ordering::SeqCst);
                async move {
                    if count < 2 {
                        Err("fail")
                    } else {
                        Ok("success")
                    }
                }
            },
            config,
            |_| true,
        )
        .await;

        assert_eq!(result.unwrap(), "success");
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_circuit_breaker() {
        let cb = CircuitBreaker::new(2, Duration::from_millis(10));
        let counter = Arc::new(AtomicU32::new(0));

        // First failure
        let result = cb
            .call({
                let counter = counter.clone();
                move || async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err::<(), _>("error")
                }
            })
            .await;
        assert!(matches!(result, Err(CircuitBreakerError::Operation(_))));
        assert_eq!(cb.state(), CircuitState::Closed);

        // Second failure - should open circuit
        let result = cb
            .call({
                let counter = counter.clone();
                move || async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err::<(), _>("error")
                }
            })
            .await;
        assert!(matches!(result, Err(CircuitBreakerError::Operation(_))));
        assert_eq!(cb.state(), CircuitState::Open);

        // Third attempt - should fail fast
        let result: Result<(), CircuitBreakerError<&str>> = cb
            .call({
                let counter = counter.clone();
                move || async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            })
            .await;
        assert!(matches!(result, Err(CircuitBreakerError::Open)));

        // Counter should only be 2 (third attempt was blocked)
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_measure_time_macro() {
        let result = measure_time!("test_operation", {
            std::thread::sleep(Duration::from_millis(1));
            42
        });
        assert_eq!(result, 42);
    }
}
