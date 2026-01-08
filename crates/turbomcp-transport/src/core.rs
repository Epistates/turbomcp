//! Core transport traits, types, and errors.
//!
//! This module re-exports the foundational abstractions from [`turbomcp_transport_traits`]
//! for sending and receiving MCP messages over different communication protocols.
//!
//! ## v3.0 Migration Note
//!
//! As of TurboMCP v3.0, the core transport traits and types have been extracted to the
//! `turbomcp-transport-traits` crate for modular builds. This module re-exports all
//! types from that crate for backward compatibility.
//!
//! The central piece is the [`Transport`] trait, which provides a generic interface
//! for all transport implementations.

// Re-export everything from turbomcp-transport-traits
pub use turbomcp_transport_traits::{
    AtomicMetrics,

    BidirectionalTransport,
    // Bidirectional utilities
    ConnectionState,
    CorrelationContext,
    // Config
    LimitsConfig,
    MessageDirection,
    StreamingTransport,
    TimeoutConfig,
    TlsConfig,
    TlsVersion,

    // Traits
    Transport,
    TransportCapabilities,
    TransportConfig,

    // Error types
    TransportError,
    // Events
    TransportEvent,
    TransportEventEmitter,

    TransportFactory,

    // Message types
    TransportMessage,
    TransportMessageMetadata,

    // Metrics
    TransportMetrics,
    TransportResult,
    TransportState,
    // Core types
    TransportType,
    validate_request_size,
    validate_response_size,
};
