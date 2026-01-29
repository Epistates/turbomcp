//! Platform-adaptive marker traits for cross-platform compatibility.
//!
//! On native targets, `MaybeSend` requires `Send` for multi-threaded executors.
//! On WASM targets, `MaybeSend` has no bounds since WASM is single-threaded.

/// Marker trait that requires `Send` on native targets, nothing on WASM.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}

#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> MaybeSend for T {}

/// Marker trait with no bounds on WASM (single-threaded).
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}

#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSend for T {}
