//! Watching the requests a client sends: the seam client-side telemetry
//! (`turbomcp-telemetry`'s `ClientTelemetry`) plugs into.
//!
//! Register a [`RequestObserver`] with
//! [`ClientBuilder::with_observer`](crate::ClientBuilder::with_observer). It
//! sees every request as it goes out, including the handshake and retries,
//! and the [`RequestScope`] it hands back sees how the request ended.

use serde_json::{Map, Value};
use turbomcp_core::{ProtocolVersion, RequestId};

use crate::ClientError;

/// One request on its way out.
#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
pub struct OutboundRequest<'a> {
    /// The JSON-RPC id it goes out under.
    pub id: &'a RequestId,
    /// The method.
    pub method: &'a str,
    /// Its params, as the client built them.
    pub params: Option<&'a Map<String, Value>>,
    /// The revision it goes out under, once the session has one.
    pub protocol_version: Option<&'a ProtocolVersion>,
    /// The connection it rides, as the transport describes it: the network
    /// attributes, and the server's address.
    pub network: Option<&'a turbomcp_service::NetworkFacts>,
}

/// Watches the requests a client sends.
pub trait RequestObserver: Send + Sync {
    /// `request` is about to go out. The scope returned sees how it ends.
    fn start(&self, request: &OutboundRequest<'_>) -> Box<dyn RequestScope>;
}

/// One observed request, from [`RequestObserver::start`]. Dropped without
/// [`finish`](Self::finish), the request was abandoned (its caller dropped
/// the call or cancelled it).
pub trait RequestScope: Send {
    /// Entries to add to the request's `params._meta`: trace context
    /// (`traceparent`, `tracestate`, `baggage`), say. An entry the request
    /// already has is left as the caller set it.
    fn meta(&self) -> Map<String, Value> {
        Map::new()
    }

    /// The request ended with `outcome`.
    fn finish(self: Box<Self>, outcome: Result<&Value, &ClientError>);
}

/// Merge `extra` into `params._meta`, creating either as needed and keeping
/// whatever the caller already set. Params that aren't an object are left
/// alone.
pub(crate) fn merge_meta(params: &mut Option<Value>, extra: Map<String, Value>) {
    if extra.is_empty() {
        return;
    }
    let params = params.get_or_insert_with(|| Value::Object(Map::new()));
    let Some(params) = params.as_object_mut() else {
        return;
    };
    let Some(meta) = params
        .entry("_meta")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
    else {
        return;
    };
    for (key, value) in extra {
        meta.entry(key).or_insert(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn merged_meta_keeps_what_the_caller_set() {
        let mut params = Some(json!({ "name": "t", "_meta": { "traceparent": "mine" } }));
        let mut extra = Map::new();
        extra.insert("traceparent".into(), json!("observer"));
        extra.insert("baggage".into(), json!("k=v"));
        merge_meta(&mut params, extra);
        let meta = &params.unwrap()["_meta"];
        assert_eq!(meta["traceparent"], "mine");
        assert_eq!(meta["baggage"], "k=v");

        let mut none = None;
        let mut extra = Map::new();
        extra.insert("traceparent".into(), json!("t"));
        merge_meta(&mut none, extra);
        assert_eq!(none.unwrap()["_meta"]["traceparent"], "t");
    }
}
