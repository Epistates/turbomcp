//! What an operation connects to: an endpoint URL, a command to launch over
//! stdio, or a server named in a configuration file.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use turbomcp::client::{Client, ClientBuilder, ConnectMode, HttpClientTransport};
use turbomcp::proxy::Upstream;

/// Where to connect, and how.
#[derive(Args, Debug, Clone)]
pub struct Target {
    /// Use the server named NAME in the configuration file (`--config`).
    #[arg(long, value_name = "NAME", requires = "config")]
    pub server: Option<String>,

    /// An mcpServers configuration file, for `--server`.
    #[arg(long, value_name = "FILE", env = "TURBOMCP_CONFIG")]
    pub config: Option<PathBuf>,

    /// The protocol revision to negotiate.
    #[arg(long, value_enum, default_value_t = Mode::Auto)]
    pub mode: Mode,

    /// A bearer token for an HTTP or WebSocket server.
    #[arg(long, env = "TURBOMCP_BEARER", hide_env_values = true)]
    pub bearer: Option<String>,

    /// A header for an HTTP or WebSocket server, as `Name: value`
    /// (repeatable).
    #[arg(long = "header", value_name = "NAME: VALUE")]
    pub headers: Vec<String>,

    /// Give up on a request after this many seconds.
    #[arg(long, value_name = "SECONDS", default_value_t = 30)]
    pub timeout: u64,

    /// The server: an `http(s)://` or `ws(s)://` URL, or a command and its
    /// arguments (put options before it).
    #[arg(
        trailing_var_arg = true,
        allow_hyphen_values = true,
        value_name = "SERVER"
    )]
    pub words: Vec<String>,
}

/// The protocol revision to negotiate.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `2026-07-28` where the server serves it, else the newest it does.
    Auto,
    /// `2026-07-28` only.
    Modern,
    /// `2025-11-25` (or `2025-06-18`).
    Legacy,
}

impl From<Mode> for ConnectMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::Auto => Self::Auto,
            Mode::Modern => Self::Modern,
            Mode::Legacy => Self::Legacy,
        }
    }
}

/// A live connection, and the child process behind it when there is one.
pub struct Connected {
    pub client: Client,
    child: Option<tokio::process::Child>,
}

impl Connected {
    /// End the connection (and the child: closing its stdin is the spec's
    /// shutdown, and a server that ignores it is killed).
    pub async fn close(mut self) {
        self.client.close().await;
        if let Some(child) = &mut self.child
            && tokio::time::timeout(Duration::from_secs(2), child.wait())
                .await
                .is_err()
        {
            let _ = child.kill().await;
        }
    }
}

impl Target {
    /// The upstream this target names.
    pub fn upstream(&self) -> Result<Upstream> {
        let mut upstream = match (&self.server, self.words.as_slice()) {
            (Some(_), [_, ..]) => bail!("give either `--server` or a SERVER, not both"),
            (None, []) => bail!("no server: give a URL, a command, or `--config` and `--server`"),
            (Some(name), []) => {
                let path = self
                    .config
                    .as_ref()
                    .context("`--server` needs `--config`")?;
                crate::config::load(path)?
                    .into_iter()
                    .find(|s| &s.name == name)
                    .map(|s| s.upstream)
                    .with_context(|| format!("no enabled server `{name}` in {}", path.display()))?
            }
            (None, [url]) if url.starts_with("http://") || url.starts_with("https://") => {
                Upstream::http(url.clone())
            }
            (None, [url]) if url.starts_with("ws://") || url.starts_with("wss://") => {
                Upstream::websocket(url.clone())
            }
            (None, [command, args @ ..]) => Upstream::stdio(command.clone(), args.iter().cloned()),
        };
        for header in &self.headers {
            let (name, value) = header
                .split_once(':')
                .with_context(|| format!("header `{header}` is not `Name: value`"))?;
            upstream = upstream.header(name.trim(), value.trim());
        }
        if let Some(token) = &self.bearer {
            upstream = upstream.header("Authorization", format!("Bearer {token}"));
        }
        Ok(upstream)
    }

    /// A client configured as this target says.
    pub fn client(&self, mode: ConnectMode) -> ClientBuilder {
        ClientBuilder::new("turbomcp-cli", env!("CARGO_PKG_VERSION"))
            .with_connect_mode(mode)
            .with_timeout(Duration::from_secs(self.timeout))
    }

    /// Connect, negotiating as `--mode` says.
    pub async fn connect(&self) -> Result<Connected> {
        self.connect_as(self.mode.into()).await
    }

    /// Connect, negotiating with `mode`.
    pub async fn connect_as(&self, mode: ConnectMode) -> Result<Connected> {
        let builder = self.client(mode);
        let connected = match self.upstream()? {
            Upstream::Stdio {
                command,
                args,
                env,
                cwd,
                ..
            } => {
                let mut cmd = tokio::process::Command::new(&command);
                cmd.args(&args).envs(&env).kill_on_drop(true);
                if let Some(cwd) = cwd {
                    cmd.current_dir(cwd);
                }
                let (client, child) = turbomcp::client::connect_child(builder, cmd)
                    .await
                    .with_context(|| format!("starting `{command}`"))?;
                Connected {
                    client,
                    child: Some(child),
                }
            }
            Upstream::Http { url, headers } => {
                let mut transport = HttpClientTransport::new(url.clone())
                    .with_context(|| format!("endpoint {url}"))?;
                if !headers.is_empty() {
                    let mut map = http::HeaderMap::new();
                    for (name, value) in &headers {
                        map.insert(
                            http::HeaderName::from_bytes(name.as_bytes())?,
                            value.parse()?,
                        );
                    }
                    transport = transport.with_headers(map)?;
                }
                let client = builder
                    .connect(transport)
                    .await
                    .with_context(|| format!("connecting to {url}"))?;
                Connected {
                    client,
                    child: None,
                }
            }
            Upstream::WebSocket { url, headers } => {
                let mut request = http::Request::builder().uri(url.as_str());
                for (name, value) in &headers {
                    request = request.header(name.as_str(), value.as_str());
                }
                let transport =
                    turbomcp::client::WebSocketClientTransport::connect(request.body(())?)
                        .await
                        .with_context(|| format!("connecting to {url}"))?;
                let client = builder
                    .connect(transport)
                    .await
                    .with_context(|| format!("connecting to {url}"))?;
                Connected {
                    client,
                    child: None,
                }
            }
            other => bail!("unsupported server {other:?}"),
        };
        Ok(connected)
    }
}
