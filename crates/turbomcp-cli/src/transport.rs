//! Transport factory and auto-detection

use crate::cli::{Connection, TransportKind};
use crate::error::{CliError, CliResult};
use std::time::Duration;
use turbomcp_client::Client;
use turbomcp_transport::{
    child_process::{ChildProcessConfig, ChildProcessTransport},
    tcp::TcpTransportBuilder,
    unix::UnixTransportBuilder,
};

/// Create a client with appropriate transport based on connection settings
pub async fn create_client(conn: &Connection) -> CliResult<ClientType> {
    let transport_kind = determine_transport(conn);

    match transport_kind {
        TransportKind::Stdio => {
            let transport = create_stdio_transport(conn)?;
            Ok(ClientType::Stdio(Client::new(transport)))
        }
        TransportKind::Http => {
            return Err(CliError::NotSupported(
                "HTTP transport - use turbomcp-transport's HTTP SSE implementation".to_string(),
            ));
        }
        TransportKind::Ws => {
            return Err(CliError::NotSupported(
                "WebSocket transport - use turbomcp-transport's WebSocket implementation"
                    .to_string(),
            ));
        }
        TransportKind::Tcp => {
            let transport = create_tcp_transport(conn).await?;
            Ok(ClientType::Tcp(Client::new(transport)))
        }
        TransportKind::Unix => {
            let transport = create_unix_transport(conn).await?;
            Ok(ClientType::Unix(Client::new(transport)))
        }
    }
}

/// Client type wrapper for different transports
pub enum ClientType {
    Stdio(Client<ChildProcessTransport>),
    Tcp(Client<turbomcp_transport::tcp::TcpTransport>),
    Unix(Client<turbomcp_transport::unix::UnixTransport>),
}

/// Determine transport type from connection config
pub fn determine_transport(conn: &Connection) -> TransportKind {
    // Use explicit transport if provided
    if let Some(transport) = &conn.transport {
        return transport.clone();
    }

    // Auto-detect based on URL/command patterns
    let url = &conn.url;

    if conn.command.is_some() {
        return TransportKind::Stdio;
    }

    if url.starts_with("tcp://") {
        return TransportKind::Tcp;
    }

    if url.starts_with("unix://") || url.starts_with("/") {
        return TransportKind::Unix;
    }

    if url.starts_with("ws://") || url.starts_with("wss://") {
        return TransportKind::Ws;
    }

    if url.starts_with("http://") || url.starts_with("https://") {
        return TransportKind::Http;
    }

    // Default to STDIO for executable paths
    TransportKind::Stdio
}

/// Create STDIO transport from connection
fn create_stdio_transport(conn: &Connection) -> CliResult<ChildProcessTransport> {
    // Use --command if provided, otherwise use --url
    let command_str = conn.command.as_deref().unwrap_or(&conn.url);

    // Parse command and arguments
    let parts: Vec<&str> = command_str.split_whitespace().collect();
    if parts.is_empty() {
        return Err(CliError::InvalidArguments(
            "No command specified for STDIO transport".to_string(),
        ));
    }

    let command = parts[0].to_string();
    let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

    // Create config
    let config = ChildProcessConfig {
        command,
        args,
        working_directory: None,
        environment: None,
        startup_timeout: Duration::from_secs(conn.timeout),
        shutdown_timeout: Duration::from_secs(5),
        max_message_size: 10 * 1024 * 1024, // 10MB
        buffer_size: 8192,                  // 8KB buffer
        kill_on_drop: true,                 // Kill process when client is dropped
    };

    // Create transport
    Ok(ChildProcessTransport::new(config))
}

/// Create TCP transport from connection
async fn create_tcp_transport(
    conn: &Connection,
) -> CliResult<turbomcp_transport::tcp::TcpTransport> {
    let url = &conn.url;

    // Parse TCP URL
    let addr_str = url
        .strip_prefix("tcp://")
        .ok_or_else(|| CliError::InvalidArguments(format!("Invalid TCP URL: {}", url)))?;

    // Parse into SocketAddr
    let socket_addr: std::net::SocketAddr = addr_str.parse().map_err(|e| {
        CliError::InvalidArguments(format!("Invalid address '{}': {}", addr_str, e))
    })?;

    let transport = TcpTransportBuilder::new().remote_addr(socket_addr).build();

    Ok(transport)
}

/// Create Unix socket transport from connection
async fn create_unix_transport(
    conn: &Connection,
) -> CliResult<turbomcp_transport::unix::UnixTransport> {
    let path = conn.url.strip_prefix("unix://").unwrap_or(&conn.url);

    let transport = UnixTransportBuilder::new_client().socket_path(path).build();

    Ok(transport)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_transport() {
        // STDIO detection
        let conn = Connection {
            transport: None,
            url: "./my-server".to_string(),
            command: None,
            auth: None,
            timeout: 30,
        };
        assert_eq!(determine_transport(&conn), TransportKind::Stdio);

        // Command override
        let conn = Connection {
            transport: None,
            url: "http://localhost".to_string(),
            command: Some("python server.py".to_string()),
            auth: None,
            timeout: 30,
        };
        assert_eq!(determine_transport(&conn), TransportKind::Stdio);

        // TCP detection
        let conn = Connection {
            transport: None,
            url: "tcp://localhost:8080".to_string(),
            command: None,
            auth: None,
            timeout: 30,
        };
        assert_eq!(determine_transport(&conn), TransportKind::Tcp);

        // Unix detection
        let conn = Connection {
            transport: None,
            url: "/tmp/mcp.sock".to_string(),
            command: None,
            auth: None,
            timeout: 30,
        };
        assert_eq!(determine_transport(&conn), TransportKind::Unix);

        // Explicit override
        let conn = Connection {
            transport: Some(TransportKind::Tcp),
            url: "http://localhost".to_string(),
            command: None,
            auth: None,
            timeout: 30,
        };
        assert_eq!(determine_transport(&conn), TransportKind::Tcp);
    }
}
