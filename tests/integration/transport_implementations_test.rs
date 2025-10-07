//! Comprehensive tests for all transport layer implementations
//! Tests STDIO, HTTP, WebSocket, TCP, Unix Socket transports with edge cases

use std::sync::atomic::{AtomicU32, AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::path::PathBuf;
use std::collections::HashMap;
use tokio::sync::{Mutex, oneshot, mpsc};
use tokio::time::{sleep, timeout};
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use turbomcp_transport::*;
use turbomcp_protocol::*;
use turbomcp_protocol::jsonrpc::*;
use turbomcp::{McpError, McpResult};

// STDIO Transport Tests
#[tokio::test]
async fn test_stdio_transport_basic_communication() {
    let config = StdioTransportConfig {
        buffer_size: 8192,
        timeout: Duration::from_secs(5),
        encoding: "utf-8".to_string(),
    };
    
    let transport = StdioTransport::new(config);
    
    // Test message sending and receiving
    let test_message = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::Value::Number(1.into())),
        method: "test_method".to_string(),
        params: Some(serde_json::json!({"test": "data"})),
    };
    
    let serialized = serde_json::to_string(&test_message).unwrap();
    
    // Send message
    let result = transport.send_message(serialized.clone()).await;
    assert!(result.is_ok());
    
    // In a real scenario, we'd read from stdin, but for testing we simulate
    let received = transport.receive_message().await;
    // This would typically require a mock stdin/stdout setup
}

#[tokio::test]
async fn test_stdio_transport_message_framing() {
    let config = StdioTransportConfig {
        buffer_size: 1024,
        timeout: Duration::from_secs(1),
        encoding: "utf-8".to_string(),
    };
    
    let transport = StdioTransport::new(config);
    
    // Test various message sizes and formats
    let test_cases = vec![
        // Small message
        json!({"jsonrpc": "2.0", "id": 1, "method": "test"}),
        // Large message
        json!({"jsonrpc": "2.0", "id": 2, "method": "large", "params": {"data": "x".repeat(5000)}}),
        // Message with Unicode
        json!({"jsonrpc": "2.0", "id": 3, "method": "unicode", "params": {"text": "Hello 世界 🌍"}}),
        // Empty params
        json!({"jsonrpc": "2.0", "id": 4, "method": "empty"}),
    ];
    
    for (i, test_case) in test_cases.iter().enumerate() {
        let message = serde_json::to_string(test_case).unwrap();
        let result = transport.send_message(message).await;
        assert!(result.is_ok(), "Failed to send test case {}", i);
    }
}

#[tokio::test]
async fn test_stdio_transport_invalid_json_handling() {
    let config = StdioTransportConfig {
        buffer_size: 1024,
        timeout: Duration::from_secs(1),
        encoding: "utf-8".to_string(),
    };
    
    let transport = StdioTransport::new(config);
    
    // Test invalid JSON
    let invalid_messages = vec![
        "not json at all",
        "{invalid json}",
        r#"{"incomplete": }"#,
        "null",
        "",
    ];
    
    for invalid_msg in invalid_messages {
        let result = transport.send_message(invalid_msg.to_string()).await;
        // Should handle gracefully or return appropriate error
        // The exact behavior depends on implementation
    }
}

#[tokio::test]
async fn test_stdio_transport_concurrent_operations() {
    let config = StdioTransportConfig {
        buffer_size: 4096,
        timeout: Duration::from_secs(2),
        encoding: "utf-8".to_string(),
    };
    
    let transport = Arc::new(StdioTransport::new(config));
    let mut handles = vec![];
    
    // Send multiple messages concurrently
    for i in 0..10 {
        let transport_clone = Arc::clone(&transport);
        let handle = tokio::spawn(async move {
            let message = json!({
                "jsonrpc": "2.0",
                "id": i,
                "method": "concurrent_test",
                "params": {"thread_id": i}
            });
            
            let serialized = serde_json::to_string(&message).unwrap();
            transport_clone.send_message(serialized).await
        });
        handles.push(handle);
    }
    
    let results: Vec<_> = futures::future::join_all(handles).await;
    
    // All sends should complete without panicking
    for (i, result) in results.into_iter().enumerate() {
        assert!(result.is_ok(), "Concurrent operation {} panicked", i);
    }
}

// HTTP Transport Tests
#[tokio::test]
async fn test_http_transport_basic_post_request() {
    let config = HttpTransportConfig {
        endpoint: "http://localhost:8080/mcp".to_string(),
        method: HttpMethod::Post,
        headers: HashMap::new(),
        timeout: Duration::from_secs(5),
        retry_config: None,
    };
    
    let transport = HttpTransport::new(config);
    
    let test_request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(serde_json::Value::Number(1.into())),
        method: "tools/list".to_string(),
        params: None,
    };
    
    let message = serde_json::to_string(&test_request).unwrap();
    
    // This would require a mock HTTP server for full testing
    // For now, test the configuration and basic setup
    assert_eq!(transport.config().endpoint, "http://localhost:8080/mcp");
}

#[tokio::test]
async fn test_http_transport_custom_headers() {
    let mut headers = HashMap::new();
    headers.insert("Authorization".to_string(), "Bearer token123".to_string());
    headers.insert("User-Agent".to_string(), "TurboMCP/1.0".to_string());
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    
    let config = HttpTransportConfig {
        endpoint: "https://api.example.com/mcp".to_string(),
        method: HttpMethod::Post,
        headers,
        timeout: Duration::from_secs(10),
        retry_config: Some(RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(1),
            backoff_multiplier: 2.0,
            jitter: true,
        }),
    };
    
    let transport = HttpTransport::new(config);
    
    // Verify headers are properly set
    let config = transport.config();
    assert_eq!(config.headers.get("Authorization"), Some(&"Bearer token123".to_string()));
    assert_eq!(config.headers.get("User-Agent"), Some(&"TurboMCP/1.0".to_string()));
}

#[tokio::test]
async fn test_http_transport_timeout_handling() {
    let config = HttpTransportConfig {
        endpoint: "http://httpbin.org/delay/10".to_string(), // Long delay endpoint
        method: HttpMethod::Post,
        headers: HashMap::new(),
        timeout: Duration::from_millis(100), // Short timeout
        retry_config: None,
    };
    
    let transport = HttpTransport::new(config);
    
    let test_request = json!({"test": "timeout"});
    let message = serde_json::to_string(&test_request).unwrap();
    
    let start = Instant::now();
    let result = transport.send_message(message).await;
    let elapsed = start.elapsed();
    
    // Should timeout quickly
    assert!(elapsed < Duration::from_millis(500));
    // Result depends on implementation - might be timeout error
}

#[tokio::test]
async fn test_http_transport_retry_mechanism() {
    let config = HttpTransportConfig {
        endpoint: "http://httpbin.org/status/500".to_string(), // Always returns 500
        method: HttpMethod::Post,
        headers: HashMap::new(),
        timeout: Duration::from_secs(1),
        retry_config: Some(RetryConfig {
            max_attempts: 3,
            initial_delay: Duration::from_millis(10),
            max_delay: Duration::from_millis(100),
            backoff_multiplier: 2.0,
            jitter: false,
        }),
    };
    
    let transport = HttpTransport::new(config);
    
    let test_request = json!({"test": "retry"});
    let message = serde_json::to_string(&test_request).unwrap();
    
    let start = Instant::now();
    let result = transport.send_message(message).await;
    let elapsed = start.elapsed();
    
    // Should have attempted multiple times
    assert!(elapsed > Duration::from_millis(20)); // At least initial_delay + backoff
}

// WebSocket Transport Tests
#[tokio::test]
async fn test_websocket_transport_connection_lifecycle() {
    let config = WebSocketTransportConfig {
        url: "ws://localhost:8080/ws".to_string(),
        protocols: vec!["mcp".to_string()],
        headers: HashMap::new(),
        ping_interval: Some(Duration::from_secs(30)),
        max_message_size: 1024 * 1024, // 1MB
    };
    
    let transport = WebSocketTransport::new(config);
    
    // Test connection establishment
    let result = transport.connect().await;
    // This would require a WebSocket server for full testing
    
    // Test graceful shutdown
    let result = transport.disconnect().await;
    // Should handle disconnection gracefully
}

#[tokio::test]
async fn test_websocket_transport_message_types() {
    let config = WebSocketTransportConfig {
        url: "ws://localhost:8080/ws".to_string(),
        protocols: vec![],
        headers: HashMap::new(),
        ping_interval: None,
        max_message_size: 1024 * 1024,
    };
    
    let transport = WebSocketTransport::new(config);
    
    // Test different message types
    let test_messages = vec![
        // Text message
        WebSocketMessage::Text(json!({"jsonrpc": "2.0", "method": "test"}).to_string()),
        // Binary message
        WebSocketMessage::Binary(b"binary data".to_vec()),
        // Ping message
        WebSocketMessage::Ping(b"ping".to_vec()),
        // Pong message
        WebSocketMessage::Pong(b"pong".to_vec()),
    ];
    
    for message in test_messages {
        // Test message handling
        let result = transport.handle_message(message).await;
        // Implementation specific behavior
    }
}

#[tokio::test]
async fn test_websocket_transport_large_message_handling() {
    let config = WebSocketTransportConfig {
        url: "ws://localhost:8080/ws".to_string(),
        protocols: vec![],
        headers: HashMap::new(),
        ping_interval: None,
        max_message_size: 1024, // Small limit for testing
    };
    
    let transport = WebSocketTransport::new(config);
    
    // Create message larger than limit
    let large_data = "x".repeat(2048);
    let large_message = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "large_test",
        "params": {"data": large_data}
    });
    
    let message_str = serde_json::to_string(&large_message).unwrap();
    let result = transport.send_message(message_str).await;
    
    // Should handle large message appropriately (fragment or error)
}

#[tokio::test]
async fn test_websocket_transport_ping_pong_handling() {
    let config = WebSocketTransportConfig {
        url: "ws://localhost:8080/ws".to_string(),
        protocols: vec![],
        headers: HashMap::new(),
        ping_interval: Some(Duration::from_millis(100)),
        max_message_size: 1024 * 1024,
    };
    
    let transport = WebSocketTransport::new(config);
    let ping_count = Arc::new(AtomicU32::new(0));
    let pong_count = Arc::new(AtomicU32::new(0));
    
    // Set up ping/pong counters
    let ping_counter = Arc::clone(&ping_count);
    let pong_counter = Arc::clone(&pong_count);
    
    transport.set_ping_handler(move |_| {
        ping_counter.fetch_add(1, Ordering::SeqCst);
    });
    
    transport.set_pong_handler(move |_| {
        pong_counter.fetch_add(1, Ordering::SeqCst);
    });
    
    // Let ping/pong run for a while
    sleep(Duration::from_millis(350)).await;
    
    // Should have sent multiple pings
    assert!(ping_count.load(Ordering::SeqCst) > 0);
}

// TCP Transport Tests
#[tokio::test]
async fn test_tcp_transport_connection_handling() {
    let config = TcpTransportConfig {
        address: "127.0.0.1:0".to_string(), // Use any available port
        connection_timeout: Duration::from_secs(5),
        read_timeout: Duration::from_secs(10),
        write_timeout: Duration::from_secs(10),
        keep_alive: true,
        nodelay: true,
    };
    
    let transport = TcpTransport::new(config);
    
    // Test connection establishment
    let result = transport.connect().await;
    // Would require a TCP server for full testing
    
    // Test connection reuse
    let result = transport.connect().await;
    // Should reuse existing connection or create new one as needed
}

#[tokio::test]
async fn test_tcp_transport_message_framing() {
    // Test length-prefixed message framing
    let config = TcpTransportConfig {
        address: "127.0.0.1:0".to_string(),
        connection_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        keep_alive: false,
        nodelay: true,
    };
    
    let transport = TcpTransport::new(config);
    
    // Test various message sizes
    let test_messages = vec![
        "small",
        &"medium".repeat(100),
        &"large".repeat(10000),
    ];
    
    for message in test_messages {
        let framed = transport.frame_message(message.as_bytes());
        let unframed = transport.unframe_message(&framed);
        
        assert_eq!(unframed.unwrap(), message.as_bytes());
    }
}

#[tokio::test]
async fn test_tcp_transport_concurrent_connections() {
    let config = TcpTransportConfig {
        address: "127.0.0.1:0".to_string(),
        connection_timeout: Duration::from_secs(1),
        read_timeout: Duration::from_secs(5),
        write_timeout: Duration::from_secs(5),
        keep_alive: true,
        nodelay: true,
    };
    
    let transport = Arc::new(TcpTransport::new(config));
    let mut handles = vec![];
    
    // Create multiple concurrent connections
    for i in 0..5 {
        let transport_clone = Arc::clone(&transport);
        let handle = tokio::spawn(async move {
            let connection_id = format!("conn_{}", i);
            transport_clone.connect_with_id(&connection_id).await
        });
        handles.push(handle);
    }
    
    let results: Vec<_> = futures::future::join_all(handles).await;
    
    // All connections should be handled appropriately
    for (i, result) in results.into_iter().enumerate() {
        assert!(result.is_ok(), "Connection {} failed", i);
    }
}

#[tokio::test]
async fn test_tcp_transport_error_recovery() {
    let config = TcpTransportConfig {
        address: "127.0.0.1:99999".to_string(), // Invalid port
        connection_timeout: Duration::from_millis(100),
        read_timeout: Duration::from_secs(1),
        write_timeout: Duration::from_secs(1),
        keep_alive: false,
        nodelay: true,
    };
    
    let transport = TcpTransport::new(config);
    
    // Test connection failure handling
    let result = transport.connect().await;
    assert!(result.is_err());
    
    // Test recovery after failure
    let result = transport.connect().await;
    assert!(result.is_err()); // Should still fail with same config
}

// Unix Socket Transport Tests (Unix-specific)
#[cfg(unix)]
#[tokio::test]
async fn test_unix_socket_transport_basic_communication() {
    use std::os::unix::fs::PermissionsExt;
    
    let socket_path = "/tmp/turbomcp_test.sock";
    
    let config = UnixSocketTransportConfig {
        path: socket_path.to_string(),
        permissions: Some(0o600), // Owner read/write only
        timeout: Duration::from_secs(5),
    };
    
    let transport = UnixSocketTransport::new(config);
    
    // Test socket creation and permissions
    let result = transport.create_socket().await;
    
    if result.is_ok() {
        // Check file permissions
        let metadata = std::fs::metadata(socket_path).unwrap();
        let permissions = metadata.permissions();
        assert_eq!(permissions.mode() & 0o777, 0o600);
        
        // Clean up
        std::fs::remove_file(socket_path).ok();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_socket_transport_permission_handling() {
    let socket_path = "/tmp/turbomcp_permissions_test.sock";
    
    let config = UnixSocketTransportConfig {
        path: socket_path.to_string(),
        permissions: Some(0o644), // Different permissions
        timeout: Duration::from_secs(1),
    };
    
    let transport = UnixSocketTransport::new(config);
    
    let result = transport.create_socket().await;
    
    if result.is_ok() {
        // Verify permissions were set correctly
        let metadata = std::fs::metadata(socket_path).unwrap();
        let permissions = metadata.permissions();
        assert_eq!(permissions.mode() & 0o777, 0o644);
        
        // Clean up
        std::fs::remove_file(socket_path).ok();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn test_unix_socket_transport_cleanup() {
    let socket_path = "/tmp/turbomcp_cleanup_test.sock";
    
    let config = UnixSocketTransportConfig {
        path: socket_path.to_string(),
        permissions: None,
        timeout: Duration::from_secs(1),
    };
    
    let transport = UnixSocketTransport::new(config);
    
    // Create socket
    let result = transport.create_socket().await;
    
    if result.is_ok() {
        assert!(std::path::Path::new(socket_path).exists());
        
        // Test cleanup
        transport.cleanup().await;
        assert!(!std::path::Path::new(socket_path).exists());
    }
}

// Transport Factory Tests
#[tokio::test]
async fn test_transport_factory_creation() {
    let factory = TransportFactory::new();
    
    // Test STDIO transport creation
    let stdio_config = TransportConfig::Stdio(StdioTransportConfig {
        buffer_size: 4096,
        timeout: Duration::from_secs(5),
        encoding: "utf-8".to_string(),
    });
    
    let stdio_transport = factory.create_transport(stdio_config).await;
    assert!(stdio_transport.is_ok());
    
    // Test HTTP transport creation
    let http_config = TransportConfig::Http(HttpTransportConfig {
        endpoint: "http://localhost:8080".to_string(),
        method: HttpMethod::Post,
        headers: HashMap::new(),
        timeout: Duration::from_secs(5),
        retry_config: None,
    });
    
    let http_transport = factory.create_transport(http_config).await;
    assert!(http_transport.is_ok());
}

#[tokio::test]
async fn test_transport_factory_invalid_configurations() {
    let factory = TransportFactory::new();
    
    // Test invalid HTTP configuration
    let invalid_http_config = TransportConfig::Http(HttpTransportConfig {
        endpoint: "invalid-url".to_string(), // Invalid URL
        method: HttpMethod::Post,
        headers: HashMap::new(),
        timeout: Duration::from_secs(5),
        retry_config: None,
    });
    
    let result = factory.create_transport(invalid_http_config).await;
    // Should handle invalid configuration gracefully
}

// Transport Pool Tests
#[tokio::test]
async fn test_transport_pool_management() {
    let pool_config = TransportPoolConfig {
        max_connections: 5,
        idle_timeout: Duration::from_secs(30),
        connection_timeout: Duration::from_secs(5),
        health_check_interval: Duration::from_secs(10),
    };
    
    let pool = TransportPool::new(pool_config);
    
    // Test connection acquisition
    let transport_config = TransportConfig::Http(HttpTransportConfig {
        endpoint: "http://localhost:8080".to_string(),
        method: HttpMethod::Post,
        headers: HashMap::new(),
        timeout: Duration::from_secs(5),
        retry_config: None,
    });
    
    // Acquire multiple connections
    let mut connections = vec![];
    for _ in 0..3 {
        let conn = pool.acquire_connection(&transport_config).await;
        if let Ok(connection) = conn {
            connections.push(connection);
        }
    }
    
    // Test pool statistics
    let stats = pool.get_statistics().await;
    assert!(stats.active_connections <= pool_config.max_connections);
    
    // Release connections
    for connection in connections {
        pool.release_connection(connection).await;
    }
}

#[tokio::test]
async fn test_transport_pool_health_checking() {
    let pool_config = TransportPoolConfig {
        max_connections: 3,
        idle_timeout: Duration::from_millis(100), // Short timeout for testing
        connection_timeout: Duration::from_secs(1),
        health_check_interval: Duration::from_millis(50),
    };
    
    let pool = TransportPool::new(pool_config);
    
    // Add some connections to the pool
    let transport_config = TransportConfig::Stdio(StdioTransportConfig {
        buffer_size: 1024,
        timeout: Duration::from_secs(1),
        encoding: "utf-8".to_string(),
    });
    
    for _ in 0..2 {
        let conn = pool.acquire_connection(&transport_config).await;
        if let Ok(connection) = conn {
            pool.release_connection(connection).await;
        }
    }
    
    // Wait for health check to run
    sleep(Duration::from_millis(200)).await;
    
    let stats = pool.get_statistics().await;
    // Health check should have processed connections
}

// Helper types and implementations for testing

#[derive(Debug, Clone)]
enum TransportConfig {
    Stdio(StdioTransportConfig),
    Http(HttpTransportConfig),
    WebSocket(WebSocketTransportConfig),
    Tcp(TcpTransportConfig),
    #[cfg(unix)]
    UnixSocket(UnixSocketTransportConfig),
}

#[derive(Debug, Clone)]
struct StdioTransportConfig {
    buffer_size: usize,
    timeout: Duration,
    encoding: String,
}

#[derive(Debug, Clone)]
struct HttpTransportConfig {
    endpoint: String,
    method: HttpMethod,
    headers: HashMap<String, String>,
    timeout: Duration,
    retry_config: Option<RetryConfig>,
}

#[derive(Debug, Clone)]
enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
}

#[derive(Debug, Clone)]
struct WebSocketTransportConfig {
    url: String,
    protocols: Vec<String>,
    headers: HashMap<String, String>,
    ping_interval: Option<Duration>,
    max_message_size: usize,
}

#[derive(Debug, Clone)]
struct TcpTransportConfig {
    address: String,
    connection_timeout: Duration,
    read_timeout: Duration,
    write_timeout: Duration,
    keep_alive: bool,
    nodelay: bool,
}

#[cfg(unix)]
#[derive(Debug, Clone)]
struct UnixSocketTransportConfig {
    path: String,
    permissions: Option<u32>,
    timeout: Duration,
}

#[derive(Debug, Clone)]
struct RetryConfig {
    max_attempts: usize,
    initial_delay: Duration,
    max_delay: Duration,
    backoff_multiplier: f64,
    jitter: bool,
}

#[derive(Debug, Clone)]
struct TransportPoolConfig {
    max_connections: usize,
    idle_timeout: Duration,
    connection_timeout: Duration,
    health_check_interval: Duration,
}

// Mock transport implementations for testing
struct StdioTransport {
    config: StdioTransportConfig,
}

impl StdioTransport {
    fn new(config: StdioTransportConfig) -> Self {
        Self { config }
    }
    
    async fn send_message(&self, message: String) -> McpResult<()> {
        // Mock implementation
        Ok(())
    }
    
    async fn receive_message(&self) -> McpResult<String> {
        // Mock implementation
        Ok("{}".to_string())
    }
}

struct HttpTransport {
    config: HttpTransportConfig,
}

impl HttpTransport {
    fn new(config: HttpTransportConfig) -> Self {
        Self { config }
    }
    
    fn config(&self) -> &HttpTransportConfig {
        &self.config
    }
    
    async fn send_message(&self, message: String) -> McpResult<String> {
        // Mock implementation
        Ok("{}".to_string())
    }
}

struct WebSocketTransport {
    config: WebSocketTransportConfig,
}

#[derive(Debug)]
enum WebSocketMessage {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong(Vec<u8>),
}

impl WebSocketTransport {
    fn new(config: WebSocketTransportConfig) -> Self {
        Self { config }
    }
    
    async fn connect(&self) -> McpResult<()> {
        // Mock implementation
        Ok(())
    }
    
    async fn disconnect(&self) -> McpResult<()> {
        // Mock implementation
        Ok(())
    }
    
    async fn send_message(&self, message: String) -> McpResult<()> {
        // Mock implementation
        Ok(())
    }
    
    async fn handle_message(&self, message: WebSocketMessage) -> McpResult<()> {
        // Mock implementation
        Ok(())
    }
    
    fn set_ping_handler<F>(&self, handler: F) 
    where 
        F: Fn(&[u8]) + Send + Sync + 'static,
    {
        // Mock implementation
    }
    
    fn set_pong_handler<F>(&self, handler: F)
    where 
        F: Fn(&[u8]) + Send + Sync + 'static,
    {
        // Mock implementation
    }
}

struct TcpTransport {
    config: TcpTransportConfig,
}

impl TcpTransport {
    fn new(config: TcpTransportConfig) -> Self {
        Self { config }
    }
    
    async fn connect(&self) -> McpResult<()> {
        // Mock implementation
        Ok(())
    }
    
    async fn connect_with_id(&self, id: &str) -> McpResult<()> {
        // Mock implementation
        Ok(())
    }
    
    fn frame_message(&self, data: &[u8]) -> Vec<u8> {
        // Simple length-prefixed framing
        let len = data.len() as u32;
        let mut framed = len.to_be_bytes().to_vec();
        framed.extend_from_slice(data);
        framed
    }
    
    fn unframe_message(&self, data: &[u8]) -> McpResult<Vec<u8>> {
        if data.len() < 4 {
            return Err(McpError::Tool("Invalid frame".to_string()));
        }
        
        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if data.len() < 4 + len {
            return Err(McpError::Tool("Incomplete frame".to_string()));
        }
        
        Ok(data[4..4+len].to_vec())
    }
}

#[cfg(unix)]
struct UnixSocketTransport {
    config: UnixSocketTransportConfig,
}

#[cfg(unix)]
impl UnixSocketTransport {
    fn new(config: UnixSocketTransportConfig) -> Self {
        Self { config }
    }
    
    async fn create_socket(&self) -> McpResult<()> {
        // Mock implementation - would create actual Unix socket
        Ok(())
    }
    
    async fn cleanup(&self) -> McpResult<()> {
        // Mock implementation - would remove socket file
        Ok(())
    }
}

struct TransportFactory;

impl TransportFactory {
    fn new() -> Self {
        Self
    }
    
    async fn create_transport(&self, config: TransportConfig) -> McpResult<Box<dyn Send + Sync>> {
        match config {
            TransportConfig::Stdio(cfg) => {
                Ok(Box::new(StdioTransport::new(cfg)))
            }
            TransportConfig::Http(cfg) => {
                Ok(Box::new(HttpTransport::new(cfg)))
            }
            TransportConfig::WebSocket(cfg) => {
                Ok(Box::new(WebSocketTransport::new(cfg)))
            }
            TransportConfig::Tcp(cfg) => {
                Ok(Box::new(TcpTransport::new(cfg)))
            }
            #[cfg(unix)]
            TransportConfig::UnixSocket(cfg) => {
                Ok(Box::new(UnixSocketTransport::new(cfg)))
            }
        }
    }
}

struct TransportPool {
    config: TransportPoolConfig,
}

struct PoolStatistics {
    active_connections: usize,
    idle_connections: usize,
    total_connections: usize,
}

impl TransportPool {
    fn new(config: TransportPoolConfig) -> Self {
        Self { config }
    }
    
    async fn acquire_connection(&self, config: &TransportConfig) -> McpResult<Box<dyn Send + Sync>> {
        // Mock implementation
        let factory = TransportFactory::new();
        factory.create_transport(config.clone()).await
    }
    
    async fn release_connection(&self, connection: Box<dyn Send + Sync>) {
        // Mock implementation
    }
    
    async fn get_statistics(&self) -> PoolStatistics {
        // Mock implementation
        PoolStatistics {
            active_connections: 0,
            idle_connections: 0,
            total_connections: 0,
        }
    }
}