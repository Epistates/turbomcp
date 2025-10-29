//! Frontend transport implementations
//!
//! This module contains concrete implementations of frontend transports
//! for exposing proxy functionality via different interfaces.

pub mod stdio;

pub use stdio::StdioFrontend;
