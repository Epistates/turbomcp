//! [`CancellationToken`] — per-request cancellation, always present.
//!
//! Every in-flight request carries a fresh token. The dispatcher
//! triggers it when a matching `notifications/cancelled` arrives; the graceful
//! shutdown path cancels a parent that cascades to all children.
//!
//! On `std` this is a thin newtype over `tokio_util::sync::CancellationToken`
//! (hierarchical, awaitable via [`CancellationToken::cancelled`]). On
//! `no_std`/`wasm32` it degrades to a shared atomic flag — pollable but not
//! awaitable, and `child_token` shares the parent's flag (the wasm path does
//! not drive long-running cancellable work).

#[cfg(feature = "std")]
pub use std_impl::CancellationToken;

#[cfg(not(feature = "std"))]
pub use no_std_impl::CancellationToken;

#[cfg(feature = "std")]
mod std_impl {
    /// Hierarchical, awaitable cancellation backed by `tokio-util`.
    #[derive(Clone, Debug, Default)]
    pub struct CancellationToken(tokio_util::sync::CancellationToken);

    impl CancellationToken {
        /// Create a new root token.
        #[must_use]
        pub fn new() -> Self {
            Self(tokio_util::sync::CancellationToken::new())
        }

        /// Create a child token that is cancelled when this token (or the
        /// child itself) is cancelled, but not vice versa.
        #[must_use]
        pub fn child_token(&self) -> Self {
            Self(self.0.child_token())
        }

        /// Cancel this token (and, transitively, its children).
        pub fn cancel(&self) {
            self.0.cancel();
        }

        /// Whether cancellation has been requested.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.0.is_cancelled()
        }

        /// Resolve once the token is cancelled. Use in `tokio::select!`.
        pub async fn cancelled(&self) {
            self.0.cancelled().await;
        }

        /// Borrow the underlying tokio token (e.g. for `run_until_cancelled`).
        #[must_use]
        pub fn inner(&self) -> &tokio_util::sync::CancellationToken {
            &self.0
        }
    }

    impl From<tokio_util::sync::CancellationToken> for CancellationToken {
        fn from(t: tokio_util::sync::CancellationToken) -> Self {
            Self(t)
        }
    }
}

#[cfg(not(feature = "std"))]
mod no_std_impl {
    use alloc::sync::Arc;
    use core::sync::atomic::{AtomicBool, Ordering};

    /// Atomic-flag cancellation for `no_std`/`wasm32`. Pollable, not awaitable.
    #[derive(Clone, Debug, Default)]
    pub struct CancellationToken(Arc<AtomicBool>);

    impl CancellationToken {
        /// Create a new token.
        #[must_use]
        pub fn new() -> Self {
            Self(Arc::new(AtomicBool::new(false)))
        }

        /// Returns a token sharing this one's flag (flat, not hierarchical).
        #[must_use]
        pub fn child_token(&self) -> Self {
            Self(Arc::clone(&self.0))
        }

        /// Request cancellation.
        pub fn cancel(&self) {
            self.0.store(true, Ordering::Release);
        }

        /// Whether cancellation has been requested.
        #[must_use]
        pub fn is_cancelled(&self) -> bool {
            self.0.load(Ordering::Acquire)
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod tests {
    use super::CancellationToken;

    #[test]
    fn child_cancels_with_parent_but_not_reverse() {
        let parent = CancellationToken::new();
        let child = parent.child_token();
        assert!(!child.is_cancelled());
        parent.cancel();
        assert!(child.is_cancelled());

        let parent2 = CancellationToken::new();
        let child2 = parent2.child_token();
        child2.cancel();
        assert!(!parent2.is_cancelled());
    }
}

// Runs under `cargo test -p turbomcp-core --no-default-features`.
#[cfg(all(test, not(feature = "std")))]
mod no_std_tests {
    use super::CancellationToken;

    #[test]
    fn no_std_token_is_flat_shared_flag() {
        // Documented divergence from std: `child_token` shares the parent's
        // flag, so cancellation propagates in BOTH directions.
        let parent = CancellationToken::new();
        let child = parent.child_token();
        assert!(!parent.is_cancelled() && !child.is_cancelled());
        parent.cancel();
        assert!(child.is_cancelled());

        let parent2 = CancellationToken::new();
        let child2 = parent2.child_token();
        child2.cancel();
        assert!(parent2.is_cancelled()); // flat, not hierarchical
    }
}
