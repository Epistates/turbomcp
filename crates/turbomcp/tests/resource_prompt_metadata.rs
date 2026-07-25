//! Resource and prompt metadata end-to-end.
//!
//! `#[resource]` used to take nothing but a URI, so a resource's display name
//! was stuck as its Rust method identifier and its MIME type — the field a
//! client needs to decide how to render the bytes — was unreachable from the
//! macro entirely. `#[prompt]` could set no title. These pin that the new keys
//! reach the wire on *both* revisions, since the two wire families convert
//! independently and a field can be dropped in one and not the other.

#![cfg(feature = "client")]

use tokio::io::{BufReader, split};
use turbomcp::client::{Client, ClientBuilder, ConnectMode};
use turbomcp::prelude::*;
use turbomcp::{LegacySessionAdapter, SerdeJsonCodec, serve};
use turbomcp_transport_stdio::LineTransport;

#[derive(Clone)]
struct Described;

#[server(name = "described", version = "1.0.0")]
impl Described {
    /// The active configuration.
    #[resource(
        "config://app",
        name = "app-config",
        title = "Application configuration",
        mime_type = "application/json"
    )]
    async fn config(&self) -> McpResult<String> {
        Ok(r#"{"debug":false}"#.into())
    }

    /// A file from the project tree.
    #[resource("file://{+path}", title = "Project file", mime_type = "text/plain")]
    async fn project_file(&self, path: String) -> McpResult<String> {
        Ok(path)
    }

    /// Nothing but a URI — the pre-existing shorthand must still work.
    #[resource("config://bare")]
    async fn bare(&self) -> McpResult<String> {
        Ok("bare".into())
    }

    #[prompt(
        name = "summarize-text",
        title = "Summarize text",
        description = "Summarize"
    )]
    async fn summarize(&self, text: String) -> McpResult<String> {
        Ok(format!("summary: {text}"))
    }
}

async fn connect(mode: ConnectMode) -> Client {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let transport = LineTransport::new(BufReader::new(s_rd), s_wr, SerdeJsonCodec);
    let service = LegacySessionAdapter::new(Described.into_server().build());
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

    let resources = client.list_resources(None).await.expect("list_resources");
    let config = resources
        .resources
        .iter()
        .find(|r| r.uri == "config://app")
        .expect("config resource listed");
    assert_eq!(config.name, "app-config", "`name = …` overrides the method");
    assert_eq!(config.title.as_deref(), Some("Application configuration"));
    assert_eq!(config.mime_type.as_deref(), Some("application/json"));
    assert_eq!(
        config.description.as_deref(),
        Some("The active configuration.")
    );

    let bare = resources
        .resources
        .iter()
        .find(|r| r.uri == "config://bare")
        .expect("bare resource listed");
    assert_eq!(bare.name, "bare", "the method name is still the default");
    assert_eq!(bare.title, None);
    assert_eq!(bare.mime_type, None);

    // Templates carry the same metadata through a different wire shape.
    let templates = client
        .list_resource_templates(None)
        .await
        .expect("list_resource_templates");
    let file = templates
        .resource_templates
        .iter()
        .find(|t| t.uri_template == "file://{+path}")
        .expect("template listed");
    assert_eq!(file.name, "project_file");
    assert_eq!(file.title.as_deref(), Some("Project file"));
    assert_eq!(file.mime_type.as_deref(), Some("text/plain"));

    let prompts = client.list_prompts(None).await.expect("list_prompts");
    let summarize = prompts
        .prompts
        .iter()
        .find(|p| p.name == "summarize-text")
        .expect("prompt listed");
    assert_eq!(summarize.title.as_deref(), Some("Summarize text"));
    assert_eq!(summarize.description.as_deref(), Some("Summarize"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metadata_survives_the_draft_wire() {
    assert_metadata_survives(ConnectMode::Modern).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn metadata_survives_the_legacy_wire() {
    assert_metadata_survives(ConnectMode::Legacy).await;
}
