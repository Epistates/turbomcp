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
        /// Internal wrapper enabling full MCP 2025-06-18 bidirectional support
        struct #wrapper_name {
            inner: ::std::sync::Arc<#struct_name>,
            server_to_client: ::std::sync::Arc<dyn ::turbomcp::__macro_support::turbomcp_protocol::context::capabilities::ServerToClientRequests>,
        }

        impl #wrapper_name {
            /// Create wrapper with the given dispatcher for server-initiated requests
            pub fn with_dispatcher<D>(server: #struct_name, dispatcher: D) -> Self
            where
                D: ::turbomcp::__macro_support::turbomcp_server::routing::ServerRequestDispatcher + 'static,
            {
                let mut bidirectional = ::turbomcp::__macro_support::turbomcp_server::routing::BidirectionalRouter::new();
                bidirectional.set_dispatcher(dispatcher);

                let server_to_client: ::std::sync::Arc<dyn ::turbomcp::__macro_support::turbomcp_protocol::context::capabilities::ServerToClientRequests> =
                    ::std::sync::Arc::new(::turbomcp::__macro_support::turbomcp_server::capabilities::ServerToClientAdapter::new(bidirectional));

                Self {
                    inner: ::std::sync::Arc::new(server),
                    server_to_client,
                }
            }

            /// Returns true (bidirectional wrappers always support server-to-client)
            pub fn supports_server_to_client(&self) -> bool {
                true
            }

            /// Handle JSON-RPC request with server-to-client capabilities injected
            pub async fn handle_request_with_context(
                &self,
                req: ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcRequest,
                mut ctx: ::turbomcp::__macro_support::turbomcp_protocol::RequestContext,
            ) -> ::turbomcp::__macro_support::turbomcp_protocol::jsonrpc::JsonRpcResponse {
                ctx = ctx.with_server_to_client(::std::sync::Arc::clone(&self.server_to_client));
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

        #[::turbomcp::async_trait]
        impl ::turbomcp::__macro_support::turbomcp_protocol::JsonRpcHandler for #wrapper_name
        where
            Self: Send + Sync + 'static,
        {
            async fn handle_request(
                &self,
                mut req_value: serde_json::Value,
            ) -> serde_json::Value {
                let headers_json = req_value.get("_mcp_headers").cloned();
                let transport = req_value.get("_mcp_transport")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                if let Some(obj) = req_value.as_object_mut() {
                    obj.remove("_mcp_headers");
                    obj.remove("_mcp_transport");
                }

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

                let mut ctx = ::turbomcp::__macro_support::turbomcp_protocol::RequestContext::new();

                if let Some(t) = transport {
                    ctx = ctx.with_metadata("transport", t);
                }

                if let Some(headers) = headers_json {
                    ctx = ctx.with_metadata("http_headers", headers);
                }

                let response = self.handle_request_with_context(req, ctx).await;

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

/// Generate transport methods (run_stdio, run_http, etc.) based on requested transports
pub fn generate_bidirectional_transport_methods(
    struct_name: &Ident,
    transports: &Option<Vec<String>>,
) -> TokenStream {
    let should_generate_http = transports
        .as_ref()
        .map(|t| t.contains(&"http".to_string()))
        .unwrap_or(false);

    let should_generate_websocket = transports
        .as_ref()
        .map(|t| t.contains(&"websocket".to_string()))
        .unwrap_or(false);

    let should_generate_tcp = transports
        .as_ref()
        .map(|t| t.contains(&"tcp".to_string()))
        .unwrap_or(false);

    let should_generate_unix = transports
        .as_ref()
        .map(|t| t.contains(&"unix".to_string()))
        .unwrap_or(false);
    let http_methods = if should_generate_http {
        quote! {
            /// Run server with HTTP transport on default path `/mcp`
            #[cfg(feature = "http")]
            pub async fn run_http<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                self.run_http_with_path(addr, "/mcp").await
            }

            /// Run HTTP server with custom endpoint path
            #[cfg(feature = "http")]
            pub async fn run_http_with_path<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A,
                path: &str
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                let server = self.create_server()?;
                use ::turbomcp::__macro_support::turbomcp_transport::streamable_http::StreamableHttpConfigBuilder;

                let config = StreamableHttpConfigBuilder::new()
                    .with_endpoint_path(path)
                    .build();

                server.run_http_with_config(addr, config).await
                    .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }

            /// Run HTTP server with full configuration control
            #[cfg(feature = "http")]
            pub async fn run_http_with_config<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A,
                config: ::turbomcp::__macro_support::turbomcp_transport::streamable_http::StreamableHttpConfig
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                let server = self.create_server()?;
                server.run_http_with_config(addr, config).await
                    .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }

            /// Run HTTP server with Tower middleware layers
            #[cfg(feature = "http")]
            pub async fn run_http_with_middleware<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A,
                middleware_fn: Box<dyn FnOnce(::turbomcp::__macro_support::axum::Router) -> ::turbomcp::__macro_support::axum::Router + Send>,
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                let server = self.create_server()?;
                server.run_http_with_middleware(addr, middleware_fn).await
                    .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }
        }
    } else {
        quote! {}
    };

    let websocket_methods = if should_generate_websocket {
        quote! {
            /// Run server with WebSocket transport on default path `/ws`
            #[cfg(feature = "websocket")]
            pub async fn run_websocket<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                self.run_websocket_with_path(addr, "/ws").await
            }

            /// Run WebSocket server with custom endpoint path
            #[cfg(feature = "websocket")]
            pub async fn run_websocket_with_path<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A,
                path: &str
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                let server = self.create_server()?;
                use ::turbomcp::__macro_support::turbomcp_server::WebSocketServerConfig;
                let socket_addr = addr
                    .to_socket_addrs()?
                    .next()
                    .ok_or("No address resolved")?;

                let config = WebSocketServerConfig {
                    bind_addr: socket_addr.to_string(),
                    endpoint_path: path.to_string(),
                    max_concurrent_requests: 100,
                };

                server.run_websocket_with_config(addr, config).await
                    .map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }
        }
    } else {
        quote! {}
    };

    let tcp_methods = if should_generate_tcp {
        quote! {
            /// Run server with TCP transport
            #[cfg(feature = "tcp")]
            pub async fn run_tcp<A: ::std::net::ToSocketAddrs + Send + ::std::fmt::Debug>(
                self,
                addr: A
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                let server = self.create_server()?;
                server.run_tcp(addr).await.map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }
        }
    } else {
        quote! {}
    };

    let unix_methods = if should_generate_unix {
        quote! {
            /// Run server with Unix domain socket transport
            #[cfg(all(feature = "unix", unix))]
            pub async fn run_unix<P: AsRef<::std::path::Path> + Send + ::std::fmt::Debug>(
                self,
                path: P
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                let server = self.create_server()?;
                server.run_unix(path).await.map_err(|e| Box::new(e) as Box<dyn ::std::error::Error>)
            }
        }
    } else {
        quote! {}
    };

    quote! {
        impl #struct_name
        where
            Self: Clone + Send + Sync + 'static,
        {
            /// Run server with stdio transport
            pub async fn run_stdio(self) -> Result<(), Box<dyn ::std::error::Error>> {
                let server = self.create_server()?;
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
