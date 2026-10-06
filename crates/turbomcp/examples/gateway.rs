//! # A gateway (v4)
//!
//! A remote MCP server beside your own tools, behind one endpoint, under
//! one policy. The remote is a [`RemoteServer`]: the same kind of object as
//! a local `#[server]`, so it mounts in a [`Composite`] and everything the
//! composite does (visibility, interceptors, telemetry, the HTTP front's
//! authentication) covers its tools exactly as it covers yours.
//!
//! - The remote can speak any revision, and so can your clients: the
//!   gateway translates between `2025-06-18`, `2025-11-25` and `2026-07-28`
//!   both ways.
//! - Its requests for input (elicitation, sampling, roots) reach the client
//!   whose call caused them; its change notifications reach every client.
//! - It is launched with the environment a program needs, not the gateway's
//!   (where its secrets live), and stopped with the spec's shutdown.
//!
//! Run with any MCP server command (default: the reference "everything"
//! server, which needs Node):
//!
//! ```sh
//! cargo run -p turbomcp --example gateway --features "proxy http" -- \
//!     npx -y @modelcontextprotocol/server-everything
//! turbomcp tools http://127.0.0.1:8080/mcp     # from turbomcp-cli
//! ```
//!
//! To require OAuth bearer tokens, give `HttpConfig::with_authenticator` a
//! `turbomcp::auth::ResourceServer`; to call the remote as each caller
//! rather than as the gateway, see `OutboundAuth::TokenExchange`.

use std::sync::Arc;

use turbomcp::http::{Http, HttpConfig};
use turbomcp::prelude::*;
use turbomcp::proxy::{RemoteServer, Upstream};
use turbomcp::visibility::Visibility;
use turbomcp::{CancellationToken, Composite, Implementation};

/// The gateway's own tools.
#[derive(Clone)]
struct Ops;

#[server(name = "ops", version = "1.0.0")]
impl Ops {
    /// Whether the gateway is up.
    #[tool]
    async fn status(&self) -> String {
        "gateway up".into()
    }

    /// Maintenance, for operators: hidden from every caller below.
    #[tool(tags("internal"))]
    async fn drain(&self) -> String {
        "draining".into()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut words: Vec<String> = std::env::args().skip(1).collect();
    if words.is_empty() {
        words = ["npx", "-y", "@modelcontextprotocol/server-everything"]
            .map(String::from)
            .to_vec();
    }
    let remote =
        RemoteServer::connect(Upstream::stdio(words[0].clone(), words[1..].to_vec())).await?;
    eprintln!(
        "upstream speaks {}, keyed {:?}",
        remote.protocol_version().as_str(),
        remote.key()
    );

    // Your tools keep their names; the remote's are prefixed (`everything__echo`),
    // so the two can't collide. Resource URIs pass through untouched.
    let gateway = Composite::new(Implementation::new("gateway", "1.0.0"))
        .mount_flat(Ops.into_server())?
        .mount("everything", remote.clone().into_server())?
        .into_server()
        .with_visibility(Arc::new(Visibility::new().hiding_tagged(["internal"])));
    let server = turbomcp::Server::new(gateway.build());
    remote.forward_changes_to(server.notifier());

    let shutdown = CancellationToken::new();
    let stop = shutdown.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        stop.cancel();
    });
    let addr = "127.0.0.1:8080".parse()?;
    eprintln!("gateway on http://{addr}/mcp (ctrl-c to stop)");
    server
        .serve(Http::bind(addr).config(HttpConfig::new().with_shutdown(shutdown)))
        .await?;
    remote.shutdown().await;
    Ok(())
}
