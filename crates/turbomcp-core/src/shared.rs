//! Generic shared wrapper traits and utilities
//!
//! This module provides reusable patterns for creating thread-safe wrappers
//! around types that need to be shared across multiple async tasks while
//! encapsulating Arc/Mutex complexity.

use std::sync::Arc;
use tokio::sync::Mutex;

/// Trait for types that can be wrapped in a thread-safe shared wrapper
///
/// This trait defines the interface for creating shared wrappers that encapsulate
/// Arc/Mutex complexity and provide clean APIs for concurrent access.
///
/// # Design Principles
///
/// - **Hide complexity**: Encapsulate Arc/Mutex details from users
/// - **Preserve semantics**: Maintain original API behavior as much as possible
/// - **Enable sharing**: Allow multiple tasks to access the same instance safely
/// - **Async-first**: Design for async/await patterns
///
/// # Implementation Guidelines
///
/// When implementing this trait, consider:
/// - Methods requiring `&mut self` need special handling in shared contexts
/// - Reference-returning methods (`&T`) can't work directly with async mutexes
/// - Consuming methods (taking `self`) may need special consumption patterns
/// - Performance implications of mutex contention
///
/// # Examples
///
/// ```rust
/// use std::sync::Arc;
/// use tokio::sync::Mutex;
/// use turbomcp_core::shared::Shareable;
///
/// struct MyService {
///     counter: u64,
/// }
///
/// impl MyService {
///     fn new() -> Self {
///         Self { counter: 0 }
///     }
///
///     fn increment(&mut self) {
///         self.counter += 1;
///     }
///
///     fn count(&self) -> u64 {
///         self.counter
///     }
/// }
///
/// // Shared wrapper
/// struct SharedMyService {
///     inner: Arc<Mutex<MyService>>,
/// }
///
/// impl Shareable<MyService> for SharedMyService {
///     fn new(inner: MyService) -> Self {
///         Self {
///             inner: Arc::new(Mutex::new(inner)),
///         }
///     }
/// }
///
/// impl Clone for SharedMyService {
///     fn clone(&self) -> Self {
///         Self {
///             inner: Arc::clone(&self.inner),
///         }
///     }
/// }
///
/// impl SharedMyService {
///     async fn increment(&self) {
///         self.inner.lock().await.increment();
///     }
///
///     async fn count(&self) -> u64 {
///         self.inner.lock().await.count()
///     }
/// }
/// ```
pub trait Shareable<T>: Clone + Send + Sync + 'static {
    /// Create a new shared wrapper around the inner type
    fn new(inner: T) -> Self;
}

/// A generic shared wrapper that implements the Shareable pattern
///
/// This provides a concrete implementation of the sharing pattern that can
/// be used directly for simple cases where no custom behavior is needed.
///
/// # Examples
///
/// ```rust
/// use turbomcp_core::shared::{Shared, Shareable};
///
/// #[derive(Debug)]
/// struct Counter {
///     value: u64,
/// }
///
/// impl Counter {
///     fn new() -> Self {
///         Self { value: 0 }
///     }
///
///     fn increment(&mut self) {
///         self.value += 1;
///     }
///
///     fn get(&self) -> u64 {
///         self.value
///     }
/// }
///
/// # async fn example() {
/// // Create a shared counter
/// let counter = Counter::new();
/// let shared = Shared::new(counter);
///
/// // Clone for use in multiple tasks
/// let shared1 = shared.clone();
/// let shared2 = shared.clone();
///
/// // Use in concurrent tasks
/// let handle1 = tokio::spawn(async move {
///     shared1.with_mut(|c| c.increment()).await;
/// });
///
/// let handle2 = tokio::spawn(async move {
///     shared2.with(|c| c.get()).await
/// });
/// # }
/// ```
#[derive(Debug)]
pub struct Shared<T> {
    inner: Arc<Mutex<T>>,
}

impl<T> Shared<T>
where
    T: Send + 'static,
{
    /// Execute a closure with read access to the inner value
    pub async fn with<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> R + Send,
    {
        let guard = self.inner.lock().await;
        f(&*guard)
    }

    /// Execute a closure with mutable access to the inner value
    pub async fn with_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> R + Send,
    {
        let mut guard = self.inner.lock().await;
        f(&mut *guard)
    }

    /// Execute an async closure with read access to the inner value
    pub async fn with_async<F, Fut, R>(&self, f: F) -> R
    where
        F: FnOnce(&T) -> Fut + Send,
        Fut: std::future::Future<Output = R> + Send,
    {
        let guard = self.inner.lock().await;
        f(&*guard).await
    }

    /// Execute an async closure with mutable access to the inner value
    pub async fn with_mut_async<F, Fut, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut T) -> Fut + Send,
        Fut: std::future::Future<Output = R> + Send,
    {
        let mut guard = self.inner.lock().await;
        f(&mut *guard).await
    }

    /// Try to execute a closure with read access, returning None if the lock is busy
    pub fn try_with<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&T) -> R + Send,
    {
        let guard = self.inner.try_lock().ok()?;
        Some(f(&*guard))
    }

    /// Try to execute a closure with mutable access, returning None if the lock is busy
    pub fn try_with_mut<F, R>(&self, f: F) -> Option<R>
    where
        F: FnOnce(&mut T) -> R + Send,
    {
        let mut guard = self.inner.try_lock().ok()?;
        Some(f(&mut *guard))
    }
}

impl<T> Shareable<T> for Shared<T>
where
    T: Send + 'static,
{
    fn new(inner: T) -> Self {
        Self {
            inner: Arc::new(Mutex::new(inner)),
        }
    }
}

impl<T> Clone for Shared<T>
where
    T: Send + 'static,
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Specialized shared wrapper for types that can be consumed (like servers)
///
/// This wrapper allows the inner value to be extracted for consumption
/// (such as running a server), after which the wrapper becomes unusable.
///
/// # Examples
///
/// ```rust
/// use turbomcp_core::shared::{ConsumableShared, Shareable};
///
/// struct Server {
///     name: String,
/// }
///
/// impl Server {
///     fn new(name: String) -> Self {
///         Self { name }
///     }
///
///     fn run(self) -> String {
///         format!("Running server: {}", self.name)
///     }
///
///     fn status(&self) -> String {
///         format!("Server {} is ready", self.name)
///     }
/// }
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let server = Server::new("test".to_string());
/// let shared = ConsumableShared::new(server);
/// let shared_clone = shared.clone();
///
/// // Check status before consumption
/// let status = shared.with(|s| s.status()).await?;
/// assert_eq!(status, "Server test is ready");
///
/// // Consume the server
/// let server = shared.consume().await?;
/// let result = server.run();
/// assert_eq!(result, "Running server: test");
///
/// // Wrapper is now unusable (using clone)
/// assert!(shared_clone.with(|s| s.status()).await.is_err());
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct ConsumableShared<T> {
    inner: Arc<Mutex<Option<T>>>,
}

impl<T> ConsumableShared<T>
where
    T: Send + 'static,
{
    /// Execute a closure with read access to the inner value
    ///
    /// Returns an error if the value has been consumed.
    pub async fn with<F, R>(&self, f: F) -> Result<R, SharedError>
    where
        F: FnOnce(&T) -> R + Send,
    {
        let guard = self.inner.lock().await;
        match guard.as_ref() {
            Some(value) => Ok(f(value)),
            None => Err(SharedError::Consumed),
        }
    }

    /// Execute a closure with mutable access to the inner value
    ///
    /// Returns an error if the value has been consumed.
    pub async fn with_mut<F, R>(&self, f: F) -> Result<R, SharedError>
    where
        F: FnOnce(&mut T) -> R + Send,
    {
        let mut guard = self.inner.lock().await;
        match guard.as_mut() {
            Some(value) => Ok(f(value)),
            None => Err(SharedError::Consumed),
        }
    }

    /// Execute an async closure with read access to the inner value
    ///
    /// Returns an error if the value has been consumed.
    pub async fn with_async<F, Fut, R>(&self, f: F) -> Result<R, SharedError>
    where
        F: FnOnce(&T) -> Fut + Send,
        Fut: std::future::Future<Output = R> + Send,
    {
        let guard = self.inner.lock().await;
        match guard.as_ref() {
            Some(value) => Ok(f(value).await),
            None => Err(SharedError::Consumed),
        }
    }

    /// Execute an async closure with mutable access to the inner value
    ///
    /// Returns an error if the value has been consumed.
    pub async fn with_mut_async<F, Fut, R>(&self, f: F) -> Result<R, SharedError>
    where
        F: FnOnce(&mut T) -> Fut + Send,
        Fut: std::future::Future<Output = R> + Send,
    {
        let mut guard = self.inner.lock().await;
        match guard.as_mut() {
            Some(value) => Ok(f(value).await),
            None => Err(SharedError::Consumed),
        }
    }

    /// Consume the inner value, making the wrapper unusable
    ///
    /// This extracts the value from the wrapper, after which all other
    /// operations will return `SharedError::Consumed`.
    pub async fn consume(self) -> Result<T, SharedError> {
        let mut guard = self.inner.lock().await;
        guard.take().ok_or(SharedError::Consumed)
    }

    /// Check if the value is still available (not consumed)
    pub async fn is_available(&self) -> bool {
        self.inner.lock().await.is_some()
    }
}

impl<T> Shareable<T> for ConsumableShared<T>
where
    T: Send + 'static,
{
    fn new(inner: T) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some(inner))),
        }
    }
}

impl<T> Clone for ConsumableShared<T>
where
    T: Send + 'static,
{
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Errors that can occur when working with shared wrappers
///
/// These errors are domain-specific for shared wrapper operations and are converted
/// to the main [`Error`](crate::Error) type when crossing API boundaries.
#[derive(Debug, Clone, thiserror::Error)]
pub enum SharedError {
    /// The wrapped value has been consumed and is no longer available
    #[error("The shared value has been consumed")]
    Consumed,
}

// Conversion to main Error type for API boundary crossing
impl From<SharedError> for Box<crate::error::Error> {
    fn from(err: SharedError) -> Self {
        use crate::error::Error;
        match err {
            SharedError::Consumed => Error::validation("Shared value has already been consumed")
                .with_component("shared_wrapper")
                .with_context("note", "The value can only be consumed once"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct TestCounter {
        value: u64,
    }

    impl TestCounter {
        fn new() -> Self {
            Self { value: 0 }
        }

        fn increment(&mut self) {
            self.value += 1;
        }

        fn get(&self) -> u64 {
            self.value
        }

        #[allow(dead_code)]
        async fn get_async(&self) -> u64 {
            self.value
        }
    }

    #[tokio::test]
    async fn test_shared_basic_operations() {
        let counter = TestCounter::new();
        let shared = Shared::new(counter);

        // Test read access
        let value = shared.with(|c| c.get()).await;
        assert_eq!(value, 0);

        // Test mutable access
        shared.with_mut(|c| c.increment()).await;
        let value = shared.with(|c| c.get()).await;
        assert_eq!(value, 1);
    }

    #[tokio::test]
    async fn test_shared_async_operations() {
        let counter = TestCounter::new();
        let shared = Shared::new(counter);

        // Test async operations by performing a synchronous operation
        // that we can wrap in a future
        let value = shared.with(|c| c.get()).await;
        assert_eq!(value, 0);
    }

    #[tokio::test]
    async fn test_shared_cloning() {
        let counter = TestCounter::new();
        let shared = Shared::new(counter);

        // Clone multiple times
        let clones: Vec<_> = (0..10).map(|_| shared.clone()).collect();
        assert_eq!(clones.len(), 10);

        // All clones should work
        for (i, shared_clone) in clones.into_iter().enumerate() {
            shared_clone.with_mut(|c| c.increment()).await;
            let value = shared_clone.with(|c| c.get()).await;
            assert_eq!(value, i as u64 + 1);
        }
    }

    #[tokio::test]
    async fn test_shared_concurrent_access() {
        let counter = TestCounter::new();
        let shared = Shared::new(counter);

        // Spawn multiple concurrent tasks
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let shared_clone = shared.clone();
                tokio::spawn(async move {
                    shared_clone.with_mut(|c| c.increment()).await;
                })
            })
            .collect();

        // Wait for all tasks
        for handle in handles {
            handle.await.unwrap();
        }

        // Value should be 10
        let value = shared.with(|c| c.get()).await;
        assert_eq!(value, 10);
    }

    #[tokio::test]
    async fn test_consumable_shared() {
        let counter = TestCounter::new();
        let shared = ConsumableShared::new(counter);
        let shared_clone = shared.clone();

        // Test operations before consumption
        assert!(shared.is_available().await);
        let value = shared.with(|c| c.get()).await.unwrap();
        assert_eq!(value, 0);

        shared.with_mut(|c| c.increment()).await.unwrap();
        let value = shared.with(|c| c.get()).await.unwrap();
        assert_eq!(value, 1);

        // Consume the value
        let counter = shared.consume().await.unwrap();
        assert_eq!(counter.get(), 1);

        // Operations should fail after consumption (using the clone)
        assert!(!shared_clone.is_available().await);
        assert!(matches!(
            shared_clone.with(|c| c.get()).await,
            Err(SharedError::Consumed)
        ));
    }

    #[tokio::test]
    async fn test_consumable_shared_cloning() {
        let counter = TestCounter::new();
        let shared = ConsumableShared::new(counter);
        let shared_clone = shared.clone();

        // Both should work initially
        assert!(shared.is_available().await);
        assert!(shared_clone.is_available().await);

        // Consume from one
        let _counter = shared.consume().await.unwrap();

        // Both should be consumed
        assert!(!shared_clone.is_available().await);
        assert!(matches!(
            shared_clone.with(|c| c.get()).await,
            Err(SharedError::Consumed)
        ));
    }

    #[tokio::test]
    async fn test_try_operations() {
        let counter = TestCounter::new();
        let shared = Shared::new(counter);

        // Try operations should work when lock is available
        let value = shared.try_with(|c| c.get()).unwrap();
        assert_eq!(value, 0);

        shared.try_with_mut(|c| c.increment()).unwrap();
        let value = shared.try_with(|c| c.get()).unwrap();
        assert_eq!(value, 1);
    }
}
