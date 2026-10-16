//! Version-agnostic entry points: the legacy `initialize` handshake (version
//! negotiation + session minting + capability advertisement) and the draft
//! `server/discover` response.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Map, Value};

use turbomcp_core::{
    Extensions, Identity, Implementation, JsonRpcMessage, JsonRpcRequest, JsonRpcResponse,
    McpError, ProtocolVersion, RequestId, meta,
};
use turbomcp_protocol::neutral;
use turbomcp_protocol::v2025_06_18::types as v0618;
use turbomcp_protocol::v2025_11_25::types as legacy;
use turbomcp_protocol::v2026_07_28::types as v0728;

use turbomcp_service::ProtocolError;

use crate::extension::Extension;
use crate::router::MethodRouter;
use crate::session::SessionState;
use crate::traits::McpServerCore;

use super::{error_response, session_id};

/// Upper bound on the serialized client capabilities a session keeps.
/// Real ones are well under a kilobyte; the bound leaves room for extensions.
const MAX_CLIENT_CAPABILITIES_BYTES: usize = 64 * 1024;

/// Answer the `initialize` handshake (lifecycle spec §Version Negotiation):
/// echo the requested version when supported, otherwise our latest
/// `initialize`-speaking version. When the transport supplied a session id,
/// the negotiated state is stored under it for later context injection.
pub(super) async fn handle_initialize<S: McpServerCore>(
    server: &S,
    router: &MethodRouter<S>,
    supported: &[ProtocolVersion],
    shared: &super::Shared,
    req: &JsonRpcRequest,
    ext: &Extensions,
) -> Result<JsonRpcMessage, ProtocolError> {
    let tasks_enabled = shared.tasks.is_some() && router.has_tools();
    let id = req.id.clone();
    let Some(params) = req.params.as_ref() else {
        return Ok(error_response(
            id,
            &McpError::invalid_params("initialize requires params"),
        ));
    };
    let params: legacy::InitializeRequestParams = match serde_json::from_value(params.clone()) {
        Ok(p) => p,
        Err(e) => {
            return Ok(error_response(
                id,
                &McpError::invalid_params(format!("invalid initialize params: {e}")),
            ));
        }
    };

    // A server serving only stateless revisions has no legal `initialize`
    // answer; refuse naming what it does serve rather than inventing one.
    let Some(negotiated) = negotiate_initialize_version(&params.protocol_version, supported) else {
        return Ok(super::unsupported_version(
            id,
            Some(params.protocol_version.clone()),
            supported,
        ));
    };

    // Keep what the client sent, not a re-serialization of the typed parse:
    // the typed form exists to validate, and anything it doesn't model
    // (`extensions`, a future sub-capability) would not survive the trip.
    let client_capabilities = req
        .params
        .as_ref()
        .and_then(|p| p.get("capabilities"))
        .cloned()
        .unwrap_or(Value::Null);
    // Kept for the session's whole life, so bounded: a frame of nested
    // capabilities otherwise pinned megabytes per session, and an
    // unauthenticated `initialize` loop multiplied that by the store.
    let size = serde_json::to_vec(&client_capabilities).map_or(usize::MAX, |b| b.len());
    if size > MAX_CLIENT_CAPABILITIES_BYTES {
        return Ok(error_response(
            id,
            &McpError::invalid_params(format!(
                "client capabilities exceed {MAX_CLIENT_CAPABILITIES_BYTES} bytes"
            )),
        ));
    }
    let client_info = from_legacy_impl(params.client_info);

    // The server's say, before any session exists: an error here is the
    // handshake's answer.
    let mut base = super::params::build_context(req, ext);
    base.protocol_version = negotiated.clone();
    base.client_capabilities = Some(Arc::new(client_capabilities.clone()));
    let base = base.with_client_info(client_info.clone());
    if let Err(refused) = server
        .on_initialize(&crate::InitializeContext::new(base))
        .await
    {
        return Ok(error_response(id, &refused));
    }

    if let Some(sid) = session_id(ext) {
        let state = SessionState::new(negotiated.clone(), client_info, client_capabilities)
            .with_owner(ext.get::<Identity>().and_then(Identity::principal_key))
            .with_peer_address(
                ext.get::<turbomcp_service::NetworkFacts>()
                    .and_then(|facts| facts.peer_address.clone()),
            );
        // A store that can't take the session (down, or full) fails the
        // handshake with `503`: answering success would hand the client a
        // session id that every later request finds unknown.
        shared.sessions.insert(sid, state.clone()).await?;
        shared.session_started(sid, &state);
    }

    let result = legacy::InitializeResult {
        capabilities: build_legacy_capabilities(
            router,
            router.capability_flags(server),
            tasks_enabled,
        ),
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
    Ok(match serialized {
        Ok(mut value) => {
            // "Servers advertise extension support in the `initialize`
            // response" (SEP-2133). Neither stateful schema has the field, so
            // it is patched in, as `server/discover` does.
            advertise_extensions(&mut value, &shared.extensions, |v| *v == negotiated);
            JsonRpcResponse::success(id, value).into()
        }
        Err(e) => error_response(id, &McpError::internal(format!("serialize result: {e}"))),
    })
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
///
/// `None` means this server serves no `initialize`-speaking revision at all, so
/// there is no legal answer and the caller must refuse. A stateless version can
/// never be the answer — even when the client asked for it by name. It defines
/// no `InitializeResult`, and answering with it produces a success the client
/// cannot act on: on stdio the session adapter stamps that version onto every
/// later frame, the envelope check then finds no `clientCapabilities`, and
/// every subsequent request fails `-32602`. The versioning spec says a
/// modern-only server MUST reject `initialize` with an error naming what it
/// does support.
fn negotiate_initialize_version(
    requested: &str,
    supported: &[ProtocolVersion],
) -> Option<ProtocolVersion> {
    let requested = ProtocolVersion::from_wire(requested);
    if requested.is_stateful() && supported.contains(&requested) {
        return Some(requested);
    }
    supported.iter().rev().find(|v| v.is_stateful()).cloned()
}

fn build_legacy_capabilities<S: McpServerCore>(
    router: &MethodRouter<S>,
    flags: crate::router::CapabilityFlags,
    tasks_enabled: bool,
) -> legacy::ServerCapabilities {
    let marker = || legacy::JsonObject(Map::new());
    // `listChanged`/`subscribe` are what the server says of itself
    // (`WithTools::tools_list_changed`, …), not assumed: a gateway relays an
    // upstream that may announce nothing.
    legacy::ServerCapabilities {
        completions: router.has_completions().then(marker),
        experimental: BTreeMap::new(),
        logging: router.has_logging().then(marker),
        prompts: router
            .has_prompts()
            .then_some(legacy::ServerCapabilitiesPrompts {
                list_changed: flags.prompts_list_changed.then_some(true),
            }),
        resources: router
            .has_resources()
            .then_some(legacy::ServerCapabilitiesResources {
                list_changed: flags.resources_list_changed.then_some(true),
                subscribe: flags.resources_subscribe.then_some(true),
            }),
        // `2025-06-18` has no Tasks; its step-down conversion drops this.
        tasks: tasks_enabled.then(|| legacy::ServerCapabilitiesTasks {
            cancel: Some(marker()),
            list: Some(marker()),
            requests: Some(legacy::ServerCapabilitiesTasksRequests {
                tools: Some(legacy::ServerCapabilitiesTasksRequestsTools {
                    call: Some(marker()),
                }),
            }),
        }),
        tools: router
            .has_tools()
            .then_some(legacy::ServerCapabilitiesTools {
                list_changed: flags.tools_list_changed.then_some(true),
            }),
    }
}

/// `i` on a wire: every field the wire models (`description`, `icons`,
/// `websiteUrl` ride in [`Implementation::extra`]) is carried, which was not
/// the case when only name, title and version were copied across.
fn impl_to_wire<W: serde::de::DeserializeOwned>(
    i: Implementation,
    minimal: impl FnOnce(Implementation) -> W,
) -> W {
    match serde_json::to_value(&i).and_then(serde_json::from_value::<W>) {
        Ok(wire) => wire,
        // A malformed extra (an `icons` that isn't a list of icons) costs the
        // extras, not the handshake.
        Err(_) => minimal(i),
    }
}

fn to_legacy_impl(i: Implementation) -> legacy::Implementation {
    impl_to_wire(i, |i| legacy::Implementation {
        description: None,
        icons: Vec::new(),
        name: i.name,
        title: i.title,
        version: i.version,
        website_url: None,
    })
}

/// A peer's `Implementation`, keeping what it said beyond name, title and
/// version (in [`Implementation::extra`]).
fn from_legacy_impl(i: legacy::Implementation) -> Implementation {
    serde_json::to_value(&i)
        .and_then(serde_json::from_value)
        .unwrap_or_else(|_| {
            let mut out = Implementation::new(i.name, i.version);
            out.title = i.title;
            out
        })
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
    advertise_extensions(&mut value, extensions, ProtocolVersion::is_stateless);
    JsonRpcResponse::success(id, value).into()
}

/// Patch `capabilities.extensions[id] = settings` into a serialized
/// `initialize`/`server/discover` result for each extension speaking a
/// revision `speaks` accepts. Nothing is added when none does.
fn advertise_extensions(
    value: &mut Value,
    extensions: &[Arc<dyn Extension>],
    speaks: impl Fn(&ProtocolVersion) -> bool,
) {
    let mut advertised = extensions
        .iter()
        .filter(|ext| ext.protocol_versions().iter().any(&speaks))
        .peekable();
    if advertised.peek().is_none() {
        return;
    }
    let Some(caps) = value.get_mut("capabilities").and_then(Value::as_object_mut) else {
        return;
    };
    let ext_map = caps
        .entry("extensions")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(ext_map) = ext_map.as_object_mut() {
        for ext in advertised {
            ext_map.insert(ext.id().to_owned(), ext.settings());
        }
    }
}

fn build_discover_result<S: McpServerCore>(
    server: &S,
    router: &MethodRouter<S>,
    supported: &[ProtocolVersion],
    cache: neutral::CachePolicy,
) -> v0728::DiscoverResult {
    // `listChanged`/`subscribe` are what the server says of itself, as for
    // `initialize`.
    let flags = router.capability_flags(server);
    let capabilities = v0728::ServerCapabilities {
        // `completions` is an opaque presence marker (an empty object when the
        // server supports argument autocompletion).
        completions: router
            .has_completions()
            .then(|| v0728::JsonObject(Map::new())),
        experimental: BTreeMap::new(),
        extensions: BTreeMap::new(),
        logging: router.has_logging().then(|| v0728::JsonObject(Map::new())),
        prompts: router
            .has_prompts()
            .then_some(v0728::ServerCapabilitiesPrompts {
                list_changed: flags.prompts_list_changed.then_some(true),
            }),
        resources: router
            .has_resources()
            .then_some(v0728::ServerCapabilitiesResources {
                list_changed: flags.resources_list_changed.then_some(true),
                subscribe: flags.resources_subscribe.then_some(true),
            }),
        tools: router
            .has_tools()
            .then_some(v0728::ServerCapabilitiesTools {
                list_changed: flags.tools_list_changed.then_some(true),
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

/// Put the server's identity in a `2026-07-28` success result's `_meta`
/// (`io.modelcontextprotocol/serverInfo`), unless the result already names
/// one: a gateway relaying an upstream's result keeps the upstream's word.
pub(super) fn stamp_server_info(msg: &mut JsonRpcMessage, identity: Implementation) {
    let JsonRpcMessage::Response(response) = msg else {
        return;
    };
    let Some(Value::Object(result)) = response.result.as_mut() else {
        return;
    };
    let meta = result
        .entry("_meta")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Value::Object(meta) = meta
        && !meta.contains_key(meta::keys::SERVER_INFO)
        && let Ok(identity) = serde_json::to_value(to_draft_impl(identity))
    {
        meta.insert(meta::keys::SERVER_INFO.to_owned(), identity);
    }
}

fn to_draft_impl(i: Implementation) -> v0728::Implementation {
    impl_to_wire(i, |i| v0728::Implementation {
        description: None,
        icons: Vec::new(),
        name: i.name,
        title: i.title,
        version: i.version,
        website_url: None,
    })
}
