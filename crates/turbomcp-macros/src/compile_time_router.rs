//! Compile-time router generation for TurboMCP
//!
//! This module generates static dispatch routers at compile time,
//! eliminating all lifetime issues and providing maximum performance.

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;
// Removed unused import: use crate::uri_template::UriTemplate;

/// Generate compile-time router for HTTP transport
pub fn generate_router(
    struct_name: &Ident,
    tool_methods: &[(Ident, Ident, Ident)], // (method_name, metadata_fn, handler_fn)
    prompt_methods: &[(Ident, Ident, Ident)], // (method_name, metadata_fn, handler_fn)
    resource_methods: &[(Ident, Ident, Ident)], // (method_name, metadata_fn, handler_fn)
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

    // Generate prompt list for prompts/list method
    let prompt_list_items: Vec<_> = prompt_methods
        .iter()
        .map(|(_method_name, metadata_fn, _)| {
            quote! {
                {
                    let (name, description, arguments_schema, _tags) = Self::#metadata_fn();
                    serde_json::json!({
                        "name": name,
                        "description": description,
                        "arguments": arguments_schema
                    })
                }
            }
        })
        .collect();

    // Generate resource list for resources/list method
    let resource_list_items: Vec<_> = resource_methods
        .iter()
        .map(|(_method_name, metadata_fn, _)| {
            quote! {
                {
                    let (uri_template, name, title, description, mime_type, _tags) = Self::#metadata_fn();
                    serde_json::json!({
                        "uri": uri_template,
                        "name": name,
                        "title": title,
                        "description": description,
                        "mimeType": mime_type
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
                        _meta: None,
                    };

                    let ctx = ::turbomcp::RequestContext::new();

                    match self.#handler_fn(request, ctx).await {
                        Ok(result) => {
                            ::turbomcp_protocol::jsonrpc::JsonRpcResponse::success(
                                serde_json::json!({
                                    "content": result.content
                                }),
                                req.id.clone()
                            )
                        }
                        Err(e) => {
                            ::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: -32603,
                                    message: e.to_string(),
                                    data: None,
                                },
                                req.id.clone()
                            )
                        }
                    }
                }
            }
        })
        .collect();

    // Generate prompt dispatch cases for prompts/get
    let prompt_dispatch_cases: Vec<_> = prompt_methods
        .iter()
        .map(|(method_name, _, handler_fn)| {
            let method_str = method_name.to_string();
            quote! {
                #method_str => {
                    // Parse arguments for prompts/get - convert to HashMap<String, Value>
                    let prompt_args = params
                        .and_then(|p| p.get("arguments"))
                        .and_then(|args| args.as_object())
                        .map(|obj| {
                            let mut map = std::collections::HashMap::new();
                            for (k, v) in obj {
                                map.insert(k.clone(), v.clone());
                            }
                            map
                        });

                    let request = ::turbomcp_protocol::GetPromptRequest {
                        name: #method_str.to_string(),
                        arguments: prompt_args,
                        _meta: None,
                    };

                    let ctx = ::turbomcp::RequestContext::new();

                    match self.#handler_fn(request, ctx).await {
                        Ok(result) => {
                            // Wrap string result in proper MCP GetPromptResult format
                            let get_prompt_result = ::turbomcp_protocol::GetPromptResult {
                                description: None,
                                messages: vec![::turbomcp_protocol::types::PromptMessage {
                                    role: ::turbomcp_protocol::types::Role::User,
                                    content: ::turbomcp_protocol::types::Content::Text(::turbomcp_protocol::types::TextContent {
                                        text: result,
                                        annotations: None,
                                        meta: None,
                                    }),
                                }],
                                _meta: None,
                            };
                            ::turbomcp_protocol::jsonrpc::JsonRpcResponse::success(
                                serde_json::to_value(get_prompt_result).unwrap(),
                                req.id.clone()
                            )
                        }
                        Err(e) => {
                            ::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: -32603,
                                    message: e.to_string(),
                                    data: None,
                                },
                                req.id.clone()
                            )
                        }
                    }
                }
            }
        })
        .collect();

    // Generate unique function name for URI template matching to avoid conflicts
    let uri_match_fn_name = format_ident!(
        "{}_uri_template_matches",
        struct_name.to_string().to_lowercase()
    );

    // Generate resource dispatch cases for resources/read with compile-time URI template matching
    let resource_dispatch_cases: Vec<_> = resource_methods
        .iter()
        .map(|(_method_name, metadata_fn, handler_fn)| {
            // Get the URI template at compile time for code generation
            let template_string = quote! {
                {
                    let (uri_template, _, _, _, _, _) = Self::#metadata_fn();
                    uri_template
                }
            };

            quote! {
                resource_uri if {
                    let template = #template_string;
                    #uri_match_fn_name(resource_uri, template)
                } => {
                    let request = ::turbomcp_protocol::ReadResourceRequest {
                        uri: resource_uri.to_string(),
                        _meta: None,
                    };

                    let ctx = ::turbomcp::RequestContext::new();

                    match self.#handler_fn(request, ctx).await {
                        Ok(result) => {
                            // Wrap string result in proper MCP ReadResourceResult format
                            let read_resource_result = ::turbomcp_protocol::ReadResourceResult {
                                contents: vec![::turbomcp_protocol::types::ResourceContent::Text(::turbomcp_protocol::types::TextResourceContents {
                                    uri: resource_uri.to_string(),
                                    mime_type: Some("text/plain".to_string()),
                                    text: result,
                                    meta: None,
                                })],
                                _meta: None,
                            };
                            ::turbomcp_protocol::jsonrpc::JsonRpcResponse::success(
                                serde_json::to_value(read_resource_result).unwrap(),
                                req.id.clone()
                            )
                        }
                        Err(e) => {
                            ::turbomcp_protocol::jsonrpc::JsonRpcResponse::error_response(
                                ::turbomcp_protocol::jsonrpc::JsonRpcError {
                                    code: -32603,
                                    message: e.to_string(),
                                    data: None,
                                },
                                req.id.clone()
                            )
                        }
                    }
                }
            }
        })
        .collect();

    quote! {
        // Helper function for URI template matching - generated at compile time for maximum performance
        fn #uri_match_fn_name(uri: &str, template: &str) -> bool {
            // Simple implementation for literal templates
            if !template.contains('{') {
                return uri == template;
            }

            // Parse template for variable extraction with high performance
            let template_parts: Vec<&str> = template.split('/').filter(|s| !s.is_empty()).collect();
            let uri_parts: Vec<&str> = uri.split('/').filter(|s| !s.is_empty()).collect();

            if template_parts.len() != uri_parts.len() {
                return false;
            }

            for (template_part, uri_part) in template_parts.iter().zip(uri_parts.iter()) {
                if template_part.starts_with('{') && template_part.ends_with('}') {
                    // Variable part - matches any non-empty string
                    if uri_part.is_empty() {
                        return false;
                    }
                } else if template_part != uri_part {
                    // Literal part that doesn't match
                    return false;
                }
            }

            true
        }

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
                                    JsonRpcResponse::success(
                                        serde_json::json!({
                                            "protocolVersion": "2025-06-18",
                                            "serverInfo": {
                                                "name": #server_name,
                                                "version": #server_version
                                            },
                                            "capabilities": {
                                                "tools": {},
                                                "elicitation": {}
                                            }
                                        }),
                                        req.id.clone()
                                    )
                                }

                                "tools/list" => {
                                    let tools: Vec<serde_json::Value> = vec![#(#tool_list_items),*];
                                    JsonRpcResponse::success(
                                        serde_json::json!({
                                            "tools": tools
                                        }),
                                        req.id.clone()
                                    )
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
                                                    JsonRpcResponse::error_response(
                                                        JsonRpcError {
                                                            code: -32602,
                                                            message: format!("Unknown tool: {}", tool_name),
                                                            data: None,
                                                        },
                                                        req.id.clone()
                                                    )
                                                }
                                            }
                                        }
                                        None => {
                                            JsonRpcResponse::error_response(
                                                JsonRpcError {
                                                    code: -32602,
                                                    message: "Missing tool name".to_string(),
                                                    data: None,
                                                },
                                                req.id.clone()
                                            )
                                        }
                                    }
                                }

                                "prompts/list" => {
                                    let prompts: Vec<serde_json::Value> = vec![#(#prompt_list_items),*];
                                    JsonRpcResponse::success(
                                        serde_json::json!({
                                            "prompts": prompts
                                        }),
                                        req.id.clone()
                                    )
                                }

                                "prompts/get" => {
                                    let params = req.params.as_ref().and_then(|p| p.as_object());
                                    let prompt_name = params
                                        .and_then(|p| p.get("name"))
                                        .and_then(|n| n.as_str());
                                    match prompt_name {
                                        Some(prompt_name) => {
                                            match prompt_name {
                                                #(#prompt_dispatch_cases)*
                                                _ => {
                                                    JsonRpcResponse::error_response(
                                                        JsonRpcError {
                                                            code: -32601,
                                                            message: format!("Unknown prompt: {}", prompt_name),
                                                            data: None,
                                                        },
                                                        req.id.clone()
                                                    )
                                                }
                                            }
                                        }
                                        None => {
                                            JsonRpcResponse::error_response(
                                                JsonRpcError {
                                                    code: -32602,
                                                    message: "Missing prompt name".to_string(),
                                                    data: None,
                                                },
                                                req.id.clone()
                                            )
                                        }
                                    }
                                }

                                "resources/list" => {
                                    let resources: Vec<serde_json::Value> = vec![#(#resource_list_items),*];
                                    JsonRpcResponse::success(
                                        serde_json::json!({
                                            "resources": resources
                                        }),
                                        req.id.clone()
                                    )
                                }

                                "resources/read" => {
                                    let params = req.params.as_ref().and_then(|p| p.as_object());
                                    let resource_uri = params
                                        .and_then(|p| p.get("uri"))
                                        .and_then(|u| u.as_str());
                                    match resource_uri {
                                        Some(resource_uri) => {
                                            match resource_uri {
                                                #(#resource_dispatch_cases)*
                                                _ => {
                                                    JsonRpcResponse::error_response(
                                                        JsonRpcError {
                                                            code: -32601,
                                                            message: format!("Unknown resource: {}", resource_uri),
                                                            data: None,
                                                        },
                                                        req.id.clone()
                                                    )
                                                }
                                            }
                                        }
                                        None => {
                                            JsonRpcResponse::error_response(
                                                JsonRpcError {
                                                    code: -32602,
                                                    message: "Missing resource URI".to_string(),
                                                    data: None,
                                                },
                                                req.id.clone()
                                            )
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
                                    JsonRpcResponse::success(
                                        serde_json::json!({}),
                                        req.id.clone()
                                    )
                                }

                                _ => {
                                    JsonRpcResponse::error_response(
                                        JsonRpcError {
                                            code: -32601,
                                            message: format!("Method not found: {}", req.method),
                                            data: None,
                                        },
                                        req.id.clone()
                                    )
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

                // STDIO transport must be completely silent per MCP specification
                // stdout is reserved exclusively for JSON-RPC messages

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
                                Err(_e) => {
                                    // Silent error handling for STDIO MCP compliance
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
                        Err(_e) => {
                            // Silent error handling for STDIO MCP compliance
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
                        JsonRpcResponse::success(
                            serde_json::json!({
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
                            }),
                            req.id.clone()
                        )
                    }

                    "tools/list" => {
                        let tools: Vec<serde_json::Value> = vec![#(#tool_list_items),*];
                        JsonRpcResponse::success(
                            serde_json::json!({
                                "tools": tools
                            }),
                            req.id.clone()
                        )
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
                                JsonRpcResponse::error_response(
                                    JsonRpcError {
                                        code: -32601,
                                        message: format!("Unknown tool: {}", tool_name),
                                        data: None,
                                    },
                                    req.id.clone()
                                )
                            }
                        }
                    }

                    "prompts/list" => {
                        let prompts: Vec<serde_json::Value> = vec![#(#prompt_list_items),*];
                        JsonRpcResponse::success(
                            serde_json::json!({
                                "prompts": prompts
                            }),
                            req.id.clone()
                        )
                    }

                    "prompts/get" => {
                        let params = req.params.as_ref().and_then(|p| p.as_object());
                        let prompt_name = params
                            .and_then(|p| p.get("name"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("");
                        // Compile-time dispatch to prompt handlers
                        match prompt_name {
                            #(#prompt_dispatch_cases)*
                            _ => {
                                JsonRpcResponse::error_response(
                                    JsonRpcError {
                                        code: -32601,
                                        message: format!("Unknown prompt: {}", prompt_name),
                                        data: None,
                                    },
                                    req.id.clone()
                                )
                            }
                        }
                    }

                    "resources/list" => {
                        let resources: Vec<serde_json::Value> = vec![#(#resource_list_items),*];
                        JsonRpcResponse::success(
                            serde_json::json!({
                                "resources": resources
                            }),
                            req.id.clone()
                        )
                    }

                    "resources/read" => {
                        let params = req.params.as_ref().and_then(|p| p.as_object());
                        let resource_uri = params
                            .and_then(|p| p.get("uri"))
                            .and_then(|u| u.as_str())
                            .unwrap_or("");
                        // Compile-time dispatch to resource handlers - match by URI pattern
                        match resource_uri {
                            #(#resource_dispatch_cases)*
                            _ => {
                                JsonRpcResponse::error_response(
                                    JsonRpcError {
                                        code: -32601,
                                        message: format!("Unknown resource: {}", resource_uri),
                                        data: None,
                                    },
                                    req.id.clone()
                                )
                            }
                        }
                    }

                    _ => {
                        JsonRpcResponse::error_response(
                            JsonRpcError {
                                code: -32601,
                                message: format!("Method not found: {}", req.method),
                                data: None,
                            },
                            req.id.clone()
                        )
                    }
                }
            }
        }
    }
}
