//! Connecting to a server launched as a child process over stdio — the common
//! local-MCP case (the client spawns the server and talks to its stdin/stdout).

use tokio::io::BufReader;
use tokio::process::{Child, Command};
use turbomcp_core::codec::DefaultCodec;
use turbomcp_service::io::LineTransport;

use crate::client::{Client, ClientBuilder};
use crate::error::{ClientError, ClientResult};

/// Spawn `command` as a child process with piped stdio, connect a [`Client`] to
/// it, and run the handshake.
///
/// Returns the connected client and the [`Child`] handle so the caller controls
/// the subprocess lifecycle (e.g. `child.kill().await` on shutdown). Dropping
/// the `Child` leaves the process running detached, per Tokio's default.
///
/// # Errors
/// [`ClientError::Protocol`] if the process can't be spawned or its stdio pipes
/// can't be captured; otherwise propagates the handshake failure.
pub async fn connect_child(
    builder: ClientBuilder,
    mut command: Command,
) -> ClientResult<(Client, Child)> {
    use std::process::Stdio;

    command.stdin(Stdio::piped()).stdout(Stdio::piped());
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

    let transport = LineTransport::new(BufReader::new(stdout), stdin, DefaultCodec::default());
    match builder.connect(transport).await {
        Ok(client) => Ok((client, child)),
        Err(e) => {
            // The caller never gets the `Child`, so nobody else can stop it,
            // and tokio does not kill on drop: every failed attempt used to
            // leave a server process running. Kill it, and reap it.
            let _ = child.start_kill();
            let _ = child.wait().await;
            Err(e)
        }
    }
}
