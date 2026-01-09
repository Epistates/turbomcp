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
            server_to_client: ::std::sync::Arc<dyn ::turbomcp::__macro_support::turbomcp_protocol::context::capabilities::ServerToClientRequests>,
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
            /// ```rust,ignore
            /// use turbomcp::runtime::stdio_bidirectional::StdioDispatcher;
            ///
            /// let dispatcher = StdioDispatcher::new(/* ... */);
            /// let wrapper = MyServerBidirectional::with_dispatcher(server, dispatcher);
            /// ```
            pub fn with_dispatcher<D>(server: #struct_name, dispatcher: D) -> Self
            where
                D: ::turbomcp::__macro_support::turbomcp_server::routing::ServerRequestDispatcher + 'static,
            {
                // Create bidirectional router
                let mut bidirectional = ::turbomcp::__macro_support::turbomcp_server::routing::BidirectionalRouter::new();
                bidirectional.set_dispatcher(dispatcher);

                // Create server-to-client adapter
                let server_to_client: ::std::sync::Arc<dyn ::turbomcp::__macro_support::turbomcp_protocol::context::capabilities::ServerToClientRequests> =
                    ::std::sync::Arc::new(::turbomcp::__macro_support::turbomcp_server::capabilities::ServerToClientAdapter::new(bidirectional));

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
                req: ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcRequest,
                mut ctx: ::turbomcp::__macro_support::turbomcp_protocol::RequestContext,
            ) -> ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse {
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
        impl ::turbomcp::__macro_support::turbomcp_protocol::JsonRpcHandler for #wrapper_name
        where
            Self: Send + Sync + 'static,
        {
            async fn handle_request(
                &self,
                mut req_value: serde_json::Value,
            ) -> serde_json::Value {
                // Extract headers and transport from request metadata (injected by transport layer)
                let headers_json = req_value.get("_mcp_headers").cloned();
                let transport = req_value.get("_mcp_transport")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                // Clean up internal fields before parsing JSON-RPC
                if let Some(obj) = req_value.as_object_mut() {
                    obj.remove("_mcp_headers");
                    obj.remove("_mcp_transport");
                }

                // Parse the request
                let req: ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcRequest =
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

                // Create context with transport metadata
                let mut ctx = ::turbomcp::__macro_support::turbomcp_protocol::RequestContext::new();

                // Add transport type to context
                if let Some(t) = transport {
                    ctx = ctx.with_metadata("transport", t);
                }

                // Add HTTP headers to context (for WebSocket upgrade headers or HTTP requests)
                if let Some(headers) = headers_json {
                    ctx = ctx.with_metadata("http_headers", headers);
                }

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

            fn server_info(&self) -> ::turbomcp::__macro_support::turbomcp_protocol::ServerInfo {
                ::turbomcp::__macro_support::turbomcp_protocol::ServerInfo {
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
pub fn generate_bidirectional_transport_methods(
    struct_name: &Ident,
    transports: &Option<Vec<String>>,
) -> TokenStream {
    // Determine which transports to generate code for
    // If transports is specified, use only those; otherwise generate all
    let should_generate_http = transports
        .as_ref()
        .map(|t| t.contains(&"http".to_string()))
        .unwrap_or(true);

    let should_generate_websocket = transports
        .as_ref()
        .map(|t| t.contains(&"websocket".to_string()))
        .unwrap_or(true);

    let should_generate_tcp = transports
        .as_ref()
        .map(|t| t.contains(&"tcp".to_string()))
        .unwrap_or(true);

    let should_generate_unix = transports
        .as_ref()
        .map(|t| t.contains(&"unix".to_string()))
        .unwrap_or(true);
    // Generate HTTP methods conditionally
    let http_methods = if should_generate_http {
        quote! {
            /// Run server with HTTP transport (MCP 2025-06-18 compliant)
            ///
            /// **Note**: Requires the `http` feature to be enabled on `turbomcp`.
            /// Add `turbomcp = { version = "2.0", features = ["http"] }` to your Cargo.toml.
            #[cfg(feature = "http")]
            pub async fn run_http<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                self.run_http_with_path(addr, "/mcp").await
            }

            /// Run HTTP server with custom endpoint path (MCP 2025-11-25 compliant)
            ///
            /// **Note**: Requires the `http` feature to be enabled on `turbomcp`.
            /// Add `turbomcp = { version = "2.0", features = ["http"] }` to your Cargo.toml.
            #[cfg(feature = "http")]
            pub async fn run_http_with_path<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A,
                path: &str
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                // Create server instance using ServerBuilder pattern
                let server = self.create_server()?;

                // Configure HTTP with custom endpoint path
                use ::turbomcp::__macro_support::turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;

                let config = StreamableHttpConfigBuilder::new()
                    .with_endpoint_path(path)
                    .build();

                // Use ServerBuilder's canonical HTTP implementation
                server.run_http_with_config(addr, config).await
                    .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }

            /// Run HTTP server with custom configuration (MCP 2025-11-25 compliant)
            ///
            /// Allows full control over HTTP server configuration including CORS,
            /// rate limiting, and security settings.
            ///
            /// **Note**: Requires the `http` feature to be enabled on `turbomcp`.
            /// Add `turbomcp = { version = "2.0", features = ["http"] }` to your Cargo.toml.
            ///
            /// # Example: Enable CORS for browser-based tools
            ///
            /// ```ignore
            /// use turbomcp::prelude::*;
            /// use turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;
            ///
            /// let config = StreamableHttpConfigBuilder::new()
            ///     .with_bind_address("127.0.0.1:3000")
            ///     .with_endpoint_path("/mcp")
            ///     .allow_any_origin(true)  // Enable CORS for development
            ///     .build();
            ///
            /// server.run_http_with_config("127.0.0.1:3000", config).await?;
            /// ```
            #[cfg(feature = "http")]
            pub async fn run_http_with_config<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A,
                config: ::turbomcp::__macro_support::turbomcp_transport::streamable_http::StreamableHttpConfig
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                // Create server instance using ServerBuilder pattern
                let server = self.create_server()?;

                // Use ServerBuilder's canonical HTTP implementation
                server.run_http_with_config(addr, config).await
                    .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }

            /// Run HTTP server with Tower middleware for advanced features
            ///
            /// This method enables multi-tenancy, authentication, rate limiting, and other
            /// cross-cutting concerns by allowing Tower middleware layers to be applied to the router.
            ///
            /// **Note**: Requires the `http` feature to be enabled on `turbomcp`.
            /// Add `turbomcp = { version = "2.0", features = ["http"] }` to your Cargo.toml.
            ///
            /// # Multi-Tenancy Example
            ///
            /// ```ignore
            /// use turbomcp::prelude::*;
            /// use turbomcp_server::middleware::tenancy::{HeaderTenantExtractor, TenantExtractionLayer};
            /// use tower::ServiceBuilder;
            ///
            /// let tenant_extractor = HeaderTenantExtractor::new("X-Tenant-ID");
            /// let middleware = ServiceBuilder::new()
            ///     .layer(TenantExtractionLayer::new(tenant_extractor));
            ///
            /// server.run_http_with_middleware(
            ///     "127.0.0.1:3000",
            ///     Box::new(move |router| router.layer(middleware))
            /// ).await?;
            /// ```
            // IMPORTANT: Use ::turbomcp::__macro_support::axum::Router (NOT ::axum::Router) because:
            //
            // 1. All users of this macro have `turbomcp` as a dependency
            // 2. `turbomcp` re-exports `axum` under the `http` feature (see turbomcp/src/lib.rs ~L557)
            // 3. Using `::axum::Router` would FAIL if user doesn't have axum as a direct dependency
            //
            // "Bring Your Own Axum" compatibility:
            // - If user also has `axum = "0.8.4"` (same version as turbomcp), the types are IDENTICAL
            //   because Rust's semver resolution makes them the same underlying type
            // - If user has a DIFFERENT axum version, they get a compile error - this is CORRECT
            //   because mixing axum versions causes subtle runtime issues
            //
            // Users can write middleware using either:
            //   - `turbomcp::axum::Router` (always works)
            //   - `axum::Router` (works if their axum version matches turbomcp's)
            #[cfg(feature = "http")]
            pub async fn run_http_with_middleware<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A,
                middleware_fn: Box<dyn FnOnce(::turbomcp::__macro_support::axum::Router) -> ::turbomcp::__macro_support::axum::Router + Send>,
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                // Create server instance using ServerBuilder pattern
                let server = self.create_server()?;

                // Use ServerBuilder's canonical HTTP implementation with middleware support
                server.run_http_with_middleware(addr, middleware_fn).await
                    .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }
        }
    } else {
        quote! {}
    };

    // Generate WebSocket methods conditionally
    let websocket_methods = if should_generate_websocket {
        quote! {
            /// Run server with WebSocket transport (MCP 2025-06-18 compliant)
            ///
            /// **Note**: Requires the `websocket` feature to be enabled on `turbomcp`.
            /// Add `turbomcp = { version = "2.0", features = ["websocket"] }` to your Cargo.toml.
            #[cfg(feature = "websocket")]
            pub async fn run_websocket<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                self.run_websocket_with_path(addr, "/ws").await
            }

            /// Run WebSocket server with custom endpoint path (MCP 2025-06-18 compliant)
            ///
            /// **Note**: Requires the `websocket` feature to be enabled on `turbomcp`.
            /// Add `turbomcp = { version = "2.0", features = ["websocket"] }` to your Cargo.toml.
            #[cfg(feature = "websocket")]
            pub async fn run_websocket_with_path<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A,
                path: &str
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                // Create server instance using ServerBuilder pattern
                let server = self.create_server()?;

                // Configure WebSocket with custom endpoint path
                use ::turbomcp::__macro_support::turbomcp_server::WebSocketServerConfig;
                let socket_addr = addr
                    .to_socket_addrs()?
                    .next()
                    .ok_or("No address resolved")?;

                let config = WebSocketServerConfig {
                    bind_addr: socket_addr.to_string(),
                    endpoint_path: path.to_string(),
                    max_concurrent_requests: 100, // Use default
                };

                // Use ServerBuilder's canonical WebSocket implementation
                server.run_websocket_with_config(addr, config).await
                    .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }
        }
    } else {
        quote! {}
    };

    // Generate TCP methods conditionally
    let tcp_methods = if should_generate_tcp {
        quote! {
            /// Run server with TCP transport (MCP compliant)
            ///
            /// This method provides TCP socket transport for network communication.
            /// TCP is useful for local network communication or when stdio/HTTP aren't suitable.
            ///
            /// **Note**: Requires the `tcp` feature to be enabled on `turbomcp`.
            /// Add `turbomcp = { version = "2.0", features = ["tcp"] }` to your Cargo.toml.
            ///
            /// # Example
            ///
            /// ```rust,ignore
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
        }
    } else {
        quote! {}
    };

    // Generate Unix socket methods conditionally
    let unix_methods = if should_generate_unix {
        quote! {
            /// Run server with Unix Domain Socket transport (MCP compliant)
            ///
            /// This method provides Unix socket transport for local IPC.
            /// Unix sockets are ideal for same-machine communication with lower overhead than TCP.
            ///
            /// **Note**: Requires the `unix` feature to be enabled on `turbomcp` and is only available on Unix-like systems.
            /// Add `turbomcp = { version = "2.0", features = ["unix"] }` to your Cargo.toml.
            ///
            /// # Example
            ///
            /// ```rust,ignore
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
    } else {
        quote! {}
    };

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
            /// ```rust,ignore
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

            #http_methods
            #websocket_methods
            #tcp_methods
            #unix_methods
        }
    }
}
