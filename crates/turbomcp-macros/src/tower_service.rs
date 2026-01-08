//! Tower Service generation for macro-generated MCP servers
//!
//! This module generates Tower Layer and Service implementations that allow
//! macro-generated servers to be composed with the Tower middleware ecosystem.
//!
//! ## Generated Types
//!
//! For a server struct `MyServer`, this generates:
//! - `MyServerTowerService` - Tower Service implementation
//! - `MyServerLayer` - Tower Layer for creating services
//! - `MyServerLayerConfig` - Configuration for the layer
//!
//! ## Usage
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//!
//! let server = MyServer::new();
//! let layer = server.into_tower_layer();
//!
//! let service = ServiceBuilder::new()
//!     .layer(auth_layer)
//!     .layer(rate_limit_layer)
//!     .layer(layer)
//!     .service(inner);
//! ```

use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::Ident;

/// Generate Tower Service and Layer implementations for a server struct
///
/// This creates:
/// 1. `{StructName}TowerService` - Tower Service that wraps the server
/// 2. `{StructName}Layer` - Tower Layer for creating services
/// 3. `{StructName}LayerConfig` - Configuration for the layer
pub fn generate_tower_service(
    struct_name: &Ident,
    server_name: &str,
    server_version: &str,
) -> TokenStream {
    let service_name = format_ident!("{}TowerService", struct_name);
    let layer_name = format_ident!("{}Layer", struct_name);
    let config_name = format_ident!("{}LayerConfig", struct_name);
    let response_name = format_ident!("{}TowerResponse", struct_name);

    quote! {
        // ===================================================================
        // Tower Layer Configuration
        // ===================================================================

        /// Configuration for the Tower layer
        ///
        /// Controls behavior of the generated Tower service including
        /// timeout, logging, and method bypass options.
        #[derive(Debug, Clone)]
        pub struct #config_name {
            /// Request timeout duration
            pub timeout: ::std::time::Duration,
            /// Whether to include timing metadata in responses
            pub include_timing: bool,
            /// Methods that bypass server processing (handled directly)
            pub bypass_methods: Vec<String>,
            /// Enable request/response logging
            pub enable_logging: bool,
        }

        impl Default for #config_name {
            fn default() -> Self {
                Self {
                    timeout: ::std::time::Duration::from_secs(30),
                    include_timing: true,
                    bypass_methods: Vec::new(),
                    enable_logging: true,
                }
            }
        }

        impl #config_name {
            /// Create a new config with custom timeout
            #[must_use]
            pub fn with_timeout(timeout: ::std::time::Duration) -> Self {
                Self {
                    timeout,
                    ..Default::default()
                }
            }

            /// Set request timeout
            #[must_use]
            pub fn timeout(mut self, timeout: ::std::time::Duration) -> Self {
                self.timeout = timeout;
                self
            }

            /// Add a method to bypass server processing
            #[must_use]
            pub fn bypass_method(mut self, method: impl Into<String>) -> Self {
                self.bypass_methods.push(method.into());
                self
            }

            /// Enable or disable timing metadata
            #[must_use]
            pub fn include_timing(mut self, include: bool) -> Self {
                self.include_timing = include;
                self
            }

            /// Enable or disable logging
            #[must_use]
            pub fn enable_logging(mut self, enable: bool) -> Self {
                self.enable_logging = enable;
                self
            }

            /// Check if a method should bypass processing
            #[must_use]
            pub fn should_bypass(&self, method: &str) -> bool {
                self.bypass_methods.iter().any(|m| m == method)
            }
        }

        // ===================================================================
        // Tower Layer Implementation
        // ===================================================================

        /// Tower Layer that creates Tower services wrapping the server
        ///
        /// This layer can be composed with other Tower layers to build
        /// middleware stacks for authentication, rate limiting, etc.
        ///
        /// # Example
        ///
        /// ```rust,ignore
        /// use tower::ServiceBuilder;
        ///
        /// let layer = MyServer::new().into_tower_layer();
        ///
        /// let service = ServiceBuilder::new()
        ///     .layer(layer)
        ///     .service(inner_service);
        /// ```
        #[derive(Clone)]
        pub struct #layer_name {
            server: ::std::sync::Arc<#struct_name>,
            config: #config_name,
        }

        impl ::std::fmt::Debug for #layer_name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_struct(stringify!(#layer_name))
                    .field("server_name", &#server_name)
                    .field("server_version", &#server_version)
                    .field("config", &self.config)
                    .finish()
            }
        }

        impl #layer_name {
            /// Create a new layer from a server instance
            #[must_use]
            pub fn new(server: #struct_name) -> Self {
                Self {
                    server: ::std::sync::Arc::new(server),
                    config: #config_name::default(),
                }
            }

            /// Create a new layer with configuration
            #[must_use]
            pub fn with_config(server: #struct_name, config: #config_name) -> Self {
                Self {
                    server: ::std::sync::Arc::new(server),
                    config,
                }
            }

            /// Set the configuration
            #[must_use]
            pub fn config(mut self, config: #config_name) -> Self {
                self.config = config;
                self
            }

            /// Set request timeout
            #[must_use]
            pub fn timeout(mut self, timeout: ::std::time::Duration) -> Self {
                self.config.timeout = timeout;
                self
            }

            /// Add a method to bypass processing
            #[must_use]
            pub fn bypass_method(mut self, method: impl Into<String>) -> Self {
                self.config.bypass_methods.push(method.into());
                self
            }

            /// Enable or disable timing metadata
            #[must_use]
            pub fn include_timing(mut self, include: bool) -> Self {
                self.config.include_timing = include;
                self
            }

            /// Enable or disable logging
            #[must_use]
            pub fn enable_logging(mut self, enable: bool) -> Self {
                self.config.enable_logging = enable;
                self
            }
        }

        impl<S> ::turbomcp::tower::Layer<S> for #layer_name {
            type Service = #service_name;

            fn layer(&self, _inner: S) -> Self::Service {
                // Note: The layer replaces the inner service rather than wrapping it
                // because the MCP server IS the service
                #service_name::new(
                    ::std::sync::Arc::clone(&self.server),
                    self.config.clone()
                )
            }
        }

        // ===================================================================
        // Tower Service Implementation
        // ===================================================================

        /// Tower Service that wraps the MCP server for request handling
        ///
        /// This service implements `tower::Service` for JSON-RPC requests,
        /// enabling the server to be composed with Tower middleware.
        #[derive(Clone)]
        pub struct #service_name {
            server: ::std::sync::Arc<#struct_name>,
            config: #config_name,
        }

        impl ::std::fmt::Debug for #service_name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                f.debug_struct(stringify!(#service_name))
                    .field("server_name", &#server_name)
                    .field("server_version", &#server_version)
                    .field("config", &self.config)
                    .finish()
            }
        }

        impl #service_name {
            /// Create a new Tower service
            #[must_use]
            pub fn new(server: ::std::sync::Arc<#struct_name>, config: #config_name) -> Self {
                Self { server, config }
            }

            /// Get a reference to the underlying server
            #[must_use]
            pub fn server(&self) -> &::std::sync::Arc<#struct_name> {
                &self.server
            }

            /// Get the configuration
            #[must_use]
            pub fn config(&self) -> &#config_name {
                &self.config
            }
        }

        /// Response wrapper with timing and metadata
        #[derive(Debug, Clone)]
        pub struct #response_name {
            /// The JSON-RPC response
            pub response: ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse,
            /// Request duration
            pub duration: ::std::time::Duration,
            /// Response metadata
            pub metadata: ::std::collections::HashMap<String, serde_json::Value>,
        }

        impl #response_name {
            /// Create a new response
            #[must_use]
            pub fn new(
                response: ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse,
                duration: ::std::time::Duration
            ) -> Self {
                Self {
                    response,
                    duration,
                    metadata: ::std::collections::HashMap::new(),
                }
            }

            /// Add metadata to the response
            pub fn add_metadata(&mut self, key: impl Into<String>, value: serde_json::Value) {
                self.metadata.insert(key.into(), value);
            }
        }

        // Service implementation for JSON-RPC requests
        impl ::turbomcp::tower::Service<::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcRequest> for #service_name
        where
            #struct_name: Clone + Send + Sync + 'static,
        {
            type Response = #response_name;
            type Error = ::turbomcp::turbomcp_protocol::McpError;
            type Future = ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

            fn poll_ready(
                &mut self,
                _cx: &mut ::std::task::Context<'_>
            ) -> ::std::task::Poll<Result<(), Self::Error>> {
                ::std::task::Poll::Ready(Ok(()))
            }

            fn call(&mut self, req: ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcRequest) -> Self::Future {
                let method = req.method.clone();
                let start = ::std::time::Instant::now();
                let server = ::std::sync::Arc::clone(&self.server);
                let config = self.config.clone();

                Box::pin(async move {
                    // Check if method should bypass processing
                    if config.should_bypass(&method) {
                        return Err(::turbomcp::turbomcp_protocol::McpError::protocol(
                            format!("Method '{}' is bypassed by configuration", method)
                        ));
                    }

                    if config.enable_logging {
                        ::tracing::debug!(method = %method, "Processing request via Tower service");
                    }

                    // Convert request to JSON value for the handler
                    let req_value = serde_json::to_value(&req)
                        .map_err(|e| ::turbomcp::turbomcp_protocol::McpError::serialization(e.to_string()))?;

                    // Use the JsonRpcHandler trait implementation with fully qualified syntax
                    // to avoid conflict with the generated handle_request method
                    let response_value = <#struct_name as ::turbomcp::turbomcp_protocol::JsonRpcHandler>::handle_request(
                        server.as_ref(),
                        req_value
                    ).await;

                    // Parse response back to JsonRpcResponse
                    let response: ::turbomcp::turbomcp_protocol::jsonrpc::JsonRpcResponse =
                        serde_json::from_value(response_value)
                            .map_err(|e| ::turbomcp::turbomcp_protocol::McpError::serialization(e.to_string()))?;

                    let duration = start.elapsed();

                    if config.enable_logging {
                        ::tracing::debug!(
                            method = %method,
                            duration_ms = duration.as_millis(),
                            "Request completed via Tower service"
                        );
                    }

                    let mut tower_response = #response_name::new(response, duration);

                    if config.include_timing {
                        tower_response.add_metadata(
                            "duration_ms",
                            serde_json::json!(duration.as_millis())
                        );
                    }

                    Ok(tower_response)
                })
            }
        }

        // Service implementation for raw JSON values
        impl ::turbomcp::tower::Service<serde_json::Value> for #service_name
        where
            #struct_name: Clone + Send + Sync + 'static,
        {
            type Response = serde_json::Value;
            type Error = ::turbomcp::turbomcp_protocol::McpError;
            type Future = ::std::pin::Pin<Box<dyn ::std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>>;

            fn poll_ready(
                &mut self,
                _cx: &mut ::std::task::Context<'_>
            ) -> ::std::task::Poll<Result<(), Self::Error>> {
                ::std::task::Poll::Ready(Ok(()))
            }

            fn call(&mut self, req: serde_json::Value) -> Self::Future {
                let server = ::std::sync::Arc::clone(&self.server);
                let config = self.config.clone();

                Box::pin(async move {
                    // Extract method for logging
                    let method = req.get("method")
                        .and_then(|m| m.as_str())
                        .unwrap_or("unknown");

                    if config.enable_logging {
                        ::tracing::debug!(method = %method, "Processing raw JSON request via Tower service");
                    }

                    // Use the JsonRpcHandler trait implementation with fully qualified syntax
                    // to avoid conflict with the generated handle_request method
                    let response = <#struct_name as ::turbomcp::turbomcp_protocol::JsonRpcHandler>::handle_request(
                        server.as_ref(),
                        req
                    ).await;

                    Ok(response)
                })
            }
        }

        // ===================================================================
        // Convenience Methods on Server Struct
        // ===================================================================

        impl #struct_name
        where
            Self: Clone + Send + Sync + 'static,
        {
            /// Convert the server into a Tower Layer
            ///
            /// The returned layer can be composed with other Tower layers
            /// to build middleware stacks.
            ///
            /// # Example
            ///
            /// ```rust,ignore
            /// use tower::ServiceBuilder;
            ///
            /// let layer = server.into_tower_layer();
            /// let service = ServiceBuilder::new()
            ///     .layer(layer)
            ///     .service(inner);
            /// ```
            #[must_use]
            pub fn into_tower_layer(self) -> #layer_name {
                #layer_name::new(self)
            }

            /// Convert the server into a Tower Layer with configuration
            #[must_use]
            pub fn into_tower_layer_with_config(self, config: #config_name) -> #layer_name {
                #layer_name::with_config(self, config)
            }

            /// Convert the server into a Tower Service
            ///
            /// The returned service implements `tower::Service` for JSON-RPC requests.
            #[must_use]
            pub fn into_tower_service(self) -> #service_name {
                #service_name::new(
                    ::std::sync::Arc::new(self),
                    #config_name::default()
                )
            }

            /// Convert the server into a Tower Service with configuration
            #[must_use]
            pub fn into_tower_service_with_config(self, config: #config_name) -> #service_name {
                #service_name::new(
                    ::std::sync::Arc::new(self),
                    config
                )
            }
        }
    }
}

// Test helpers are generated inline with the Tower service code
// The generated structs include test-friendly APIs like:
// - {StructName}LayerConfig::default()
// - config.should_bypass(method)
// - config.timeout(), .include_timing(), .enable_logging() builders
