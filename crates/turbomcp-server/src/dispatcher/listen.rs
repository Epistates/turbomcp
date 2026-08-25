//! The draft `subscriptions/listen` lifecycle: filter validation and
//! intersection, the acknowledged-first stream contract, and extension filter
//! contributions (e.g. the Tasks extension's `taskIds`).

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Map, Value};

use turbomcp_core::{
    CancellationToken, JsonRpcError, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest,
    JsonRpcResponse, McpError, ProtocolVersion, meta,
};
use turbomcp_protocol::methods;
use turbomcp_protocol::v2026_07_28::types as v0728;
use turbomcp_service::ProtocolError;

use crate::extension::{Extension, SubscribeOutcome};
use crate::router::MethodRouter;
use crate::subscriptions::{SubscriptionRegistry, subscription_id_value};
use crate::traits::McpServerCore;

use super::params::build_context;
use super::{
    VersionRoute, classify_version, connection_id, context_declares_extension, error_response,
    invalid_envelope, missing_capability_response, unsupported_version,
};

// ---- subscriptions (draft `subscriptions/listen`) ------------------------------

#[derive(Deserialize)]
struct RawListenParams {
    notifications: v0728::SubscriptionFilter,
}

/// Open a subscription stream (subscriptions spec): validate the filter,
/// intersect it with the capabilities this server actually registered, push
/// `notifications/subscriptions/acknowledged` as the stream's first message,
/// and commit the subscription. Success returns `Ok(None)` — the listen
/// request never gets a JSON-RPC response; only failures answer in-band.
pub(super) async fn handle_subscriptions_listen<S: McpServerCore>(
    router: &MethodRouter<S>,
    supported: &[ProtocolVersion],
    subs: &Arc<SubscriptionRegistry>,
    extensions: &[Arc<dyn Extension>],
    req: &JsonRpcRequest,
    cancel: &CancellationToken,
) -> Result<Option<JsonRpcMessage>, ProtocolError> {
    let id = req.id.clone();
    match classify_version(req.params.as_ref(), supported) {
        VersionRoute::Modern => {}
        // The legacy path subscribes via `resources/subscribe` instead.
        VersionRoute::Legacy(_) => {
            return Ok(Some(error_response(
                id,
                &McpError::method_not_found(methods::request::SUBSCRIPTIONS_LISTEN),
            )));
        }
        VersionRoute::Unsupported(requested) => {
            return Ok(Some(unsupported_version(id, requested, supported)));
        }
        VersionRoute::InvalidEnvelope(field) => {
            return Ok(Some(invalid_envelope(id, field, supported)));
        }
    }

    // Streaming needs an ordered writer for this connection (the serve driver
    // registers one; the HTTP endpoint registers a per-stream one).
    let writer = connection_id(req.params.as_ref()).and_then(turbomcp_service::outbound::writer);
    let Some(writer) = writer else {
        let err = JsonRpcError {
            code: -32600,
            message: "subscriptions/listen requires a connection that can stream notifications"
                .to_owned(),
            data: None,
        };
        return Ok(Some(JsonRpcResponse::error(id, err).into()));
    };
    // `writer` resolving proves the id exists; keep it for the registry key.
    let conn = connection_id(req.params.as_ref())
        .unwrap_or_default()
        .to_owned();

    let requested: RawListenParams = match req
        .params
        .as_ref()
        .map(|p| serde_json::from_value(p.clone()))
    {
        Some(Ok(p)) => p,
        _ => {
            return Ok(Some(error_response(
                id,
                &McpError::invalid_params("subscriptions/listen requires a `notifications` filter"),
            )));
        }
    };

    // Honor only what the server can actually emit; unsupported types are
    // omitted from the acknowledgment (spec §Acknowledgment).
    let wanted = requested.notifications;
    let agreed = v0728::SubscriptionFilter {
        tools_list_changed: (wanted.tools_list_changed == Some(true) && router.has_tools())
            .then_some(true),
        resources_list_changed: (wanted.resources_list_changed == Some(true)
            && router.has_resources())
        .then_some(true),
        prompts_list_changed: (wanted.prompts_list_changed == Some(true) && router.has_prompts())
            .then_some(true),
        resource_subscriptions: if router.has_resources() {
            wanted.resource_subscriptions
        } else {
            Vec::new()
        },
    };

    // The acknowledgment's `notifications` echoes the core filters the server
    // agreed to honor, plus any extension-owned filters (e.g. the Tasks
    // extension's `taskIds`). Build it as a value so extensions can merge in.
    let mut ack_notifications =
        serde_json::to_value(&agreed).unwrap_or_else(|_| Value::Object(Map::new()));
    // Offer the raw `notifications` filter to each extension (it reads its own
    // fields). A non-declaring client requesting an extension's notifications
    // is `-32003` (SEP-2663); accepted filters are merged into the ack.
    if !extensions.is_empty() {
        let raw_notifications = req
            .params
            .as_ref()
            .and_then(|p| p.get("notifications"))
            .cloned()
            .unwrap_or(Value::Null);
        let ctx = build_context(req);
        for ext in extensions {
            let declared = context_declares_extension(&ctx, ext.id());
            match ext.on_subscribe(&conn, &id, &raw_notifications, declared) {
                SubscribeOutcome::NotApplicable => {}
                SubscribeOutcome::MissingCapability => {
                    return Ok(Some(missing_capability_response(id, ext.id())));
                }
                SubscribeOutcome::Subscribed(contribution) => {
                    if let (Some(ack_obj), Some(extra)) =
                        (ack_notifications.as_object_mut(), contribution.as_object())
                    {
                        for (key, value) in extra {
                            ack_obj.insert(key.clone(), value.clone());
                        }
                    }
                }
            }
        }
    }

    // Acknowledged MUST be the first message on the stream — send it before
    // the subscription can receive its first event.
    let ack = JsonRpcNotification::new(
        methods::notification::SUBSCRIPTIONS_ACKNOWLEDGED,
        Some(serde_json::json!({
            "_meta": { meta::keys::SUBSCRIPTION_ID: subscription_id_value(&id) },
            "notifications": ack_notifications,
        })),
    );
    if writer.send(ack.into()).await.is_err() {
        return Ok(None); // connection already gone; nothing to answer
    }
    subs.insert(&conn, &id, agreed);
    // A `notifications/cancelled` that raced this dispatch fired our in-flight
    // token before the insert could be seen — honor it now.
    if cancel.is_cancelled() {
        subs.remove(&conn, &id);
    }
    Ok(None)
}
