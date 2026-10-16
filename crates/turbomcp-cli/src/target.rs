//! What an operation connects to: an endpoint URL, a command to launch over
//! stdio, or a server named in a configuration file.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use clap::{Args, ValueEnum};
use turbomcp::client::{Client, ClientBuilder, ConnectMode};
use turbomcp::proxy::{Inherit, OutboundAuth, Upstream};

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

    /// What a command server inherits of this environment: the variables
    /// a program needs to run (`safe`, the default, as MCP hosts do), all of
    /// it, or nothing. Set more with the command's own environment.
    #[arg(long, value_enum, default_value_t = InheritEnv::Safe)]
    pub inherit_env: InheritEnv,

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

/// What a command server inherits.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum InheritEnv {
    /// `HOME`, `PATH`, `USER` and the like.
    Safe,
    /// Everything.
    All,
    /// Nothing.
    None,
}

impl From<InheritEnv> for Inherit {
    fn from(what: InheritEnv) -> Self {
        match what {
            InheritEnv::Safe => Self::Safe,
            InheritEnv::All => Self::All,
            InheritEnv::None => Self::Nothing,
        }
    }
}

/// A live connection, and the child process behind it when there is one.
pub struct Connected {
    pub client: Client,
    child: Option<turbomcp::client::ServerProcess>,
}

impl Connected {
    /// End the connection (and the child: closing its stdin is the spec's
    /// shutdown, then `SIGTERM` and `SIGKILL` for a server that ignores it).
    pub async fn close(self) {
        self.client.close().await;
        if let Some(child) = self.child {
            child.grace(Duration::from_secs(2)).shutdown().await;
        }
    }
}

impl Target {
    /// The upstream this target names, and what to authenticate there with.
    pub fn upstream(&self) -> Result<(Upstream, OutboundAuth)> {
        let (mut upstream, mut auth) = match (&self.server, self.words.as_slice()) {
            (Some(_), [_, ..]) => bail!("give either `--server` or a SERVER, not both"),
            (None, []) => bail!("no server: give a URL, a command, or `--config` and `--server`"),
            (Some(name), []) => {
                let path = self
                    .config
                    .as_ref()
                    .context("`--server` needs `--config`")?;
                let server = crate::config::load(path, std::slice::from_ref(name))?
                    .servers
                    .pop()
                    .with_context(|| format!("no enabled server `{name}` in {}", path.display()))?;
                (server.upstream, server.settings.auth)
            }
            (None, [url]) if url.starts_with("http://") || url.starts_with("https://") => {
                (Upstream::http(url.clone()), OutboundAuth::None)
            }
            (None, [url]) if url.starts_with("ws://") || url.starts_with("wss://") => {
                (Upstream::websocket(url.clone()), OutboundAuth::None)
            }
            (None, [command, args @ ..]) => (
                Upstream::stdio(command.clone(), args.iter().cloned())
                    .inherit_env(self.inherit_env.into()),
                OutboundAuth::None,
            ),
        };
        let stdio = matches!(upstream, Upstream::Stdio { .. });
        if stdio && (self.bearer.is_some() || !self.headers.is_empty()) {
            bail!(
                "`--bearer` and `--header` are for HTTP and WebSocket servers; a command \
                 gets credentials from its environment"
            );
        }
        for header in &self.headers {
            let (name, value) = header
                .split_once(':')
                .with_context(|| format!("header `{header}` is not `Name: value`"))?;
            upstream = upstream.header(name.trim(), value.trim());
        }
        if let Some(token) = &self.bearer {
            auth = OutboundAuth::bearer(token.clone());
        }
        Ok((upstream, auth))
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

    /// Connect, negotiating with `mode`: the gateway's own connect, so a
    /// command starts with the same environment policy, and headers get the
    /// same handling, as under `turbomcp proxy`.
    pub async fn connect_as(&self, mode: ConnectMode) -> Result<Connected> {
        let (upstream, auth) = self.upstream().context(crate::Failure::Usage)?;
        let (client, child) = turbomcp::proxy::connect(&upstream, self.client(mode), &auth)
            .await
            .map_err(|e| anyhow::Error::new(e).context(crate::Failure::Connect))?;
        Ok(Connected { client, child })
    }
}
