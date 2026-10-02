//! # Graceful shutdown over stdio
//!
//! Serve until the client closes stdin, or until Ctrl-C (SIGINT): the reader
//! stops, calls in flight get `drain_timeout` to finish and flush their
//! replies, and the process exits. It doesn't wait for the client to write
//! another line first: stdin is read on a thread the runtime doesn't wait for.
//!
//! Run with: `cargo run -p turbomcp --example graceful_shutdown`

use std::time::Duration;

use turbomcp::prelude::*;
use turbomcp::{CancellationToken, Pipe, ServeConfig};

#[derive(Clone)]
struct Clock;

#[server(name = "clock", version = "1.0.0")]
impl Clock {
    /// Seconds since the Unix epoch.
    #[tool(description = "Seconds since the Unix epoch")]
    async fn now(&self) -> McpResult<u64> {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .map_err(|e| McpError::internal(e.to_string()))
    }
}

#[tokio::main]
async fn main() -> Result<(), turbomcp::ProtocolError> {
    let shutdown = CancellationToken::new();
    let on_signal = shutdown.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            on_signal.cancel();
        }
    });
    Clock
        .into_server()
        .serve(Pipe::new(stdio()).config(ServeConfig {
            shutdown,
            drain_timeout: Duration::from_secs(5),
            ..ServeConfig::default()
        }))
        .await
}
