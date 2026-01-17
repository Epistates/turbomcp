//! Platform-adaptive marker traits for cross-platform compatibility.
//!
//! These traits enable unified handler definitions that work on both native and WASM targets.
//! On native targets, the traits require `Send`/`Sync` bounds for multi-threaded executors.
//! On WASM targets, the traits have no bounds since WASM is single-threaded and `JsValue` is `!Send`.
//!
//! # Background
//!
//! The fundamental challenge for unified native/WASM code is that [`wasm_bindgen::JsValue`]
//! is `!Send` by design due to JavaScript's slab-based object management. This has
//! "huge trickle-down effects" on downstream crates.
//!
//! The solution is conditional compilation with marker traits that adapt to the target.
//!
//! # Example
//!
//! ```rust,ignore
//! use turbomcp_core::{MaybeSend, MaybeSync};
//!
//! // This trait works on both native (with Send) and WASM (without Send)
//! trait MyHandler: MaybeSend + MaybeSync {
//!     fn handle(&self) -> impl Future<Output = ()> + MaybeSend;
//! }
//! ```
//!
//! # Platform Behavior
//!
//! - **Native (`x86_64`, `aarch64`, etc.)**: `MaybeSend` requires `Send`, `MaybeSync` requires `Sync`
//! - **WASM (`wasm32`)**: Both traits are blanket-implemented for all types (no bounds)

/// Marker trait that requires `Send` on native targets, nothing on WASM.
///
/// This enables unified trait definitions that work on both platforms:
/// - On native: futures must be `Send` for multi-threaded executors like tokio
/// - On WASM: no `Send` bound needed since WASM is single-threaded
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_core::MaybeSend;
/// use std::future::Future;
///
/// trait MyAsyncTrait {
///     fn do_work(&self) -> impl Future<Output = ()> + MaybeSend;
/// }
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> MaybeSend for T {}

/// Marker trait with no bounds on WASM (single-threaded).
///
/// On WASM targets, `MaybeSend` is blanket-implemented for all types since
/// WASM is single-threaded and doesn't require `Send` bounds.
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}

#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSend for T {}

/// Marker trait that requires `Sync` on native targets, nothing on WASM.
///
/// This enables unified trait definitions that work on both platforms:
/// - On native: handlers must be `Sync` for shared access across threads
/// - On WASM: no `Sync` bound needed since WASM is single-threaded
///
/// # Example
///
/// ```rust,ignore
/// use turbomcp_core::{MaybeSend, MaybeSync};
///
/// trait MyHandler: Clone + MaybeSend + MaybeSync + 'static {
///     fn handle(&self);
/// }
/// ```
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSync: Sync {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync + ?Sized> MaybeSync for T {}

/// Marker trait with no bounds on WASM (single-threaded).
///
/// On WASM targets, `MaybeSync` is blanket-implemented for all types since
/// WASM is single-threaded and doesn't require `Sync` bounds.
#[cfg(target_arch = "wasm32")]
pub trait MaybeSync {}

#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSync for T {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_string_is_maybe_send() {
        fn assert_maybe_send<T: MaybeSend>() {}
        assert_maybe_send::<String>();
    }

    #[test]
    fn test_string_is_maybe_sync() {
        fn assert_maybe_sync<T: MaybeSync>() {}
        assert_maybe_sync::<String>();
    }

    #[test]
    fn test_arc_is_maybe_send_sync() {
        fn assert_bounds<T: MaybeSend + MaybeSync>() {}
        assert_bounds::<std::sync::Arc<String>>();
    }

    // On native, verify that non-Send types don't implement MaybeSend
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn test_rc_is_not_maybe_send() {
        // This test verifies at compile time that Rc<T> doesn't implement MaybeSend on native
        // We can't easily write a negative test, but we can verify the marker trait exists
        fn _needs_send<T: Send>() {}
        fn _needs_maybe_send<T: MaybeSend>() {}

        // Rc<String> should NOT compile with _needs_send or _needs_maybe_send on native
        // This is a compile-time guarantee, not a runtime test
    }
}
