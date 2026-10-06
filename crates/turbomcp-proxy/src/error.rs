//! What can go wrong connecting to an upstream.

/// Connecting to, or configuring, an upstream failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProxyError {
    /// The upstream's command could not be started.
    #[error("could not start upstream `{command}`: {source}")]
    Spawn {
        /// The command.
        command: String,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// The connection or the handshake failed.
    #[error("could not connect to upstream {upstream}: {source}")]
    Connect {
        /// Which upstream (a command or URL; never a secret).
        upstream: String,
        /// Why.
        #[source]
        source: Box<turbomcp_client::ClientError>,
    },
    /// The upstream's configuration is invalid.
    #[error("invalid upstream configuration: {0}")]
    Config(String),
}
