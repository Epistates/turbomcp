//! STDIO Backend for MCP Servers
//!
//! This backend uses turbomcp-transport's `StdioTransport` and `ChildProcessTransport`
//! to communicate with MCP servers over stdin/stdout.

use std::future::Future;
use std::pin::Pin;

use bytes::Bytes;
use serde_json::Value;
use tracing::{debug, trace};
use turbomcp_protocol::{
    InitializeRequest, InitializeResult, MessageId,
    jsonrpc::{
        JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, JsonRpcResponsePayload,
        JsonRpcVersion,
    },
};
use turbomcp_transport::{
    ChildProcessConfig, ChildProcessTransport, Transport, TransportMessage,
    core::TransportMessageMetadata,
};
use uuid::Uuid;

use crate::error::{ProxyError, ProxyResult};

use super::McpBackend;

/// STDIO backend for connecting to MCP servers running as subprocesses
///
/// This uses turbomcp-transport's `ChildProcessTransport` for maximum
/// correctness and `DRYness`.
pub struct StdioBackend {
    /// The underlying transport
    transport: ChildProcessTransport,
    /// Message ID counter
    next_id: std::sync::atomic::AtomicU64,
}

impl StdioBackend {
    /// Create a new STDIO backend
    ///
    /// # Arguments
    /// * `command` - The command to execute (e.g., "python", "node")
    /// * `args` - Command arguments (e.g., `["server.py"]`)
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the subprocess fails to start or connect.
    pub async fn new(command: impl Into<String>, args: Vec<String>) -> ProxyResult<Self> {
        let config = ChildProcessConfig {
            command: command.into(),
            args,
            working_directory: None,
            environment: None,
            ..Default::default()
        };

        let transport = ChildProcessTransport::new(config);

        // Connect the transport (starts the subprocess)
        transport
            .connect()
            .await
            .map_err(|e| ProxyError::backend(format!("Failed to connect to subprocess: {e}")))?;

        Ok(Self {
            transport,
            next_id: std::sync::atomic::AtomicU64::new(1),
        })
    }

    /// Create with working directory
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the subprocess fails to start, connect, or if the working directory is invalid.
    pub async fn with_working_dir(
        command: impl Into<String>,
        args: Vec<String>,
        working_dir: String,
    ) -> ProxyResult<Self> {
        let config = ChildProcessConfig {
            command: command.into(),
            args,
            working_directory: Some(working_dir),
            environment: None,
            ..Default::default()
        };

        let transport = ChildProcessTransport::new(config);

        // Connect the transport (starts the subprocess)
        transport
            .connect()
            .await
            .map_err(|e| ProxyError::backend(format!("Failed to connect to subprocess: {e}")))?;

        Ok(Self {
            transport,
            next_id: std::sync::atomic::AtomicU64::new(1),
        })
    }

    /// Get next message ID
    fn next_message_id(&self) -> u64 {
        self.next_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    }

    /// Send a JSON-RPC request and wait for response
    async fn send_request(&self, method: &str, params: Value) -> ProxyResult<Value> {
        let id = self.next_message_id();

        let request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            // Cast u64 to i64 for JSON-RPC MessageId - IDs are sequential and won't overflow in practice
            #[allow(clippy::cast_possible_wrap)]
            id: MessageId::Number(id as i64),
            method: method.to_string(),
            params: Some(params),
        };

        trace!(method = %method, id = %id, "Sending introspection request");

        let request_json = serde_json::to_vec(&request)
            .map_err(|e| ProxyError::backend(format!("Failed to serialize request: {e}")))?;
        self.send_raw(request_json).await?;

        // The next message on the wire is not necessarily the answer. A server
        // may log, report progress, or ping in between, and taking whatever
        // arrived first as the response parsed a notification as the result
        // (or failed on it) depending on timing. Read until our id comes back.
        let expected = Value::from(id);
        loop {
            let message = self
                .transport
                .receive()
                .await
                .map_err(|e| ProxyError::backend(format!("Failed to receive response: {e}")))?
                .ok_or_else(|| {
                    ProxyError::backend("No response received (transport closed)".to_string())
                })?;

            let message: Value = serde_json::from_slice(&message.payload)
                .map_err(|e| ProxyError::backend(format!("Failed to parse message: {e}")))?;
            trace!(message = %message, "Received introspection message");

            if message.get("method").is_some() {
                self.answer_server_message(&message).await?;
                continue;
            }
            if message.get("id") != Some(&expected) {
                debug!(message = %message, "Ignoring response to an unknown request");
                continue;
            }

            let response: JsonRpcResponse = serde_json::from_value(message)
                .map_err(|e| ProxyError::backend(format!("Failed to parse response: {e}")))?;
            return match response.payload {
                JsonRpcResponsePayload::Success { result } => Ok(result),
                JsonRpcResponsePayload::Error { error } => {
                    let mut err =
                        turbomcp_protocol::Error::from_rpc_code(error.code, error.message);
                    if let Some(data) = error.data {
                        err = err.with_data(data);
                    }
                    Err(err.into())
                }
            };
        }
    }

    /// Handle a message the server originated while we wait for a response.
    ///
    /// Notifications need nothing. A request needs an answer, or the server
    /// is left waiting on us: `ping` is answered with the empty result the
    /// spec requires, and anything else with method-not-found, since the
    /// introspector declares no client capabilities.
    async fn answer_server_message(&self, message: &Value) -> ProxyResult<()> {
        let Some(id) = message.get("id") else {
            return Ok(());
        };
        let reply = if message.get("method").and_then(Value::as_str) == Some("ping") {
            serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": {} })
        } else {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": "Method not found" }
            })
        };
        let reply = serde_json::to_vec(&reply)
            .map_err(|e| ProxyError::backend(format!("Failed to serialize reply: {e}")))?;
        self.send_raw(reply).await
    }

    /// Write one JSON-RPC message to the subprocess.
    async fn send_raw(&self, payload: Vec<u8>) -> ProxyResult<()> {
        let message = TransportMessage {
            id: turbomcp_protocol::MessageId::String(Uuid::new_v4().to_string()),
            payload: Bytes::from(payload),
            metadata: TransportMessageMetadata::default(),
        };

        self.transport
            .send(message)
            .await
            .map_err(|e| ProxyError::backend(format!("Failed to send message: {e}")))
    }
}

impl McpBackend for StdioBackend {
    fn initialize(
        &mut self,
        request: InitializeRequest,
    ) -> Pin<Box<dyn Future<Output = ProxyResult<InitializeResult>> + Send + '_>> {
        Box::pin(async move {
            debug!("Initializing STDIO backend via turbomcp-transport");

            let params = serde_json::to_value(&request).map_err(|e| {
                ProxyError::backend(format!("Failed to serialize initialize request: {e}"))
            })?;

            let result = self.send_request("initialize", params).await?;

            let init_result: InitializeResult = serde_json::from_value(result).map_err(|e| {
                ProxyError::backend(format!("Failed to deserialize initialize result: {e}"))
            })?;

            debug!(
                server_name = %init_result.server_info.name,
                server_version = %init_result.server_info.version,
                protocol_version = %init_result.protocol_version,
                "Server initialized successfully"
            );

            // Send initialized notification
            self.send_notification("notifications/initialized", serde_json::json!({}))
                .await?;

            Ok(init_result)
        })
    }

    fn call_method<'a>(
        &'a mut self,
        method: &'a str,
        params: Value,
    ) -> Pin<Box<dyn Future<Output = ProxyResult<Value>> + Send + 'a>> {
        Box::pin(async move { self.send_request(method, params).await })
    }

    fn send_notification<'a>(
        &'a mut self,
        method: &'a str,
        params: Value,
    ) -> Pin<Box<dyn Future<Output = ProxyResult<()>> + Send + 'a>> {
        Box::pin(async move {
            let notification = JsonRpcNotification {
                jsonrpc: JsonRpcVersion,
                method: method.to_string(),
                params: Some(params),
            };

            let notification_json = serde_json::to_vec(&notification).map_err(|e| {
                ProxyError::backend(format!("Failed to serialize notification: {e}"))
            })?;

            trace!(method = %method, "Sending notification");
            self.send_raw(notification_json).await
        })
    }

    fn shutdown(&mut self) -> Pin<Box<dyn Future<Output = ProxyResult<()>> + Send + '_>> {
        Box::pin(async move {
            debug!("Shutting down STDIO backend");

            // ChildProcessTransport handles cleanup on drop
            // No explicit shutdown needed - process will be killed on drop if kill_on_drop is true

            Ok(())
        })
    }

    fn description(&self) -> String {
        "STDIO backend via turbomcp-transport".to_string()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    // `/bin/cat` is POSIX-mandated on every Unix (macOS, Linux, *BSD), making it
    // a deterministic subprocess for spawn tests. It reads stdin and stays alive
    // until EOF, which lets `ChildProcessTransport::wait_for_ready` observe a
    // running process without needing a real MCP server binary on PATH.
    const TEST_SUBPROCESS: &str = "/bin/cat";

    #[tokio::test]
    async fn test_stdio_backend_creation() {
        let backend = StdioBackend::new(TEST_SUBPROCESS, Vec::new()).await;
        assert!(backend.is_ok(), "backend should spawn: {:?}", backend.err());
    }

    #[tokio::test]
    async fn test_stdio_backend_with_working_dir() {
        let backend =
            StdioBackend::with_working_dir(TEST_SUBPROCESS, Vec::new(), "/tmp".to_string()).await;
        assert!(backend.is_ok(), "backend should spawn: {:?}", backend.err());
    }

    /// A server may log or ping before it answers. The introspector used to
    /// take whatever line came next as the response, so a server that logged
    /// during `initialize` failed introspection outright. The scripted server
    /// here sends a log notification and a ping ahead of its answers, and
    /// only lists its tools if the ping was replied to first.
    #[tokio::test]
    async fn responses_are_matched_by_id_not_arrival_order() {
        use crate::introspection::McpIntrospector;

        let script = r#"
            log='{"jsonrpc":"2.0","method":"notifications/message","params":{"level":"info","data":"warming up"}}'
            read -r _init
            printf '%s\n' "$log" '{"jsonrpc":"2.0","id":"srv-1","method":"ping"}'
            printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"chatty","version":"1.0.0"}}}'
            read -r pong
            read -r _initialized
            read -r _list
            case "$pong" in
              *'"srv-1"'*'"result"'*|*'"result"'*'"srv-1"'*)
                printf '%s\n' "$log" '{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}' ;;
              *)
                printf '%s\n' '{"jsonrpc":"2.0","id":2,"error":{"code":-32603,"message":"ping went unanswered"}}' ;;
            esac
            read -r _
        "#;

        let mut backend = StdioBackend::new("sh", vec!["-c".to_string(), script.to_string()])
            .await
            .expect("scripted server spawns");
        let spec = McpIntrospector::new()
            .introspect(&mut backend)
            .await
            .expect("interleaved notifications and pings do not derail introspection");

        assert_eq!(spec.server_info.name, "chatty");
        assert_eq!(spec.tools.len(), 1);
        assert_eq!(spec.tools[0].name, "echo");
    }
}
