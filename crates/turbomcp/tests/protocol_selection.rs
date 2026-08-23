//! `#[server(protocols(…))]` — pinning which protocol revisions a server
//! accepts.
//!
//! The default is every revision this build serves, which is right for most
//! servers but wrong for one shipping to production before the draft freezes:
//! the draft's wire shapes can still change. These tests pin that the narrowing
//! is real at the wire, not just advertised — a request naming an excluded
//! version must be refused, and `server/discover` must report only what is
//! served.

use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp::prelude::*;
use turbomcp::{JsonRpcMessage, JsonRpcRequest, McpServerCore, ProtocolVersion, codes};

const DRAFT_META: &str = "2026-07-28";
const LEGACY: &str = "2025-11-25";
const PREVIOUS: &str = "2025-06-18";

/// Every supported revision — what a server gets without `protocols(…)`.
#[derive(Clone)]
struct DualStack;

#[server(name = "dual", version = "1.0.0")]
impl DualStack {
    #[tool]
    async fn noop(&self) -> String {
        "ok".into()
    }
}

/// Pinned to the frozen stable revision: the posture for shipping before the
/// draft freezes.
#[derive(Clone)]
struct StableOnly;

#[server(name = "stable-only", version = "1.0.0", protocols("2025-11-25"))]
impl StableOnly {
    #[tool]
    async fn noop(&self) -> String {
        "ok".into()
    }
}

/// Pinned to the previous stable revision, for a deployment whose clients
/// haven't moved yet.
#[derive(Clone)]
struct PreviousOnly;

#[server(name = "previous-only", version = "1.0.0", protocols("2025-06-18"))]
impl PreviousOnly {
    #[tool]
    async fn noop(&self) -> String {
        "ok".into()
    }
}

/// Draft-only: for testing against the in-development spec.
#[derive(Clone)]
struct DraftOnly;

#[server(
    name = "draft-only",
    version = "1.0.0",
    protocols("2026-07-28"),
    instructions = "draft only"
)]
impl DraftOnly {
    #[tool]
    async fn noop(&self) -> String {
        "ok".into()
    }
}

/// Dispatch one request against an already-built dispatcher.
///
/// Takes the *built* service rather than the server value: `#[server]` emits an
/// inherent `into_server()` that pre-registers the discovered capabilities, and
/// inherent methods only shadow the blanket `IntoServerBuilder::into_server`
/// when the receiver's type is concrete. A generic helper would silently get the
/// capability-less blanket one.
async fn call<S>(mut svc: turbomcp::VersionDispatcher<S>, req: JsonRpcRequest) -> Value
where
    S: McpServerCore + Clone,
{
    let resp = svc
        .ready()
        .await
        .expect("ready")
        .call(JsonRpcMessage::Request(req))
        .await
        .expect("dispatch");
    match resp {
        Some(JsonRpcMessage::Response(r)) => serde_json::to_value(r).expect("serialize"),
        other => panic!("expected a response, got {other:?}"),
    }
}

fn tools_list(version: &str) -> JsonRpcRequest {
    JsonRpcRequest::new(
        1,
        "tools/list",
        Some(json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": version } })),
    )
}

#[test]
fn the_default_serves_every_supported_revision() {
    assert_eq!(DualStack.supported_versions(), ProtocolVersion::SUPPORTED);
    assert_eq!(
        ProtocolVersion::SUPPORTED,
        &[
            ProtocolVersion::V2025_06_18,
            ProtocolVersion::V2025_11_25,
            ProtocolVersion::V2026_07_28
        ],
        "the macro's `protocols(…)` table must cover exactly this set"
    );
}

#[test]
fn protocols_narrows_what_the_server_declares() {
    assert_eq!(
        StableOnly.supported_versions(),
        &[ProtocolVersion::V2025_11_25]
    );
    assert_eq!(
        DraftOnly.supported_versions(),
        &[ProtocolVersion::V2026_07_28]
    );
    assert_eq!(
        PreviousOnly.supported_versions(),
        &[ProtocolVersion::V2025_06_18]
    );
}

/// Every version in `SUPPORTED` must be expressible in `protocols(…)`. If the
/// draft freezes to a dated variant and the macro's table isn't updated, the
/// two fall out of step and this fails.
#[test]
fn protocols_accepts_every_supported_version() {
    let expressible = [
        PreviousOnly.supported_versions(),
        StableOnly.supported_versions(),
        DraftOnly.supported_versions(),
    ]
    .concat();
    for version in ProtocolVersion::SUPPORTED {
        assert!(
            expressible.contains(version),
            "{version} is in ProtocolVersion::SUPPORTED but no test server pins it — \
             the macro's protocol_variant table is probably stale"
        );
    }
}

#[tokio::test]
async fn a_stable_only_server_refuses_the_draft() {
    let body = call(StableOnly.into_server().build(), tools_list(DRAFT_META)).await;
    let err = body.get("error").expect("draft must be refused");
    assert_eq!(
        err["code"],
        codes::UNSUPPORTED_PROTOCOL_VERSION,
        "unsupported protocol version"
    );
    // The error names what *is* served, so the client can re-issue.
    let supported = err["data"]["supported"]
        .as_array()
        .expect("supported list")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(supported, [LEGACY]);
    assert_eq!(err["data"]["requested"], DRAFT_META);
}

#[tokio::test]
async fn a_stable_only_server_still_serves_its_own_version() {
    // The legacy path is session-based, so `initialize` is the entry point; it
    // must negotiate to 2025-11-25 rather than being refused.
    let init = call(
        StableOnly.into_server().build(),
        JsonRpcRequest::new(
            1,
            "initialize",
            Some(json!({
                "protocolVersion": LEGACY,
                "capabilities": {},
                "clientInfo": { "name": "c", "version": "1.0" }
            })),
        ),
    )
    .await;
    assert_eq!(init["result"]["protocolVersion"], LEGACY);
}

#[tokio::test]
async fn a_draft_only_server_refuses_the_legacy_handshake() {
    // With 2025-11-25 excluded there is no version to negotiate down to, so
    // `initialize` answers the draft — which a legacy client will reject,
    // which is the correct outcome: this server does not speak its protocol.
    let init = call(
        DraftOnly.into_server().build(),
        JsonRpcRequest::new(
            1,
            "initialize",
            Some(json!({
                "protocolVersion": LEGACY,
                "capabilities": {},
                "clientInfo": { "name": "c", "version": "1.0" }
            })),
        ),
    )
    .await;
    assert_eq!(init["result"]["protocolVersion"], DRAFT_META);
}

#[tokio::test]
async fn discover_advertises_only_the_pinned_versions() {
    let body = call(
        DraftOnly.into_server().build(),
        JsonRpcRequest::new(1, "server/discover", None),
    )
    .await;
    let versions = body["result"]["supportedVersions"]
        .as_array()
        .expect("supportedVersions")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(versions, [DRAFT_META]);

    let body = call(
        DualStack.into_server().build(),
        JsonRpcRequest::new(1, "server/discover", None),
    )
    .await;
    let versions = body["result"]["supportedVersions"]
        .as_array()
        .expect("supportedVersions")
        .iter()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>();
    assert_eq!(versions, [PREVIOUS, LEGACY, DRAFT_META]);
}

/// The mirror of the stable-only case: pinning to the previous revision must
/// refuse the newer ones just as firmly, and still serve its own handshake.
#[tokio::test]
async fn a_previous_only_server_refuses_the_newer_revisions() {
    for excluded in [LEGACY, DRAFT_META] {
        let body = call(PreviousOnly.into_server().build(), tools_list(excluded)).await;
        let err = body
            .get("error")
            .unwrap_or_else(|| panic!("{excluded} must be refused: {body}"));
        assert_eq!(
            err["code"],
            codes::UNSUPPORTED_PROTOCOL_VERSION,
            "{excluded}: {body}"
        );
        assert_eq!(err["data"]["supported"], json!([PREVIOUS]));
    }

    let init = call(
        PreviousOnly.into_server().build(),
        JsonRpcRequest::new(
            1,
            "initialize",
            Some(json!({
                "protocolVersion": PREVIOUS,
                "capabilities": {},
                "clientInfo": { "name": "c", "version": "1.0" }
            })),
        ),
    )
    .await;
    assert_eq!(init["result"]["protocolVersion"], PREVIOUS);
}
