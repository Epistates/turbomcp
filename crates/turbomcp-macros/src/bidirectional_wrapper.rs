//! MCP wrapper generation for macro servers
//!
//! This module generates the internal wrapper struct that enables
//! full MCP 2025-06-18 support (including server-to-client capabilities:
//! sampling, elicitation, roots, ping) for servers created with the
//! #[turbomcp::server] macro.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;

/// Generate the internal MCP wrapper struct and its implementation
///
/// This creates an internal `{ServerName}Bidirectional` wrapper which
/// enables full MCP 2025-06-18 support including server-to-client capabilities.
/// The wrapper is private and automatically managed by the transport methods.
pub fn generate_bidirectional_wrapper(
    struct_name: &Ident,
    server_name: &str,
    server_version: &str,
) -> TokenStream {
    let wrapper_name = format_ident!("{}Bidirectional", struct_name);

    quote! {
        // ===================================================================
        // Internal MCP Wrapper - Full MCP 2025-06-18 Support
        // ===================================================================

        /// Internal wrapper that enables full MCP 2025-06-18 support
        ///
        /// This wrapper is an internal implementation detail that enables
        /// the server to support all MCP capabilities including:
        /// - Client → Server: tools, resources, prompts
        /// - Server → Client: sampling, elicitation, roots, ping
        ///
        /// **Note**: This type is private and automatically managed by the
        /// `#[turbomcp::server]` macro. You interact with your server through
        /// the simple `run_stdio()`, `run_http()`, and `run_websocket()` methods.
        struct #wrapper_name {
            /// The underlying server implementation
            inner: ::std::sync::Arc<#struct_name>,
            /// Server-to-client capabilities interface
            server_to_client: ::std::sync::Arc<dyn turbomcp_protocol::context::capabilities::ServerToClientRequests>,
        }

        impl #wrapper_name {
            /// Create a new bidirectional wrapper with a configured dispatcher
            ///
            /// # Arguments
            ///
            /// * `server` - The server implementation to wrap
            /// * `dispatcher` - Transport-specific dispatcher for server-initiated requests
            ///
            /// # Example
            ///
            /// ```no_run
            /// use turbomcp::runtime::stdio_bidirectional::StdioDispatcher;
            ///
            /// let dispatcher = StdioDispatcher::new(/* ... */);
            /// let wrapper = MyServerBidirectional::with_dispatcher(server, dispatcher);
            /// ```
            pub fn with_dispatcher<D>(server: #struct_name, dispatcher: D) -> Self
            where
                D: turbomcp_server::routing::ServerRequestDispatcher + 'static,
            {
                // Create bidirectional router
                let mut bidirectional = turbomcp_server::routing::BidirectionalRouter::new();
                bidirectional.set_dispatcher(dispatcher);

                // Create server-to-client adapter
                let server_to_client: ::std::sync::Arc<dyn turbomcp_protocol::context::capabilities::ServerToClientRequests> =
                    ::std::sync::Arc::new(turbomcp_server::capabilities::ServerToClientAdapter::new(bidirectional));

                Self {
                    inner: ::std::sync::Arc::new(server),
                    server_to_client,
                }
            }

            /// Check if full MCP 2025-06-18 support is available
            ///
            /// Returns `true` if server-to-client capabilities (sampling,
            /// elicitation, roots, ping) are available.
            pub fn supports_server_to_client(&self) -> bool {
                // Always true for properly constructed wrappers
                true
            }

            /// Handle a JSON-RPC request with full MCP context
            ///
            /// This is the internal request handler that:
            /// 1. Creates a RequestContext with server-to-client capabilities
            /// 2. Routes the request to the appropriate handler
            /// 3. Returns the JSON-RPC response
            ///
            /// **Note**: This method is called by the transport layer and should
            /// not be invoked directly.
            pub async fn handle_request_with_context(
                &self,
                req: turbomcp_protocol::jsonrpc::JsonRpcRequest,
                mut ctx: turbomcp_protocol::RequestContext,
            ) -> turbomcp_protocol::jsonrpc::JsonRpcResponse {
                // Inject server-to-client capabilities into the context
                // This enables ctx.create_message(), ctx.elicit(), ctx.list_roots(), etc.
                ctx = ctx.with_server_to_client(::std::sync::Arc::clone(&self.server_to_client));

                // Delegate to the inner server's request handler with full context
                // Clone the Arc to satisfy the Arc<Self> signature
                ::std::sync::Arc::clone(&self.inner).handle_request_with_context(req, ctx).await
            }
        }

        impl ::std::fmt::Debug for #wrapper_name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_struct(stringify!(#wrapper_name))
                    .field("server_name", &#server_name)
                    .field("server_version", &#server_version)
                    .field("mcp_support", &"full-2025-06-18")
                    .finish()
            }
        }

        // Implement JsonRpcHandler for the wrapper (for HTTP transport compatibility)
        #[::turbomcp::async_trait]
        impl turbomcp_protocol::JsonRpcHandler for #wrapper_name
        where
            Self: Send + Sync + 'static,
        {
            async fn handle_request(
                &self,
                req_value: serde_json::Value,
            ) -> serde_json::Value {
                // Parse the request
                let req: turbomcp_protocol::jsonrpc::JsonRpcRequest =
                    match serde_json::from_value(req_value) {
                        Ok(r) => r,
                        Err(e) => {
                            return serde_json::json!({
                                "jsonrpc": "2.0",
                                "error": {
                                    "code": -32700,
                                    "message": format!("Parse error: {}", e)
                                },
                                "id": null
                            });
                        }
                    };

                // Create default context
                let ctx = turbomcp_protocol::RequestContext::new();

                // Handle with full context
                let response = self.handle_request_with_context(req, ctx).await;

                // Serialize response
                match serde_json::to_value(&response) {
                    Ok(v) => v,
                    Err(e) => {
                        serde_json::json!({
                            "jsonrpc": "2.0",
                            "error": {
                                "code": -32603,
                                "message": format!("Internal error: {}", e)
                            },
                            "id": response.id
                        })
                    }
                }
            }

            fn server_info(&self) -> turbomcp_protocol::ServerInfo {
                turbomcp_protocol::ServerInfo {
                    name: #server_name.to_string(),
                    version: #server_version.to_string(),
                }
            }

            fn capabilities(&self) -> serde_json::Value {
                // Advertise all capabilities when bidirectional is supported
                serde_json::json!({
                    "tools": {},
                    "prompts": {},
                    "resources": {},
                    "sampling": {},
                    "elicitation": {},
                    "roots": {
                        "listChanged": false
                    }
                })
            }
        }
    }
}

/// Generate transport methods that provide full MCP 2025-06-18 support
///
/// These methods delegate to ServerBuilder's canonical implementations,
/// ensuring consistent MCP protocol compliance across all patterns.
pub fn generate_bidirectional_transport_methods(struct_name: &Ident) -> TokenStream {
    quote! {
        // ===================================================================
        // MCP Transport Methods - Full MCP 2025-06-18 Support
        // ===================================================================

        impl #struct_name
        where
            Self: Clone + Send + Sync + 'static,
        {
            /// Run server with stdio transport (MCP 2025-06-18 compliant)
            ///
            /// This method provides complete MCP support over stdio:
            /// - Client→Server: Tools, prompts, resources
            /// - Server→Client: Sampling, elicitation, roots, ping
            ///
            /// # Example
            ///
            /// ```no_run
            /// #[tokio::main]
            /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
            ///     MyServer.run_stdio().await
            /// }
            /// ```
            pub async fn run_stdio(self) -> Result<(), Box<dyn ::std::error::Error>> {
                // Create server instance using ServerBuilder pattern
                let server = self.create_server()?;

                // Use ServerBuilder's canonical STDIO implementation
                server.run_stdio().await
                    .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }

            /// Run server with HTTP transport (MCP 2025-06-18 compliant)
            #[cfg(feature = "http")]
            pub async fn run_http<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                self.run_http_with_path(addr, "/mcp").await
            }

            /// Run HTTP server with custom endpoint path (MCP 2025-06-18 compliant)
            #[cfg(feature = "http")]
            pub async fn run_http_with_path<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A,
                path: &str
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                // Create server instance using ServerBuilder pattern
                let server = self.create_server()?;

                // Configure HTTP with custom endpoint path
                use ::turbomcp_transport::streamable_http_v2::StreamableHttpConfigBuilder;

                let config = StreamableHttpConfigBuilder::new()
                    .with_endpoint_path(path)
                    .build();

                // Use ServerBuilder's canonical HTTP implementation
                server.run_http_with_config(addr, config).await
                    .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }

            /// Run server with WebSocket transport (MCP 2025-06-18 compliant)
            #[cfg(feature = "websocket")]
            pub async fn run_websocket<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                self.run_websocket_with_path(addr, "/ws").await
            }

            /// Run WebSocket server with custom endpoint path (MCP 2025-06-18 compliant)
            #[cfg(feature = "websocket")]
            pub async fn run_websocket_with_path<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A,
                path: &str
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                // Create server instance using ServerBuilder pattern
                let server = self.create_server()?;

                // Configure WebSocket with custom endpoint path
                use ::turbomcp_server::WebSocketServerConfig;
                let socket_addr = addr
                    .to_socket_addrs()?
                    .next()
                    .ok_or("No address resolved")?;

                let config = WebSocketServerConfig {
                    bind_addr: socket_addr.to_string(),
                    endpoint_path: path.to_string(),
                };

                // Use ServerBuilder's canonical WebSocket implementation
                server.run_websocket_with_config(addr, config).await
                    .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }

            /// Run server with TCP transport (MCP compliant)
            ///
            /// This method provides TCP socket transport for network communication.
            /// TCP is useful for local network communication or when stdio/HTTP aren't suitable.
            ///
            /// # Example
            ///
            /// ```no_run
            /// #[tokio::main]
            /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
            ///     MyServer.run_tcp("127.0.0.1:8765").await
            /// }
            /// ```
            #[cfg(feature = "tcp")]
            pub async fn run_tcp<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                // Create server instance and delegate to Server::run_tcp
                let server = self.create_server()?;
                server.run_tcp(addr).await.map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }

            /// Run server with Unix Domain Socket transport (MCP compliant)
            ///
            /// This method provides Unix socket transport for local IPC.
            /// Unix sockets are ideal for same-machine communication with lower overhead than TCP.
            ///
            /// # Example
            ///
            /// ```no_run
            /// #[tokio::main]
            /// async fn main() -> Result<(), Box<dyn std::error::Error>> {
            ///     MyServer.run_unix("/tmp/mcp.sock").await
            /// }
            /// ```
            #[cfg(all(feature = "unix", unix))]
            pub async fn run_unix<P: AsRef<::std::path::Path> + Send + ::std::fmt::Debug>(
                self,
                path: P
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                // Create server instance and delegate to Server::run_unix
                let server = self.create_server()?;
                server.run_unix(path).await.map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }
        }
    }
}
