//! Wire-frame parsing: per-request context builders (draft `_meta` identity /
//! legacy session state) and the raw JSON-RPC param parsers shared by both
//! dispatch paths.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::{Map, Value};

use turbomcp_core::{
    Implementation, JsonRpcError, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpError,
    ProtocolVersion, RequestContext, meta,
};
use turbomcp_protocol::{methods, neutral, version};
use turbomcp_service::ProtocolError;

use crate::session::SessionBackend;

use super::session_id;

/// Resolve the session for a legacy request and build its [`RequestContext`]
/// (negotiated version + client info/capabilities from `initialize`, plus this
/// request's propagated `_meta`).
///
/// Three-way outcome, hence the nested `Result`:
/// - `Err(ProtocolError::UnknownSession)` — id supplied but not live; the HTTP
///   transport maps this to `404` so the client re-initializes (spec §Session
///   Management).
/// - `Ok(Err(response))` — no session id at all: the connection never ran
///   `initialize`, answered in-band as a JSON-RPC error.
/// - `Ok(Ok(ctx))` — live session.
pub(super) async fn legacy_context(
    sessions: &dyn SessionBackend,
    req: &JsonRpcRequest,
) -> Result<Result<RequestContext, JsonRpcMessage>, ProtocolError> {
    let Some(sid) = session_id(req.params.as_ref()) else {
        let err = JsonRpcError {
            code: -32002,
            message: "server not initialized: send `initialize` first".to_owned(),
            data: None,
        };
        return Ok(Err(JsonRpcResponse::error(req.id.clone(), err).into()));
    };
    let Some(state) = sessions.get(sid).await else {
        return Err(ProtocolError::UnknownSession(sid.to_owned()));
    };
    let owner = req
        .params
        .as_ref()
        .and_then(|p| p.get("_meta"))
        .and_then(Value::as_object)
        .and_then(|m| meta::extract_identity(m).principal_key());
    if state.owner != owner {
        return Err(ProtocolError::UnknownSession(sid.to_owned()));
    }
    let mut ctx = RequestContext::new(state.version).with_client_info(state.client_info);
    ctx.client_capabilities = Some(state.client_capabilities);
    ctx.log_level = state.log_level;
    if let Some(m) = req
        .params
        .as_ref()
        .and_then(|p| p.get("_meta"))
        .and_then(Value::as_object)
    {
        ctx.identity = meta::extract_identity(m);
        ctx.trace_context = meta::extract_trace_context(m);
        let (_consumed, propagated) = meta::partition(m.clone());
        ctx = ctx.with_propagated_meta(propagated);
    }
    Ok(Ok(ctx))
}

#[derive(Deserialize)]
struct RawClientInfo {
    name: String,
    version: String,
    #[serde(default)]
    title: Option<String>,
}

/// Build the per-request context from the wire frame: version, the draft's
/// per-request client identity/capabilities (`_meta` keys), and propagated
/// user `_meta`. (Transport identity extraction joins with Auth in Phase 7.)
pub(super) fn build_context(req: &JsonRpcRequest) -> RequestContext {
    let version =
        version::request_protocol_version(req.params.as_ref()).unwrap_or(ProtocolVersion::LATEST);
    let mut ctx = RequestContext::new(version);
    if let Some(meta) = req
        .params
        .as_ref()
        .and_then(|p| p.get("_meta"))
        .and_then(Value::as_object)
    {
        ctx.identity = turbomcp_core::meta::extract_identity(meta);
        ctx.trace_context = turbomcp_core::meta::extract_trace_context(meta);
        let (consumed, propagated) = turbomcp_core::meta::partition(meta.clone());
        if let Some(info) = consumed
            .get(meta::keys::CLIENT_INFO)
            .and_then(|v| serde_json::from_value::<RawClientInfo>(v.clone()).ok())
        {
            let mut implementation = Implementation::new(info.name, info.version);
            implementation.title = info.title;
            ctx = ctx.with_client_info(implementation);
        }
        if let Some(caps) = consumed.get(meta::keys::CLIENT_CAPABILITIES) {
            ctx.client_capabilities = Some(caps.clone());
        }
        ctx = ctx.with_propagated_meta(propagated);
    }
    ctx
}

#[derive(Deserialize)]
struct RawCallToolParams {
    name: String,
    #[serde(default)]
    arguments: Map<String, Value>,
}

pub(super) fn parse_call_tool_params(
    params: Option<&Value>,
) -> Result<neutral::CallToolParams, McpError> {
    let params = params.ok_or_else(|| McpError::invalid_params("tools/call requires params"))?;
    let raw: RawCallToolParams = serde_json::from_value(params.clone())
        .map_err(|e| McpError::invalid_params(format!("invalid tools/call params: {e}")))?;
    Ok(neutral::CallToolParams::new(raw.name, raw.arguments))
}

/// Lenient pagination-cursor extraction: a missing `params` or absent `cursor`
/// is simply a first-page request, never an error.
pub(super) fn parse_list_params(params: Option<&Value>) -> neutral::ListParams {
    match params.and_then(|p| p.get("cursor")).and_then(Value::as_str) {
        Some(cursor) => neutral::ListParams::with_cursor(cursor),
        None => neutral::ListParams::new(),
    }
}

#[derive(Deserialize)]
struct RawReadResourceParams {
    uri: String,
}

/// Parse `logging/setLevel` params: `{ "level": <RFC 5424 level> }`. An
/// unrecognized level answers `-32602` (logging spec §Error Handling).
pub(super) fn parse_set_level_params(
    params: Option<&Value>,
) -> Result<turbomcp_core::LogLevel, McpError> {
    let level = params
        .and_then(|p| p.get("level"))
        .ok_or_else(|| McpError::invalid_params("logging/setLevel requires a level"))?;
    serde_json::from_value(level.clone())
        .map_err(|_| McpError::invalid_params(format!("invalid log level: {level}")))
}

/// Parse the draft per-request log-level opt-in (`_meta`
/// `io.modelcontextprotocol/logLevel`). `Err` means present but unrecognized
/// — the logging spec says reject the request with `-32602`.
pub(super) fn extract_log_level(
    params: Option<&Value>,
) -> Result<Option<turbomcp_core::LogLevel>, McpError> {
    let Some(value) = params
        .and_then(|p| p.get("_meta"))
        .and_then(|m| m.get(meta::keys::LOG_LEVEL))
    else {
        return Ok(None);
    };
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|_| McpError::invalid_params(format!("invalid logLevel: {value}")))
}

/// Extract the `uri` field shared by `resources/read|subscribe|unsubscribe`.
pub(super) fn parse_uri_param(params: Option<&Value>, method: &str) -> Result<String, McpError> {
    let params =
        params.ok_or_else(|| McpError::invalid_params(format!("{method} requires params")))?;
    let raw: RawReadResourceParams = serde_json::from_value(params.clone())
        .map_err(|e| McpError::invalid_params(format!("invalid {method} params: {e}")))?;
    Ok(raw.uri)
}

pub(super) fn parse_read_resource_params(
    params: Option<&Value>,
) -> Result<neutral::ReadResourceParams, McpError> {
    parse_uri_param(params, methods::request::RESOURCES_READ).map(neutral::ReadResourceParams::new)
}

#[derive(Deserialize)]
struct RawGetPromptParams {
    name: String,
    #[serde(default)]
    arguments: BTreeMap<String, String>,
}

pub(super) fn parse_get_prompt_params(
    params: Option<&Value>,
) -> Result<neutral::GetPromptParams, McpError> {
    let params = params.ok_or_else(|| McpError::invalid_params("prompts/get requires params"))?;
    let raw: RawGetPromptParams = serde_json::from_value(params.clone())
        .map_err(|e| McpError::invalid_params(format!("invalid prompts/get params: {e}")))?;
    Ok(neutral::GetPromptParams::new(raw.name, raw.arguments))
}

#[derive(Deserialize)]
struct RawCompleteParams {
    #[serde(rename = "ref")]
    reference: RawCompletionRef,
    argument: RawCompletionArgument,
    #[serde(default)]
    context: Option<RawCompletionContext>,
}

#[derive(Deserialize)]
struct RawCompletionRef {
    #[serde(rename = "type")]
    type_: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    uri: Option<String>,
}

#[derive(Deserialize)]
struct RawCompletionArgument {
    name: String,
    value: String,
}

#[derive(Deserialize)]
struct RawCompletionContext {
    #[serde(default)]
    arguments: BTreeMap<String, String>,
}

pub(super) fn parse_complete_params(
    params: Option<&Value>,
) -> Result<neutral::CompleteParams, McpError> {
    let params =
        params.ok_or_else(|| McpError::invalid_params("completion/complete requires params"))?;
    let raw: RawCompleteParams = serde_json::from_value(params.clone()).map_err(|e| {
        McpError::invalid_params(format!("invalid completion/complete params: {e}"))
    })?;

    // The `ref` is a discriminated union keyed on `type` (`ref/prompt` carries
    // `name`; `ref/resource` carries `uri`).
    let reference = match raw.reference.type_.as_str() {
        "ref/prompt" => neutral::CompletionReference::Prompt {
            name: raw
                .reference
                .name
                .ok_or_else(|| McpError::invalid_params("ref/prompt completion requires `name`"))?,
        },
        "ref/resource" => neutral::CompletionReference::ResourceTemplate {
            uri: raw.reference.uri.ok_or_else(|| {
                McpError::invalid_params("ref/resource completion requires `uri`")
            })?,
        },
        other => {
            return Err(McpError::invalid_params(format!(
                "unknown completion ref type: {other}"
            )));
        }
    };

    let mut out = neutral::CompleteParams::new(
        reference,
        neutral::CompletionArgument::new(raw.argument.name, raw.argument.value),
    );
    if let Some(ctx) = raw.context {
        out.context_arguments = ctx.arguments;
    }
    Ok(out)
}
