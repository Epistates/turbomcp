//! What can go wrong connecting to an upstream.
//!
//! Each error names what failed; why is its [`source`](std::error::Error::source),
//! not repeated in its message, so a reporter that prints the chain prints
//! each cause once.

/// Connecting to, or configuring, an upstream failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProxyError {
    /// The upstream's command could not be started.
    #[error("could not start upstream `{command}`")]
    Spawn {
        /// The command.
        command: String,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// The connection or the handshake failed.
    #[error("could not connect to upstream {upstream}")]
    Connect {
        /// Which upstream (a command or URL; never a secret).
        upstream: String,
        /// Why.
        #[source]
        source: Box<turbomcp_client::ClientError>,
    },
    /// Opening a transport of the embedding application's own failed
    /// ([`RemoteServer::dial`](crate::RemoteServer::dial)).
    #[error("could not reach upstream {upstream}")]
    Dial {
        /// Which upstream.
        upstream: String,
        /// Why.
        #[source]
        source: std::io::Error,
    },
    /// The upstream's configuration is invalid.
    #[error("invalid upstream configuration: {0}")]
    Config(String),
}

/// `error` and each of its causes, joined by `: `, for a log line.
pub(crate) fn chain(error: &dyn std::error::Error) -> String {
    let mut out = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        out.push_str(": ");
        out.push_str(&cause.to_string());
        source = cause.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_cause_appears_once() {
        let error = ProxyError::Spawn {
            command: "server".into(),
            source: std::io::Error::other("no such file"),
        };
        assert_eq!(
            chain(&error),
            "could not start upstream `server`: no such file"
        );
    }
}
