//! The other half of the protocol: a **client** that owns its server process.
//!
//! This is the shape most local MCP integrations take — the client spawns the
//! server as a subprocess and talks to it over that child's stdio. Here the
//! server is this repo's `hello_world` example, so the whole exchange runs
//! end to end with nothing else installed:
//!
//! ```text
//! cargo run -p turbomcp --features client --example client
//! ```
//!
//! It covers the pieces a real client needs: the handshake and what it
//! negotiated, enumerating the server's capabilities, calling a tool and
//! reading its content back, and handling server→client requests
//! (elicitation) through a [`ClientHandler`].

use std::path::PathBuf;

use serde_json::{Map, json};
use tokio::process::Command;
use turbomcp::client::{ClientBuilder, ClientHandler, connect_child};
use turbomcp::neutral;

/// How this client answers server→client requests.
///
/// A server can ask the *client* for things mid-request: input from the user
/// (`elicitation/create`), an LLM completion (`sampling/createMessage`), or the
/// filesystem roots it may touch (`roots/list`). The trait's defaults decline
/// sampling and report no roots, so you implement only what you support — but
/// `elicit` has no safe default and is always yours to answer.
struct Cli;

#[turbomcp::client::async_trait]
impl ClientHandler for Cli {
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        // A real client would render `request.message` and
        // `request.requested_schema` and collect the user's answer. Declining
        // is always a valid response — servers must handle it.
        eprintln!("server asked: {}", request.message);
        neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
    }

    async fn on_notification(&self, method: String, _params: Option<serde_json::Value>) {
        eprintln!("notification: {method}");
    }
}

/// The `hello_world` example binary, built next to this one by `cargo run`.
fn hello_world_bin() -> PathBuf {
    let mut path = std::env::current_exe().expect("current exe path");
    path.pop(); // …/debug/examples/client → …/debug/examples
    path.push(format!("hello_world{}", std::env::consts::EXE_SUFFIX));
    path
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = hello_world_bin();
    if !server.is_file() {
        eprintln!(
            "build the server first: cargo build -p turbomcp --example hello_world\n\
             (expected it at {})",
            server.display()
        );
        return Ok(());
    }

    // Spawn the server and run the handshake. `connect_child` hands back the
    // `Child` too, so process lifetime stays the caller's to manage.
    let (client, mut child) = connect_child(
        ClientBuilder::new("example-client", "1.0.0").with_handler(Cli),
        {
            let mut cmd = Command::new(&server);
            cmd.kill_on_drop(true);
            cmd
        },
    )
    .await?;

    // What the handshake settled on. The client speaks both protocol
    // revisions; the server picked this one.
    println!("connected to {:?}", client.server_info());
    println!("protocol:    {}", client.protocol_version().as_str());
    if let Some(instructions) = client.instructions() {
        println!("instructions: {instructions}");
    }

    // Enumerate tools. Use `list_all_tools` rather than `list_tools(None)`:
    // a paginating server answers the latter with only the first page.
    let tools = client.list_all_tools().await?;
    println!("\n{} tool(s):", tools.len());
    for tool in &tools {
        let description = tool.description.as_deref().unwrap_or("(no description)");
        println!("  - {}: {description}", tool.name);
    }

    // Call one. A *tool-level* failure is not an `Err` here — it comes back as
    // `is_error: true` with the reason in `content`, which is exactly what a
    // model needs in order to correct itself.
    let mut args = Map::new();
    args.insert("name".into(), json!("world"));
    let result = client.call_tool("hello", args).await?;
    println!("\ncall_tool(hello) -> is_error={:?}", result.is_error);
    for block in &result.content {
        match block {
            neutral::Content::Text { text, .. } => println!("  text: {text}"),
            other => println!("  {other:?}"),
        }
    }

    // Resources and prompts are enumerated the same way; this server has none,
    // and asking a server that doesn't advertise the capability is an error, so
    // check what it announced first.
    let caps = client.server_capabilities();
    if caps.get("resources").is_some() {
        println!("\nresources: {:?}", client.list_all_resources().await?);
    } else {
        println!("\nserver advertises no resources capability");
    }

    child.kill().await?;
    Ok(())
}
