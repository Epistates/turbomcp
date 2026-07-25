//! # turbomcp-telemetry
//!
//! OpenTelemetry observability for TurboMCP v4: a transport-agnostic
//! [`TraceContextLayer`] that continues the caller's W3C distributed trace
//! (propagated over MCP `_meta`, SEP-414) and records **PII-safe** identity
//! attributes on each request span, plus an optional turnkey OTLP export
//! pipeline (feature `otlp`).
//!
//! ## Compose the layer
//!
//! [`TraceContextLayer`] is a [`tower::Layer`] over `Service<JsonRpcMessage>`,
//! so it wraps a dispatcher like any shared RPC middleware and works identically
//! under stdio, HTTP, and WS:
//!
//! ```ignore
//! use tower::Layer;
//! use turbomcp_telemetry::TraceContextLayer;
//!
//! let traced = TraceContextLayer::new().layer(dispatcher);
//! // serve `traced` over any transport.
//! ```
//!
//! ## Redaction
//!
//! By default a span records the caller's subject as a stable, non-reversible
//! hash ([`RedactedSubject`](turbomcp_core::RedactedSubject)) and the claim
//! *keys* only — never claim values — so emails/org-ids in a JWT never reach
//! telemetry. Opt into raw subjects with [`SpanPolicy::unredacted`].
//!
//! ## Export
//!
//! With the `otlp` feature, [`init_otlp`] builds an OTLP/gRPC exporter and
//! installs a `tracing` subscriber that exports the layer's spans. Without it,
//! the spans flow to whatever `tracing` subscriber the host installs.
#![forbid(unsafe_code)]
// docs.rs builds with `--cfg docsrs` on nightly so every feature-gated item
// renders with the feature that unlocks it.
#![cfg_attr(docsrs, feature(doc_cfg))]
#![warn(missing_docs)]

mod layer;
mod metrics;
mod propagation;

pub use layer::{TraceContext, TraceContextLayer};
pub use metrics::{Metrics, MetricsLayer};
pub use propagation::{extract as extract_context, inject as inject_context};

#[cfg(feature = "otlp")]
#[cfg_attr(docsrs, doc(cfg(feature = "otlp")))]
mod otlp;
#[cfg(feature = "otlp")]
#[cfg_attr(docsrs, doc(cfg(feature = "otlp")))]
pub use otlp::{OtlpConfig, TelemetryGuard, init_otlp};

/// How [`TraceContextLayer`] records the caller's identity on a span.
///
/// The default is fully redacted (hashed subject, claim keys only) — PII never
/// reaches telemetry unless you opt out.
#[derive(Debug, Clone, Copy)]
pub struct SpanPolicy {
    /// Record the subject as a stable hash rather than the raw value (default
    /// `true`).
    pub redact_subject: bool,
    /// Record the set of claim *keys* (never values) on the span (default
    /// `true`).
    pub record_claim_keys: bool,
}

impl Default for SpanPolicy {
    fn default() -> Self {
        Self {
            redact_subject: true,
            record_claim_keys: true,
        }
    }
}

impl SpanPolicy {
    /// Record the raw subject (no hashing). Use only where the subject is not
    /// considered PII in your telemetry backend. Claim values are still never
    /// recorded.
    #[must_use]
    pub fn unredacted() -> Self {
        Self {
            redact_subject: false,
            record_claim_keys: true,
        }
    }
}

/// Errors from installing the OTLP pipeline (feature `otlp`).
#[cfg(feature = "otlp")]
#[cfg_attr(docsrs, doc(cfg(feature = "otlp")))]
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TelemetryError {
    /// The OTLP exporter could not be built.
    #[error("otlp exporter build failed: {0}")]
    Exporter(String),
    /// A global `tracing` subscriber was already installed.
    #[error("subscriber init failed: {0}")]
    Subscriber(String),
}
