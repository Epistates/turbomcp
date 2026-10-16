//! Test support: every JSON-RPC message TurboMCP emits, checked against the
//! official MCP schema for the revision it goes out under.
//!
//! The typed round trips in `turbomcp-protocol` cover the generated types, but
//! much of what a server or client sends is built by hand (progress, listen
//! acknowledgements, error data, the `_meta` a client stamps on a `2026-07-28`
//! request). Wrapped in [`Checked`] (a server's service) or
//! [`CheckedTransport`] (a client's transport), a test fails on the first
//! message that does not conform, naming the definition and the violation.
//!
//! Only this side's output is checked. A test feeds in malformed input on
//! purpose, and what the peer sends is the peer's business; what this side
//! answers with is ours. A message the core schema does not define (an
//! extension's method) is checked only for its envelope.
//!
//! The schemas are the ones `turbomcp-protocol`'s types are generated from,
//! vendored and drift-checked by `just codegen`.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, LazyLock};
use std::task::{Context, Poll};
use std::time::Instant;

use parking_lot::Mutex;
use serde_json::Value;
use turbomcp_core::{Extensions, JsonRpcMessage, McpRequest, ProtocolVersion};
use turbomcp_service::{ProtocolError, Transport, TransportCapabilities, WireVersion};

/// The `_meta` key a `2026-07-28` request names its revision under.
const VERSION_KEY: &str = "io.modelcontextprotocol/protocolVersion";

macro_rules! schema {
    ($rev:literal) => {
        include_str!(concat!(
            "../../turbomcp-protocol/tests/fixtures/schema/",
            $rev,
            "/schema.json"
        ))
    };
}

static REVISIONS: LazyLock<Vec<(ProtocolVersion, Revision)>> = LazyLock::new(|| {
    vec![
        (
            ProtocolVersion::V2025_06_18,
            Revision::load(schema!("2025-06-18")),
        ),
        (
            ProtocolVersion::V2025_11_25,
            Revision::load(schema!("2025-11-25")),
        ),
        (
            ProtocolVersion::V2026_07_28,
            Revision::load(schema!("2026-07-28")),
        ),
    ]
});

/// One revision's schema, indexed by method.
struct Revision {
    root: Value,
    defs_key: &'static str,
    /// Method → the definition of a request carrying it.
    requests: HashMap<String, String>,
    /// Method → the definition of a notification carrying it.
    notifications: HashMap<String, String>,
    validators: Mutex<HashMap<String, Arc<jsonschema::Validator>>>,
}

impl Revision {
    fn load(raw: &str) -> Self {
        let root: Value = serde_json::from_str(raw).expect("a vendored schema is JSON");
        let defs_key = if root.get("$defs").is_some() {
            "$defs"
        } else {
            "definitions"
        };
        let mut requests = HashMap::new();
        let mut notifications = HashMap::new();
        for (name, def) in root[defs_key].as_object().expect("definitions") {
            let props = &def["properties"];
            let Some(method) = props["method"]["const"].as_str() else {
                continue;
            };
            // A request has an `id`; an MRTR input request (no `id`) is not
            // something either side sends as a frame.
            if name.ends_with("Request") && props.get("id").is_some() {
                requests.insert(method.to_owned(), name.clone());
            } else if name.ends_with("Notification") {
                notifications.insert(method.to_owned(), name.clone());
            }
        }
        Self {
            root,
            defs_key,
            requests,
            notifications,
            validators: Mutex::default(),
        }
    }

    fn has(&self, def: &str) -> bool {
        self.root[self.defs_key].get(def).is_some()
    }

    fn first_of<'a>(&self, defs: &[&'a str]) -> Option<&'a str> {
        defs.iter().copied().find(|def| self.has(def))
    }

    fn violations(&self, def: &str, instance: &Value) -> Vec<String> {
        let validator = self
            .validators
            .lock()
            .entry(def.to_owned())
            .or_insert_with(|| {
                let mut schema = self.root.clone();
                schema["$ref"] = Value::String(format!("#/{}/{def}", self.defs_key));
                Arc::new(jsonschema::validator_for(&schema).expect("a vendored schema compiles"))
            })
            .clone();
        validator
            .iter_errors(instance)
            .map(|e| format!("{def}: {e} at {}", e.instance_path()))
            .collect()
    }

    /// The definition a result answering `method` must satisfy.
    fn result_def(&self, method: &str, result: &Value) -> Option<String> {
        if result["resultType"] == "input_required" && self.has("InputRequiredResult") {
            return Some("InputRequiredResult".into());
        }
        if result.get("task").is_some() && self.has("CreateTaskResult") {
            return Some("CreateTaskResult".into());
        }
        let request = self.requests.get(method)?;
        let own = format!("{}Result", request.strip_suffix("Request")?);
        if self.has(&own) {
            Some(own)
        } else {
            // `ping`, `logging/setLevel`, `resources/subscribe`: any result.
            self.has("Result").then(|| "Result".into())
        }
    }
}

fn revision(version: &ProtocolVersion) -> Option<&'static Revision> {
    REVISIONS
        .iter()
        .find(|(v, _)| v == version)
        .map(|(_, revision)| revision)
}

/// Panic unless `message`, sent under `version`, conforms. `answering` is the
/// method of the request a response answers.
///
/// # Panics
/// On the first nonconforming message, with every violation in it.
pub fn check(version: &ProtocolVersion, message: &JsonRpcMessage, answering: Option<&str>) {
    let Some(rev) = revision(version) else {
        return;
    };
    let json = serde_json::to_value(message).expect("a message serializes");
    let problems = match message {
        JsonRpcMessage::Request(r) => rev
            .requests
            .get(&r.method)
            .map(|def| rev.violations(def, &json))
            .unwrap_or_default(),
        JsonRpcMessage::Notification(n) => rev
            .notifications
            .get(&n.method)
            .map(|def| rev.violations(def, &json))
            .unwrap_or_default(),
        JsonRpcMessage::Response(r) if r.error.is_some() => rev
            .first_of(&["JSONRPCErrorResponse", "JSONRPCError"])
            .map(|def| rev.violations(def, &json))
            .unwrap_or_default(),
        JsonRpcMessage::Response(_) => {
            let mut problems = rev
                .first_of(&["JSONRPCResultResponse", "JSONRPCResponse"])
                .map(|def| rev.violations(def, &json))
                .unwrap_or_default();
            if let (Some(method), Some(result)) = (answering, json.get("result"))
                && let Some(def) = rev.result_def(method, result)
            {
                problems.extend(rev.violations(&def, result));
            }
            problems
        }
    };
    assert!(
        problems.is_empty(),
        "{version} message violates the schema{}:\n  {}\nmessage: {json:#}",
        answering
            .map(|m| format!(" (answering {m})"))
            .unwrap_or_default(),
        problems.join("\n  ")
    );
}

/// What a connection has settled: its revision once negotiated, and the
/// methods of the requests in flight, by id.
#[derive(Default)]
struct Session {
    version: Mutex<Option<ProtocolVersion>>,
    /// Requests this side sent, to read the peer's `initialize` answer.
    ours: Mutex<HashMap<String, String>>,
    /// Requests the peer sent, to know what this side's responses answer.
    theirs: Mutex<HashMap<String, String>>,
}

impl Session {
    fn version(&self) -> Option<ProtocolVersion> {
        self.version.lock().clone()
    }

    /// An `initialize` result settles the revision.
    fn settle(&self, method: Option<&str>, message: &JsonRpcMessage) {
        if method == Some("initialize")
            && let JsonRpcMessage::Response(r) = message
            && let Some(version) = r
                .result
                .as_ref()
                .and_then(|result| result["protocolVersion"].as_str())
        {
            *self.version.lock() = Some(ProtocolVersion::from_wire(version));
        }
    }
}

fn id_key(message: &JsonRpcMessage) -> Option<String> {
    match message {
        JsonRpcMessage::Request(r) => serde_json::to_string(&r.id).ok(),
        JsonRpcMessage::Response(r) => r.id.as_ref().and_then(|id| serde_json::to_string(id).ok()),
        JsonRpcMessage::Notification(_) => None,
    }
}

/// The revision a `2026-07-28` request names in its own `_meta`.
fn named_version(message: &JsonRpcMessage) -> Option<ProtocolVersion> {
    let JsonRpcMessage::Request(r) = message else {
        return None;
    };
    let version = r
        .params
        .as_ref()?
        .get("_meta")?
        .get(VERSION_KEY)?
        .as_str()?;
    Some(ProtocolVersion::from_wire(version))
}

/// A server's service whose every response is checked. Clones (the serve
/// driver makes one per request) share the connection's session.
pub struct Checked<S> {
    inner: S,
    session: Arc<Session>,
}

impl<S: Clone> Clone for Checked<S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            session: self.session.clone(),
        }
    }
}

impl<S> Checked<S> {
    pub fn new(inner: S) -> Self {
        Self {
            inner,
            session: Arc::default(),
        }
    }

    pub fn get_ref(&self) -> &S {
        &self.inner
    }
}

impl<S> tower::Service<McpRequest> for Checked<S>
where
    S: tower::Service<McpRequest, Response = Option<JsonRpcMessage>, Error = ProtocolError>,
    S::Future: Send + 'static,
{
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: McpRequest) -> Self::Future {
        let method = match &req.message {
            JsonRpcMessage::Request(r) => Some(r.method.clone()),
            _ => None,
        };
        let named = named_version(&req.message);
        let session = self.session.clone();
        let answer = self.inner.call(req);
        Box::pin(async move {
            let out = answer.await?;
            if let Some(message) = &out {
                session.settle(method.as_deref(), message);
                if let Some(version) = named.or_else(|| session.version()) {
                    check(&version, message, method.as_deref());
                }
            }
            Ok(out)
        })
    }
}

/// A client's transport whose every outgoing frame is checked.
pub struct CheckedTransport<T> {
    inner: T,
    session: Arc<Session>,
}

impl<T> CheckedTransport<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner,
            session: Arc::default(),
        }
    }

    fn outgoing(&self, message: &JsonRpcMessage, version: Option<ProtocolVersion>) {
        let answering = match message {
            JsonRpcMessage::Request(r) => {
                if let Some(id) = id_key(message) {
                    self.session.ours.lock().insert(id, r.method.clone());
                }
                None
            }
            JsonRpcMessage::Response(_) => {
                id_key(message).and_then(|id| self.session.theirs.lock().remove(&id))
            }
            JsonRpcMessage::Notification(_) => None,
        };
        let version = version
            .or_else(|| named_version(message))
            .or_else(|| self.session.version());
        if let Some(version) = version {
            check(&version, message, answering.as_deref());
        }
    }

    fn incoming(&self, message: &JsonRpcMessage) {
        match message {
            JsonRpcMessage::Request(r) => {
                if let Some(id) = id_key(message) {
                    self.session.theirs.lock().insert(id, r.method.clone());
                }
            }
            JsonRpcMessage::Response(_) => {
                let method = id_key(message).and_then(|id| self.session.ours.lock().remove(&id));
                self.session.settle(method.as_deref(), message);
            }
            JsonRpcMessage::Notification(_) => {}
        }
    }
}

impl<T: Transport> Transport for CheckedTransport<T> {
    type Error = T::Error;

    fn capabilities(&self) -> TransportCapabilities {
        self.inner.capabilities()
    }

    fn send_with(
        &mut self,
        msg: JsonRpcMessage,
        facts: Extensions,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.outgoing(&msg, facts.get::<WireVersion>().map(|v| v.0.clone()));
        self.inner.send_with(msg, facts)
    }

    fn invalid_frame(error: Self::Error) -> Result<turbomcp_core::InvalidFrame, Self::Error> {
        T::invalid_frame(error)
    }

    fn send(
        &mut self,
        msg: JsonRpcMessage,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.outgoing(&msg, None);
        self.inner.send(msg)
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
        let message = self.inner.recv().await?;
        if let Some(message) = &message {
            self.incoming(message);
        }
        Ok(message)
    }

    async fn recv_with(&mut self) -> Result<Option<(JsonRpcMessage, Extensions)>, Self::Error> {
        let received = self.inner.recv_with().await?;
        if let Some((message, _)) = &received {
            self.incoming(message);
        }
        Ok(received)
    }

    fn close(self) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.close()
    }

    fn graceful_shutdown(
        self,
        deadline: Instant,
    ) -> impl Future<Output = Result<(), Self::Error>> + Send {
        self.inner.graceful_shutdown(deadline)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use turbomcp_core::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

    const V0728: ProtocolVersion = ProtocolVersion::V2026_07_28;
    const V1125: ProtocolVersion = ProtocolVersion::V2025_11_25;

    fn result(value: serde_json::Value) -> JsonRpcMessage {
        JsonRpcResponse::success(1, value).into()
    }

    #[test]
    fn a_conforming_tool_result_passes() {
        let ok = json!({
            "content": [{ "type": "text", "text": "hi" }],
            "resultType": "complete"
        });
        check(&V0728, &result(ok), Some("tools/call"));
    }

    #[test]
    #[should_panic(expected = "CallToolResult")]
    fn a_result_is_checked_against_its_methods_definition() {
        check(
            &V1125,
            &result(json!({ "content": "not an array" })),
            Some("tools/call"),
        );
    }

    #[test]
    #[should_panic(expected = "ProgressNotification")]
    fn a_notification_is_checked() {
        let progress = JsonRpcNotification::new(
            "notifications/progress",
            Some(json!({ "progressToken": "t", "progress": "half" })),
        );
        check(&V1125, &progress.into(), None);
    }

    #[test]
    #[should_panic(expected = "CallToolRequest")]
    fn a_2026_request_without_its_meta_fails() {
        let call = JsonRpcRequest::new(1, "tools/call", Some(json!({ "name": "x" })));
        check(&V0728, &call.into(), None);
    }

    #[test]
    fn an_extension_method_is_checked_for_its_envelope_only() {
        let call = JsonRpcRequest::new(1, "io.example/whatever", Some(json!({ "any": 1 })));
        check(&V0728, &call.into(), None);
    }
}
