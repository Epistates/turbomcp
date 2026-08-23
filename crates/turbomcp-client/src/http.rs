//! The Streamable HTTP client transport (feature `http`).
//!
//! Streamable HTTP is request-scoped — each JSON-RPC request is its own POST —
//! yet the [`Connection`](crate::Connection) actor wants the persistent
//! [`Transport`] `send`/`recv` shape. This transport bridges the two: `send`
//! fires a POST in a spawned task whose response (a single `application/json`
//! frame, or a `text/event-stream` of frames) is funneled into an inbound
//! channel that `recv` drains. Response correlation happens one layer up (by
//! request id in the `Connection`), so the transport never has to match
//! requests to responses itself — it only has to deliver every inbound frame.
//!
//! The `Mcp-Session-Id` minted by a legacy `initialize` response is captured
//! and replayed on subsequent requests (the stateful path); the stateless draft
//! path simply never sets it.
//!
//! Per-request failures are scoped to that request: a POST that fails (network,
//! HTTP status, decode) synthesizes a JSON-RPC error *response* for the
//! request's id rather than tearing down the whole connection — only the one
//! waiting caller sees the error.

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use eventsource_stream::Eventsource;
use futures::StreamExt;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use tokio::sync::mpsc;
use turbomcp_core::{JsonRpcError, JsonRpcMessage, JsonRpcResponse, ProtocolVersion};
use turbomcp_service::{Transport, mcp_headers};

use crate::client::{Client, ClientBuilder};
use crate::error::{ClientError, ClientResult};

/// Failures specific to the HTTP client transport.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HttpClientError {
    /// The transport's inbound channel closed (connection torn down).
    #[error("http client transport closed")]
    Closed,
}

const SESSION_HEADER: &str = "mcp-session-id";

/// Shared state for the spawned POST tasks: the HTTP client, target URL, the
/// captured session id, the last negotiated protocol version, and the inbound
/// delivery channel.
struct Shared {
    http: reqwest::Client,
    url: String,
    session: Mutex<Option<String>>,
    /// The negotiated protocol version last seen on an outbound signal —
    /// the `MCP-Protocol-Version` header fallback for messages that carry no
    /// signal of their own (responses to server requests, notifications).
    version: Mutex<Option<String>>,
    inbound_tx: mpsc::Sender<JsonRpcMessage>,
}

/// A Streamable HTTP transport to a single MCP endpoint URL.
pub struct HttpClientTransport {
    shared: Arc<Shared>,
    inbound_rx: mpsc::Receiver<JsonRpcMessage>,
}

impl HttpClientTransport {
    /// Build a transport targeting `url` (e.g. `http://127.0.0.1:8080/mcp`).
    ///
    /// # Errors
    /// [`ClientError::Protocol`] if the underlying HTTP client can't be built.
    pub fn new(url: impl Into<String>) -> ClientResult<Self> {
        let http = reqwest::Client::builder()
            .build()
            .map_err(|e| ClientError::Protocol(format!("http client build failed: {e}")))?;
        let (inbound_tx, inbound_rx) = mpsc::channel(1024);
        Ok(Self {
            shared: Arc::new(Shared {
                http,
                url: url.into(),
                session: Mutex::new(None),
                version: Mutex::new(None),
                inbound_tx,
            }),
            inbound_rx,
        })
    }
}

impl Transport for HttpClientTransport {
    type Error = HttpClientError;

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), Self::Error> {
        // POST and pump the response in the background so the driver can keep
        // sending; HTTP requests are independent and may run concurrently.
        let shared = Arc::clone(&self.shared);
        tokio::spawn(post_and_pump(shared, msg));
        Ok(())
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
        // `None` here means every sender (the Shared in this transport + any
        // in-flight pump task) has dropped — a clean end-of-stream.
        Ok(self.inbound_rx.recv().await)
    }

    async fn close(self) -> Result<(), Self::Error> {
        // Best-effort session termination (the spec's explicit DELETE).
        let sid = self.shared.session.lock().expect("session mutex").clone();
        if let Some(sid) = sid {
            let _ = self
                .shared
                .http
                .delete(&self.shared.url)
                .header(SESSION_HEADER, sid)
                .timeout(Duration::from_secs(5))
                .send()
                .await;
        }
        Ok(())
    }
}

/// POST `msg` and feed whatever comes back into the inbound channel.
async fn post_and_pump(shared: Arc<Shared>, msg: JsonRpcMessage) {
    // The request id (if this is a request) so a failure can be reported to just
    // this caller as an error response rather than killing the connection.
    let request_id = match &msg {
        JsonRpcMessage::Request(r) => Some(r.id.clone()),
        _ => None,
    };

    if let Err(err) = pump(&shared, msg).await {
        match request_id {
            Some(id) => {
                // Surface the failure to the one waiting caller. This is a
                // locally synthesized *transport* error, so it takes the
                // implementation-defined floor — the same code
                // `McpError::Transport` maps to. (It read `-32001` when that
                // was our header-mismatch code, which this is not.)
                let resp = JsonRpcResponse::error(
                    id,
                    JsonRpcError {
                        code: -32000,
                        message: err,
                        data: None,
                    },
                );
                let _ = shared.inbound_tx.send(JsonRpcMessage::Response(resp)).await;
            }
            // Notifications / responses have no waiter — just log.
            None => tracing::debug!(error = %err, "http client POST failed (no waiter)"),
        }
    }
}

/// The fallible body of a POST + response pump. Errors are returned as a string
/// for [`post_and_pump`] to route.
async fn pump(shared: &Shared, mut msg: JsonRpcMessage) -> Result<(), String> {
    // Lift the transport signals out of the body before serialization so they
    // never reach the wire: the `x-mcp-header` mirror map and the negotiated
    // protocol version.
    let mirror_headers = extract_header_params(&mut msg);
    let version = extract_protocol_version(&mut msg, shared);

    let body = serde_json::to_string(&msg).map_err(|e| format!("encode failed: {e}"))?;

    let mut req = shared
        .http
        .post(&shared.url)
        .header(ACCEPT, "application/json, text/event-stream")
        .header(CONTENT_TYPE, "application/json")
        .body(body);
    if let Some(sid) = shared.session.lock().expect("session mutex").clone() {
        req = req.header(SESSION_HEADER, sid);
    }
    // `MCP-Protocol-Version` is required on every POST (both versions'
    // transports specs; on `2025-11-25` from the first post-`initialize`
    // request onward — the handshake itself negotiates in-band).
    if let Some(v) = &version {
        req = req.header(mcp_headers::PROTOCOL_VERSION, v);
    }
    // The draft's standard request headers mirror body fields for
    // intermediaries: `Mcp-Method` on every request POST, `Mcp-Name` for
    // `tools/call`/`resources/read`/`prompts/get`. `2025-11-25` doesn't
    // define them.
    let is_draft = version
        .as_deref()
        .is_some_and(|v| ProtocolVersion::from_wire(v) == ProtocolVersion::V2026_07_28);
    if is_draft && let JsonRpcMessage::Request(r) = &msg {
        req = req.header(mcp_headers::MCP_METHOD, &r.method);
        if let Some(field) = mcp_headers::name_field_for(&r.method)
            && let Some(value) = r
                .params
                .as_ref()
                .and_then(|p| p.get(field))
                .and_then(serde_json::Value::as_str)
        {
            req = req.header(mcp_headers::MCP_NAME, mcp_headers::encode_value(value));
        }
    }
    for (name, value) in mirror_headers {
        req = req.header(format!("{}{name}", mcp_headers::MCP_PARAM_PREFIX), value);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;

    // Capture (or refresh) the session id from any response.
    if let Some(sid) = resp
        .headers()
        .get(SESSION_HEADER)
        .and_then(|v| v.to_str().ok())
    {
        *shared.session.lock().expect("session mutex") = Some(sid.to_string());
    }

    let status = resp.status();
    if !status.is_success() {
        return Err(format!("http status {status}"));
    }

    let is_sse = resp
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/event-stream"));

    if is_sse {
        // Each SSE `data:` event is one JSON-RPC frame; the stream ends after the
        // final response. Deliver every frame — the Connection correlates them.
        let mut events = resp.bytes_stream().eventsource();
        while let Some(event) = events.next().await {
            let event = event.map_err(|e| format!("sse stream error: {e}"))?;
            if event.data.is_empty() {
                continue; // keep-alive comment / empty event
            }
            let frame: JsonRpcMessage = serde_json::from_str(&event.data)
                .map_err(|e| format!("sse frame decode failed: {e}"))?;
            if shared.inbound_tx.send(frame).await.is_err() {
                break; // receiver gone
            }
        }
    } else {
        // application/json: a single response frame (202 with no body → nothing).
        let text = resp.text().await.map_err(|e| format!("read body: {e}"))?;
        if text.trim().is_empty() {
            return Ok(());
        }
        let frame: JsonRpcMessage =
            serde_json::from_str(&text).map_err(|e| format!("json decode failed: {e}"))?;
        let _ = shared.inbound_tx.send(frame).await;
    }
    Ok(())
}

/// Pull the `x-mcp-header` mirror signal — a map of header-name portion →
/// already-encoded value, built by the typed client from the tool's schema —
/// out of a request's `_meta`.
///
/// Mutates `msg`: removes the [`HEADER_PARAMS_META_KEY`](crate::client::HEADER_PARAMS_META_KEY)
/// entry so it never reaches the wire. The param values stay in `arguments`
/// (mirroring — the header is an HTTP-visible copy, not a move).
fn extract_header_params(msg: &mut JsonRpcMessage) -> Vec<(String, String)> {
    let JsonRpcMessage::Request(req) = msg else {
        return Vec::new();
    };
    req.params
        .as_mut()
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|params| params.get_mut("_meta"))
        .and_then(serde_json::Value::as_object_mut)
        .and_then(|meta| meta.remove(crate::client::HEADER_PARAMS_META_KEY))
        .and_then(|v| match v {
            serde_json::Value::Object(map) => Some(
                map.into_iter()
                    .filter_map(|(name, value)| match value {
                        serde_json::Value::String(s) => Some((name, s)),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

/// Resolve the `MCP-Protocol-Version` header value for `msg` and strip the
/// internal signal from the body: the typed client's negotiated-version
/// signal first, else the body's public `_meta` protocol version (draft
/// requests), else the last version seen on this connection (covers
/// responses to server requests and notifications, which carry no signal).
/// Remembers whatever it resolves. An emptied `_meta` is dropped entirely.
fn extract_protocol_version(msg: &mut JsonRpcMessage, shared: &Shared) -> Option<String> {
    let params = match msg {
        JsonRpcMessage::Request(r) => r.params.as_mut(),
        JsonRpcMessage::Notification(n) => n.params.as_mut(),
        JsonRpcMessage::Response(_) => None,
    }
    .and_then(serde_json::Value::as_object_mut);

    let mut version = None;
    if let Some(params) = params {
        if let Some(meta) = params
            .get_mut("_meta")
            .and_then(serde_json::Value::as_object_mut)
        {
            version = meta
                .remove(crate::client::NEGOTIATED_VERSION_META_KEY)
                .and_then(|v| v.as_str().map(str::to_owned))
                .or_else(|| {
                    meta.get(turbomcp_core::meta::keys::PROTOCOL_VERSION)
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned)
                });
        }
        if params
            .get("_meta")
            .and_then(serde_json::Value::as_object)
            .is_some_and(serde_json::Map::is_empty)
        {
            params.remove("_meta");
        }
    }

    let mut last = shared.version.lock().expect("version mutex");
    match version {
        Some(v) => {
            *last = Some(v.clone());
            Some(v)
        }
        None => last.clone(),
    }
}

/// Connect a [`Client`] to an MCP server over Streamable HTTP at `url`, running
/// the handshake.
///
/// # Errors
/// Propagates transport construction and handshake failures.
pub async fn connect_http(builder: ClientBuilder, url: impl Into<String>) -> ClientResult<Client> {
    let transport = HttpClientTransport::new(url)?;
    builder.connect(transport).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use turbomcp_core::JsonRpcRequest;

    #[test]
    fn extracts_marked_header_params_and_strips_the_signal() {
        // The signal is a map of header-name portion → already-encoded value.
        let mut msg = JsonRpcMessage::Request(JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({
                "name": "locate",
                "arguments": { "city": "SF", "region": "us-west", "n": 3 },
                "_meta": {
                    crate::client::HEADER_PARAMS_META_KEY: { "region": "us-west", "n": "3" },
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                },
            })),
        ));

        let mut headers = extract_header_params(&mut msg);
        headers.sort();
        assert_eq!(
            headers,
            vec![
                ("n".to_owned(), "3".to_owned()),
                ("region".to_owned(), "us-west".to_owned()),
            ]
        );

        // The signal is stripped; the values remain in `arguments`; other `_meta`
        // keys are untouched.
        let JsonRpcMessage::Request(req) = &msg else {
            unreachable!()
        };
        let params = req.params.as_ref().unwrap();
        assert!(
            params["_meta"]
                .get(crate::client::HEADER_PARAMS_META_KEY)
                .is_none()
        );
        assert_eq!(
            params["_meta"]["io.modelcontextprotocol/protocolVersion"],
            "2026-07-28"
        );
        assert_eq!(params["arguments"]["region"], "us-west");
    }

    /// The signal is built by the typed client, but it rides in `_meta` where
    /// anything can put a value. A non-string entry is dropped rather than
    /// stringified: `reqwest` would reject (or mangle) a header built from an
    /// object, and a silently coerced `"[object]"` is worse than an absent
    /// header. A signal that isn't a map at all yields nothing.
    #[test]
    fn malformed_header_signals_are_dropped_not_coerced() {
        let mut msg = JsonRpcMessage::Request(JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({
                "name": "locate",
                "arguments": {},
                "_meta": {
                    crate::client::HEADER_PARAMS_META_KEY: {
                        "good": "kept",
                        "nested": { "not": "a header" },
                        "numeric": 7,
                    },
                },
            })),
        ));
        assert_eq!(
            extract_header_params(&mut msg),
            vec![("good".to_owned(), "kept".to_owned())]
        );

        let mut msg = JsonRpcMessage::Request(JsonRpcRequest::new(
            2,
            "tools/call",
            Some(json!({
                "name": "locate",
                "_meta": { crate::client::HEADER_PARAMS_META_KEY: "not-a-map" },
            })),
        ));
        assert!(extract_header_params(&mut msg).is_empty());
    }

    #[test]
    fn no_signal_yields_no_headers() {
        let mut msg = JsonRpcMessage::Request(JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({ "name": "x", "arguments": { "a": 1 } })),
        ));
        assert!(extract_header_params(&mut msg).is_empty());
    }

    #[test]
    fn protocol_version_signal_is_lifted_and_remembered() {
        let shared = Shared {
            http: reqwest::Client::new(),
            url: "http://unused/mcp".into(),
            session: Mutex::new(None),
            version: Mutex::new(None),
            inbound_tx: mpsc::channel(1).0,
        };

        // A legacy request carries only the internal signal — lifted,
        // stripped, and the emptied `_meta` dropped from the wire body.
        let mut msg = JsonRpcMessage::Request(JsonRpcRequest::new(
            1,
            "tools/list",
            Some(json!({
                "_meta": { crate::client::NEGOTIATED_VERSION_META_KEY: "2025-11-25" },
            })),
        ));
        assert_eq!(
            extract_protocol_version(&mut msg, &shared).as_deref(),
            Some("2025-11-25")
        );
        let JsonRpcMessage::Request(req) = &msg else {
            unreachable!()
        };
        assert!(
            req.params.as_ref().unwrap().get("_meta").is_none(),
            "an emptied _meta is dropped"
        );

        // A signal-less follow-up (e.g. a response to a server request) falls
        // back to the remembered version.
        let mut response = JsonRpcMessage::Response(JsonRpcResponse::success(
            turbomcp_core::RequestId::from(2),
            json!({}),
        ));
        assert_eq!(
            extract_protocol_version(&mut response, &shared).as_deref(),
            Some("2025-11-25")
        );
    }
}
