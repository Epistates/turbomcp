//! Phase 8d exit criterion: a tool that elicits user input round-trips with the
//! typed [`Client`] + a [`ElicitationHandler`], on **both protocol versions** — the
//! MRTR loop on the draft (input-required → re-issue with `inputResponses`) and
//! inline bidi on legacy (a server→client `elicitation/create` answered by the
//! actor). The same handler drives both.

#![cfg(feature = "client")]

use serde_json::{Map, json};
use tokio::io::{BufReader, split};
use turbomcp::client::{Client, ClientBuilder, ConnectMode, ElicitationHandler, async_trait};
use turbomcp::prelude::*;
use turbomcp::{LegacySessionAdapter, SerdeJsonCodec, serve};
use turbomcp_transport_stdio::LineTransport;

#[derive(Clone)]
struct FileManager;

#[server(name = "file-manager", version = "1.0.0")]
impl FileManager {
    /// Delete a path — but only after the user confirms via elicitation.
    #[tool(description = "Delete a path after user confirmation")]
    async fn delete(&self, ctx: &CallToolContext, path: String) -> McpResult<String> {
        let outcome = ctx
            .client
            .elicit(
                "confirm_delete",
                neutral::ElicitParams::new(
                    format!("Really delete {path}?"),
                    json!({
                        "type": "object",
                        "properties": { "ok": { "type": "boolean" } },
                        "required": ["ok"],
                    }),
                ),
            )
            .await?;

        if outcome.accepted() && outcome.content.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            Ok(format!("deleted {path}"))
        } else {
            Ok(format!("kept {path}"))
        }
    }
}

/// A handler that answers every elicitation with a fixed accept/decline.
struct Confirm {
    ok: bool,
}

#[async_trait]
impl ElicitationHandler for Confirm {
    async fn elicit(&self, _req: neutral::ElicitParams) -> neutral::ElicitOutcome {
        let mut content = Map::new();
        content.insert("ok".into(), json!(self.ok));
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, content)
    }
}

/// Connect a client (declaring the `elicitation` capability + a [`Confirm`]
/// handler) to the dual-stack file-manager server over a duplex pipe.
async fn connect(mode: ConnectMode, ok: bool) -> Client {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let transport = LineTransport::new(BufReader::new(s_rd), s_wr, SerdeJsonCodec);
    let service = LegacySessionAdapter::new(FileManager.into_server().build());
    tokio::spawn(serve(transport, service));

    let (c_rd, c_wr) = split(client_io);
    let client_transport = LineTransport::new(BufReader::new(c_rd), c_wr, SerdeJsonCodec);
    ClientBuilder::new("confirmer", "1.0.0")
        .with_connect_mode(mode)
        .with_elicitation(Confirm { ok })
        .connect(client_transport)
        .await
        .expect("handshake")
}

async fn call_delete(client: &Client) -> String {
    let mut args = Map::new();
    args.insert("path".into(), json!("/tmp/x"));
    let result = client.call_tool("delete", args).await.expect("call_tool");
    match &result.content[0] {
        neutral::Content::Text { text, .. } => text.clone(),
        other => panic!("unexpected content {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn modern_mrtr_elicitation_confirms() {
    // Draft path: input_required → handler answers ok=true → re-issue → deleted.
    let client = connect(ConnectMode::Modern, true).await;
    assert_eq!(call_delete(&client).await, "deleted /tmp/x");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn modern_mrtr_elicitation_declines() {
    let client = connect(ConnectMode::Modern, false).await;
    assert_eq!(call_delete(&client).await, "kept /tmp/x");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_inline_bidi_elicitation_confirms() {
    // Legacy path: the server elicits inline; the actor answers via the handler.
    let client = connect(ConnectMode::Legacy, true).await;
    assert_eq!(call_delete(&client).await, "deleted /tmp/x");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_inline_bidi_elicitation_declines() {
    let client = connect(ConnectMode::Legacy, false).await;
    assert_eq!(call_delete(&client).await, "kept /tmp/x");
}
