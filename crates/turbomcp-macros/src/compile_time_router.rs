//! Compile-time router generation for TurboMCP
//!
//! This module generates static dispatch routers at compile time,
//! eliminating all lifetime issues and providing maximum performance.

use proc_macro2::TokenStream;
use quote::quote;
use syn::Ident;

/// Generate compile-time router for HTTP transport
pub fn generate_router(
    struct_name: &Ident,
    tool_methods: &[(Ident, Ident, Ident)], // (method_name, metadata_fn, handler_fn)
    server_name: &str,
    server_version: &str,
) -> TokenStream {
    // Generate tool list for tools/list method
    let tool_list_items: Vec<_> = tool_methods
        .iter()
        .map(|(_method_name, metadata_fn, _)| {
            quote! {
                {
                    let (name, description, schema) = Self::#metadata_fn();
                    serde_json::json!({
                        "name": name,
                        "description": description,
                        "inputSchema": schema
                    })
                }
            }
        })
        .collect();

    // Generate tool dispatch cases
    let tool_dispatch_cases: Vec<_> = tool_methods
        .iter()
        .map(|(method_name, _, handler_fn)| {
            let method_str = method_name.to_string();
            quote! {
                #method_str => {
                    // Parse arguments
                    let args = params
                        .and_then(|p| p.get("arguments"))
                        .and_then(|a| a.as_object())
                        .map(|obj| {
                            let mut map = ::std::collections::HashMap::new();
                            for (k, v) in obj {
                                map.insert(k.clone(), v.clone());
                            }
                            map
                        });

                    let request = ::turbomcp::CallToolRequest {
                        name: tool_name.to_string(),
                        arguments: args,
                    };

                    let ctx = ::turbomcp::RequestContext::new();

                    match self.#handler_fn(request, ctx).await {
                        Ok(result) => {
                            ::turbomcp_protocol::jsonrpc::JsonRpcResponse {
                                jsonrpc: ::turbomcp_protocol::jsonrpc::JsonRpcVersion,
                                result: Some(serde_json::json!({
                                    "content": result.content
                                })),
                                error: None,
                                id: Some(req.id.clone()),
                            }
                        }
                        Err(e) => {
                            ::turbomcp_protocol::jsonrpc::JsonRpcResponse {
                                jsonrpc: ::turbomcp_protocol::jsonrpc::JsonRpcVersion,
                                result: None,
                                error: Some(::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: -32603,
                                    message: e.to_string(),
                                    data: None,
                                }),
                                id: Some(req.id.clone()),
                            }
                        }
                    }
                }
            }
        })
        .collect();

    quote! {
        impl #struct_name
        where
            Self: Clone + Send + Sync + 'static,
        {
            /// Convert server into an Axum router with default "/mcp" path (compile-time routing)
            ///
            /// This method generates a static dispatch router with zero runtime overhead.
            /// All handler dispatch is done at compile time via match statements.
            #[cfg(feature = "http")]
            pub fn into_router(self: ::std::sync::Arc<Self>) -> axum::Router {
                self.into_router_with_path("/mcp")
            }

            /// Convert server into an Axum router with custom path (compile-time routing)
            ///
            /// This method generates a static dispatch router with zero runtime overhead.
            /// All handler dispatch is done at compile time via match statements.
            #[cfg(feature = "http")]
            pub fn into_router_with_path(self: ::std::sync::Arc<Self>, path: &str) -> axum::Router {
                use axum::{Json, routing::post, Router};
                use ::turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcVersion, JsonRpcError};
                use ::turbomcp_protocol::types::RequestId;

                Router::new()
                    .route(path, post(move |Json(req): Json<JsonRpcRequest>| {
                        let server = self.clone();
                        async move {
                            let response = match req.method.as_str() {
                                "initialize" => {
                                    JsonRpcResponse {
                                        jsonrpc: JsonRpcVersion,
                                        result: Some(serde_json::json!({
                                            "protocolVersion": "2025-06-18",
                                            "serverInfo": {
                                                "name": #server_name,
                                                "version": #server_version
                                            },
                                            "capabilities": {
                                                "tools": {},
                                                "elicitation": {}
                                            }
                                        })),
                                        error: None,
                                        id: Some(req.id.clone()),
                                    }
                                }

                                "tools/list" => {
                                    let tools = vec![#(#tool_list_items),*];
                                    JsonRpcResponse {
                                        jsonrpc: JsonRpcVersion,
                                        result: Some(serde_json::json!({
                                            "tools": tools
                                        })),
                                        error: None,
                                        id: Some(req.id.clone()),
                                    }
                                }

                                "tools/call" => {
                                    let params = req.params.as_ref().and_then(|p| p.as_object());
                                    let tool_name = params
                                        .and_then(|p| p.get("name"))
                                        .and_then(|n| n.as_str());

                                    match tool_name {
                                        Some(tool_name) => {
                                            match tool_name {
                                                #(#tool_dispatch_cases)*
                                                _ => {
                                                    JsonRpcResponse {
                                                        jsonrpc: JsonRpcVersion,
                                                        result: None,
                                                        error: Some(JsonRpcError {
                                                            code: -32602,
                                                            message: format!("Unknown tool: {}", tool_name),
                                                            data: None,
                                                        }),
                                                        id: Some(req.id.clone()),
                                                    }
                                                }
                                            }
                                        }
                                        None => {
                                            JsonRpcResponse {
                                                jsonrpc: JsonRpcVersion,
                                                result: None,
                                                error: Some(JsonRpcError {
                                                    code: -32602,
                                                    message: "Missing tool name".to_string(),
                                                    data: None,
                                                }),
                                                id: Some(req.id.clone()),
                                            }
                                        }
                                    }
                                }

                                "elicitation/create" => {
                                    // Handle elicitation responses from client
                                    // This is where we'd receive ElicitationCreateResult from the client
                                    // and deliver it to the waiting tool

                                    // For now, return success to acknowledge receipt
                                    // The actual implementation would use an ElicitationManager to
                                    // correlate responses with pending requests
                                    JsonRpcResponse {
                                        jsonrpc: JsonRpcVersion,
                                        result: Some(serde_json::json!({})),
                                        error: None,
                                        id: Some(req.id.clone()),
                                    }
                                }

                                _ => {
                                    JsonRpcResponse {
                                        jsonrpc: JsonRpcVersion,
                                        result: None,
                                        error: Some(JsonRpcError {
                                            code: -32601,
                                            message: format!("Method not found: {}", req.method),
                                            data: None,
                                        }),
                                        id: Some(req.id.clone()),
                                    }
                                }
                            };

                            Json(response)
                        }
                    }))
            }

            /// Run server with stdio transport (MCP spec compliant)
            /// Server reads JSON-RPC from stdin, writes to stdout
            pub async fn run_stdio(self) -> Result<(), Box<dyn ::std::error::Error>> {
                use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
                use ::turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse, JsonRpcVersion};

                let server = ::std::sync::Arc::new(self);
                let stdin = tokio::io::stdin();
                let mut stdout = tokio::io::stdout();
                let mut reader = BufReader::new(stdin);
                let mut line = String::new();

                // Send initialize capability notification
                tracing::info!("Starting stdio server");

                loop {
                    line.clear();
                    match reader.read_line(&mut line).await {
                        Ok(0) => break, // EOF
                        Ok(_) => {
                            if line.trim().is_empty() {
                                continue;
                            }

                            // Parse JSON-RPC request
                            let request: JsonRpcRequest = match serde_json::from_str(&line) {
                                Ok(req) => req,
                                Err(e) => {
                                    tracing::error!("Failed to parse request: {}", e);
                                    continue;
                                }
                            };

                            // Process request using compile-time dispatch
                            let response = server.clone().handle_request(request).await;

                            // Write response
                            let response_str = serde_json::to_string(&response)?;
                            stdout.write_all(response_str.as_bytes()).await?;
                            stdout.write_all(b"\n").await?;
                            stdout.flush().await?;
                        }
                        Err(e) => {
                            tracing::error!("Failed to read from stdin: {}", e);
                            break;
                        }
                    }
                }

                Ok(())
            }

            /// Run server with TCP transport
            #[cfg(feature = "tcp")]
            pub async fn run_tcp<A: ::std::net::ToSocketAddrs>(
                self,
                addr: A
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                use ::turbomcp_transport::tcp::TcpTransport;
                use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
                use ::turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse};

                // Resolve address
                let socket_addr = addr
                    .to_socket_addrs()?
                    .next()
                    .ok_or("No address resolved")?;

                // Create TCP listener
                let listener = ::tokio::net::TcpListener::bind(socket_addr).await?;
                tracing::info!("TCP server listening on {}", socket_addr);

                loop {
                    let (stream, _) = listener.accept().await?;
                    let server = ::std::sync::Arc::new(self.clone());

                    tokio::spawn(async move {
                        let (reader, mut writer) = stream.into_split();
                        let mut reader = BufReader::new(reader);
                        let mut line = String::new();

                        loop {
                            line.clear();
                            match reader.read_line(&mut line).await {
                                Ok(0) => break,
                                Ok(_) => {
                                    if line.trim().is_empty() {
                                        continue;
                                    }

                                    let request: JsonRpcRequest = match serde_json::from_str(&line) {
                                        Ok(req) => req,
                                        Err(_) => continue,
                                    };

                                    let response = server.clone().handle_request(request).await;
                                    let response_str = serde_json::to_string(&response).unwrap();
                                    let _ = writer.write_all(response_str.as_bytes()).await;
                                    let _ = writer.write_all(b"\n").await;
                                    let _ = writer.flush().await;
                                }
                                Err(_) => break,
                            }
                        }
                    });
                }
            }

            /// Run server with Unix domain socket
            #[cfg(all(unix, feature = "unix"))]
            pub async fn run_unix<P: AsRef<::std::path::Path>>(
                self,
                path: P
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
                use ::turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse};

                let path = path.as_ref();

                // Ensure parent directory exists
                if let Some(parent) = path.parent() {
                    if !parent.exists() {
                        return Err(format!("Parent directory does not exist: {:?}", parent).into());
                    }
                }

                // Remove existing socket file if it exists
                if path.exists() {
                    ::std::fs::remove_file(path)?;
                }

                // Create Unix listener
                let listener = ::tokio::net::UnixListener::bind(path)?;
                tracing::info!("Unix socket server listening on {:?}", path);

                loop {
                    let (stream, _) = listener.accept().await?;
                    let server = ::std::sync::Arc::new(self.clone());

                    tokio::spawn(async move {
                        let (reader, mut writer) = stream.into_split();
                        let mut reader = BufReader::new(reader);
                        let mut line = String::new();

                        loop {
                            line.clear();
                            match reader.read_line(&mut line).await {
                                Ok(0) => break,
                                Ok(_) => {
                                    if line.trim().is_empty() {
                                        continue;
                                    }

                                    let request: JsonRpcRequest = match serde_json::from_str(&line) {
                                        Ok(req) => req,
                                        Err(_) => continue,
                                    };

                                    let response = server.clone().handle_request(request).await;
                                    let response_str = serde_json::to_string(&response).unwrap();
                                    let _ = writer.write_all(response_str.as_bytes()).await;
                                    let _ = writer.write_all(b"\n").await;
                                    let _ = writer.flush().await;
                                }
                                Err(_) => break,
                            }
                        }
                    });
                }
            }

            /// Run server with HTTP transport (compile-time routing, zero lifetime issues!)
            #[cfg(feature = "http")]
            /// Run HTTP server with default "/mcp" endpoint
            pub async fn run_http<A: ::std::net::ToSocketAddrs>(
                self,
                addr: A
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                self.run_http_with_path(addr, "/mcp").await
            }

            /// Run HTTP server with configurable endpoint path
            pub async fn run_http_with_path<A: ::std::net::ToSocketAddrs>(
                self,
                addr: A,
                path: &str
            ) -> Result<(), Box<dyn ::std::error::Error>> {
                use tokio::net::TcpListener;

                let router = ::std::sync::Arc::new(self).into_router_with_path(path);

                // Resolve address
                let socket_addr = addr
                    .to_socket_addrs()?
                    .next()
                    .ok_or("No address resolved")?;

                let listener = TcpListener::bind(socket_addr).await?;

                tracing::info!("🚀 TurboMCP server on http://{}", socket_addr);
                tracing::info!("  📡 MCP endpoint: http://{}{}", socket_addr, path);

                axum::serve(listener, router).await?;
                Ok(())
            }

            /// Handle a single JSON-RPC request with compile-time dispatch
            async fn handle_request(
                self: ::std::sync::Arc<Self>,
                req: turbomcp_protocol::jsonrpc::JsonRpcRequest
            ) -> turbomcp_protocol::jsonrpc::JsonRpcResponse {
                use ::turbomcp_protocol::jsonrpc::{JsonRpcResponse, JsonRpcVersion, JsonRpcError};

                match req.method.as_str() {
                    "initialize" => {
                        JsonRpcResponse {
                            jsonrpc: JsonRpcVersion,
                            result: Some(serde_json::json!({
                                "protocolVersion": ::turbomcp_core::PROTOCOL_VERSION,
                                "serverInfo": {
                                    "name": #server_name,
                                    "version": #server_version
                                },
                                "capabilities": {
                                    "tools": {},
                                    "prompts": {},
                                    "resources": {},
                                    "sampling": {}
                                }
                            })),
                            error: None,
                            id: Some(req.id.clone()),
                        }
                    }

                    "tools/list" => {
                        let tools = vec![#(#tool_list_items),*];
                        JsonRpcResponse {
                            jsonrpc: JsonRpcVersion,
                            result: Some(serde_json::json!({
                                "tools": tools
                            })),
                            error: None,
                            id: Some(req.id.clone()),
                        }
                    }

                    "tools/call" => {
                        let params = req.params.as_ref().and_then(|p| p.as_object());
                        let tool_name = params
                            .and_then(|p| p.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("");

                        // Compile-time dispatch to tool handlers
                        match tool_name {
                            #(#tool_dispatch_cases)*
                            _ => {
                                JsonRpcResponse {
                                    jsonrpc: JsonRpcVersion,
                                    result: None,
                                    error: Some(JsonRpcError {
                                        code: -32601,
                                        message: format!("Unknown tool: {}", tool_name),
                                        data: None,
                                    }),
                                    id: Some(req.id.clone()),
                                }
                            }
                        }
                    }

                    _ => {
                        JsonRpcResponse {
                            jsonrpc: JsonRpcVersion,
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32601,
                                message: format!("Method not found: {}", req.method),
                                data: None,
                            }),
                            id: Some(req.id.clone()),
                        }
                    }
                }
            }
        }
    }
}
