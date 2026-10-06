//! `turbomcp proxy`: the servers in an mcpServers configuration, served as
//! one, over stdio (one entry in a host's own configuration standing for
//! all of them) or Streamable HTTP.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Args;
use turbomcp::proxy::RemoteServer;
use turbomcp::{CancellationToken, Composite, Implementation, McpServerCore};

/// Arguments of `turbomcp proxy`.
#[derive(Args, Debug)]
pub struct ProxyArgs {
    /// The mcpServers configuration listing the servers to serve.
    #[arg(long, value_name = "FILE", env = "TURBOMCP_CONFIG")]
    pub config: PathBuf,

    /// Serve Streamable HTTP on this address instead of stdio.
    #[arg(long, value_name = "ADDR")]
    pub http: Option<SocketAddr>,

    /// The MCP endpoint's path, with `--http`.
    #[arg(long, value_name = "PATH", default_value = "/mcp", requires = "http")]
    pub path: String,

    /// Serve only these servers (comma-separated names).
    #[arg(long, value_delimiter = ',', value_name = "NAMES")]
    pub only: Vec<String>,

    /// What joins a server's name to its tools' and prompts' names.
    #[arg(long, default_value = "__")]
    pub separator: String,

    /// Fail if any server can't be reached, instead of serving the others.
    #[arg(long)]
    pub strict: bool,
}

pub async fn run(args: ProxyArgs) -> Result<()> {
    let mut servers = crate::config::load(&args.config)?;
    if !args.only.is_empty() {
        for name in &args.only {
            if !servers.iter().any(|s| &s.name == name) {
                bail!("no enabled server `{name}` in {}", args.config.display());
            }
        }
        servers.retain(|s| args.only.contains(&s.name));
    }
    if servers.is_empty() {
        bail!("no enabled servers in {}", args.config.display());
    }

    // Every upstream at once: a slow one doesn't hold up the rest.
    let connecting = servers.into_iter().map(|server| async move {
        let connected = RemoteServer::connect(server.upstream).await;
        (server.name, connected)
    });
    let mut remotes = Vec::new();
    for (name, connected) in futures::future::join_all(connecting).await {
        match connected {
            Ok(remote) => remotes.push((name, remote)),
            Err(e) if !args.strict => {
                tracing::warn!(
                    server = %name,
                    error = format!("{:#}", anyhow::Error::new(e)),
                    "unreachable; serving the others"
                );
            }
            Err(e) => return Err(e).with_context(|| format!("server `{name}`")),
        }
    }
    if remotes.is_empty() {
        bail!("no server could be reached");
    }

    let mut gateway = Composite::new(Implementation::new(
        "turbomcp-proxy",
        env!("CARGO_PKG_VERSION"),
    ))
    .separator(args.separator.clone())?;
    let instructions: Vec<String> = remotes
        .iter()
        .filter_map(|(name, remote)| {
            remote
                .instructions()
                .map(|text| format!("{name}{}: {text}", args.separator))
        })
        .collect();
    if !instructions.is_empty() {
        gateway = gateway.instructions(instructions.join("\n\n"));
    }
    for (name, remote) in &remotes {
        gateway = gateway
            .mount(name, remote.clone().into_server())
            .with_context(|| format!("server `{name}`"))?;
    }
    let server = turbomcp::Server::new(gateway.into_server().build());
    for (_, remote) in &remotes {
        remote.forward_changes_to(server.notifier());
    }
    let names: Vec<&str> = remotes.iter().map(|(name, _)| name.as_str()).collect();
    tracing::info!(servers = ?names, "serving");

    let shutdown = CancellationToken::new();
    let serving = async {
        match args.http {
            Some(addr) => {
                if !addr.ip().is_loopback() {
                    tracing::warn!(
                        %addr,
                        "serving beyond loopback with no authentication: anyone who can reach \
                         it can use every server behind it"
                    );
                }
                let config = turbomcp::http::HttpConfig::new()
                    .path(args.path.clone())
                    .with_shutdown(shutdown.clone());
                server
                    .serve(turbomcp::http::Http::bind(addr).config(config))
                    .await
            }
            None => server.serve(turbomcp::stdio()).await,
        }
    };
    let result = tokio::select! {
        result = serving => result.context("serving"),
        _ = tokio::signal::ctrl_c() => {
            shutdown.cancel();
            Ok(())
        }
    };
    // Each stdio upstream gets the spec's shutdown, whatever ended serving.
    futures::future::join_all(remotes.iter().map(|(_, remote)| remote.shutdown())).await;
    result
}
