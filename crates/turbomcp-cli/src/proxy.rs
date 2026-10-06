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

    /// Require OAuth 2.1 bearer tokens on `--http`: JWTs issued by this
    /// authorization server (its issuer identifier).
    #[arg(long, value_name = "URL", requires_all = ["http", "auth_jwks"])]
    pub auth_issuer: Option<String>,

    /// Where the issuer publishes its signing keys (its `jwks_uri`).
    #[arg(long, value_name = "URL", requires = "auth_issuer")]
    pub auth_jwks: Option<String>,

    /// The gateway's resource identifier, which tokens must name in `aud`
    /// (default: the endpoint's URL on the bound address).
    #[arg(long, value_name = "URL", requires = "auth_issuer")]
    pub auth_audience: Option<String>,

    /// Scopes every request's token must carry (comma-separated).
    #[arg(
        long,
        value_delimiter = ',',
        value_name = "SCOPES",
        requires = "auth_issuer"
    )]
    pub auth_scopes: Vec<String>,
}

/// The front door for `--http`: bearer tokens checked against the issuer's
/// keys, `aud`, `iss` and `exp`, with the RFC 9728 metadata a client needs
/// to find the issuer.
fn authenticator(
    args: &ProxyArgs,
    addr: SocketAddr,
) -> Result<Option<std::sync::Arc<dyn turbomcp::HttpAuthenticator>>> {
    let (Some(issuer), Some(jwks)) = (&args.auth_issuer, &args.auth_jwks) else {
        return Ok(None);
    };
    let resource = args
        .auth_audience
        .clone()
        .unwrap_or_else(|| format!("http://{addr}{}", args.path));
    let parsed: http::Uri = resource
        .parse()
        .with_context(|| format!("--auth-audience `{resource}` is not a URL"))?;
    let origin = format!(
        "{}://{}",
        parsed.scheme_str().unwrap_or("http"),
        parsed
            .authority()
            .map(http::uri::Authority::as_str)
            .unwrap_or_default()
    );
    // RFC 9728 §3.1: the well-known name goes between the origin and the path.
    let metadata_url = format!(
        "{origin}/.well-known/oauth-protected-resource{}",
        parsed.path().trim_end_matches('/')
    );
    let keys = turbomcp_auth::HttpJwks::new(jwks.clone(), std::time::Duration::from_secs(3600));
    let validator = turbomcp_auth::JwtValidator::new(keys, resource.clone(), issuer.clone());
    let mut server = turbomcp_auth::ResourceServer::new(
        validator,
        turbomcp_auth::ResourceMetadata::new(resource, [issuer.clone()]),
        metadata_url,
    );
    if !args.auth_scopes.is_empty() {
        server = server.required_scopes(args.auth_scopes.clone());
    }
    Ok(Some(std::sync::Arc::new(server)))
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
                let authenticator = authenticator(&args, addr)?;
                if authenticator.is_none() && !addr.ip().is_loopback() {
                    tracing::warn!(
                        %addr,
                        "serving beyond loopback with no authentication: anyone who can reach \
                         it can use every server behind it (see --auth-issuer)"
                    );
                }
                let mut config = turbomcp::http::HttpConfig::new()
                    .path(args.path.clone())
                    .with_shutdown(shutdown.clone());
                if let Some(authenticator) = authenticator {
                    config = config.with_authenticator(authenticator);
                }
                server
                    .serve(turbomcp::http::Http::bind(addr).config(config))
                    .await
                    .context("serving")
            }
            None => server.serve(turbomcp::stdio()).await.context("serving"),
        }
    };
    let result = tokio::select! {
        result = serving => result,
        _ = tokio::signal::ctrl_c() => {
            shutdown.cancel();
            Ok(())
        }
    };
    // Each stdio upstream gets the spec's shutdown, whatever ended serving.
    futures::future::join_all(remotes.iter().map(|(_, remote)| remote.shutdown())).await;
    result
}
