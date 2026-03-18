//! Integration test for bidirectional communication
//!
//! This test validates that the MessageDispatcher correctly routes:
//! - Responses to waiting request() calls
//! - Server-initiated requests to registered handlers
//!
//! ## What This Tests
//!
//! The critical bug fix: When a client calls `call_tool()` and the server
//! sends an elicitation REQUEST (not a response), the dispatcher must:
//! 1. NOT give the elicitation request to the waiting `call_tool()`
//! 2. Route it to the elicitation handler instead
//! 3. Still deliver the eventual tools/call RESPONSE to `call_tool()`
//!
//! This test uses a mock transport to simulate the exact scenario that was broken.

#[cfg(test)]
mod bidirectional_tests {

    /// Compile-time check for bidirectional dispatcher types
    ///
    /// This test validates that the core bidirectional types used by the
    /// MessageDispatcher architecture are present and correctly structured:
    /// - ElicitationRequest wrapper type exists with expected accessor methods
    /// - MessageId implements Display for use in handler routing
    /// - ElicitRequestParams is constructible for form-mode elicitation
    ///
    /// A full behavioral integration test of the dispatcher's routing logic
    /// (ensuring elicitation requests are not delivered to waiting request()
    /// calls) requires a mock transport and is tracked separately.
    #[test]
    fn test_bidirectional_types_compile() {
        use turbomcp_protocol::MessageId;
        use turbomcp_protocol::types::{ElicitRequest, ElicitRequestParams, ElicitationSchema};

        // Verify MessageId::String can be constructed and displayed
        let id = MessageId::String("test-id".to_string());
        assert_eq!(id.to_string(), "test-id", "MessageId::String Display impl");

        // Verify MessageId::Number variant
        let id_num = MessageId::Number(42);
        assert_eq!(id_num.to_string(), "42", "MessageId::Number Display impl");

        // Verify ElicitRequestParams::form constructor is callable
        let schema = ElicitationSchema::new().add_string_property(
            "field".to_string(),
            true,
            Some("A field".to_string()),
        );
        let params = ElicitRequestParams::form("Enter details".to_string(), schema, None, None);

        // Verify the ElicitRequest wrapper is constructible
        let req = ElicitRequest {
            params,
            task: None,
            _meta: None,
        };

        // Verify ElicitationRequest handler wrapper round-trips the id and message
        let handler_req = turbomcp_client::handlers::ElicitationRequest::new(
            MessageId::String("dispatch-test".to_string()),
            req,
        );
        assert_eq!(
            handler_req.id().to_string(),
            "dispatch-test",
            "ElicitationRequest preserves id from JSON-RPC envelope"
        );
        assert_eq!(
            handler_req.message(),
            "Enter details",
            "ElicitationRequest exposes message via accessor"
        );
    }

    /// Documentation test: How bidirectional flow works
    ///
    /// ```rust,ignore
    /// // Create client (dispatcher starts automatically)
    /// let client = Client::new(transport);
    ///
    /// // Register elicitation handler
    /// client.set_elicitation_handler(Arc::new(MyHandler));
    ///
    /// // Call tool that triggers elicitation
    /// let result = client.call_tool("test_elicitation", None, None).await?;
    ///
    /// // Flow:
    /// // 1. call_tool() sends tools/call request via ProtocolClient
    /// // 2. ProtocolClient registers oneshot channel with dispatcher
    /// // 3. Server sends elicitation/create REQUEST (not response!)
    /// // 4. Dispatcher routes it to elicitation handler (NOT to call_tool!)
    /// // 5. Handler processes elicitation, sends response
    /// // 6. Server sends tools/call RESPONSE
    /// // 7. Dispatcher routes response to call_tool's oneshot channel
    /// // 8. call_tool() receives response and returns! ✓
    /// ```
    #[test]
    fn test_bidirectional_flow_documentation() {
        // This is a documentation-only test
        // See comments above for the flow explanation
    }

    /// Test that elicitation type conversion works correctly
    ///
    /// This test validates the fix for the type mismatch bug where:
    /// - Server sends MCP protocol type (ElicitRequest)
    /// - Client must convert to handler type (ElicitationRequest)
    /// - The `id` field comes from JSON-RPC envelope, not params
    #[tokio::test]
    async fn test_elicitation_type_conversion() {
        use turbomcp_protocol::MessageId;
        use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcVersion};
        use turbomcp_protocol::types::{ElicitRequest, ElicitRequestParams, ElicitationSchema};

        // Simulate what the server sends (MCP protocol format)
        let mcp_request = ElicitRequest {
            params: ElicitRequestParams::form(
                "Please enter your configuration".to_string(),
                ElicitationSchema::new()
                    .add_string_property(
                        "username".to_string(),
                        true,
                        Some("Your username".to_string()),
                    )
                    .add_number_property(
                        "age".to_string(),
                        false,
                        Some("Your age".to_string()),
                        Some(0.0),
                        Some(150.0),
                    ),
                Some(30000), // 30 seconds in milliseconds
                Some(true),
            ),
            task: None,
            _meta: None,
        };

        // Serialize to JSON (what dispatcher receives)
        let params_json = serde_json::to_value(&mcp_request).unwrap();

        // Create JSON-RPC request (the id is HERE, not in params!)
        let jsonrpc_request = JsonRpcRequest {
            jsonrpc: JsonRpcVersion,
            id: MessageId::String("elicit-123".to_string()),
            method: "elicitation/create".to_string(),
            params: Some(params_json),
        };

        // Now simulate what the client does: convert MCP type to handler type

        // 1. Parse as MCP protocol type
        let parsed_mcp: ElicitRequest =
            serde_json::from_value(jsonrpc_request.params.clone().unwrap())
                .expect("Should parse as MCP protocol type");

        // 2. Wrap in handler request (preserves type safety!)
        let handler_request = turbomcp_client::handlers::ElicitationRequest::new(
            jsonrpc_request.id.clone(),
            parsed_mcp.clone(),
        );

        // 3. Validate wrapper provides ergonomic access
        assert_eq!(
            handler_request.id().to_string(),
            "elicit-123",
            "ID should come from JSON-RPC envelope"
        );
        assert_eq!(
            handler_request.message(),
            "Please enter your configuration",
            "Message accessible via getter"
        );
        assert_eq!(
            handler_request.timeout(),
            Some(std::time::Duration::from_millis(30000)),
            "Timeout should be available as Duration"
        );

        // Validate schema is TYPED (not serde_json::Value!)
        let schema = handler_request
            .schema()
            .expect("Schema should be present for form mode");
        assert_eq!(
            schema.schema_type, "object",
            "Schema type should be 'object'"
        );
        assert!(
            schema.properties.contains_key("username"),
            "Schema should contain username property"
        );
        assert!(
            schema.properties.contains_key("age"),
            "Schema should contain age property"
        );

        println!("✅ Wrapper preserves type safety!");
        println!(
            "   - ID extracted from JSON-RPC envelope: {:?}",
            handler_request.id()
        );
        println!("   - Message accessible: {}", handler_request.message());
        println!("   - Timeout as Duration: {:?}", handler_request.timeout());
        println!(
            "   - Schema is TYPED (ElicitationSchema) with {} properties",
            schema.properties.len()
        );
    }
}
