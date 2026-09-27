//! Tool metadata end-to-end: `#[tool(title = …, read_only, …)]` behavior
//! hints survive from the macro through each real wire version into the typed
//! client's neutral result — the safety annotations a catalog policy keys off
//! must never be silently dropped (they are core spec fields on both
//! `2025-11-25` and the draft).

#![cfg(feature = "client")]

use tokio::io::{BufReader, split};
use turbomcp::client::{Client, ClientBuilder, ConnectMode};
use turbomcp::prelude::*;
use turbomcp::{LegacySessionAdapter, SerdeJsonCodec, serve};
use turbomcp_service::io::LineTransport;

#[derive(Clone)]
struct Annotated;

#[server(name = "annotated", version = "1.0.0")]
impl Annotated {
    /// Inspect the ledger.
    #[tool(title = "Inspect ledger", read_only, idempotent)]
    async fn inspect(&self) -> McpResult<String> {
        Ok("ok".into())
    }

    /// Append an entry (additive, closed-world).
    #[tool(destructive = false, open_world = false)]
    async fn append(&self) -> McpResult<String> {
        Ok("ok".into())
    }

    /// No hints declared.
    #[tool]
    async fn plain(&self) -> McpResult<String> {
        Ok("ok".into())
    }
}

async fn connect(mode: ConnectMode) -> Client {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let transport = LineTransport::new(BufReader::new(s_rd), s_wr, SerdeJsonCodec);
    let service = LegacySessionAdapter::new(Annotated.into_server().build());
    tokio::spawn(serve(transport, service));

    let (c_rd, c_wr) = split(client_io);
    let client_transport = LineTransport::new(BufReader::new(c_rd), c_wr, SerdeJsonCodec);
    ClientBuilder::new("metadata-test", "1.0.0")
        .with_connect_mode(mode)
        .connect(client_transport)
        .await
        .expect("handshake succeeds")
}

async fn assert_metadata_survives(mode: ConnectMode) {
    let client = connect(mode).await;
    let tools = client.list_tools(None).await.expect("list_tools");
    let by_name = |n: &str| {
        tools
            .tools
            .iter()
            .find(|t| t.name == n)
            .unwrap_or_else(|| panic!("tool {n} listed"))
    };

    let inspect = by_name("inspect");
    assert_eq!(inspect.title.as_deref(), Some("Inspect ledger"));
    let a = inspect.annotations.as_ref().expect("inspect annotations");
    assert_eq!(a.read_only_hint, Some(true));
    assert_eq!(a.idempotent_hint, Some(true));
    assert_eq!(a.destructive_hint, None, "undeclared hints stay unset");

    let append = by_name("append");
    let a = append.annotations.as_ref().expect("append annotations");
    assert_eq!(a.destructive_hint, Some(false));
    assert_eq!(a.open_world_hint, Some(false));
    assert_eq!(a.read_only_hint, None);

    assert!(
        by_name("plain").annotations.is_none(),
        "no declared hints → no annotations object on the wire"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_hints_survive_the_draft_wire() {
    assert_metadata_survives(ConnectMode::Modern).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tool_hints_survive_the_legacy_wire() {
    assert_metadata_survives(ConnectMode::Legacy).await;
}
