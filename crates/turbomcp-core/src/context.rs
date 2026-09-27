//! [`RequestContext`] — read-only metadata about who/where/when, plus the
//! cross-version-stable neutral types it carries.
//!
//! `RequestContext` is *pure metadata*. MRTR fields (`request_state`,
//! `input_responses`) live in the typed request body, not here (round-1 C2.3).
//! Version-specific negotiated capabilities are injected via [`Extensions`] by
//! the service-layer negotiation/legacy adapter rather than typed into core —
//! this keeps `turbomcp-core` the bottom layer with no dependency on
//! `turbomcp-protocol` (a deliberate refinement of PLAN.md §4.2, which named a
//! version-specific `ClientCapabilities` type that would invert the layering).

use crate::{CancellationToken, Identity, ProtocolVersion};
use alloc::string::String;
use alloc::sync::Arc;
use core::any::{Any, TypeId};
use core::fmt;
use hashbrown::HashMap;
use serde_json::{Map, Value};

/// Server/client implementation identity (`Implementation` in the spec).
///
/// Neutral-safe: evolves additively across versions. Unknown fields are
/// preserved in `extra` for forward compatibility.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct Implementation {
    /// Programmatic name (e.g. `"my-server"`).
    pub name: String,
    /// Human-friendly title, if provided.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Version string.
    pub version: String,
    /// Any additional fields present on the wire (forward compatibility).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Implementation {
    /// Construct an [`Implementation`].
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: None,
            version: version.into(),
            extra: Map::new(),
        }
    }
}

/// MCP logging severity (`LoggingLevel`). Stable across versions.
///
/// Variants are declared in ascending RFC 5424 severity, so `Ord` compares
/// severity: a client-requested minimum of `Info` admits `level >= Info`.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    /// Debug-level detail.
    Debug,
    /// Informational.
    Info,
    /// Normal but significant.
    Notice,
    /// Warning.
    Warning,
    /// Error.
    Error,
    /// Critical.
    Critical,
    /// Action must be taken immediately.
    Alert,
    /// System is unusable.
    Emergency,
}

/// W3C Trace Context, extracted from `_meta` (or HTTP headers on legacy).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[non_exhaustive]
pub struct TraceContext {
    /// `traceparent` header value.
    pub traceparent: String,
    /// `tracestate` header value, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tracestate: Option<String>,
    /// `baggage` header value, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baggage: Option<String>,
}

/// A type-map for typed facts that travel beside a message: what the transport
/// knows about a request (who sent it, which connection and session it rode,
/// where replies go) and anything middleware adds.
///
/// A client can't write to it. Facts that used to ride in reserved `_meta` keys
/// had to be stripped from every inbound message first, or a client could
/// forge them; a Rust type-map has no wire form to forge.
///
/// Cloning is cheap and keeps every value: the map is shared, and copied only
/// when a clone is written to. Values are stored behind `Arc`, so `T` itself
/// need not be `Clone`.
#[derive(Clone, Default)]
pub struct Extensions {
    map: Arc<HashMap<TypeId, Arc<dyn Any + Send + Sync>>>,
}

impl Extensions {
    /// Create an empty type-map.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a value, replacing any previous value of the same type.
    pub fn insert<T: Any + Send + Sync>(&mut self, val: T) {
        Arc::make_mut(&mut self.map).insert(TypeId::of::<T>(), Arc::new(val));
    }

    /// Builder form of [`insert`](Self::insert).
    #[must_use]
    pub fn with<T: Any + Send + Sync>(mut self, val: T) -> Self {
        self.insert(val);
        self
    }

    /// Get a shared reference to a value of type `T`, if present.
    #[must_use]
    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref::<T>())
    }

    /// Remove the value of type `T`, returning whether there was one.
    pub fn remove<T: Any + Send + Sync>(&mut self) -> bool {
        if !self.map.contains_key(&TypeId::of::<T>()) {
            return false;
        }
        Arc::make_mut(&mut self.map)
            .remove(&TypeId::of::<T>())
            .is_some()
    }

    /// Whether a value of type `T` is present.
    #[must_use]
    pub fn contains<T: Any + Send + Sync>(&self) -> bool {
        self.map.contains_key(&TypeId::of::<T>())
    }

    /// Copy every value from `other` in, replacing values of the same type.
    pub fn extend(&mut self, other: &Self) {
        if other.map.is_empty() {
            return;
        }
        let map = Arc::make_mut(&mut self.map);
        for (key, value) in other.map.iter() {
            map.insert(*key, Arc::clone(value));
        }
    }

    /// Number of stored values.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Extensions")
            .field("len", &self.map.len())
            .finish()
    }
}

/// Read-only metadata about a single request (PLAN.md §4.2).
///
/// `#[non_exhaustive]`: construct via [`RequestContext::new`] + the `with_*`
/// builders (the framework) or [`RequestContext::test_default`] (downstream
/// tests, behind the `test-util` feature).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct RequestContext {
    /// Negotiated/declared protocol version for this request.
    pub protocol_version: ProtocolVersion,
    /// Client implementation identity, if known.
    pub client_info: Option<Implementation>,
    /// Raw advertised client capabilities (version-specific typed access is
    /// provided one layer up via per-RPC context types / [`Extensions`]).
    pub client_capabilities: Option<Value>,
    /// Requested logging level, if set.
    pub log_level: Option<LogLevel>,
    /// W3C trace context, if present.
    pub trace_context: Option<TraceContext>,
    /// Who made the request.
    pub identity: Identity,
    /// Per-request cancellation; always present, fresh per request.
    pub cancellation: CancellationToken,
    /// `_meta` keys not consumed by the framework (echoed on responses).
    pub propagated_meta: Map<String, Value>,
    /// Typed facts from the transport and middleware: the connection and
    /// session the request rode, where server-initiated messages go, and
    /// anything a layer added. See [`crate::envelope`].
    pub extensions: Extensions,
}

impl RequestContext {
    /// Create a context for the given protocol version with default everything
    /// else (anonymous identity, fresh cancellation token, empty maps).
    #[must_use]
    pub fn new(protocol_version: ProtocolVersion) -> Self {
        Self {
            protocol_version,
            client_info: None,
            client_capabilities: None,
            log_level: None,
            trace_context: None,
            identity: Identity::Anonymous,
            cancellation: CancellationToken::new(),
            propagated_meta: Map::new(),
            extensions: Extensions::new(),
        }
    }

    /// Builder: set the identity.
    #[must_use]
    pub fn with_identity(mut self, identity: Identity) -> Self {
        self.identity = identity;
        self
    }

    /// Builder: set the client implementation identity.
    #[must_use]
    pub fn with_client_info(mut self, info: Implementation) -> Self {
        self.client_info = Some(info);
        self
    }

    /// Builder: set the trace context.
    #[must_use]
    pub fn with_trace_context(mut self, tc: TraceContext) -> Self {
        self.trace_context = Some(tc);
        self
    }

    /// Builder: set the propagated `_meta` map.
    #[must_use]
    pub fn with_propagated_meta(mut self, meta: Map<String, Value>) -> Self {
        self.propagated_meta = meta;
        self
    }

    /// A default context for downstream handler unit tests (round-3 SC-4).
    ///
    /// Available behind the `test-util` feature so that `#[non_exhaustive]`
    /// doesn't make `RequestContext` impossible to construct in tests.
    #[cfg(any(feature = "test-util", test))]
    #[must_use]
    pub fn test_default() -> Self {
        Self::new(ProtocolVersion::LATEST)
    }
}

impl Default for RequestContext {
    fn default() -> Self {
        Self::new(ProtocolVersion::LATEST)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extensions_typed_roundtrip() {
        #[derive(Debug, PartialEq)]
        struct Tenant(u32);
        let mut ext = Extensions::new();
        ext.insert(Tenant(42));
        assert_eq!(ext.get::<Tenant>(), Some(&Tenant(42)));
        ext.insert(Tenant(7));
        assert_eq!(ext.get::<Tenant>(), Some(&Tenant(7)), "insert replaces");
        assert!(ext.remove::<Tenant>());
        assert!(!ext.remove::<Tenant>());
        assert!(ext.is_empty());
    }

    /// A clone keeps every value (the old map came back empty, so a cloned
    /// context silently lost what the transport put there), and writing to
    /// one copy leaves the other alone.
    #[test]
    fn a_clone_keeps_its_values_and_writes_are_private() {
        #[derive(Debug, PartialEq)]
        struct Session(&'static str);
        #[derive(Debug, PartialEq)]
        struct Extra(u8);
        let original = Extensions::new().with(Session("s-1"));
        let mut copy = original.clone();
        assert_eq!(copy.get::<Session>(), Some(&Session("s-1")));
        copy.insert(Extra(1));
        assert!(original.get::<Extra>().is_none());
        assert_eq!(copy.len(), 2);

        let mut merged = Extensions::new().with(Session("s-2"));
        merged.extend(&original);
        assert_eq!(merged.get::<Session>(), Some(&Session("s-1")));
    }

    #[test]
    fn context_builders_set_fields() {
        let tc = TraceContext {
            traceparent: "00-abc-def-01".into(),
            tracestate: None,
            baggage: None,
        };
        let ctx = RequestContext::new(ProtocolVersion::V2025_11_25)
            .with_trace_context(tc.clone())
            .with_client_info(Implementation::new("c", "1.0"));
        assert_eq!(ctx.trace_context, Some(tc));
        assert_eq!(ctx.client_info.as_ref().unwrap().name, "c");
    }

    #[test]
    fn implementation_preserves_unknown_fields() {
        let json = json!({"name":"s","version":"1.0","websiteUrl":"https://x"});
        let imp: Implementation = serde_json::from_value(json).unwrap();
        assert_eq!(imp.name, "s");
        assert_eq!(imp.extra.get("websiteUrl").unwrap(), &json!("https://x"));
    }

    #[test]
    fn test_default_constructs() {
        let ctx = RequestContext::test_default();
        assert_eq!(ctx.protocol_version, ProtocolVersion::LATEST);
        assert!(!ctx.identity.is_authenticated());
        assert!(!ctx.cancellation.is_cancelled());
    }
}
