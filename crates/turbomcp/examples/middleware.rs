//! # RPC middleware (v4)
//!
//! Cross-cutting concerns — audit logging, access control, quotas — are
//! [`tower::Layer`]s wrapped around the built dispatcher. One `call` sees every
//! method under every transport; there is no per-operation hook list to keep in
//! sync with the protocol. (v3 users: this replaces `McpMiddleware`'s
//! `on_call_tool` / `on_read_resource` / … hooks — see `MIGRATION.md`.)
//!
//! Two layers, covering the two things middleware ever does:
//!
//! - [`AuditLayer`] **observes**: it times every RPC and records the outcome,
//!   including failures the handler itself produced.
//! - [`PolicyLayer`] **intercepts**: it refuses `tools/call` for a denied tool
//!   and answers the client itself, so the dispatcher never runs.
//!
//! Run the in-process demo (drives frames through the stack and prints the
//! audit trail):
//!
//! ```text
//! cargo run -p turbomcp --example middleware
//! ```
//!
//! Or serve the same stack over stdio, which is what a real binary does:
//!
//! ```text
//! cargo run -p turbomcp --example middleware -- --stdio
//! ```

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::Instant;

use serde_json::{Value, json};
use turbomcp::methods::request;
use turbomcp::prelude::*;
use turbomcp::tower::{Layer, Service, ServiceBuilder, ServiceExt};
use turbomcp::{
    JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, LegacySessionAdapter, ProtocolError,
    VersionDispatcher, mcp_to_jsonrpc_error, serve_stdio,
};

// ---- the server being wrapped ------------------------------------------------

#[derive(Clone)]
struct Files;

#[server(name = "files", version = "1.0.0")]
impl Files {
    /// Read a file's contents.
    #[tool(description = "Read a file")]
    async fn read(&self, path: String) -> McpResult<String> {
        Ok(format!("(contents of {path})"))
    }

    /// Delete a file. Guarded by [`PolicyLayer`] below.
    #[tool(description = "Delete a file")]
    async fn delete(&self, path: String) -> McpResult<String> {
        Ok(format!("deleted {path}"))
    }
}

// ---- layer 1: observe --------------------------------------------------------

/// Records the method, outcome, and duration of every RPC into a shared log.
///
/// The log lives behind an `Arc` because the driver clones the whole service
/// stack for each inbound frame — a layer that owned its state would count each
/// request into a different copy.
#[derive(Clone, Default)]
struct AuditLayer {
    log: Arc<Mutex<Vec<String>>>,
}

impl AuditLayer {
    fn entries(&self) -> Vec<String> {
        self.log.lock().expect("audit log poisoned").clone()
    }
}

impl<S> Layer<S> for AuditLayer {
    type Service = Audit<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Audit {
            inner,
            log: Arc::clone(&self.log),
        }
    }
}

#[derive(Clone)]
struct Audit<S> {
    inner: S,
    log: Arc<Mutex<Vec<String>>>,
}

impl<S> Service<JsonRpcMessage> for Audit<S>
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error = ProtocolError>,
    S::Future: Send + 'static,
{
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: JsonRpcMessage) -> Self::Future {
        let method = req.method().unwrap_or("(response)").to_owned();
        let log = Arc::clone(&self.log);
        let started = Instant::now();
        let fut = self.inner.call(req);

        Box::pin(async move {
            let result = fut.await;
            // A handler that returned `McpError` is a *successful* call carrying
            // an error response — `Err` here means the connection machinery
            // failed, which is a much rarer thing and worth distinguishing.
            let outcome = match &result {
                Ok(Some(JsonRpcMessage::Response(r))) if r.is_error() => "error",
                Ok(_) => "ok",
                Err(_) => "protocol-error",
            };
            log.lock()
                .expect("audit log poisoned")
                .push(format!("{method} → {outcome} in {:?}", started.elapsed()));
            result
        })
    }
}

// ---- layer 2: intercept ------------------------------------------------------

/// Refuses `tools/call` for any tool in the deny list, answering the client
/// directly instead of calling the inner service.
#[derive(Clone)]
struct PolicyLayer {
    denied: Arc<[&'static str]>,
}

impl PolicyLayer {
    fn denying(tools: &'static [&'static str]) -> Self {
        Self {
            denied: tools.into(),
        }
    }
}

impl<S> Layer<S> for PolicyLayer {
    type Service = Policy<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Policy {
            inner,
            denied: Arc::clone(&self.denied),
        }
    }
}

#[derive(Clone)]
struct Policy<S> {
    inner: S,
    denied: Arc<[&'static str]>,
}

impl<S> Policy<S> {
    /// The tool a `tools/call` request names, if this frame is one.
    fn called_tool(req: &JsonRpcMessage) -> Option<&str> {
        let JsonRpcMessage::Request(r) = req else {
            return None;
        };
        if r.method != request::TOOLS_CALL {
            return None;
        }
        r.params.as_ref()?.get("name")?.as_str()
    }
}

impl<S> Service<JsonRpcMessage> for Policy<S>
where
    S: Service<JsonRpcMessage, Response = Option<JsonRpcMessage>, Error = ProtocolError>,
    S::Future: Send + 'static,
{
    type Response = Option<JsonRpcMessage>;
    type Error = ProtocolError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: JsonRpcMessage) -> Self::Future {
        let refuse = Self::called_tool(&req).is_some_and(|tool| self.denied.contains(&tool));
        if refuse {
            // Only a `Request` reaches here (`called_tool` matched one), so the
            // id needed to answer is always present.
            let JsonRpcMessage::Request(r) = &req else {
                unreachable!("called_tool only matches requests")
            };
            // `mcp_to_jsonrpc_error` keeps the code identical to what the
            // dispatcher would have produced for the same refusal — a
            // hand-picked number here would drift from the rest of the SDK.
            let error = mcp_to_jsonrpc_error(&McpError::permission_denied(
                "this deployment does not permit that tool",
            ));
            let response = JsonRpcResponse::error(r.id.clone(), error);
            return Box::pin(std::future::ready(Ok(Some(response.into()))));
        }
        Box::pin(self.inner.call(req))
    }
}

// ---- wiring ------------------------------------------------------------------

/// The composed stack. `ServiceBuilder` applies layers outside-in: `Audit` wraps
/// `Policy`, so the audit log records refusals too.
///
/// Both layers sit *outside* [`LegacySessionAdapter`], seeing frames as the
/// client sent them. Wrapping the other way
/// (`LegacySessionAdapter::new(layers.service(dispatcher))`) puts them inside,
/// where the negotiated protocol version and session id are already stamped into
/// `_meta` — the right side for anything that varies by protocol revision.
///
/// The return type spells the stack out: layers are types, so a mis-stacked
/// service is a compile error rather than a runtime surprise.
fn stack(audit: AuditLayer) -> Audit<Policy<LegacySessionAdapter<VersionDispatcher<Files>>>> {
    ServiceBuilder::new()
        .layer(audit)
        .layer(PolicyLayer::denying(&["delete"]))
        .service(LegacySessionAdapter::new(Files.into_server().build()))
}

#[tokio::main]
async fn main() -> Result<(), ProtocolError> {
    let audit = AuditLayer::default();
    let service = stack(audit.clone());

    // The production path: identical stack, driven by the stdio transport.
    if std::env::args().any(|a| a == "--stdio") {
        return serve_stdio(service).await;
    }

    // The demo path: drive frames through the same stack in-process.
    let send = |frame: JsonRpcRequest| {
        let svc = service.clone();
        async move { svc.oneshot(frame.into()).await }
    };

    let init = send(JsonRpcRequest::new(
        1,
        request::INITIALIZE,
        Some(json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "demo", "version": "1.0.0" },
        })),
    ))
    .await?;
    println!("initialize   → {}", summarize(init.as_ref()));

    let allowed = send(JsonRpcRequest::new(
        2,
        request::TOOLS_CALL,
        Some(json!({ "name": "read", "arguments": { "path": "/etc/hosts" } })),
    ))
    .await?;
    println!("read         → {}", summarize(allowed.as_ref()));

    let refused = send(JsonRpcRequest::new(
        3,
        request::TOOLS_CALL,
        Some(json!({ "name": "delete", "arguments": { "path": "/etc/hosts" } })),
    ))
    .await?;
    println!("delete       → {}", summarize(refused.as_ref()));

    println!("\naudit log (the outermost layer saw all three):");
    for entry in audit.entries() {
        println!("  {entry}");
    }

    Ok(())
}

/// One line describing what came back, for the demo output.
fn summarize(msg: Option<&JsonRpcMessage>) -> String {
    let Some(JsonRpcMessage::Response(r)) = msg else {
        return "(no response)".to_string();
    };
    match (&r.result, &r.error) {
        (_, Some(e)) => format!("error {} — {}", e.code, e.message),
        (Some(result), _) => {
            let text = result
                .get("content")
                .and_then(Value::as_array)
                .and_then(|c| c.first())
                .and_then(|b| b.get("text"))
                .and_then(Value::as_str);
            text.map_or_else(|| "ok".to_string(), |t| format!("ok — {t}"))
        }
        _ => "ok".to_string(),
    }
}
