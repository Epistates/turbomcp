//! Where a [`RemoteServer`](crate::RemoteServer) connects, and how it
//! authenticates there.

use std::collections::BTreeMap;
use std::path::PathBuf;

use zeroize::Zeroizing;

/// An upstream MCP server: a command to launch over stdio, or an endpoint.
#[derive(Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Upstream {
    /// A server launched as a child process, speaking newline-delimited
    /// JSON-RPC on its stdin and stdout.
    ///
    /// Nothing runs unless the operator names it: there is no implicit
    /// command, shell, or `PATH` search beyond the OS's own for `command`.
    Stdio {
        /// The program.
        command: String,
        /// Its arguments.
        args: Vec<String>,
        /// Variables set for it, on top of what it inherits.
        env: BTreeMap<String, String>,
        /// What it inherits of the proxy's environment.
        inherit: Inherit,
        /// Its working directory, when not the proxy's.
        cwd: Option<PathBuf>,
    },
    /// A Streamable HTTP endpoint (feature `http`).
    #[cfg(feature = "http")]
    Http {
        /// The MCP endpoint URL.
        url: String,
    },
    /// A WebSocket endpoint (feature `websocket`).
    #[cfg(feature = "websocket")]
    WebSocket {
        /// The `ws://` or `wss://` URL.
        url: String,
    },
}

impl core::fmt::Debug for Upstream {
    /// Never renders `env`: it is where operators put upstream secrets.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Stdio {
                command,
                args,
                env,
                inherit,
                cwd,
            } => f
                .debug_struct("Stdio")
                .field("command", command)
                .field("args", args)
                .field("env", &env.keys().collect::<Vec<_>>())
                .field("inherit", inherit)
                .field("cwd", cwd)
                .finish(),
            #[cfg(feature = "http")]
            Self::Http { url } => f.debug_struct("Http").field("url", url).finish(),
            #[cfg(feature = "websocket")]
            Self::WebSocket { url } => f.debug_struct("WebSocket").field("url", url).finish(),
        }
    }
}

impl Upstream {
    /// Launch `command` with `args` over stdio.
    pub fn stdio<I, S>(command: impl Into<String>, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self::Stdio {
            command: command.into(),
            args: args.into_iter().map(Into::into).collect(),
            env: BTreeMap::new(),
            inherit: Inherit::default(),
            cwd: None,
        }
    }

    /// Connect to the Streamable HTTP endpoint at `url`.
    #[cfg(feature = "http")]
    #[cfg_attr(docsrs, doc(cfg(feature = "http")))]
    pub fn http(url: impl Into<String>) -> Self {
        Self::Http { url: url.into() }
    }

    /// Connect to the WebSocket endpoint at `url`.
    #[cfg(feature = "websocket")]
    #[cfg_attr(docsrs, doc(cfg(feature = "websocket")))]
    pub fn websocket(url: impl Into<String>) -> Self {
        Self::WebSocket { url: url.into() }
    }

    /// Set an environment variable for a stdio upstream (ignored otherwise).
    #[must_use]
    // Irrefutable when only stdio upstreams are compiled in.
    #[allow(irrefutable_let_patterns)]
    pub fn env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        if let Self::Stdio { env, .. } = &mut self {
            env.insert(key.into(), value.into());
        }
        self
    }

    /// Choose what a stdio upstream inherits of the proxy's environment
    /// (default [`Inherit::Safe`]; ignored otherwise).
    #[must_use]
    #[allow(irrefutable_let_patterns)]
    pub fn inherit_env(mut self, what: Inherit) -> Self {
        if let Self::Stdio { inherit, .. } = &mut self {
            *inherit = what;
        }
        self
    }

    /// Run a stdio upstream in `dir` (ignored otherwise).
    #[must_use]
    #[allow(irrefutable_let_patterns)]
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        if let Self::Stdio { cwd, .. } = &mut self {
            *cwd = Some(dir.into());
        }
        self
    }

    /// A short label for logs and errors (never a secret).
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Stdio { command, .. } => format!("stdio:{command}"),
            #[cfg(feature = "http")]
            Self::Http { url } => url.clone(),
            #[cfg(feature = "websocket")]
            Self::WebSocket { url } => url.clone(),
        }
    }
}

/// What a stdio upstream inherits of the proxy's environment.
///
/// A proxy's environment tends to hold its own secrets (cloud credentials,
/// API keys, registry tokens), and an upstream is often a third-party package
/// fetched at launch, so by default it gets only what a program needs to run:
/// the variables the official MCP SDKs pass on, and nothing else. Give it
/// more by name with [`Upstream::env`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum Inherit {
    /// `HOME`, `LOGNAME`, `PATH`, `SHELL`, `TERM` and `USER` (on Windows,
    /// `APPDATA`, `HOMEDRIVE`, `HOMEPATH`, `LOCALAPPDATA`, `PATH`,
    /// `PROCESSOR_ARCHITECTURE`, `SYSTEMDRIVE`, `SYSTEMROOT`, `TEMP`,
    /// `USERNAME`, `USERPROFILE` and `PROGRAMFILES`): the official SDKs'
    /// list. A value that is an exported shell function is never passed on.
    #[default]
    Safe,
    /// Everything (the operator vouches for the upstream).
    All,
    /// Nothing but what [`Upstream::env`] sets.
    Nothing,
}

impl Inherit {
    /// The variables [`Inherit::Safe`] passes on.
    pub(crate) const SAFE: &'static [&'static str] = if cfg!(windows) {
        &[
            "APPDATA",
            "HOMEDRIVE",
            "HOMEPATH",
            "LOCALAPPDATA",
            "PATH",
            "PROCESSOR_ARCHITECTURE",
            "SYSTEMDRIVE",
            "SYSTEMROOT",
            "TEMP",
            "USERNAME",
            "USERPROFILE",
            "PROGRAMFILES",
        ]
    } else {
        &["HOME", "LOGNAME", "PATH", "SHELL", "TERM", "USER"]
    };
}

/// How the proxy authenticates to an upstream, as itself.
///
/// The downstream caller's own token is never forwarded: the authorization
/// specification forbids passing through the token a server received ("MUST
/// NOT pass through the token it received from the MCP client").
#[derive(Clone, Default)]
#[non_exhaustive]
pub enum OutboundAuth {
    /// No credentials (a local stdio server, a public endpoint).
    #[default]
    None,
    /// A fixed bearer token from configuration, presented on every request
    /// to an HTTP or WebSocket upstream. Wiped from memory when dropped.
    Static(Zeroizing<String>),
}

impl core::fmt::Debug for OutboundAuth {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::None => f.write_str("None"),
            Self::Static(_) => f.write_str("Static(<redacted>)"),
        }
    }
}

impl OutboundAuth {
    /// A fixed bearer token.
    pub fn bearer(token: impl Into<String>) -> Self {
        Self::Static(Zeroizing::new(token.into()))
    }
}
