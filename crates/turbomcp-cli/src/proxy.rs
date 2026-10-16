//! `turbomcp proxy`: the servers in an mcpServers configuration, served as
//! one, over stdio (one entry in a host's own configuration standing for
//! all of them) or Streamable HTTP.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::Args;
use turbomcp::proxy::{OutboundAuth, ProxyError, RemoteServer};
use turbomcp::{CancellationToken, Composite, Implementation, McpServerCore};

use crate::Failure;
use crate::config::Server;

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

    /// Fail if any server can't be used (an entry this command can't read,
    /// or a server that can't be reached), instead of serving the others.
    #[arg(long)]
    pub strict: bool,

    /// Seconds to keep trying a server that can't be reached at startup
    /// (one still starting, or fetching itself on first run) before
    /// serving without it; 0 tries once.
    #[arg(long, value_name = "SECONDS", default_value_t = 30)]
    pub retry_for: u64,

    /// Seconds an upstream may go silent on a call before it fails (default
    /// 60). Each progress report it sends restarts the clock, so a long
    /// operation that keeps reporting runs on, for up to an hour.
    #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
    pub timeout: Option<u64>,

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

    /// Serve `--http` beyond loopback with no authentication (or fetch the
    /// issuer's keys over plain HTTP). Without it either is refused:
    /// anyone who can reach the gateway can use every server behind it,
    /// commands included.
    #[arg(long, requires = "http")]
    pub insecure: bool,

    /// A browser origin allowed to call `--http` (repeatable; by default
    /// only requests without an `Origin`, and loopback origins on a
    /// loopback address, are).
    #[arg(long = "allow-origin", value_name = "ORIGIN", requires = "http")]
    pub allow_origins: Vec<String>,

    /// A `Host` the `--http` endpoint answers to beyond the bound address
    /// (repeatable): the name clients reach it by behind a proxy.
    #[arg(long = "allow-host", value_name = "HOST", requires = "http")]
    pub allow_hosts: Vec<String>,

    /// Upstream connections the gateway keeps open at most, over all
    /// servers' callers; past it a call is refused, not someone else's
    /// connection closed.
    #[arg(long, value_name = "N", default_value_t = 1000)]
    pub max_connections: u64,

    /// Upstream connections one caller may hold per server (one per
    /// session, for a server keyed by session).
    #[arg(long, value_name = "N", default_value_t = 32)]
    pub max_connections_per_caller: u64,

    /// Seconds an unused per-caller upstream connection stays open.
    #[arg(long, value_name = "SECONDS", default_value_t = 600)]
    pub idle_timeout: u64,

    /// Seconds in-flight `--http` requests get to finish on shutdown.
    #[arg(long, value_name = "SECONDS", default_value_t = 10)]
    pub drain: u64,
}

/// The front door for `--http`: bearer tokens checked against the issuer's
/// keys, `aud`, `iss` and `exp`, with the RFC 9728 metadata a client needs
/// to find the issuer.
fn authenticator(
    args: &ProxyArgs,
    addr: SocketAddr,
    retain_tokens: bool,
) -> Result<Option<std::sync::Arc<dyn turbomcp::HttpAuthenticator>>> {
    let (Some(issuer), Some(jwks)) = (&args.auth_issuer, &args.auth_jwks) else {
        return Ok(None);
    };
    if jwks.starts_with("http://") && !args.insecure && !is_loopback_url(jwks) {
        bail!(
            "--auth-jwks `{jwks}` is plain HTTP: whoever is on the path could hand the gateway \
             their own keys (use https, or --insecure)"
        );
    }
    if args.auth_audience.is_none() && addr.ip().is_unspecified() {
        bail!(
            "bound to every address, the gateway can't tell which URL clients reach it by: \
             give it with --auth-audience"
        );
    }
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
    // Token exchange needs the caller's token after authentication.
    server = server.retain_token(retain_tokens);
    Ok(Some(std::sync::Arc::new(server)))
}

/// Whether `url`'s host is a loopback address or `localhost`.
fn is_loopback_url(url: &str) -> bool {
    url.parse::<http::Uri>()
        .ok()
        .and_then(|u| u.host().map(str::to_owned))
        .is_some_and(|host| {
            let host = host.trim_start_matches('[').trim_end_matches(']');
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        })
}

/// Connect to `server`, retrying for `retry_for` while it can't be reached
/// (a server still starting, an `npx` package still downloading). A command
/// that can't start, or a configuration the library refuses, isn't retried.
async fn connect(
    server: Server,
    args: &ProxyArgs,
    retry_for: Duration,
) -> (String, Result<RemoteServer, ProxyError>) {
    let deadline = tokio::time::Instant::now() + retry_for;
    let mut backoff = Duration::from_millis(500);
    loop {
        let settings = &server.settings;
        let mut remote = RemoteServer::builder(server.upstream.clone())
            .auth(settings.auth.clone())
            .forward_sampling(settings.forward_sampling)
            .forward_roots(settings.forward_roots)
            .tags(settings.tags.clone())
            .max_connections(args.max_connections)
            .max_connections_per_caller(args.max_connections_per_caller)
            .idle_timeout(Duration::from_secs(args.idle_timeout));
        if let Some(timeout) = settings.timeout.or(args.timeout.map(Duration::from_secs)) {
            remote = remote.timeout(timeout);
        }
        if let Some(key) = settings.key {
            remote = remote.key(key);
        }
        let outcome = remote.connect().await;
        let retryable = matches!(
            outcome,
            Err(ProxyError::Connect { .. } | ProxyError::Dial { .. })
        );
        if !retryable || tokio::time::Instant::now() + backoff > deadline {
            return (server.name, outcome);
        }
        tracing::info!(server = %server.name, "not reachable yet; retrying");
        tokio::time::sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(5));
    }
}

/// Resolves on `SIGINT` (Ctrl-C) or, on Unix, `SIGTERM` (what `docker stop`
/// and systemd send).
async fn terminated() {
    #[cfg(unix)]
    {
        let mut term =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(term) => term,
                Err(e) => {
                    tracing::warn!(error = %e, "SIGTERM can't be handled; Ctrl-C still can");
                    let _ = tokio::signal::ctrl_c().await;
                    return;
                }
            };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

pub async fn run(args: ProxyArgs) -> Result<()> {
    let loaded = crate::config::load(&args.config, &args.only).context(Failure::Usage)?;
    for (name, reason) in &loaded.skipped {
        if args.strict {
            return Err(anyhow::anyhow!("server `{name}`: {reason:#}").context(Failure::Usage));
        }
        tracing::warn!(server = %name, reason = format!("{reason:#}"), "skipped");
    }
    let servers = loaded.servers;
    if servers.is_empty() {
        return Err(
            anyhow::anyhow!("no usable servers in {}", args.config.display())
                .context(Failure::Usage),
        );
    }
    let exchanging = servers
        .iter()
        .any(|s| matches!(s.settings.auth, OutboundAuth::TokenExchange(_)));
    if exchanging && args.auth_issuer.is_none() {
        return Err(anyhow::anyhow!(
            "a server uses `tokenExchange`, which exchanges each caller's token: \
             authenticate callers with --auth-issuer"
        )
        .context(Failure::Usage));
    }
    let front = match args.http {
        Some(addr) => {
            let authenticator = authenticator(&args, addr, exchanging).context(Failure::Usage)?;
            if authenticator.is_none() && !addr.ip().is_loopback() && !args.insecure {
                return Err(anyhow::anyhow!(
                    "serving {addr} beyond loopback with no authentication would let anyone \
                     who can reach it use every server behind it: authenticate callers with \
                     --auth-issuer, or pass --insecure"
                )
                .context(Failure::Usage));
            }
            Some((addr, authenticator))
        }
        None => None,
    };

    // Every upstream at once: a slow one doesn't hold up the rest.
    let retry_for = Duration::from_secs(args.retry_for);
    let connecting = servers
        .into_iter()
        .map(|server| connect(server, &args, retry_for));
    let mut remotes = Vec::new();
    for (name, connected) in futures::future::join_all(connecting).await {
        match connected {
            Ok(remote) => remotes.push((name, remote)),
            Err(e) if !args.strict => {
                tracing::warn!(
                    server = %name,
                    error = format!("{:#}", anyhow::Error::new(e)),
                    "unreachable; serving the others (restart to try it again)"
                );
            }
            Err(e) => {
                return Err(anyhow::Error::new(e)
                    .context(Failure::Connect)
                    .context(format!("server `{name}`")));
            }
        }
    }
    if remotes.is_empty() {
        return Err(anyhow::anyhow!("no server could be reached").context(Failure::Connect));
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
    let drains = front.is_some();
    let serving = async {
        match front {
            Some((addr, authenticator)) => {
                if authenticator.is_none() && !addr.ip().is_loopback() {
                    tracing::warn!(%addr, "serving beyond loopback with no authentication (--insecure)");
                }
                let mut config = turbomcp::http::HttpConfig::new()
                    .path(args.path.clone())
                    .with_shutdown(shutdown.clone());
                if let Some(authenticator) = authenticator {
                    config = config.with_authenticator(authenticator);
                }
                for origin in &args.allow_origins {
                    config = config.allow_origin(origin.clone());
                }
                for host in &args.allow_hosts {
                    config = config.allow_host(host.clone());
                }
                server
                    .serve(turbomcp::http::Http::bind(addr).config(config))
                    .await
                    .context("serving")
            }
            None => server.serve(turbomcp::stdio()).await.context("serving"),
        }
    };
    tokio::pin!(serving);
    let result = tokio::select! {
        result = &mut serving => result,
        () = terminated() => {
            // In-flight requests get to finish, within the drain window.
            // Over stdio there is nothing to drain.
            shutdown.cancel();
            let drain = Duration::from_secs(if drains { args.drain } else { 0 });
            match tokio::time::timeout(drain, &mut serving).await {
                Ok(result) => result,
                Err(_) => {
                    if drains {
                        tracing::warn!("requests still running after the drain window; stopping");
                    }
                    Ok(())
                }
            }
        }
    };
    // Each stdio upstream gets the spec's shutdown, whatever ended serving.
    futures::future::join_all(remotes.iter().map(|(_, remote)| remote.shutdown())).await;
    result
}
