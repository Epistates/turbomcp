//! Transport factory and auto-detection

// A build with no transport features compiles every client method down to an
// empty match; the parameters and imports those methods use go unused.
#![cfg_attr(
    not(any(
        feature = "stdio",
        feature = "tcp",
        all(feature = "unix", unix),
        feature = "http",
        feature = "websocket"
    )),
    allow(unused, unreachable_code)
)]

use crate::cli::{Connection, TransportKind};
use crate::error::{CliError, CliResult};
use std::collections::HashMap;
#[cfg(any(feature = "stdio", feature = "http"))]
use std::time::Duration;
use turbomcp_client::Client;
use turbomcp_protocol::types::Tool;

#[cfg(feature = "stdio")]
use turbomcp_transport::child_process::{ChildProcessConfig, ChildProcessTransport};

#[cfg(feature = "tcp")]
use turbomcp_transport::tcp::TcpTransportBuilder;

#[cfg(all(feature = "unix", unix))]
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

/// One variant per compiled-in transport.
///
/// With no transport features this enum is empty, and every `match` on it
/// has no arms. The matches are on the place (`match self.inner`, binding
/// `ref client`) rather than on `&self.inner`, because Rust accepts an
/// arm-less match on an uninhabited place but not on a reference to one. A
/// transport-less build still compiles, and `create_client` reports which
/// feature to enable.
enum ClientInner {
    #[cfg(feature = "stdio")]
    Stdio(Client<ChildProcessTransport>),
    #[cfg(feature = "tcp")]
    Tcp(Client<turbomcp_transport::tcp::TcpTransport>),
    #[cfg(all(feature = "unix", unix))]
    Unix(Client<turbomcp_transport::unix::UnixTransport>),
    #[cfg(feature = "http")]
    Http(Client<StreamableHttpClientTransport>),
    #[cfg(feature = "websocket")]
    WebSocket(Client<WebSocketBidirectionalTransport>),
}

impl UnifiedClient {
    pub async fn initialize(&self) -> CliResult<turbomcp_client::InitializeResult> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => Ok(client.initialize().await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => Ok(client.initialize().await?),
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => Ok(client.initialize().await?),
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => Ok(client.initialize().await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => Ok(client.initialize().await?),
        }
    }

    pub async fn list_tools(&self) -> CliResult<Vec<Tool>> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => Ok(client.list_tools().await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => Ok(client.list_tools().await?),
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => Ok(client.list_tools().await?),
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => Ok(client.list_tools().await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => Ok(client.list_tools().await?),
        }
    }

    pub async fn call_tool(
        &self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> CliResult<serde_json::Value> {
        let result: turbomcp_protocol::types::CallToolResult = match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => client.call_tool(name, arguments, None).await?,
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => client.call_tool(name, arguments, None).await?,
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => client.call_tool(name, arguments, None).await?,
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => client.call_tool(name, arguments, None).await?,
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => client.call_tool(name, arguments, None).await?,
        };

        // Serialize CallToolResult to JSON for CLI display
        Ok(serde_json::to_value(result)?)
    }

    pub async fn list_resources(&self) -> CliResult<Vec<turbomcp_protocol::types::Resource>> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => Ok(client.list_resources().await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => Ok(client.list_resources().await?),
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => Ok(client.list_resources().await?),
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => Ok(client.list_resources().await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => Ok(client.list_resources().await?),
        }
    }

    pub async fn read_resource(
        &self,
        uri: &str,
    ) -> CliResult<turbomcp_protocol::types::ReadResourceResult> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => Ok(client.read_resource(uri).await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => Ok(client.read_resource(uri).await?),
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => Ok(client.read_resource(uri).await?),
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => Ok(client.read_resource(uri).await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => Ok(client.read_resource(uri).await?),
        }
    }

    pub async fn list_resource_templates(
        &self,
    ) -> CliResult<Vec<turbomcp_protocol::types::ResourceTemplate>> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => Ok(client.list_resource_templates().await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => Ok(client.list_resource_templates().await?),
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => Ok(client.list_resource_templates().await?),
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => Ok(client.list_resource_templates().await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => Ok(client.list_resource_templates().await?),
        }
    }

    pub async fn subscribe(&self, uri: &str) -> CliResult<turbomcp_protocol::types::EmptyResult> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => Ok(client.subscribe(uri).await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => Ok(client.subscribe(uri).await?),
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => Ok(client.subscribe(uri).await?),
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => Ok(client.subscribe(uri).await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => Ok(client.subscribe(uri).await?),
        }
    }

    pub async fn unsubscribe(&self, uri: &str) -> CliResult<turbomcp_protocol::types::EmptyResult> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => Ok(client.unsubscribe(uri).await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => Ok(client.unsubscribe(uri).await?),
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => Ok(client.unsubscribe(uri).await?),
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => Ok(client.unsubscribe(uri).await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => Ok(client.unsubscribe(uri).await?),
        }
    }

    pub async fn list_prompts(&self) -> CliResult<Vec<turbomcp_protocol::types::Prompt>> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => Ok(client.list_prompts().await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => Ok(client.list_prompts().await?),
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => Ok(client.list_prompts().await?),
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => Ok(client.list_prompts().await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => Ok(client.list_prompts().await?),
        }
    }

    pub async fn get_prompt(
        &self,
        name: &str,
        arguments: Option<HashMap<String, serde_json::Value>>,
    ) -> CliResult<turbomcp_protocol::types::GetPromptResult> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => Ok(client.get_prompt(name, arguments).await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => Ok(client.get_prompt(name, arguments).await?),
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => Ok(client.get_prompt(name, arguments).await?),
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => Ok(client.get_prompt(name, arguments).await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => Ok(client.get_prompt(name, arguments).await?),
        }
    }

    pub async fn complete_prompt(
        &self,
        prompt_name: &str,
        argument_name: &str,
        argument_value: &str,
        context: Option<turbomcp_protocol::types::CompletionContext>,
    ) -> CliResult<turbomcp_protocol::types::CompletionResponse> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => Ok(client
                .complete_prompt(prompt_name, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => Ok(client
                .complete_prompt(prompt_name, argument_name, argument_value, context)
                .await?),
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => Ok(client
                .complete_prompt(prompt_name, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => Ok(client
                .complete_prompt(prompt_name, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => Ok(client
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
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => Ok(client
                .complete_resource(resource_uri, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => Ok(client
                .complete_resource(resource_uri, argument_name, argument_value, context)
                .await?),
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => Ok(client
                .complete_resource(resource_uri, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => Ok(client
                .complete_resource(resource_uri, argument_name, argument_value, context)
                .await?),
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => Ok(client
                .complete_resource(resource_uri, argument_name, argument_value, context)
                .await?),
        }
    }

    pub async fn ping(&self) -> CliResult<()> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => {
                client.ping().await?;
                Ok(())
            }
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => {
                client.ping().await?;
                Ok(())
            }
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => {
                client.ping().await?;
                Ok(())
            }
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => {
                client.ping().await?;
                Ok(())
            }
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => {
                client.ping().await?;
                Ok(())
            }
        }
    }

    pub async fn set_log_level(&self, level: turbomcp_protocol::types::LogLevel) -> CliResult<()> {
        match self.inner {
            #[cfg(feature = "stdio")]
            ClientInner::Stdio(ref client) => {
                client.set_log_level(level).await?;
                Ok(())
            }
            #[cfg(feature = "tcp")]
            ClientInner::Tcp(ref client) => {
                client.set_log_level(level).await?;
                Ok(())
            }
            #[cfg(all(feature = "unix", unix))]
            ClientInner::Unix(ref client) => {
                client.set_log_level(level).await?;
                Ok(())
            }
            #[cfg(feature = "http")]
            ClientInner::Http(ref client) => {
                client.set_log_level(level).await?;
                Ok(())
            }
            #[cfg(feature = "websocket")]
            ClientInner::WebSocket(ref client) => {
                client.set_log_level(level).await?;
                Ok(())
            }
        }
    }
}

/// Create a unified client that hides transport type complexity from the executor
pub async fn create_client(conn: &Connection) -> CliResult<UnifiedClient> {
    let transport_kind = determine_transport(conn);

    // The --auth / MCP_AUTH bearer token is only consumed by the HTTP transport.
    // Warn (without echoing the token) when the user supplies it for a transport
    // that has no notion of authentication so they don't assume it was sent.
    if conn.auth.is_some() && !matches!(transport_kind, TransportKind::Http | TransportKind::Ws) {
        eprintln!(
            "Warning: --auth is currently only honored by the HTTP transport; ignoring for {:?}.",
            transport_kind
        );
    }

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
        #[cfg(all(feature = "unix", unix))]
        TransportKind::Unix => {
            let transport = create_unix_transport(conn).await?;
            Ok(UnifiedClient {
                inner: ClientInner::Unix(Client::new(transport)),
            })
        }
        #[cfg(not(all(feature = "unix", unix)))]
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

    // Honor shell quoting/escaping so paths with spaces and `bash -c "..."`
    // wrappers parse correctly. `split_whitespace` would fragment them.
    let parts = shell_words::split(command_str)
        .map_err(|e| CliError::InvalidArguments(format!("Invalid --command quoting: {e}")))?;
    if parts.is_empty() {
        return Err(CliError::InvalidArguments(
            "No command specified for STDIO transport".to_string(),
        ));
    }

    let command = parts[0].clone();
    let args: Vec<String> = parts[1..].to_vec();

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
        ..Default::default()
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
#[cfg(all(feature = "unix", unix))]
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
    let (base_url, endpoint_path) = split_http_endpoint(&conn.url)?;

    let config = StreamableHttpClientConfig {
        base_url,
        endpoint_path,
        timeout: Duration::from_secs(conn.timeout),
        auth_token: conn.auth.clone(),
        ..Default::default()
    };

    StreamableHttpClientTransport::new(config).map_err(|e| {
        crate::CliError::Transport(turbomcp_protocol::Error::transport(format!(
            "Failed to build HTTP transport: {e}"
        )))
    })
}

/// Split an MCP endpoint URL into the transport's base URL and endpoint path.
///
/// The transport requests `base_url + endpoint_path`. `--url` is the whole
/// endpoint (`http://localhost:8080/mcp` is the default), and passing it as
/// the base with a fixed `/mcp` path posted every request to `/mcp/mcp`. A URL
/// with no path gets the conventional `/mcp`.
#[cfg(feature = "http")]
fn split_http_endpoint(url: &str) -> CliResult<(String, String)> {
    let parsed = url::Url::parse(url)
        .map_err(|e| CliError::InvalidArguments(format!("Invalid HTTP URL '{url}': {e}")))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(CliError::InvalidArguments(format!(
            "Invalid HTTP URL '{url}': must start with http:// or https://"
        )));
    }

    let mut endpoint_path = match parsed.path() {
        "" | "/" => "/mcp".to_string(),
        path => path.to_string(),
    };
    if let Some(query) = parsed.query() {
        endpoint_path.push('?');
        endpoint_path.push_str(query);
    }
    Ok((parsed.origin().ascii_serialization(), endpoint_path))
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

    /// PX-R7: `--url` names the whole endpoint, so its path is the endpoint
    /// path. Appending a fixed `/mcp` to it posted to `/mcp/mcp`.
    #[cfg(feature = "http")]
    #[test]
    fn http_url_is_the_endpoint_not_its_base() {
        let split = |url| split_http_endpoint(url).unwrap();
        assert_eq!(
            split("http://localhost:8080/mcp"),
            ("http://localhost:8080".to_string(), "/mcp".to_string())
        );
        assert_eq!(
            split("https://api.example.com/v1/mcp?tenant=a"),
            (
                "https://api.example.com".to_string(),
                "/v1/mcp?tenant=a".to_string()
            )
        );
        assert_eq!(
            split("http://localhost:8080"),
            ("http://localhost:8080".to_string(), "/mcp".to_string())
        );
        assert!(split_http_endpoint("ftp://example.com/mcp").is_err());
    }
}
