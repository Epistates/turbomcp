//! STDIO frontend: serve a proxied MCP server over stdin/stdout.
//!
//! The frontend is `turbomcp-server`'s own stdio transport serving a
//! [`ProxyService`]. It used to be a hand-written read-dispatch-write loop,
//! and that loop was an MCP server in name only: `initialize` answered with a
//! bare capabilities object (no `protocolVersion`, no `serverInfo`), `ping`
//! was an unknown method, and every notification got a reply. Serving through
//! the server stack makes the handshake, version negotiation, `ping`,
//! notifications, cancellation, and framing identical to any other `TurboMCP`
//! server, and leaves the proxy with only its own job: forwarding.

use tokio::io::BufReader;
use tracing::debug;
use turbomcp_server::transport::{LineReader, LineTransportRunner, LineWriter};
use turbomcp_server::{McpHandler, RequestContext, ServerConfig};

use crate::error::{ProxyError, ProxyResult};
use crate::proxy::ProxyService;

/// STDIO frontend for CLI-friendly access
///
/// Reads newline-delimited JSON-RPC from stdin and writes responses to stdout.
/// Logs and diagnostics must go to stderr to keep stdout clean for protocol
/// messages.
pub struct StdioFrontend {
    /// The proxied server
    service: ProxyService,

    /// Server configuration (protocol versions, message size limit)
    config: ServerConfig,
}

impl StdioFrontend {
    /// Create a new STDIO frontend
    ///
    /// # Arguments
    /// * `service` - The proxy service to serve (backend already introspected)
    /// * `config` - Server configuration for the frontend
    #[must_use]
    pub fn new(service: ProxyService, config: ServerConfig) -> Self {
        Self { service, config }
    }

    /// Serve on stdin/stdout until stdin reaches EOF.
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the transport fails.
    pub async fn run(self) -> ProxyResult<()> {
        debug!("Starting STDIO frontend");
        turbomcp_server::transport::stdio::run_with_config(&self.service, &self.config)
            .await
            .map_err(|e| ProxyError::backend(format!("STDIO frontend error: {e}")))
    }

    /// Serve on an arbitrary line-oriented reader/writer pair.
    ///
    /// This is [`Self::run`] with the pipes supplied by the caller: useful for
    /// embedding the proxy behind something other than the process's own
    /// stdio, and for testing it.
    ///
    /// # Errors
    ///
    /// Returns `ProxyError` if the transport fails.
    pub async fn serve<R, W>(self, reader: R, writer: W) -> ProxyResult<()>
    where
        R: tokio::io::AsyncRead + Unpin + Send + 'static,
        BufReader<R>: LineReader,
        W: LineWriter,
    {
        self.service
            .on_initialize()
            .await
            .map_err(ProxyError::from)?;
        let runner = LineTransportRunner::with_config(self.service.clone(), self.config);
        let result = runner
            .run(BufReader::new(reader), writer, RequestContext::stdio)
            .await
            .map_err(|e| ProxyError::backend(format!("STDIO frontend error: {e}")));
        self.service.on_shutdown().await.map_err(ProxyError::from)?;
        result
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    use super::*;
    use crate::proxy::BackendConnector;

    /// A client speaking to a [`StdioFrontend`] over an in-memory pipe.
    struct Wire {
        to_proxy: tokio::io::DuplexStream,
        from_proxy: tokio::io::Lines<BufReader<tokio::io::DuplexStream>>,
    }

    impl Wire {
        async fn over(backend: BackendConnector) -> Self {
            let spec = backend.introspect().await.expect("introspection");
            let frontend = StdioFrontend::new(
                ProxyService::new(backend, spec),
                ServerConfig::builder().build(),
            );

            let (to_proxy, proxy_in) = tokio::io::duplex(64 * 1024);
            let (proxy_out, from_proxy) = tokio::io::duplex(64 * 1024);
            tokio::spawn(frontend.serve(proxy_in, proxy_out));

            Self {
                to_proxy,
                from_proxy: BufReader::new(from_proxy).lines(),
            }
        }

        async fn send(&mut self, message: Value) {
            let mut line = serde_json::to_vec(&message).expect("serializes");
            line.push(b'\n');
            self.to_proxy.write_all(&line).await.expect("proxy reads");
        }

        async fn recv(&mut self) -> Value {
            let line = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.from_proxy.next_line(),
            )
            .await
            .expect("the proxy answers")
            .expect("readable")
            .expect("a line");
            serde_json::from_str(&line).expect("the proxy writes JSON")
        }

        async fn initialize(&mut self, version: &str) -> Value {
            self.send(json!({
                "jsonrpc": "2.0",
                "id": 0,
                "method": "initialize",
                "params": {
                    "protocolVersion": version,
                    "capabilities": {},
                    "clientInfo": { "name": "wire-test", "version": "1.0.0" }
                }
            }))
            .await;
            let response = self.recv().await;
            self.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
                .await;
            response
        }
    }

    fn catalogue_backend() -> BackendConnector {
        BackendConnector::from_static_data_for_test(
            vec![turbomcp_protocol::types::Tool {
                name: "echo".to_string(),
                ..Default::default()
            }],
            vec![],
            vec![],
            vec![],
        )
    }

    /// MCP §Lifecycle: the `initialize` result carries `protocolVersion`,
    /// `capabilities`, and `serverInfo`, and the version is the client's own
    /// when the server supports it. The old loop answered with the upstream's
    /// bare capabilities object, which no client could complete a handshake
    /// against.
    #[tokio::test]
    async fn initialize_is_a_real_handshake() {
        for version in ["2025-11-25", "2025-06-18"] {
            let mut wire = Wire::over(catalogue_backend()).await;
            let response = wire.initialize(version).await;

            assert_eq!(response["id"], 0);
            let result = &response["result"];
            assert_eq!(result["protocolVersion"], version);
            assert_eq!(result["serverInfo"]["name"], "test-backend-proxy");
            assert_eq!(result["capabilities"]["tools"], json!({}));
        }
    }

    /// `ping` must be answered with an empty result, and a notification must
    /// never be answered at all. The old loop parsed every line as a request,
    /// so `notifications/initialized` drew a `-32700` with `id: null`, and
    /// `ping` came back "method not found".
    #[tokio::test]
    async fn ping_is_answered_and_notifications_are_not() {
        let mut wire = Wire::over(catalogue_backend()).await;
        wire.initialize("2025-11-25").await;

        // `initialize` above already sent `notifications/initialized`; send
        // one more notification, then a ping. If either notification had
        // been answered, that answer would be the next line instead.
        wire.send(json!({
            "jsonrpc": "2.0",
            "method": "notifications/cancelled",
            "params": { "requestId": 99 }
        }))
        .await;
        wire.send(json!({ "jsonrpc": "2.0", "id": 1, "method": "ping" }))
            .await;

        assert_eq!(
            wire.recv().await,
            json!({ "jsonrpc": "2.0", "id": 1, "result": {} })
        );
    }

    /// An upstream JSON-RPC error has to reach the client as the upstream
    /// sent it. `-32042` is the sharpest case: its `data.elicitations` holds
    /// the URLs the user must visit, and without them the error is a dead end.
    /// The old loop flattened every upstream error to `-32603` with the text
    /// in `data`.
    #[tokio::test]
    async fn upstream_errors_are_forwarded_with_their_data() {
        let elicitations = json!({
            "elicitations": [{
                "mode": "url",
                "elicitationId": "e-1",
                "url": "https://example.com/connect",
                "message": "Connect your account"
            }]
        });
        let upstream = turbomcp_protocol::Error::from_rpc_code(-32042, "Authorization required")
            .with_data(elicitations.clone());
        let mut wire = Wire::over(BackendConnector::failing_tool_calls_for_test(
            "connect", upstream,
        ))
        .await;
        wire.initialize("2025-11-25").await;

        wire.send(json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": { "name": "connect", "arguments": {} }
        }))
        .await;
        let response = wire.recv().await;

        assert_eq!(response["id"], 7);
        assert_eq!(response["error"]["code"], -32042);
        assert_eq!(response["error"]["data"], elicitations);
    }

    /// JSON-RPC 2.0 §5.1: an unknown method is `-32601`. The old loop
    /// reported it correctly but every other local failure as `-32603`; the
    /// server router is now the only place that decides.
    #[tokio::test]
    async fn an_unknown_method_is_method_not_found() {
        let mut wire = Wire::over(catalogue_backend()).await;
        wire.initialize("2025-11-25").await;

        wire.send(json!({ "jsonrpc": "2.0", "id": 3, "method": "no/such/method" }))
            .await;
        let response = wire.recv().await;
        assert_eq!(response["error"]["code"], -32601);
    }
}
