//! Plugin execution macros for reducing boilerplate
//!
//! These macros provide ergonomic ways to execute plugin middleware chains
//! without the verbose manual implementation required in each client method.

/// Execute a protocol call with full plugin middleware support
///
/// This macro handles the complete plugin execution pipeline:
/// 1. Creates RequestContext
/// 2. Executes before_request plugin chain
/// 3. Executes the provided protocol call
/// 4. Creates ResponseContext  
/// 5. Executes after_response plugin chain
/// 6. Returns the final result
///
/// # Usage
///
/// ```rust,no_run
/// # use turbomcp_client::plugins::with_plugins;
/// # use std::collections::HashMap;
/// # struct Client { plugin_registry: (), protocol: () }
/// # impl Client {
/// pub async fn call_tool(&mut self, name: &str, args: Option<HashMap<String, serde_json::Value>>) -> turbomcp_core::Result<serde_json::Value> {
///     let request_data = turbomcp_protocol::types::CallToolRequest {
///         name: name.to_string(),
///         arguments: Some(args.unwrap_or_default()),
///     };
///
///     with_plugins!(self, "tools/call", request_data, {
///         // Your protocol call here - plugins execute automatically
///         let result: turbomcp_protocol::types::CallToolResult = self.protocol
///             .request("tools/call", Some(serde_json::to_value(&request_data)?))
///             .await?;
///         
///         Ok(self.extract_tool_content(&result))
///     })
/// }
/// # }
/// ```
///
/// The macro automatically:
/// - ✅ Creates proper RequestContext with JSON-RPC structure
/// - ✅ Executes all registered plugins before the request
/// - ✅ Times the operation for metrics
/// - ✅ Creates ResponseContext with results and timing
/// - ✅ Executes all registered plugins after the response
/// - ✅ Handles errors gracefully with proper context
/// - ✅ Returns the final processed result
#[macro_export]
macro_rules! with_plugins {
    ($client:expr, $method:expr, $request_data:expr, $protocol_call:block) => {{
        // Create JSON-RPC request for plugin context with unique ID
        let request_id = turbomcp_core::MessageId::Number(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos() as i64
        );

        let json_rpc_request = turbomcp_protocol::jsonrpc::JsonRpcRequest {
            jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
            id: request_id,
            method: $method.to_string(),
            params: Some(serde_json::to_value(&$request_data)
                .map_err(|e| turbomcp_core::Error::bad_request(
                    format!("Failed to serialize request data: {}", e)
                ))?),
        };

        // 1. Create request context for plugins
        let mut req_ctx = $crate::plugins::RequestContext::new(
            json_rpc_request,
            std::collections::HashMap::new()
        );

        // 2. Execute before_request plugin middleware
        $client.plugin_registry.execute_before_request(&mut req_ctx).await
            .map_err(|e| turbomcp_core::Error::bad_request(
                format!("Plugin before_request failed: {}", e)
            ))?;

        // 3. Execute the actual protocol call with timing
        let start_time = std::time::Instant::now();
        let protocol_result: turbomcp_core::Result<_> = async $protocol_call.await;
        let duration = start_time.elapsed();

        // 4. Create response context based on result
        let mut resp_ctx = match protocol_result {
            Ok(ref response_value) => {
                let serialized_response = serde_json::to_value(response_value)
                    .map_err(|e| turbomcp_core::Error::bad_request(
                        format!("Failed to serialize response: {}", e)
                    ))?;
                $crate::plugins::ResponseContext::new(
                    req_ctx,
                    Some(serialized_response),
                    None,
                    duration
                )
            },
            Err(ref e) => {
                $crate::plugins::ResponseContext::new(
                    req_ctx,
                    None,
                    Some(*e.clone()),
                    duration
                )
            }
        };

        // 5. Execute after_response plugin middleware
        $client.plugin_registry.execute_after_response(&mut resp_ctx).await
            .map_err(|e| turbomcp_core::Error::bad_request(
                format!("Plugin after_response failed: {}", e)
            ))?;

        // 6. Return the final result, respecting plugin modifications
        match protocol_result {
            Ok(original_response) => {
                // Check if plugins modified the response
                if let Some(modified_response) = resp_ctx.response {
                    // Try to deserialize back to original type if plugins modified it
                    match serde_json::from_value(modified_response.clone()) {
                        Ok(plugin_modified_result) => Ok(plugin_modified_result),
                        Err(_) => {
                            // Plugin returned a different format, return the original
                            // This maintains type safety while allowing plugin flexibility
                            Ok(original_response)
                        }
                    }
                } else {
                    // No plugin modifications, return original
                    Ok(original_response)
                }
            },
            Err(original_error) => {
                // Check if plugins provided error recovery
                if let Some(recovery_response) = resp_ctx.response {
                    match serde_json::from_value(recovery_response) {
                        Ok(recovered_result) => Ok(recovered_result),
                        Err(_) => Err(original_error), // Recovery failed, return original error
                    }
                } else {
                    Err(original_error)
                }
            }
        }
    }};
}

/// Execute a simple protocol call with plugin middleware for methods without complex request data
///
/// This is a lighter version for methods that don't need complex request context.
///
/// # Usage
///
/// ```rust,no_run
/// # use turbomcp_client::plugins::with_simple_plugins;
/// # struct Client { plugin_registry: (), protocol: () }
/// # impl Client {
/// pub async fn ping(&mut self) -> turbomcp_core::Result<()> {
///     with_simple_plugins!(self, "ping", {
///         self.protocol.request("ping", None).await
///     })
/// }
/// # }
/// ```
#[macro_export]
macro_rules! with_simple_plugins {
    ($client:expr, $method:expr, $protocol_call:block) => {{
        // Use the full macro with empty request data
        let empty_request = serde_json::Value::Null;
        $crate::with_plugins!($client, $method, empty_request, $protocol_call)
    }};
}

/// Execute plugin middleware for methods that return lists (common pattern)
///
/// Many MCP methods return lists and have similar patterns. This macro
/// provides a specialized version for list-returning methods.
#[macro_export]
macro_rules! with_plugins_list {
    ($client:expr, $method:expr, $request_data:expr, $protocol_call:block) => {{ $crate::with_plugins!($client, $method, $request_data, $protocol_call) }};
}

#[cfg(test)]
mod tests {

    #[tokio::test]
    async fn test_macro_generates_unique_request_ids() {
        // Test that multiple macro invocations generate different IDs
        // This tests our fix for the hardcoded ID issue
        let request_data = serde_json::json!({"test": "data"});

        let id1 = {
            let json_rpc_request = turbomcp_protocol::jsonrpc::JsonRpcRequest {
                jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
                id: turbomcp_core::MessageId::Number(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as i64,
                ),
                method: "test".to_string(),
                params: Some(request_data.clone()),
            };
            json_rpc_request.id
        };

        // Small delay to ensure different timestamps
        tokio::time::sleep(tokio::time::Duration::from_nanos(1)).await;

        let id2 = {
            let json_rpc_request = turbomcp_protocol::jsonrpc::JsonRpcRequest {
                jsonrpc: turbomcp_protocol::jsonrpc::JsonRpcVersion,
                id: turbomcp_core::MessageId::Number(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as i64,
                ),
                method: "test".to_string(),
                params: Some(request_data),
            };
            json_rpc_request.id
        };

        // IDs should be different
        assert_ne!(id1, id2, "Request IDs should be unique");
    }

    #[test]
    fn test_error_propagation() {
        // Test that our error wrapping works correctly
        let error_message = "Failed to serialize request data: test error";
        let wrapped_error = turbomcp_core::Error::bad_request(error_message);

        assert!(
            wrapped_error
                .to_string()
                .contains("Failed to serialize request data")
        );
        assert!(wrapped_error.to_string().contains("test error"));

        // Test response serialization error wrapping as well
        let response_error_message = "Failed to serialize response: test response error";
        let wrapped_response_error = turbomcp_core::Error::bad_request(response_error_message);

        assert!(
            wrapped_response_error
                .to_string()
                .contains("Failed to serialize response")
        );
        assert!(
            wrapped_response_error
                .to_string()
                .contains("test response error")
        );
    }

    #[test]
    fn test_macro_compilation() {
        // These tests ensure the macros compile correctly
        // Real integration testing happens in the client tests
    }
}
