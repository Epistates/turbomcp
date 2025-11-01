//! Transport factory and auto-detection

use crate::cli::{Connection, TransportKind};
use crate::error::{CliError, CliResult};
use std::collections::HashMap;
use std::time::Duration;
use turbomcp_client::Client;
use turbomcp_protocol::types::Tool;

#[cfg(feature = "stdio")]
use turbomcp_transport::child_process::{ChildProcessConfig, ChildProcessTransport};

#[cfg(feature = "tcp")]
use turbomcp_transport::tcp::TcpTransportBuilder;

#[cfg(feature = "unix")]
use turbomcp_transport::unix::UnixTransportBuilder;

#[cfg(feature = "http")]
use turbomcp_transport::streamable_http_client::{
    StreamableHttpClientConfig, StreamableHttpClientTransport,
};

#[cfg(feature = "websocket")]
use turbomcp_transport::{WebSocketBidirectionalConfig, WebSocketBidirectionalTransport};

/// Wrapper for unified client operations, hiding transport implementation details
pub struct UnifiedClient {
    inner: ClientInner,
}

enum ClientInner {
    #[cfg(feature = "stdio")]
    Stdio(Client<ChildProcessTransport>),
    #[cfg(feature = "tcp")]
    Tcp(Client<turbomcp_transport::tcp::TcpTransport>),
    #[cfg(feature = "unix")]
    Unix(Client<turbomcp_transport::unix::UnixTransport>),
    #[cfg(feature = "http")]
    Http(Client<StreamableHttpClientTransport>),
    #[cfg(feature = "websocket")]
    WebSocket(Client<WebSocketBidirectionalTransport>),
}

impl UnifiedClient {
    pub async fn initialize(&self) -> CliResult<turbomcp_client::InitializeResult> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client.initialize().await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client.initialize().await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client.initialize().await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client.initialize().await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client.initialize().await?),
        }
    }

    pub async fn list_tools(&self) -> CliResult<Vec<Tool>> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client.list_tools().await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client.list_tools().await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client.list_tools().await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client.list_tools().await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client.list_tools().await?),
        }
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> CliResult<serde_json::Value> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client.call_tool(name, arguments).await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client.call_tool(name, arguments).await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client.call_tool(name, arguments).await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client.call_tool(name, arguments).await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client.call_tool(name, arguments).await?),
        }
    }

    pub async fn list_resources(&self) -> CliResult<Vec<turbomcp_protocol::types::Resource>> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client.list_resources().await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client.list_resources().await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client.list_resources().await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client.list_resources().await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client.list_resources().await?),
        }
    }

    pub async fn read_resource(
        &self,
        uri: &str,
    ) -> CliResult<turbomcp_protocol::types::ReadResourceResult> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client.read_resource(uri).await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client.read_resource(uri).await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client.read_resource(uri).await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client.read_resource(uri).await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client.read_resource(uri).await?),
        }
    }

    pub async fn list_resource_templates(&self) -> CliResult<Vec<String>> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client.list_resource_templates().await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client.list_resource_templates().await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client.list_resource_templates().await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client.list_resource_templates().await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client.list_resource_templates().await?),
        }
    }

    pub async fn subscribe(&self, uri: &str) -> CliResult<turbomcp_protocol::types::EmptyResult> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client.subscribe(uri).await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client.subscribe(uri).await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client.subscribe(uri).await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client.subscribe(uri).await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client.subscribe(uri).await?),
        }
    }

    pub async fn unsubscribe(&self, uri: &str) -> CliResult<turbomcp_protocol::types::EmptyResult> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client.unsubscribe(uri).await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client.unsubscribe(uri).await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client.unsubscribe(uri).await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client.unsubscribe(uri).await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client.unsubscribe(uri).await?),
        }
    }

    pub async fn list_prompts(&self) -> CliResult<Vec<turbomcp_protocol::types::Prompt>> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client.list_prompts().await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client.list_prompts().await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client.list_prompts().await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client.list_prompts().await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client.list_prompts().await?),
        }
    }

    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> CliResult<turbomcp_protocol::types::GetPromptResult> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client.get_prompt(name, arguments).await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client.get_prompt(name, arguments).await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client.get_prompt(name, arguments).await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client.get_prompt(name, arguments).await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client.get_prompt(name, arguments).await?),
        }
    }

    pub async fn complete_prompt(
        &self,
        prompt_name: &str,
        argument_name: &str,
        argument_value: &str,
        context: Option<turbomcp_protocol::types::CompletionContext>,
    ) -> CliResult<turbomcp_protocol::types::CompletionResponse> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client
                .complete_prompt(prompt_name, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client
                .complete_prompt(prompt_name, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client
                .complete_prompt(prompt_name, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client
                .complete_prompt(prompt_name, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client
                .complete_prompt(prompt_name, argument_name, argument_value, context)
                .await?),
        }
    }

    pub async fn complete_resource(
        &self,
        resource_uri: &str,
        argument_name: &str,
        argument_value: &str,
        context: Option<turbomcp_protocol::types::CompletionContext>,
    ) -> CliResult<turbomcp_protocol::types::CompletionResponse> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => Ok(client
                .complete_resource(resource_uri, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => Ok(client
                .complete_resource(resource_uri, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => Ok(client
                .complete_resource(resource_uri, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "http")]
            ClientInner::Http(client) => Ok(client
                .complete_resource(resource_uri, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => Ok(client
                .complete_resource(resource_uri, argument_name, argument_value, context)
                .await?),
        }
    }

    pub async fn ping(&self) -> CliResult<()> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => {
                client.ping().await?;
                Ok(())
            }
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => {
                client.ping().await?;
                Ok(())
            }
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => {
                client.ping().await?;
                Ok(())
            }
            #[cfg(feature = "http")]
            ClientInner::Http(client) => {
                client.ping().await?;
                Ok(())
            }
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => {
                client.ping().await?;
                Ok(())
            }
        }
    }

    pub async fn set_log_level(&self, level: turbomcp_protocol::types::LogLevel) -> CliResult<()> {
        match &self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(client) => {
                client.set_log_level(level).await?;
                Ok(())
            }
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(client) => {
                client.set_log_level(level).await?;
                Ok(())
            }
            #[cfg(feature = "unix")]
            ClientInner::Unix(client) => {
                client.set_log_level(level).await?;
                Ok(())
            }
            #[cfg(feature = "http")]
            ClientInner::Http(client) => {
                client.set_log_level(level).await?;
                Ok(())
            }
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(client) => {
                client.set_log_level(level).await?;
                Ok(())
            }
        }
    }
}

/// Create a unified client that hides transport type complexity from the executor
pub async fn create_client(conn: &Connection) -> CliResult<UnifiedClient> {
    let transport_kind = determine_transport(conn);

    match transport_kind {
        #[cfg(feature = "stdio")]
        TransportKind::Stdio => {
            let transport = create_stdio_transport(conn)?;
            Ok(UnifiedClient {
                inner: ClientInner::Stdio(Client::new(transport)),
            })
        }
        #[cfg(not(feature = "stdio"))]
        TransportKind::Stdio => {
            Err(CliError::NotSupported(
                "STDIO transport is not enabled (missing 'stdio' feature)".to_string(),
            ))
        }
        #[cfg(feature = "http")]
        TransportKind::Http => {
            let transport = create_http_transport(conn).await?;
            Ok(UnifiedClient {
                inner: ClientInner::Http(Client::new(transport)),
            })
        }
        #[cfg(not(feature = "http"))]
        TransportKind::Http => {
            Err(CliError::NotSupported(
                "HTTP transport is not enabled. Rebuild with --features http or --features all"
                    .to_string(),
            ))
        }
        #[cfg(feature = "websocket")]
        TransportKind::Ws => {
            let transport = create_websocket_transport(conn).await?;
            Ok(UnifiedClient {
                inner: ClientInner::WebSocket(Client::new(transport)),
            })
        }
        #[cfg(not(feature = "websocket"))]
        TransportKind::Ws => {
            Err(CliError::NotSupported(
                "WebSocket transport is not enabled. Rebuild with --features websocket or --features all"
                    .to_string(),
            ))
        }
        #[cfg(feature = "tcp")]
        TransportKind::Tcp => {
            let transport = create_tcp_transport(conn).await?;
            Ok(UnifiedClient {
                inner: ClientInner::Tcp(Client::new(transport)),
            })
        }
        #[cfg(not(feature = "tcp"))]
        TransportKind::Tcp => {
            Err(CliError::NotSupported(
                "TCP transport is not enabled (missing 'tcp' feature)".to_string(),
            ))
        }
        #[cfg(feature = "unix")]
        TransportKind::Unix => {
            let transport = create_unix_transport(conn).await?;
            Ok(UnifiedClient {
                inner: ClientInner::Unix(Client::new(transport)),
            })
        }
        #[cfg(not(feature = "unix"))]
        TransportKind::Unix => {
            Err(CliError::NotSupported(
                "Unix socket transport is not enabled (missing 'unix' feature)".to_string(),
            ))
        }
    }
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
#[cfg(feature = "stdio")]
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
#[cfg(feature = "tcp")]
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
#[cfg(feature = "unix")]
async fn create_unix_transport(
    conn: &Connection,
) -> CliResult<turbomcp_transport::unix::UnixTransport> {
    let path = conn.url.strip_prefix("unix://").unwrap_or(&conn.url);

    let transport = UnixTransportBuilder::new_client().socket_path(path).build();

    Ok(transport)
}

/// Create HTTP transport from connection
#[cfg(feature = "http")]
async fn create_http_transport(conn: &Connection) -> CliResult<StreamableHttpClientTransport> {
    let url = &conn.url;

    // Parse HTTP URL (remove http:// or https://)
    let base_url = if let Some(stripped) = url.strip_prefix("https://") {
        format!("https://{}", stripped)
    } else if let Some(stripped) = url.strip_prefix("http://") {
        format!("http://{}", stripped)
    } else {
        url.clone()
    };

    let config = StreamableHttpClientConfig {
        base_url,
        endpoint_path: "/mcp".to_string(),
        timeout: Duration::from_secs(conn.timeout),
        ..Default::default()
    };

    Ok(StreamableHttpClientTransport::new(config))
}

/// Create WebSocket transport from connection
#[cfg(feature = "websocket")]
async fn create_websocket_transport(
    conn: &Connection,
) -> CliResult<WebSocketBidirectionalTransport> {
    let url = &conn.url;

    // Validate URL is a proper WebSocket URL
    if !url.starts_with("ws://") && !url.starts_with("wss://") {
        return Err(CliError::InvalidArguments(format!(
            "Invalid WebSocket URL: {} (must start with ws:// or wss://)",
            url
        )));
    }

    let config = WebSocketBidirectionalConfig::client(url.clone());

    WebSocketBidirectionalTransport::new(config)
        .await
        .map_err(|e| CliError::ConnectionFailed(e.to_string()))
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
