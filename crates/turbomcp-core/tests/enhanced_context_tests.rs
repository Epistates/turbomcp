//! Integration tests for enhanced context types supporting MCP 2025-06-18 features

use serde_json::json;
use std::collections::HashMap;
use turbomcp_core::context::*;

#[test]
fn test_elicitation_context_creation() {
    let schema = json!({
        "type": "object",
        "properties": {
            "username": {"type": "string"},
            "age": {"type": "integer"}
        },
        "required": ["username"]
    });

    let ctx = ElicitationContext::new(
        "Please enter your username and age".to_string(),
        schema.clone(),
    );

    assert!(!ctx.elicitation_id.is_empty());
    assert_eq!(ctx.message, "Please enter your username and age");
    assert_eq!(ctx.schema, schema);
    assert_eq!(ctx.state, ElicitationState::Pending);
    assert_eq!(ctx.timeout_ms, Some(30000));
    assert!(ctx.cancellable);
}

#[test]
fn test_elicitation_state_transitions() {
    let mut ctx = ElicitationContext::new("Test message".to_string(), json!({}));

    assert_eq!(ctx.state, ElicitationState::Pending);
    assert!(!ctx.is_complete());

    ctx.set_state(ElicitationState::Accepted);
    assert_eq!(ctx.state, ElicitationState::Accepted);
    assert!(ctx.is_complete());

    ctx.set_state(ElicitationState::Declined);
    assert_eq!(ctx.state, ElicitationState::Declined);
    assert!(ctx.is_complete());

    ctx.set_state(ElicitationState::TimedOut);
    assert_eq!(ctx.state, ElicitationState::TimedOut);
    assert!(ctx.is_complete());
}

#[test]
fn test_completion_context_creation() {
    let completion_ref = CompletionReference::Tool {
        name: "file_search".to_string(),
        argument: "path".to_string(),
    };

    let ctx = CompletionContext::new(completion_ref.clone());

    assert!(!ctx.completion_id.is_empty());
    assert_eq!(ctx.max_completions, Some(100));
    assert!(!ctx.has_more);
    assert!(ctx.resolved_arguments.is_empty());
}

#[test]
fn test_completion_context_with_resolved_args() {
    let completion_ref = CompletionReference::ResourceTemplate {
        name: "api_endpoint".to_string(),
        parameter: "endpoint".to_string(),
    };

    let mut resolved = HashMap::new();
    resolved.insert(
        "base_url".to_string(),
        "https://api.example.com".to_string(),
    );
    resolved.insert("version".to_string(), "v2".to_string());

    let ctx = CompletionContext::new(completion_ref).with_resolved_arguments(resolved.clone());

    assert_eq!(ctx.resolved_arguments, resolved);
}

#[test]
fn test_completion_capabilities() {
    let caps = CompletionCapabilities {
        supports_pagination: true,
        supports_fuzzy: true,
        max_batch_size: 50,
        supports_descriptions: true,
    };

    assert!(caps.supports_pagination);
    assert!(caps.supports_fuzzy);
    assert_eq!(caps.max_batch_size, 50);
    assert!(caps.supports_descriptions);
}

#[test]
fn test_server_initiated_context() {
    let ctx =
        ServerInitiatedContext::new(ServerInitiatedType::CreateMessage, "server-001".to_string());

    assert_eq!(ctx.request_type, ServerInitiatedType::CreateMessage);
    assert_eq!(ctx.server_id, "server-001");
    assert!(!ctx.correlation_id.is_empty());
    assert!(ctx.client_capabilities.is_none());
}

#[test]
fn test_server_initiated_with_capabilities() {
    let caps = ClientCapabilities {
        sampling: true,
        roots: true,
        elicitation: false,
        max_concurrent_requests: 10,
        experimental: HashMap::new(),
    };

    let ctx = ServerInitiatedContext::new(ServerInitiatedType::ListRoots, "server-002".to_string())
        .with_capabilities(caps.clone());

    assert!(ctx.client_capabilities.is_some());
    let client_caps = ctx.client_capabilities.unwrap();
    assert!(client_caps.sampling);
    assert!(client_caps.roots);
    assert!(!client_caps.elicitation);
    assert_eq!(client_caps.max_concurrent_requests, 10);
}

#[test]
fn test_bidirectional_context_validation() {
    // Test valid server-initiated request
    let ctx = BidirectionalContext::new(
        CommunicationDirection::ServerToClient,
        CommunicationInitiator::Server,
    )
    .with_request_type("sampling/createMessage".to_string());

    assert!(ctx.validate_direction().is_ok());

    // Test invalid direction for server-initiated request
    let ctx_invalid = BidirectionalContext::new(
        CommunicationDirection::ClientToServer,
        CommunicationInitiator::Client,
    )
    .with_request_type("sampling/createMessage".to_string());

    assert!(ctx_invalid.validate_direction().is_err());

    // Test valid client-initiated request
    let ctx_client = BidirectionalContext::new(
        CommunicationDirection::ClientToServer,
        CommunicationInitiator::Client,
    )
    .with_request_type("tools/call".to_string());

    assert!(ctx_client.validate_direction().is_ok());

    // Test bidirectional ping
    let ctx_ping = BidirectionalContext::new(
        CommunicationDirection::ServerToClient,
        CommunicationInitiator::Server,
    )
    .with_request_type("ping".to_string());

    assert!(ctx_ping.validate_direction().is_ok());

    let ctx_ping2 = BidirectionalContext::new(
        CommunicationDirection::ClientToServer,
        CommunicationInitiator::Client,
    )
    .with_request_type("ping".to_string());

    assert!(ctx_ping2.validate_direction().is_ok());
}

#[test]
fn test_elicitation_with_client_session() {
    use chrono::Utc;

    let session = ClientSession {
        client_id: "session-123".to_string(),
        client_name: Some("Test Client".to_string()),
        connected_at: Utc::now(),
        last_activity: Utc::now(),
        request_count: 5,
        transport_type: "websocket".to_string(),
        authenticated: true,
        capabilities: None,
        metadata: HashMap::new(),
    };

    let ctx = ElicitationContext::new("Enter credentials".to_string(), json!({}))
        .with_client_session(session.clone());

    assert!(ctx.client_session.is_some());
    let client_session = ctx.client_session.unwrap();
    assert_eq!(client_session.client_id, "session-123");
    assert_eq!(client_session.request_count, 5);
}

#[test]
fn test_completion_add_options() {
    let mut ctx = CompletionContext::new(CompletionReference::Prompt {
        name: "code_review".to_string(),
        argument: "file_path".to_string(),
    });

    let option1 = CompletionOption {
        value: "/src/main.rs".to_string(),
        label: Some("Main source file".to_string()),
        completion_type: Some("file".to_string()),
        documentation: Some("The main Rust source file".to_string()),
        sort_priority: Some(1),
        insert_text: None,
    };

    let option2 = CompletionOption {
        value: "/src/lib.rs".to_string(),
        label: Some("Library file".to_string()),
        completion_type: Some("file".to_string()),
        documentation: Some("The library source file".to_string()),
        sort_priority: Some(2),
        insert_text: None,
    };

    ctx.add_completion(option1);
    ctx.add_completion(option2);

    assert_eq!(ctx.completions.len(), 2);
    assert_eq!(ctx.completions[0].value, "/src/main.rs");
    assert_eq!(ctx.completions[1].value, "/src/lib.rs");
}

#[test]
fn test_server_initiated_metadata() {
    let ctx =
        ServerInitiatedContext::new(ServerInitiatedType::Elicitation, "server-003".to_string())
            .with_metadata("priority".to_string(), json!("high"))
            .with_metadata("retry_count".to_string(), json!(2));

    assert_eq!(ctx.metadata.get("priority"), Some(&json!("high")));
    assert_eq!(ctx.metadata.get("retry_count"), Some(&json!(2)));
}

#[test]
fn test_all_server_initiated_types() {
    let types = vec![
        ServerInitiatedType::CreateMessage,
        ServerInitiatedType::ListRoots,
        ServerInitiatedType::Elicitation,
        ServerInitiatedType::Ping,
    ];

    for request_type in types {
        let ctx = ServerInitiatedContext::new(request_type.clone(), "test-server".to_string());

        assert_eq!(ctx.request_type, request_type);
    }
}

#[test]
fn test_communication_directions() {
    let client_to_server = CommunicationDirection::ClientToServer;
    let server_to_client = CommunicationDirection::ServerToClient;

    assert_ne!(client_to_server, server_to_client);

    // Test serialization
    let json_c2s = serde_json::to_string(&client_to_server).unwrap();
    let json_s2c = serde_json::to_string(&server_to_client).unwrap();

    assert!(json_c2s.contains("ClientToServer"));
    assert!(json_s2c.contains("ServerToClient"));
}

#[test]
fn test_bidirectional_correlation_id() {
    let ctx1 = BidirectionalContext::new(
        CommunicationDirection::ServerToClient,
        CommunicationInitiator::Server,
    );

    let ctx2 = BidirectionalContext::new(
        CommunicationDirection::ServerToClient,
        CommunicationInitiator::Server,
    );

    // Each context should have a unique correlation ID
    assert_ne!(ctx1.correlation_id, ctx2.correlation_id);
    assert!(!ctx1.correlation_id.is_empty());
    assert!(!ctx2.correlation_id.is_empty());
}
