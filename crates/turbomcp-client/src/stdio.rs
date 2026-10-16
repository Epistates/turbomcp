//! Connecting to a server launched as a child process over stdio — the common
//! local-MCP case (the client spawns the server and talks to its stdin/stdout).

use tokio::io::BufReader;
use tokio::process::Command;
use turbomcp_core::codec::DefaultCodec;
use turbomcp_service::io::LineTransport;

use crate::client::{Client, ClientBuilder};
use crate::error::{ClientError, ClientResult};
use crate::process::ServerProcess;

/// Spawn `command` as a child process with piped stdio, connect a [`Client`] to
/// it, and run the handshake.
///
/// Returns the connected client and the [`ServerProcess`]: close the client,
/// then [`shutdown`](ServerProcess::shutdown) the process for the spec's
/// sequence (wait, `SIGTERM`, `SIGKILL`). Dropping the process kills it. On
/// Unix the server gets its own process group, so the sequence reaches
/// whatever it starts. Its stderr is left as `command` configured it; pipe it
/// and call [`ServerProcess::log_stderr`] to route it into `tracing`.
///
/// # Errors
/// [`ClientError::Protocol`] if the process can't be spawned or its stdio pipes
/// can't be captured; otherwise propagates the handshake failure, after
/// killing the process.
pub async fn connect_child(
    builder: ClientBuilder,
    mut command: Command,
) -> ClientResult<(Client, ServerProcess)> {
    use std::process::Stdio;

    command.stdin(Stdio::piped()).stdout(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|e| ClientError::Protocol(format!("failed to spawn server process: {e}")))?;

    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| ClientError::Protocol("child stdin not captured".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ClientError::Protocol("child stdout not captured".into()))?;
    // Owned from here, so a failed handshake (or a panic) kills it.
    let process = ServerProcess::new(child);

    let transport = LineTransport::new(BufReader::new(stdout), stdin, DefaultCodec::default());
    let client = builder.connect(transport).await?;
    Ok((client, process))
}
