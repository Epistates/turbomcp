//! # TurboMCP Transport Traits
//!
//! Core transport traits and types for the TurboMCP Model Context Protocol SDK.
//! This crate provides the foundational abstractions that all transport implementations depend on.
//!
//! ## Overview
//!
//! This crate defines:
//! - **Traits**: [`Transport`], [`BidirectionalTransport`], [`StreamingTransport`], [`TransportFactory`]
//! - **Types**: [`TransportType`], [`TransportState`], [`TransportCapabilities`], [`TransportMessage`]
//! - **Errors**: [`TransportError`], [`TransportResult`]
//! - **Config**: [`LimitsConfig`], [`TimeoutConfig`], [`TlsConfig`]
//! - **Metrics**: [`TransportMetrics`], [`AtomicMetrics`]
//!
//! ## Usage
//!
//! Transport implementations should depend on this crate and implement the [`Transport`] trait:
//!
//! ```rust,ignore
//! use turbomcp_transport_traits::{Transport, TransportResult, TransportMessage};
//! use async_trait::async_trait;
//!
//! struct MyTransport { /* ... */ }
//!
//! #[async_trait]
//! impl Transport for MyTransport {
//!     fn transport_type(&self) -> TransportType { /* ... */ }
//!     // ... other trait methods
//! }
//! ```

#![warn(
    missing_docs,
    missing_debug_implementations,
    rust_2018_idioms,
    unreachable_pub,
    clippy::all
)]
#![deny(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![allow(
    clippy::module_name_repetitions,
    clippy::missing_errors_doc,
    clippy::must_use_candidate
)]

mod config;
mod error;
mod events;
mod message;
mod metrics;
mod traits;
mod types;

// Re-export all public items
pub use config::{LimitsConfig, TimeoutConfig, TlsConfig, TlsVersion};
pub use error::{TransportError, TransportResult};
pub use events::{TransportEvent, TransportEventEmitter};
pub use message::{TransportMessage, TransportMessageMetadata};
pub use metrics::{AtomicMetrics, TransportMetrics};
pub use traits::{BidirectionalTransport, StreamingTransport, Transport, TransportFactory};
pub use types::{TransportCapabilities, TransportConfig, TransportState, TransportType};

// Re-export validation functions
pub use error::{validate_request_size, validate_response_size};
