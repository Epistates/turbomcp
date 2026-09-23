//! The HTTP edge shared by every WASM entry point.
//!
//! Until 3.5.0 the crate had six JSON-RPC dispatchers — `McpServer::handle`,
//! the Streamable HTTP handler, the middleware stack, the visibility layer, the
//! composite server and `WasmHandlerExt` — and only the last one went through
//! `turbomcp_core::router`. Every protocol fix made in core (notifications
//! answered with 202, version echo and fallback, `-32002` for a missing
//! resource, cursor validation, template matching) therefore reached one entry
//! point out of six. This module is the one place the rest now share:
//!
//! - [`route`] is the only JSON-RPC dispatch in the crate. It delegates to the
//!   core router and applies the protocol-version adapter for the session's
//!   negotiated version, which is how a 2025-06-18 client is kept from seeing
//!   2025-11-25-only fields.
//! - [`parse_message`] turns a body into a request or the JSON-RPC error the
//!   spec prescribes for it (`-32700`, or `-32600` with `id: null`).
//! - [`check_origin`] is the DNS-rebinding guard, with the same semantics as the
//!   native HTTP transport.
//! - [`answer_post`] is the stateless POST endpoint, written against plain data
//!   so it can be exercised without a Workers runtime; [`serve`] is the thin
//!   adapter from a `worker::Request` onto it.

use std::collections::HashMap;
use std::future::Future;

use serde_json::Value;
use turbomcp_core::context::RequestContext;
use turbomcp_core::error::McpError;
use turbomcp_core::handler::McpHandler;
use turbomcp_core::jsonrpc::{JsonRpcIncoming, JsonRpcOutgoing};
use turbomcp_core::router::{RouteConfig, parse_request_from_value, route_request};
use turbomcp_protocol::versioning::adapter::{VersionAdapter, adapter_for_version};
use turbomcp_types::ProtocolVersion;

/// Header carrying the negotiated protocol version on every post-initialize
/// request (MCP 2025-06-18 and later).
pub(crate) const PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";

/// Default request body limit (1 MiB), the limit the WASM server has always
/// applied.
pub(crate) const DEFAULT_MAX_BODY_SIZE: usize = 1024 * 1024;

/// Request headers the CORS layer lets a browser send.
const CORS_ALLOW_HEADERS: &str = "Content-Type, Accept, Authorization, X-Request-ID, \
     Mcp-Session-Id, MCP-Protocol-Version, Last-Event-ID";

/// Response headers a browser script may read.
const CORS_EXPOSE_HEADERS: &str = "Mcp-Session-Id, WWW-Authenticate";

/// HTTP-level policy for the plain (stateless) JSON-RPC endpoint.
///
/// The defaults match the native HTTP transport: an `Origin` header, when a
/// browser sends one, must name a loopback host or an origin on the
/// allowlist, and anything else is refused with `403` before the body is read.
/// Requests without `Origin` — curl, SDK clients, server-to-server calls — are
/// unaffected, because DNS rebinding needs a browser and browsers always send
/// it.
///
/// A Worker serving a browser application on another origin must list that
/// origin:
///
/// ```ignore
/// let config = EndpointConfig::default().allow_origin("https://app.example.com");
/// server.handle_worker_request_with_config(req, &config).await
/// ```
#[derive(Debug, Clone)]
pub struct EndpointConfig {
    /// Origins allowed in addition to loopback ones.
    pub allowed_origins: Vec<String>,
    /// Whether `http(s)://localhost`, `127.0.0.1` and `[::1]` origins are
    /// allowed. Default: `true`.
    pub allow_localhost_origins: bool,
    /// Disable origin validation entirely. Default: `false`.
    pub allow_any_origin: bool,
    /// Maximum request body in bytes. Default: 1 MiB.
    pub max_body_size: usize,
}

impl Default for EndpointConfig {
    fn default() -> Self {
        Self {
            allowed_origins: Vec::new(),
            allow_localhost_origins: true,
            allow_any_origin: false,
            max_body_size: DEFAULT_MAX_BODY_SIZE,
        }
    }
}

impl EndpointConfig {
    /// Allow a browser origin, e.g. `https://app.example.com`.
    #[must_use]
    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        self.allowed_origins.push(origin.into());
        self
    }

    /// Allow several browser origins.
    #[must_use]
    pub fn allow_origins<I, S>(mut self, origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_origins
            .extend(origins.into_iter().map(Into::into));
        self
    }

    /// Control whether loopback origins are accepted.
    #[must_use]
    pub fn allow_localhost_origins(mut self, allow: bool) -> Self {
        self.allow_localhost_origins = allow;
        self
    }

    /// Accept any origin. Only for endpoints that are not reachable from a
    /// browser, or that authenticate every request.
    #[must_use]
    pub fn allow_any_origin(mut self, allow: bool) -> Self {
        self.allow_any_origin = allow;
        self
    }

    /// Set the request body limit in bytes.
    #[must_use]
    pub fn max_body_size(mut self, size: usize) -> Self {
        self.max_body_size = size;
        self
    }
}

// =============================================================================
// Origin validation
// =============================================================================

/// Decide whether a request's `Origin` may reach the server.
///
/// Same rules as the native HTTP transport: an absent header passes (only
/// browsers send one, and they always do cross-origin), a present one must be
/// on the allowlist or — when `allow_localhost` — name a loopback host.
/// Comparison is on the canonical `(scheme, host, port)` so `https://Example.com`
/// and `https://example.com:443` are the same origin, and `http://localhost.evil.com`
/// or `http://localhost@evil.com` are not a loopback one.
pub(crate) fn check_origin(
    origin: Option<&str>,
    allowed: &[String],
    allow_localhost: bool,
    allow_any: bool,
) -> Result<(), String> {
    if allow_any {
        return Ok(());
    }
    let Some(origin) = origin else {
        return Ok(());
    };
    let Some(canonical) = canonical_origin(origin) else {
        return Err(format!("Origin '{origin}' is not a valid origin"));
    };
    if allowed
        .iter()
        .filter_map(|entry| canonical_origin(entry))
        .any(|entry| entry == canonical)
    {
        return Ok(());
    }
    if allow_localhost && is_loopback_origin(&canonical.0, &canonical.1) {
        return Ok(());
    }
    Err(format!("Origin '{origin}' not allowed"))
}

/// `scheme://host[:port]` reduced to its canonical parts, or `None` for
/// anything carrying a path, query, fragment or userinfo.
fn canonical_origin(input: &str) -> Option<(String, String, u16)> {
    let url = url::Url::parse(input.trim()).ok()?;
    if !(url.path().is_empty() || url.path() == "/") {
        return None;
    }
    if url.query().is_some() || url.fragment().is_some() || !url.username().is_empty() {
        return None;
    }
    let host = match url.host()? {
        url::Host::Domain(domain) => domain.to_ascii_lowercase(),
        url::Host::Ipv4(ip) => ip.to_string(),
        url::Host::Ipv6(ip) => format!("[{ip}]"),
    };
    Some((
        url.scheme().to_ascii_lowercase(),
        host,
        url.port_or_known_default()?,
    ))
}

fn is_loopback_origin(scheme: &str, host: &str) -> bool {
    if scheme != "http" && scheme != "https" {
        return false;
    }
    if host == "localhost" {
        return true;
    }
    if let Ok(v4) = host.parse::<std::net::Ipv4Addr>() {
        return v4.is_loopback();
    }
    host.strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .and_then(|inner| inner.parse::<std::net::Ipv6Addr>().ok())
        .is_some_and(|v6| v6.is_loopback())
}

// =============================================================================
// Replies
// =============================================================================

/// An HTTP reply before it becomes a `worker::Response`.
///
/// Kept as plain data so the endpoint logic is testable on the host, where the
/// Workers types cannot be constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Reply {
    pub status: u16,
    pub content_type: Option<&'static str>,
    pub body: String,
    pub headers: Vec<(&'static str, String)>,
}

impl Reply {
    /// A JSON-RPC message.
    pub fn rpc(status: u16, message: &JsonRpcOutgoing) -> Self {
        let body = serde_json::to_string(message).unwrap_or_else(|_| {
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Failed to serialize response"}}"#
                .to_string()
        });
        Self {
            status,
            content_type: Some("application/json"),
            body,
            headers: Vec::new(),
        }
    }

    /// No body. `202` for accepted notifications, `204` for preflight and
    /// session deletion.
    pub fn empty(status: u16) -> Self {
        Self {
            status,
            content_type: None,
            body: String::new(),
            headers: Vec::new(),
        }
    }

    /// An HTTP-level refusal, explained in plain text.
    pub fn text(status: u16, message: impl Into<String>) -> Self {
        Self {
            status,
            content_type: Some("text/plain; charset=utf-8"),
            body: message.into(),
            headers: Vec::new(),
        }
    }

    /// An SSE body.
    pub fn sse(body: String) -> Self {
        Self {
            status: 200,
            content_type: Some("text/event-stream"),
            body,
            headers: vec![("Cache-Control", "no-cache".to_string())],
        }
    }

    /// Add a response header.
    #[must_use]
    pub fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    /// `405`, which RFC 9110 requires to carry `Allow`.
    pub fn method_not_allowed(allow: &'static str) -> Self {
        Self::text(405, "Method not allowed").with_header("Allow", allow)
    }
}

// =============================================================================
// JSON-RPC
// =============================================================================

/// What a POSTed body turned out to be.
#[derive(Debug)]
pub(crate) enum Inbound {
    /// A request or notification.
    Message(JsonRpcIncoming),
    /// A JSON-RPC response from the client. WASM servers never send requests to
    /// clients, so there is nothing waiting for it; the transport spec still
    /// asks for `202 Accepted`.
    ClientResponse,
    /// Not a message at all, answered with the JSON-RPC error the spec
    /// prescribes and `id: null`, since no id could be trusted.
    Invalid(JsonRpcOutgoing),
}

/// Parse a request body.
///
/// Malformed JSON is `-32700`; valid JSON of the wrong shape — a batch array,
/// `jsonrpc` other than `"2.0"`, an `id` that is `null`, fractional or not a
/// string or integer — is `-32600`. MCP forbids a `null` id outright, so it is
/// refused rather than taken to mean "notification".
pub(crate) fn parse_message(body: &str) -> Inbound {
    let value: Value = match serde_json::from_str(body) {
        Ok(value) => value,
        Err(error) => {
            return Inbound::Invalid(JsonRpcOutgoing::error(
                None,
                McpError::parse_error(format!("Parse error: {error}")),
            ));
        }
    };

    if let Value::Object(object) = &value
        && !object.contains_key("method")
        && object.contains_key("id")
        && (object.contains_key("result") || object.contains_key("error"))
    {
        return Inbound::ClientResponse;
    }

    match parse_request_from_value(value) {
        Ok(request) => Inbound::Message(request),
        Err(error) => Inbound::Invalid(JsonRpcOutgoing::error(None, error)),
    }
}

/// Parse an `MCP-Protocol-Version` header value.
///
/// The transport spec requires `400 Bad Request` for a version the server does
/// not support; the caller turns the `Err` into that.
pub(crate) fn protocol_version_from_header(
    value: Option<&str>,
) -> Result<Option<ProtocolVersion>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    let version = ProtocolVersion::from(value.trim());
    if ProtocolVersion::STABLE.contains(&version) {
        Ok(Some(version))
    } else {
        Err(format!(
            "Unsupported MCP-Protocol-Version '{value}'; supported: {}",
            ProtocolVersion::STABLE
                .iter()
                .map(ProtocolVersion::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ))
    }
}

/// Route one JSON-RPC message through the core router.
///
/// `initialize` is answered with the version core negotiated — the client's
/// when supported, otherwise the latest — and filtered through that version's
/// adapter. Every later request is filtered through the adapter for
/// `version`, the one the session negotiated; `None` (a stateless endpoint
/// with no `MCP-Protocol-Version` header) leaves the response as core built it.
pub(crate) async fn route<H: McpHandler>(
    handler: &H,
    request: JsonRpcIncoming,
    ctx: &RequestContext,
    version: Option<&ProtocolVersion>,
) -> JsonRpcOutgoing {
    let config = RouteConfig::default();
    if request.is_notification() {
        return route_request(handler, request, ctx, &config).await;
    }

    let method = request.method.clone();
    if method == "initialize" {
        let response = route_request(handler, request, ctx, &config).await;
        return match negotiated_version(&response) {
            Some(negotiated) => filter(adapter_for_version(&negotiated), &method, response),
            None => response,
        };
    }

    let Some(version) = version else {
        return route_request(handler, request, ctx, &config).await;
    };
    let adapter = adapter_for_version(version);
    if let Err(reason) = adapter.validate_method(&method) {
        return JsonRpcOutgoing::error(request.id, McpError::method_not_found(reason));
    }
    let response = route_request(handler, request, ctx, &config).await;
    filter(adapter, &method, response)
}

/// The `protocolVersion` a successful `initialize` response settled on.
pub(crate) fn negotiated_version(response: &JsonRpcOutgoing) -> Option<ProtocolVersion> {
    response
        .result
        .as_ref()?
        .get("protocolVersion")?
        .as_str()
        .map(ProtocolVersion::from)
}

fn filter(
    adapter: &dyn VersionAdapter,
    method: &str,
    mut response: JsonRpcOutgoing,
) -> JsonRpcOutgoing {
    if let Some(result) = response.result.take() {
        response.result = Some(adapter.filter_result(method, result));
    }
    response
}

// =============================================================================
// The stateless POST endpoint
// =============================================================================

/// Request headers, keyed by lower-case name.
pub(crate) type HeaderMap = HashMap<String, String>;

/// Checks that need only the method and headers, so a refused request is
/// turned away before its body is read.
pub(crate) fn precheck(
    method: &str,
    headers: &HeaderMap,
    config: &EndpointConfig,
) -> Result<(), Reply> {
    check_origin(
        headers.get("origin").map(String::as_str),
        &config.allowed_origins,
        config.allow_localhost_origins,
        config.allow_any_origin,
    )
    .map_err(|reason| Reply::text(403, reason))?;

    if method == "OPTIONS" {
        return Err(Reply::empty(204));
    }
    if method != "POST" {
        return Err(Reply::method_not_allowed("POST, OPTIONS"));
    }
    if !is_json_content_type(headers.get("content-type").map(String::as_str)) {
        return Err(Reply::text(
            415,
            "Unsupported Media Type. Use Content-Type: application/json",
        ));
    }
    if headers
        .get("content-length")
        .and_then(|length| length.parse::<usize>().ok())
        .is_some_and(|length| length > config.max_body_size)
    {
        return Err(Reply::text(413, "Request body too large"));
    }
    Ok(())
}

/// Whether a `Content-Type` names JSON. A missing header is refused: browsers
/// default `fetch()` bodies to `text/plain`, which is exactly what a simple
/// (preflight-free) cross-origin request looks like.
pub(crate) fn is_json_content_type(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|value| {
        let mime = value.split(';').next().unwrap_or("").trim();
        mime.eq_ignore_ascii_case("application/json")
    })
}

/// Build the request context from the HTTP headers.
///
/// No session id is taken from the request: a stateless endpoint issues none,
/// and trusting a client-chosen `Mcp-Session-Id` would let any caller read and
/// write another caller's `RichContextExt` session state by naming it.
pub(crate) fn request_context(headers: &HeaderMap) -> RequestContext {
    super::context::from_worker_request(
        headers.get("x-request-id").cloned(),
        None,
        headers.iter().map(|(k, v)| (k.clone(), v.clone())),
    )
}

/// Answer a POST whose body has been read.
///
/// `admit` runs once the method is known and before anything is dispatched;
/// it may enrich the context (the authentication wrapper attaches the
/// principal there) or refuse the request with its own reply. It is given
/// `None` for a JSON-RPC response from the client.
pub(crate) async fn answer_post<H, A, Fut>(
    handler: &H,
    body: &str,
    headers: &HeaderMap,
    ctx: RequestContext,
    admit: A,
) -> Reply
where
    H: McpHandler,
    A: FnOnce(Option<String>, RequestContext) -> Fut,
    Fut: Future<Output = Result<RequestContext, Reply>>,
{
    let version = match protocol_version_from_header(
        headers.get(PROTOCOL_VERSION_HEADER).map(String::as_str),
    ) {
        Ok(version) => version,
        Err(reason) => return Reply::text(400, reason),
    };

    let request = match parse_message(body) {
        Inbound::Message(request) => request,
        Inbound::ClientResponse => {
            return match admit(None, ctx).await {
                Ok(_) => Reply::empty(202),
                Err(reply) => reply,
            };
        }
        Inbound::Invalid(error) => return Reply::rpc(400, &error),
    };

    let ctx = match admit(Some(request.method.clone()), ctx).await {
        Ok(ctx) => ctx,
        Err(reply) => return reply,
    };

    let response = route(handler, request, &ctx, version.as_ref()).await;
    if response.should_send() {
        Reply::rpc(200, &response)
    } else {
        // Notifications get no JSON-RPC response; the transport spec's answer
        // is 202 with an empty body.
        Reply::empty(202)
    }
}

/// Admit every request unchanged.
pub(crate) async fn admit_all(
    _method: Option<String>,
    ctx: RequestContext,
) -> Result<RequestContext, Reply> {
    Ok(ctx)
}

// =============================================================================
// Workers glue
// =============================================================================

/// Collect a Worker request's headers. Header names from the Fetch API are
/// already lower-case.
pub(crate) fn header_map(req: &worker::Request) -> HeaderMap {
    req.headers().entries().collect()
}

/// Read a request body, holding it to `max` bytes.
///
/// Workers buffer the whole body before handing it over, so the length check
/// after the read is what catches chunked uploads and a lying
/// `Content-Length`; `precheck` has already refused an honest oversized one.
pub(crate) async fn read_body(req: &mut worker::Request, max: usize) -> Result<String, Reply> {
    match req.text().await {
        Ok(body) if body.len() > max => Err(Reply::text(413, "Request body too large")),
        Ok(body) => Ok(body),
        Err(error) => Err(Reply::text(
            400,
            format!("Failed to read request body: {error}"),
        )),
    }
}

/// Serve the stateless JSON-RPC endpoint for any handler.
pub(crate) async fn serve<H, A, Fut>(
    handler: &H,
    mut req: worker::Request,
    config: &EndpointConfig,
    customize: impl FnOnce(RequestContext) -> RequestContext,
    admit: A,
) -> worker::Result<worker::Response>
where
    H: McpHandler,
    A: FnOnce(Option<String>, RequestContext) -> Fut,
    Fut: Future<Output = Result<RequestContext, Reply>>,
{
    let headers = header_map(&req);
    let origin = headers.get("origin").cloned();
    let method = req.method().to_string().to_ascii_uppercase();

    let reply = match precheck(&method, &headers, config) {
        Err(reply) => reply,
        Ok(()) => match read_body(&mut req, config.max_body_size).await {
            Err(reply) => reply,
            Ok(body) => {
                let ctx = customize(request_context(&headers));
                answer_post(handler, &body, &headers, ctx, admit).await
            }
        },
    };
    into_response(reply, origin.as_deref(), "POST, OPTIONS")
}

/// Turn a [`Reply`] into a `worker::Response` with CORS headers.
///
/// The request `Origin` is echoed rather than answered with `*`, so a
/// credentialed browser request is only ever granted to the origin that made
/// it; a refused origin (`403`) is granted nothing.
pub(crate) fn into_response(
    reply: Reply,
    origin: Option<&str>,
    allow_methods: &str,
) -> worker::Result<worker::Response> {
    let headers = worker::Headers::new();
    if reply.status != 403 {
        match origin {
            Some(origin) => {
                headers.set("Access-Control-Allow-Origin", origin)?;
                headers.set("Vary", "Origin")?;
            }
            None => headers.set("Access-Control-Allow-Origin", "*")?,
        }
        headers.set("Access-Control-Allow-Methods", allow_methods)?;
        headers.set("Access-Control-Allow-Headers", CORS_ALLOW_HEADERS)?;
        headers.set("Access-Control-Expose-Headers", CORS_EXPOSE_HEADERS)?;
        headers.set("Access-Control-Max-Age", "86400")?;
    }
    if let Some(content_type) = reply.content_type {
        headers.set("Content-Type", content_type)?;
    }
    for (name, value) in &reply.headers {
        headers.set(name, value)?;
    }

    let response = if reply.body.is_empty() {
        worker::Response::empty()?
    } else {
        worker::Response::ok(reply.body)?
    };
    Ok(response.with_status(reply.status).with_headers(headers))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wasm_server::McpServer;

    fn server() -> McpServer {
        McpServer::builder("endpoint-test", "1.0.0")
            .tool_raw(
                "echo",
                "Echo",
                |args: Value| async move { args.to_string() },
            )
            .build()
    }

    fn json_headers() -> HeaderMap {
        HashMap::from([("content-type".to_string(), "application/json".to_string())])
    }

    async fn post(body: &str, headers: &HeaderMap) -> Reply {
        answer_post(
            &server(),
            body,
            headers,
            request_context(headers),
            admit_all,
        )
        .await
    }

    fn body_json(reply: &Reply) -> Value {
        serde_json::from_str(&reply.body).expect("reply body is JSON")
    }

    #[tokio::test]
    async fn notification_is_accepted_with_an_empty_202() {
        let reply = post(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            &json_headers(),
        )
        .await;
        assert_eq!(reply, Reply::empty(202));

        // Unknown notifications are tolerated, not answered with an error.
        let reply = post(
            r#"{"jsonrpc":"2.0","method":"notifications/whatever"}"#,
            &json_headers(),
        )
        .await;
        assert_eq!(reply, Reply::empty(202));
    }

    #[tokio::test]
    async fn client_response_is_accepted_with_an_empty_202() {
        let reply = post(r#"{"jsonrpc":"2.0","id":7,"result":{}}"#, &json_headers()).await;
        assert_eq!(reply, Reply::empty(202));
    }

    #[tokio::test]
    async fn malformed_json_is_a_parse_error_with_null_id() {
        let reply = post("{not json", &json_headers()).await;
        assert_eq!(reply.status, 400);
        let body = body_json(&reply);
        assert_eq!(body["error"]["code"], -32700);
        assert!(body.as_object().unwrap().contains_key("id"));
        assert!(body["id"].is_null());
    }

    #[tokio::test]
    async fn bad_envelopes_are_invalid_requests_with_null_id() {
        for body in [
            r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":1.5,"method":"ping"}"#,
            r#"{"jsonrpc":"2.0","id":{"a":1},"method":"ping"}"#,
            r#"{"jsonrpc":"1.0","id":1,"method":"ping"}"#,
            r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#,
            r#"{"jsonrpc":"2.0","id":1}"#,
        ] {
            let reply = post(body, &json_headers()).await;
            assert_eq!(reply.status, 400, "{body}");
            let json = body_json(&reply);
            assert_eq!(json["error"]["code"], -32600, "{body}");
            assert!(json["id"].is_null(), "{body}");
        }
    }

    #[tokio::test]
    async fn ping_is_answered() {
        let reply = post(
            r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            &json_headers(),
        )
        .await;
        assert_eq!(reply.status, 200);
        assert_eq!(body_json(&reply)["result"], serde_json::json!({}));
    }

    #[tokio::test]
    async fn initialize_echoes_a_supported_version_and_falls_back_otherwise() {
        let init = |version: &str| {
            format!(
                r#"{{"jsonrpc":"2.0","id":1,"method":"initialize","params":{{"protocolVersion":"{version}","capabilities":{{}},"clientInfo":{{"name":"c","version":"1"}}}}}}"#
            )
        };

        let reply = post(&init("2025-06-18"), &json_headers()).await;
        assert_eq!(body_json(&reply)["result"]["protocolVersion"], "2025-06-18");

        let reply = post(&init("1999-01-01"), &json_headers()).await;
        assert_eq!(
            body_json(&reply)["result"]["protocolVersion"],
            turbomcp_core::PROTOCOL_VERSION
        );
    }

    #[tokio::test]
    async fn initialize_requires_client_info() {
        let reply = post(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{}}}"#,
            &json_headers(),
        )
        .await;
        assert_eq!(body_json(&reply)["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn unsupported_protocol_version_header_is_a_400() {
        let mut headers = json_headers();
        headers.insert(PROTOCOL_VERSION_HEADER.into(), "1999-01-01".into());
        let reply = post(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#, &headers).await;
        assert_eq!(reply.status, 400);
    }

    #[tokio::test]
    async fn protocol_version_header_steps_results_down_to_2025_06_18() {
        let server = McpServer::builder("endpoint-test", "1.0.0")
            .tool_raw(
                "echo",
                "Echo",
                |args: Value| async move { args.to_string() },
            )
            .build();
        let mut headers = json_headers();
        headers.insert(PROTOCOL_VERSION_HEADER.into(), "2025-06-18".into());

        // `tasks/list` did not exist in 2025-06-18.
        let reply = answer_post(
            &server,
            r#"{"jsonrpc":"2.0","id":1,"method":"tasks/list"}"#,
            &headers,
            request_context(&headers),
            admit_all,
        )
        .await;
        assert_eq!(body_json(&reply)["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn admit_can_refuse_before_dispatch() {
        let headers = json_headers();
        let reply = answer_post(
            &server(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            &headers,
            request_context(&headers),
            |method, _ctx| async move {
                assert_eq!(method.as_deref(), Some("tools/list"));
                Err(Reply::text(401, "no"))
            },
        )
        .await;
        assert_eq!(reply.status, 401);
    }

    #[test]
    fn request_context_ignores_client_session_headers() {
        let headers = HashMap::from([
            ("mcp-session-id".to_string(), "someone-else".to_string()),
            ("x-session-id".to_string(), "someone-else".to_string()),
        ]);
        assert_eq!(request_context(&headers).session_id(), None);
    }

    #[test]
    fn origin_rules_match_the_native_transport() {
        let allowed = vec!["https://app.example.com".to_string()];
        // Absent: non-browser clients pass.
        assert!(check_origin(None, &[], true, false).is_ok());
        // Loopback passes by default, in every spelling.
        for origin in [
            "http://localhost:3000",
            "http://127.0.0.1",
            "https://[::1]:8443",
        ] {
            assert!(
                check_origin(Some(origin), &[], true, false).is_ok(),
                "{origin}"
            );
        }
        // Lookalikes of loopback do not.
        for origin in [
            "http://localhost.evil.com",
            "http://localhost@evil.com",
            "https://evil.com",
            "null",
        ] {
            assert!(
                check_origin(Some(origin), &[], true, false).is_err(),
                "{origin}"
            );
        }
        // The allowlist compares canonical origins.
        assert!(check_origin(Some("https://App.Example.com:443"), &allowed, false, false).is_ok());
        assert!(check_origin(Some("https://other.example.com"), &allowed, false, false).is_err());
        // Loopback can be switched off, and validation can be switched off.
        assert!(check_origin(Some("http://localhost"), &[], false, false).is_err());
        assert!(check_origin(Some("https://evil.com"), &[], true, true).is_ok());
    }

    #[test]
    fn precheck_refuses_before_the_body_is_read() {
        let config = EndpointConfig::default();

        let mut headers = json_headers();
        headers.insert("origin".into(), "https://evil.com".into());
        assert_eq!(precheck("POST", &headers, &config).unwrap_err().status, 403);

        assert_eq!(
            precheck("GET", &json_headers(), &config).unwrap_err(),
            Reply::method_not_allowed("POST, OPTIONS")
        );
        assert_eq!(
            precheck("PUT", &json_headers(), &config)
                .unwrap_err()
                .status,
            405
        );
        assert_eq!(
            precheck("OPTIONS", &json_headers(), &config).unwrap_err(),
            Reply::empty(204)
        );

        let mut headers = HashMap::new();
        headers.insert("content-type".to_string(), "text/plain".to_string());
        assert_eq!(precheck("POST", &headers, &config).unwrap_err().status, 415);

        let mut headers = json_headers();
        headers.insert(
            "content-length".into(),
            (DEFAULT_MAX_BODY_SIZE + 1).to_string(),
        );
        assert_eq!(precheck("POST", &headers, &config).unwrap_err().status, 413);

        assert!(precheck("POST", &json_headers(), &config).is_ok());
    }

    #[test]
    fn content_type_parameters_are_allowed() {
        assert!(is_json_content_type(Some(
            "application/json; charset=utf-8"
        )));
        assert!(is_json_content_type(Some("Application/JSON")));
        assert!(!is_json_content_type(Some("text/json")));
        assert!(!is_json_content_type(Some("application/jsonp")));
        assert!(!is_json_content_type(None));
    }
}
