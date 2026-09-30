//! The responses the endpoint answers with instead of dispatching.

use std::time::Duration;

use axum::Json;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use turbomcp_core::{InvalidFrame, JsonRpcResponse, ProtocolVersion, RequestId};
use turbomcp_service::ProtocolError;

/// Build an auth-challenge response: the status (401/403) plus the
/// `WWW-Authenticate` header.
pub(super) fn challenge_response(status: u16, www_authenticate: &str) -> Response {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::UNAUTHORIZED);
    let header = HeaderValue::from_str(www_authenticate)
        .unwrap_or_else(|_| HeaderValue::from_static("Bearer"));
    (status, [(axum::http::header::WWW_AUTHENTICATE, header)]).into_response()
}

/// `429 Too Many Requests` with a `Retry-After` header (seconds, rounded up).
pub(super) fn too_many_requests(retry_after: Duration) -> Response {
    retry_later(
        StatusCode::TOO_MANY_REQUESTS,
        retry_after,
        "rate limit exceeded",
    )
}

/// `403` + a JSON-RPC error with no id (the check runs before the body is
/// read): "the HTTP response body MAY comprise a JSON-RPC error response that
/// has no `id`".
pub(super) fn forbidden(detail: &str) -> Response {
    transport_error(
        StatusCode::FORBIDDEN,
        None,
        turbomcp_core::codes::SERVER_ERROR,
        detail.to_owned(),
        None,
    )
}

/// `405` + `Allow: POST` + a JSON-RPC error. A 2026-07-28 client reads a
/// `405` whose body is a recognized JSON-RPC error as a modern server; a
/// plain-text one as a legacy HTTP+SSE server.
pub(super) fn method_not_allowed(detail: &str) -> Response {
    let mut response = transport_error(
        StatusCode::METHOD_NOT_ALLOWED,
        None,
        turbomcp_core::codes::SERVER_ERROR,
        detail.to_owned(),
        None,
    );
    response
        .headers_mut()
        .insert(header::ALLOW, HeaderValue::from_static("POST"));
    response
}

/// `404` for a session this endpoint doesn't have (expired, deleted, or
/// never minted): "the server MUST respond to requests containing that
/// session ID with HTTP 404 Not Found", and the client starts over with a
/// fresh `initialize`.
pub(super) fn session_not_found(id: Option<&RequestId>) -> Response {
    transport_error(
        StatusCode::NOT_FOUND,
        id,
        turbomcp_core::codes::NO_ACTIVE_SESSION,
        "session not found; initialize a new one".to_owned(),
        None,
    )
}

/// `501` for authenticated sessions with no ownership backend to tie a
/// session to its principal.
pub(super) fn sessions_need_an_owner() -> Response {
    transport_error(
        StatusCode::NOT_IMPLEMENTED,
        None,
        turbomcp_core::codes::SERVER_ERROR,
        "authenticated sessions require a session ownership backend".to_owned(),
        None,
    )
}

/// `504` + a JSON-RPC error: the request did not get as far as its response
/// headers within `request_timeout` (admission, authentication, the body).
/// The id is unknown here: the deadline wraps the whole handler.
pub(super) fn deadline_passed() -> Response {
    transport_error(
        StatusCode::GATEWAY_TIMEOUT,
        None,
        turbomcp_core::codes::SERVER_ERROR,
        "the request did not reach its response within the request deadline".to_owned(),
        None,
    )
}

/// `429` for a caller already holding as many open streams as one caller may.
pub(super) fn too_many_streams() -> Response {
    retry_later(
        StatusCode::TOO_MANY_REQUESTS,
        Duration::from_secs(1),
        "too many open streams for this client",
    )
}

/// `503 Service Unavailable` + `Retry-After`: the endpoint as a whole is
/// full. A `429` would tell the caller it had used up its own quota, which
/// it may not have.
pub(super) fn service_unavailable(message: &str) -> Response {
    retry_later(
        StatusCode::SERVICE_UNAVAILABLE,
        Duration::from_secs(1),
        message,
    )
}

fn retry_later(status: StatusCode, retry_after: Duration, message: &str) -> Response {
    // Round up to whole seconds; a sub-second wait still asks for at least 1s.
    let secs = retry_after.as_secs() + u64::from(retry_after.subsec_nanos() > 0);
    let secs = secs.max(1);
    let mut response = transport_error(
        status,
        None,
        turbomcp_core::codes::SERVER_ERROR,
        message.to_owned(),
        None,
    );
    if let Ok(value) = HeaderValue::from_str(&secs.to_string()) {
        response.headers_mut().insert(header::RETRY_AFTER, value);
    }
    response
}

/// A transport-level JSON-RPC error response, carrying the request's own id.
///
/// SEP-2575 requires every error response to echo the id, and a rejection the
/// client cannot correlate is one it cannot act on — with several requests in
/// flight it can't even tell which one failed. `None` is reserved for the
/// cases where there genuinely is no id to echo: an unparseable body, or a
/// check that runs before the body is read.
pub(super) fn transport_error(
    status: StatusCode,
    id: Option<&RequestId>,
    code: i32,
    message: String,
    data: Option<serde_json::Value>,
) -> Response {
    let mut error = serde_json::json!({ "code": code, "message": message });
    if let (Some(obj), Some(data)) = (error.as_object_mut(), data) {
        obj.insert("data".into(), data);
    }
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error,
    });
    (status, Json(body)).into_response()
}

/// `400` + a `HeaderMismatch` JSON-RPC error: an HTTP header did not match the
/// corresponding request-body value, or a required header is missing or
/// malformed (transports spec §Server Validation).
pub(super) fn header_mismatch_rejection(id: Option<&RequestId>, detail: &str) -> Response {
    transport_error(
        StatusCode::BAD_REQUEST,
        id,
        turbomcp_core::codes::HEADER_MISMATCH,
        format!("header mismatch: {detail}"),
        None,
    )
}

/// `400` + `-32602` for a stateless request whose `_meta` envelope is missing
/// a field SEP-2575 requires.
pub(super) fn envelope_rejection(id: &RequestId, field: &str) -> Response {
    transport_error(
        StatusCode::BAD_REQUEST,
        Some(id),
        turbomcp_core::codes::INVALID_PARAMS,
        format!("request `_meta` is missing the required field `{field}`"),
        Some(serde_json::json!({ "missingField": field })),
    )
}

/// `400` for an explicit but unsupported `MCP-Protocol-Version` header
/// (`UnsupportedProtocolVersionError`, with the spec-required
/// `data: { supported, requested }`).
pub(super) fn version_header_rejection(
    id: Option<&RequestId>,
    requested: &str,
    serves: &[ProtocolVersion],
) -> Response {
    let supported: Vec<&str> = serves.iter().map(ProtocolVersion::as_str).collect();
    transport_error(
        StatusCode::BAD_REQUEST,
        id,
        turbomcp_core::codes::UNSUPPORTED_PROTOCOL_VERSION,
        format!("unsupported MCP-Protocol-Version header: {requested}"),
        Some(serde_json::json!({ "supported": supported, "requested": requested })),
    )
}

/// `400` for a message on the session (pre-2026-07-28) path without an
/// `Mcp-Session-Id`: "Servers that require a session ID SHOULD respond to
/// requests without an `MCP-Session-Id` header (other than initialization)
/// with HTTP 400 Bad Request."
pub(super) fn session_required_rejection(id: Option<&RequestId>) -> Response {
    transport_error(
        StatusCode::BAD_REQUEST,
        id,
        turbomcp_core::codes::NO_ACTIVE_SESSION,
        "missing Mcp-Session-Id: initialize a session first, or send a 2026-07-28 request \
         with its MCP-Protocol-Version header"
            .to_owned(),
        None,
    )
}

/// `406` + a JSON-RPC error body for a request whose `Accept` header doesn't
/// cover the response types this endpoint produces (transports spec: POST
/// clients MUST list both `application/json` and `text/event-stream`; GET
/// clients MUST list `text/event-stream`).
pub(super) fn not_acceptable_rejection(detail: &str) -> Response {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": { "code": turbomcp_core::codes::SERVER_ERROR, "message": format!("not acceptable: {detail}") },
    });
    (StatusCode::NOT_ACCEPTABLE, Json(body)).into_response()
}

/// A body that isn't a valid message: `400` with the JSON-RPC error it is owed
/// (Parse error, or Invalid Request echoing the id when one was readable).
/// "The HTTP response body MAY comprise a JSON-RPC error response that has no
/// `id`", which covers a broken POSTed response too.
pub(super) fn invalid_frame_response(bad: &InvalidFrame) -> Response {
    let body = bad
        .response()
        .unwrap_or_else(|| JsonRpcResponse::error_without_id(bad.error()));
    (StatusCode::BAD_REQUEST, Json(body)).into_response()
}

/// Map a service/transport [`ProtocolError`] to an HTTP status + JSON-RPC error
/// body (PLAN §4.10). User `McpError`s never reach here — the dispatcher renders
/// them as `Ok` error *responses*; this is for parse/version/shutdown conditions.
/// The request's id is echoed whenever the caller has it: a multiplexing
/// client can't correlate an error that doesn't name its request.
pub(super) fn protocol_error_response(err: &ProtocolError, id: Option<RequestId>) -> Response {
    let status = match err {
        ProtocolError::Parse(_) => StatusCode::BAD_REQUEST,
        // Spec §Session Management: an expired/unknown session answers 404 so
        // the client starts over with a fresh initialize.
        ProtocolError::UnknownSession(_) => StatusCode::NOT_FOUND,
        ProtocolError::Transport(_) | ProtocolError::ServerShuttingDown => {
            StatusCode::SERVICE_UNAVAILABLE
        }
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let error = err.to_jsonrpc_error();
    let body = match id {
        Some(id) => JsonRpcResponse::error(id, error),
        None => JsonRpcResponse::error_without_id(error),
    };
    (status, Json(body)).into_response()
}
