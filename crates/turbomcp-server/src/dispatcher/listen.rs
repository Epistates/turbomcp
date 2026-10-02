//! The draft `subscriptions/listen` lifecycle: filter validation and
//! intersection, the acknowledged-first stream contract, and extension filter
//! contributions (e.g. the Tasks extension's `taskIds`).

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Map, Value};

use turbomcp_core::{
    CancellationToken, Extensions, JsonRpcMessage, JsonRpcNotification, JsonRpcRequest, McpError,
    ProtocolVersion, meta,
};
use turbomcp_protocol::methods;
use turbomcp_protocol::v2026_07_28::types as v0728;
use turbomcp_service::{Peer, ProtocolError};

use crate::extension::SubscribeOutcome;
use crate::router::MethodRouter;
use crate::subscriptions::subscription_id_value;
use crate::traits::McpServerCore;

use super::capability::resource_hidden;
use super::params::build_context;
use super::{
    Shared, VersionRoute, classify_version, error_response, invalid_envelope,
    missing_capability_response, unsupported_version,
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
    server: &S,
    router: &MethodRouter<S>,
    supported: &[ProtocolVersion],
    shared: &Shared,
    req: &JsonRpcRequest,
    ext: &Extensions,
    cancel: &CancellationToken,
) -> Result<Option<JsonRpcMessage>, ProtocolError> {
    let subs = &shared.subs;
    let extensions = shared.extensions.as_slice();
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
    // attaches one; the HTTP endpoint attaches a per-stream one).
    let Some(peer) = ext.get::<Peer>().filter(|p| p.is_open()).cloned() else {
        return Ok(Some(error_response(
            id,
            &McpError::invalid_request(
                "subscriptions/listen requires a connection that can stream notifications",
            ),
        )));
    };
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
    // Resource URIs the policy hides: acknowledged like any other, but never
    // watched. See the comment on `resource_subscriptions` below.
    let mut unwatched = Vec::new();
    let mut agreed = v0728::SubscriptionFilter {
        tools_list_changed: (wanted.tools_list_changed == Some(true) && router.has_tools())
            .then_some(true),
        resources_list_changed: (wanted.resources_list_changed == Some(true)
            && router.has_resources())
        .then_some(true),
        prompts_list_changed: (wanted.prompts_list_changed == Some(true) && router.has_prompts())
            .then_some(true),
        // Each requested URI is judged the way a `resources/read` of it would
        // be: `notifications/resources/updated` names the URI, so watching one
        // the policy hides is the same disclosure on a timer. It must also be
        // *acknowledged* exactly as a URI the server does not have is — echoed
        // back, since nothing checks existence — or the difference between the
        // requested and acknowledged lists enumerates what is hidden. So a
        // hidden URI is acknowledged and simply never watched.
        resource_subscriptions: if router.has_resources() {
            let ctx = build_context(req, ext);
            for uri in &wanted.resource_subscriptions {
                match resource_hidden(shared, router, server, &ctx, uri).await {
                    Ok(false) => {}
                    Ok(true) => unwatched.push(uri.clone()),
                    Err(e) => return Ok(Some(error_response(id, &e))),
                }
            }
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
    // Extensions that agreed start sending only once the acknowledgement is
    // queued: activating one as it answered let a task that changed status in
    // between push its notification ahead of the acknowledgement, and left it
    // registered when a later extension refused the listen.
    let mut accepted_by: Vec<(Arc<dyn crate::extension::Extension>, Value)> = Vec::new();
    let ctx = build_context(req, ext);
    // Offer the raw `notifications` filter to each extension (it reads its own
    // fields). A non-declaring client requesting an extension's notifications
    // is `-32021` (SEP-2663); accepted filters are merged into the ack.
    if !extensions.is_empty() {
        let raw_notifications = req
            .params
            .as_ref()
            .and_then(|p| p.get("notifications"))
            .cloned()
            .unwrap_or(Value::Null);
        for extension in extensions {
            let declared = ctx.supports_extension(extension.id());
            match extension
                .on_subscribe(&peer, &id, &raw_notifications, declared, &ctx)
                .await
            {
                SubscribeOutcome::NotApplicable => {}
                SubscribeOutcome::MissingCapability => {
                    return Ok(Some(missing_capability_response(
                        id,
                        &ctx.protocol_version,
                        extension.id(),
                    )));
                }
                SubscribeOutcome::Subscribed(contribution) => {
                    if let (Some(ack_obj), Some(extra)) =
                        (ack_notifications.as_object_mut(), contribution.as_object())
                    {
                        for (key, value) in extra {
                            ack_obj.insert(key.clone(), value.clone());
                        }
                    }
                    accepted_by.push((Arc::clone(extension), contribution));
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
    let Ok(slot) = peer.reserve().await else {
        return Ok(None); // connection already gone; nothing to answer
    };
    agreed
        .resource_subscriptions
        .retain(|uri| !unwatched.contains(uri));
    subs.insert_acknowledged(&peer, &id, agreed, slot, ack.into());
    for (extension, accepted) in &accepted_by {
        extension.activate(&peer, &id, accepted, &ctx).await;
    }
    // A `notifications/cancelled` that raced this dispatch fired our in-flight
    // token before the insert could be seen — honor it now.
    if cancel.is_cancelled() {
        subs.remove(peer.id().as_str(), &id);
        for (extension, _) in &accepted_by {
            extension.on_unsubscribe(peer.id(), &id);
        }
    }
    Ok(None)
}
