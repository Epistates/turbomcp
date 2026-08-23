//! Version-agnostic entry points: the legacy `initialize` handshake (version
//! negotiation + session minting + capability advertisement) and the draft
//! `server/discover` response.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Map, Value};

use turbomcp_core::{
    Implementation, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse, McpError, ProtocolVersion,
    RequestId,
};
use turbomcp_protocol::neutral;
use turbomcp_protocol::v2025_06_18::types as v0618;
use turbomcp_protocol::v2025_11_25::types as legacy;
use turbomcp_protocol::v2026_07_28::types as v0728;

use crate::extension::Extension;
use crate::router::MethodRouter;
use crate::session::{SessionBackend, SessionState};
use crate::traits::McpServerCore;

use super::{error_response, session_id};

/// Answer the `initialize` handshake (lifecycle spec §Version Negotiation):
/// echo the requested version when supported, otherwise our latest
/// `initialize`-speaking version. When the transport supplied a session id,
/// the negotiated state is stored under it for later context injection.
pub(super) async fn handle_initialize<S: McpServerCore>(
    server: &S,
    router: &MethodRouter<S>,
    supported: &[ProtocolVersion],
    sessions: &dyn SessionBackend,
    tasks_enabled: bool,
    req: &JsonRpcRequest,
) -> JsonRpcMessage {
    let id = req.id.clone();
    let Some(params) = req.params.as_ref() else {
        return error_response(id, &McpError::invalid_params("initialize requires params"));
    };
    let params: legacy::InitializeRequestParams = match serde_json::from_value(params.clone()) {
        Ok(p) => p,
        Err(e) => {
            return error_response(
                id,
                &McpError::invalid_params(format!("invalid initialize params: {e}")),
            );
        }
    };

    let negotiated = negotiate_initialize_version(&params.protocol_version, supported);

    if let Some(sid) = session_id(req.params.as_ref()) {
        let client_capabilities = serde_json::to_value(&params.capabilities).unwrap_or(Value::Null);
        sessions
            .insert(
                sid,
                SessionState {
                    version: negotiated.clone(),
                    client_info: from_legacy_impl(params.client_info),
                    client_capabilities,
                    log_level: None,
                },
            )
            .await;
    }

    let result = legacy::InitializeResult {
        capabilities: build_legacy_capabilities(router),
        instructions: server.instructions(),
        meta: Map::new(),
        protocol_version: negotiated.as_str().to_owned(),
        server_info: to_legacy_impl(server.server_info()),
    };
    // `2025-06-18` predates `icons`, the extra `Implementation` fields, and
    // Tasks; the step-down conversion is what drops them (see
    // `v2025_06_18::convert`) rather than this function knowing the list.
    let serialized = if negotiated == ProtocolVersion::V2025_06_18 {
        serde_json::to_value(v0618::InitializeResult::from(result))
    } else {
        serde_json::to_value(&result)
    };
    let mut value = match serialized {
        Ok(v) => v,
        Err(e) => {
            return error_response(id, &McpError::internal(format!("serialize result: {e}")));
        }
    };
    // The generated capability types model presence-markers (`completions: {}`,
    // the `tasks` sub-objects) as maps that serde skips when empty, so an
    // advertised-but-empty marker would vanish. Patch them into the serialized
    // form instead.
    if let Some(caps) = value.get_mut("capabilities").and_then(Value::as_object_mut) {
        if router.has_completions() {
            caps.insert("completions".to_owned(), serde_json::json!({}));
        }
        if router.has_logging() {
            caps.insert("logging".to_owned(), serde_json::json!({}));
        }
        // Tasks are `2025-11-25`-only. Advertising them to a `2025-06-18`
        // client would invite `tasks/*` calls that revision's dispatch can only
        // answer `-32601`.
        if tasks_enabled && negotiated.has_core_tasks() {
            caps.insert(
                "tasks".to_owned(),
                serde_json::json!({
                    "list": {},
                    "cancel": {},
                    "requests": { "tools": { "call": {} } },
                }),
            );
        }
    }
    JsonRpcResponse::success(id, value).into()
}

/// Pick the version to answer `initialize` with.
///
/// The spec: echo the requested version if supported, else "another version
/// [the server] supports … SHOULD be the latest", and the client then
/// disconnects if it can't speak what it gets back. So echoing matters — a
/// server that answers a `2025-06-18` client with `2025-11-25` is telling it to
/// go away.
///
/// The fallback is the latest *`initialize`-speaking* version, which is
/// `2025-11-25`: the draft negotiates per-request and has no handshake to
/// answer with.
fn negotiate_initialize_version(requested: &str, supported: &[ProtocolVersion]) -> ProtocolVersion {
    let requested = ProtocolVersion::from_wire(requested);
    if supported.contains(&requested) {
        return requested;
    }
    supported
        .iter()
        .rev()
        .find(|v| v.is_stateful())
        .or_else(|| supported.first())
        .cloned()
        .unwrap_or(ProtocolVersion::LATEST)
}

fn build_legacy_capabilities<S: McpServerCore>(
    router: &MethodRouter<S>,
) -> legacy::ServerCapabilities {
    // `listChanged`/`subscribe` are true: the subscription registry delivers
    // them for every registered capability (`resources/subscribe` + the
    // session's notification stream).
    legacy::ServerCapabilities {
        // `completions` is a presence marker the generated type can't express
        // (empty map ⇒ skipped); patched post-serialization in
        // `handle_initialize`.
        completions: Map::new(),
        experimental: BTreeMap::new(),
        logging: Map::new(),
        prompts: router
            .has_prompts()
            .then_some(legacy::ServerCapabilitiesPrompts {
                list_changed: Some(true),
            }),
        resources: router
            .has_resources()
            .then_some(legacy::ServerCapabilitiesResources {
                list_changed: Some(true),
                subscribe: Some(true),
            }),
        tasks: None,
        tools: router
            .has_tools()
            .then_some(legacy::ServerCapabilitiesTools {
                list_changed: Some(true),
            }),
    }
}

fn to_legacy_impl(i: Implementation) -> legacy::Implementation {
    legacy::Implementation {
        description: None,
        icons: Vec::new(),
        name: i.name,
        title: i.title,
        version: i.version,
        website_url: None,
    }
}

fn from_legacy_impl(i: legacy::Implementation) -> Implementation {
    let mut out = Implementation::new(i.name, i.version);
    out.title = i.title;
    out
}

// ---- response builders -------------------------------------------------------

/// Build the `server/discover` response, patching each registered extension's
/// settings into `capabilities.extensions[id]`. The generated `extensions` map
/// holds the draft `JsonObject` newtype; merging arbitrary settings JSON is
/// simplest post-serialization (the same presence-marker patch trick
/// `handle_initialize` uses).
pub(super) fn discover_response<S: McpServerCore>(
    id: RequestId,
    server: &S,
    router: &MethodRouter<S>,
    supported: &[ProtocolVersion],
    extensions: &[Arc<dyn Extension>],
    cache: neutral::CachePolicy,
) -> JsonRpcMessage {
    let result = build_discover_result(server, router, supported, cache);
    let mut value = match serde_json::to_value(&result) {
        Ok(v) => v,
        Err(e) => return error_response(id, &McpError::internal(format!("serialize result: {e}"))),
    };
    if !extensions.is_empty()
        && let Some(caps) = value.get_mut("capabilities").and_then(Value::as_object_mut)
    {
        let ext_map = caps
            .entry("extensions")
            .or_insert_with(|| Value::Object(Map::new()));
        if let Some(ext_map) = ext_map.as_object_mut() {
            for ext in extensions {
                ext_map.insert(ext.id().to_owned(), ext.settings());
            }
        }
    }
    JsonRpcResponse::success(id, value).into()
}

fn build_discover_result<S: McpServerCore>(
    server: &S,
    router: &MethodRouter<S>,
    supported: &[ProtocolVersion],
    cache: neutral::CachePolicy,
) -> v0728::DiscoverResult {
    // `listChanged`/`subscribe` are true: the subscription registry delivers
    // these for every registered capability (`subscriptions/listen`).
    let capabilities = v0728::ServerCapabilities {
        // `completions` is an opaque presence marker (an empty object when the
        // server supports argument autocompletion).
        completions: router
            .has_completions()
            .then(|| v0728::JsonObject(BTreeMap::new())),
        experimental: BTreeMap::new(),
        extensions: BTreeMap::new(),
        logging: router
            .has_logging()
            .then(|| v0728::JsonObject(BTreeMap::new())),
        prompts: router
            .has_prompts()
            .then_some(v0728::ServerCapabilitiesPrompts {
                list_changed: Some(true),
            }),
        resources: router
            .has_resources()
            .then_some(v0728::ServerCapabilitiesResources {
                list_changed: Some(true),
                subscribe: Some(true),
            }),
        tools: router
            .has_tools()
            .then_some(v0728::ServerCapabilitiesTools {
                list_changed: Some(true),
            }),
    };
    v0728::DiscoverResult {
        cache_scope: match cache.scope {
            neutral::CacheScope::Private => v0728::DiscoverResultCacheScope::Private,
            neutral::CacheScope::Public => v0728::DiscoverResultCacheScope::Public,
        },
        capabilities,
        instructions: server.instructions(),
        // Server identity rides in the result's `_meta`. The 2026-07-28 RC had
        // briefly promoted it to a first-class `serverInfo` field; the frozen
        // spec reverted that, and `server/discover` has no other place to put
        // it — the stateless model has no `initialize` result.
        meta: Some(v0728::ResultMetaObject {
            io_modelcontextprotocol_server_info: Some(to_draft_impl(server.server_info())),
            extra: Map::new(),
        }),
        result_type: neutral::result_type::COMPLETE.to_string(),
        supported_versions: supported.iter().map(|v| v.as_str().to_owned()).collect(),
        ttl_ms: cache.ttl_ms,
    }
}

fn to_draft_impl(i: Implementation) -> v0728::Implementation {
    v0728::Implementation {
        description: None,
        icons: Vec::new(),
        name: i.name,
        title: i.title,
        version: i.version,
        website_url: None,
    }
}
