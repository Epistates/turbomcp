//! Comprehensive tests for the context module to improve coverage
//!
//! This test suite targets the context module which provides request/response contexts,
//! client management, session tracking, and analytics for MCP applications.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use turbomcp_core::context::*;

// ============================================================================
// RequestContext Core Functionality Tests
// ============================================================================

#[test]
fn test_request_context_new() {
    let ctx = RequestContext::new();

    // Verify basic fields are properly initialized
    assert!(!ctx.request_id.is_empty());
    assert!(ctx.user_id.is_none());
    assert!(ctx.session_id.is_none());
    assert!(ctx.client_id.is_none());
    assert!(ctx.metadata.is_empty());
    assert!(ctx.cancellation_token.is_none());

    // Verify elapsed time is reasonable
    assert!(ctx.elapsed() < Duration::from_millis(100));
}

#[test]
fn test_request_context_default() {
    let ctx = RequestContext::default();
    assert!(!ctx.request_id.is_empty());
    assert!(ctx.user_id.is_none());
}

#[test]
fn test_request_context_with_id() {
    let ctx = RequestContext::with_id("test-123");
    assert_eq!(ctx.request_id, "test-123");
    assert!(ctx.user_id.is_none());
}

#[test]
fn test_request_context_with_user_id() {
    let ctx = RequestContext::new().with_user_id("user-456");
    assert_eq!(ctx.user_id, Some("user-456".to_string()));

    // Test user_id field
    assert_eq!(ctx.user_id.as_deref(), Some("user-456"));
}

#[test]
fn test_request_context_with_session_id() {
    let ctx = RequestContext::new().with_session_id("session-789");
    assert_eq!(ctx.session_id, Some("session-789".to_string()));
}

#[test]
fn test_request_context_with_client_id() {
    let ctx = RequestContext::new().with_client_id("client-abc");
    assert_eq!(ctx.client_id, Some("client-abc".to_string()));
}

#[test]
fn test_request_context_with_metadata() {
    let ctx = RequestContext::new()
        .with_metadata("key1", "value1")
        .with_metadata("key2", 42)
        .with_metadata("key3", true);

    assert_eq!(ctx.get_metadata("key1"), Some(&serde_json::json!("value1")));
    assert_eq!(ctx.get_metadata("key2"), Some(&serde_json::json!(42)));
    assert_eq!(ctx.get_metadata("key3"), Some(&serde_json::json!(true)));
    assert!(ctx.get_metadata("missing").is_none());
}

#[test]
fn test_request_context_with_cancellation_token() {
    let token = Arc::new(CancellationToken::new());
    let mut ctx = RequestContext::new();
    ctx.cancellation_token = Some(token.clone());

    assert!(ctx.cancellation_token.is_some());
    assert!(!ctx.is_cancelled());

    // Cancel the token and verify detection
    token.cancel();
    assert!(ctx.is_cancelled());
}

#[test]
fn test_request_context_is_cancelled_without_token() {
    let ctx = RequestContext::new();
    assert!(!ctx.is_cancelled());
}

#[test]
fn test_request_context_elapsed() {
    let ctx = RequestContext::new();
    std::thread::sleep(Duration::from_millis(10));
    let elapsed = ctx.elapsed();
    assert!(elapsed >= Duration::from_millis(10));
}

#[test]
fn test_request_context_derive() {
    let parent = RequestContext::new()
        .with_user_id("user123")
        .with_session_id("session456")
        .with_client_id("client789")
        .with_metadata("parent_key", "parent_value");

    // Create child context manually (replacing derive() convenience method)
    let mut child = RequestContext::new();
    child.user_id = parent.user_id.clone();
    child.session_id = parent.session_id.clone();
    child.client_id = parent.client_id.clone();
    child.metadata = parent.metadata.clone();
    child.cancellation_token = parent.cancellation_token.clone();

    // Should have new request ID and timestamp
    assert_ne!(parent.request_id, child.request_id);
    assert_ne!(parent.timestamp, child.timestamp);

    // Should inherit user info
    assert_eq!(parent.user_id, child.user_id);
    assert_eq!(parent.session_id, child.session_id);
    assert_eq!(parent.client_id, child.client_id);

    // Should inherit metadata (shared reference)
    assert_eq!(
        parent.get_metadata("parent_key"),
        child.get_metadata("parent_key")
    );

    // Should inherit cancellation token
    let token = Arc::new(CancellationToken::new());
    let mut parent_with_token = parent.clone();
    parent_with_token.cancellation_token = Some(token.clone());

    // Create child context manually (replacing derive() convenience method)
    let mut child_with_token = RequestContext::new();
    child_with_token.user_id = parent_with_token.user_id.clone();
    child_with_token.session_id = parent_with_token.session_id.clone();
    child_with_token.client_id = parent_with_token.client_id.clone();
    child_with_token.metadata = parent_with_token.metadata.clone();
    child_with_token.cancellation_token = parent_with_token.cancellation_token.clone();
    assert!(child_with_token.cancellation_token.is_some());
}

#[test]
fn test_request_context_builder_chain() {
    let ctx = RequestContext::with_id("custom-123")
        .with_user_id("alice")
        .with_session_id("sess-456")
        .with_client_id("client-789")
        .with_metadata("env", "test")
        .with_metadata("version", "1.0");

    assert_eq!(ctx.request_id, "custom-123");
    assert_eq!(ctx.user_id.as_deref(), Some("alice"));
    assert_eq!(ctx.session_id, Some("sess-456".to_string()));
    assert_eq!(ctx.client_id, Some("client-789".to_string()));
    assert_eq!(ctx.get_metadata("env"), Some(&serde_json::json!("test")));
    assert_eq!(ctx.get_metadata("version"), Some(&serde_json::json!("1.0")));
}

// ============================================================================
// Authentication and Authorization Tests
// ============================================================================

#[test]
fn test_request_context_authentication_via_client_id() {
    // Not authenticated by default (no client_id)
    let ctx = RequestContext::new();
    let is_auth = ctx
        .client_id
        .as_ref()
        .and_then(|id| {
            crate::ClientId::Header(id.clone())
                .is_authenticated()
                .then_some(true)
        })
        .unwrap_or(false);
    assert!(!is_auth);

    // Authenticated with Token client ID
    let auth_ctx = RequestContext::new().with_client_id("bearer-token");
    // Simulate TokenClientId authentication check
    let token_client_id = crate::ClientId::Token(auth_ctx.client_id.unwrap());
    assert!(token_client_id.is_authenticated());

    // Not authenticated with Header client ID
    let header_ctx = RequestContext::new().with_client_id("header-id");
    let header_client_id = crate::ClientId::Header(header_ctx.client_id.unwrap());
    assert!(!header_client_id.is_authenticated());

    // Authenticated with Session client ID
    let session_ctx = RequestContext::new().with_client_id("session-id");
    let session_client_id = crate::ClientId::Session(session_ctx.client_id.unwrap());
    assert!(session_client_id.is_authenticated());
}

#[test]
fn test_request_context_roles() {
    // No roles by default
    let ctx = RequestContext::new();
    // Check for roles via metadata access
    let empty_vec = vec![];
    let roles = ctx
        .get_metadata("auth")
        .and_then(|auth| auth.get("roles"))
        .and_then(|roles| roles.as_array())
        .unwrap_or(&empty_vec);
    assert!(roles.is_empty());

    // Roles from auth.roles metadata
    let ctx_with_roles = RequestContext::new().with_metadata(
        "auth",
        serde_json::json!({
            "roles": ["admin", "user", "moderator"]
        }),
    );
    let roles = ctx_with_roles
        .get_metadata("auth")
        .and_then(|auth| auth.get("roles"))
        .and_then(|roles| roles.as_array())
        .unwrap();
    assert_eq!(roles.len(), 3);
    assert!(roles.contains(&serde_json::Value::String("admin".to_string())));
    assert!(roles.contains(&serde_json::Value::String("user".to_string())));
    assert!(roles.contains(&serde_json::Value::String("moderator".to_string())));

    // Empty roles array
    let ctx_empty_roles = RequestContext::new().with_metadata(
        "auth",
        serde_json::json!({
            "roles": []
        }),
    );
    let empty_roles = ctx_empty_roles
        .get_metadata("auth")
        .and_then(|auth| auth.get("roles"))
        .and_then(|roles| roles.as_array())
        .unwrap();
    assert!(empty_roles.is_empty());

    // Invalid roles structure
    let ctx_invalid = RequestContext::new().with_metadata(
        "auth",
        serde_json::json!({
            "roles": "not-an-array"
        }),
    );
    let invalid_roles = ctx_invalid
        .get_metadata("auth")
        .and_then(|auth| auth.get("roles"))
        .and_then(|roles| roles.as_array());
    assert!(invalid_roles.is_none()); // Invalid structure returns None

    // Mixed types in roles array
    let ctx_mixed = RequestContext::new().with_metadata(
        "auth",
        serde_json::json!({
            "roles": ["admin", 123, null, "user"]
        }),
    );
    let mixed_roles = ctx_mixed
        .get_metadata("auth")
        .and_then(|auth| auth.get("roles"))
        .and_then(|roles| roles.as_array())
        .unwrap();
    assert_eq!(mixed_roles.len(), 4); // All elements present
    assert!(mixed_roles.contains(&serde_json::Value::String("admin".to_string())));
    assert!(mixed_roles.contains(&serde_json::Value::String("user".to_string())));
}

#[test]
fn test_request_context_role_checking_via_metadata() {
    let ctx = RequestContext::new().with_metadata(
        "auth",
        serde_json::json!({
            "roles": ["editor", "viewer"]
        }),
    );

    // Helper function to check if user has any required role
    let has_any_role = |ctx: &RequestContext, required_roles: &[&str]| {
        if required_roles.is_empty() {
            return true; // Empty requirement always passes
        }

        let empty_vec = vec![];
        let user_roles = ctx
            .get_metadata("auth")
            .and_then(|auth| auth.get("roles"))
            .and_then(|roles| roles.as_array())
            .unwrap_or(&empty_vec);

        required_roles
            .iter()
            .any(|required| user_roles.contains(&serde_json::Value::String(required.to_string())))
    };

    // Should return true if user has any required role
    assert!(has_any_role(&ctx, &["admin", "editor"]));
    assert!(has_any_role(&ctx, &["viewer"]));
    assert!(has_any_role(&ctx, &["editor", "viewer"]));

    // Should return false if user has none of the required roles
    assert!(!has_any_role(&ctx, &["admin", "superuser"]));

    // Should return true if no roles are required (empty requirement)
    assert!(has_any_role(&ctx, &[] as &[&str]));

    // User with no roles
    let no_role_ctx = RequestContext::new();
    assert!(!has_any_role(&no_role_ctx, &["admin"]));
    assert!(has_any_role(&no_role_ctx, &[] as &[&str])); // Empty requirement always passes
}

// ============================================================================
// ResponseContext Tests
// ============================================================================

#[test]
fn test_response_context_success() {
    let duration = Duration::from_millis(150);
    let ctx = ResponseContext::success("req-123", duration);

    assert_eq!(ctx.request_id, "req-123");
    assert_eq!(ctx.duration, duration);
    // Check status directly instead of convenience methods
    assert_eq!(ctx.status, ResponseStatus::Success);
    match &ctx.status {
        ResponseStatus::Success => {}
        ResponseStatus::Error { .. } => panic!("Expected success status"),
        _ => panic!("Unexpected status"),
    }
    assert!(ctx.metadata.is_empty());
}

#[test]
fn test_response_context_error() {
    let duration = Duration::from_millis(75);
    let ctx = ResponseContext::error("req-456", duration, -32600, "Invalid Request");

    assert_eq!(ctx.request_id, "req-456");
    assert_eq!(ctx.duration, duration);
    // Check status directly instead of convenience methods
    match &ctx.status {
        ResponseStatus::Success => panic!("Expected error status"),
        ResponseStatus::Error { code, message } => {
            assert_eq!(*code, -32600);
            assert_eq!(message, "Invalid Request");
        }
        _ => panic!("Unexpected status"),
    }
}

#[test]
fn test_response_context_cancelled() {
    let duration = Duration::from_millis(200);
    // Create cancelled response context manually
    let mut ctx = ResponseContext::success("req-789", duration);
    ctx.status = ResponseStatus::Cancelled;

    assert_eq!(ctx.request_id, "req-789");
    assert_eq!(ctx.duration, duration);
    // Check status directly instead of convenience methods
    assert_eq!(ctx.status, ResponseStatus::Cancelled);
    match &ctx.status {
        ResponseStatus::Cancelled => {}
        _ => panic!("Expected cancelled status"),
    }

    // Test status directly
    assert!(matches!(ctx.status, ResponseStatus::Cancelled));
}

#[test]
fn test_response_context_with_metadata() {
    let mut ctx = ResponseContext::success("req-123", Duration::from_millis(100));
    // Add metadata manually since with_metadata doesn't exist
    std::sync::Arc::make_mut(&mut ctx.metadata)
        .insert("cache_hit".to_string(), serde_json::json!(true));
    std::sync::Arc::make_mut(&mut ctx.metadata)
        .insert("processing_time".to_string(), serde_json::json!(95));

    assert_eq!(
        ctx.metadata.get("cache_hit"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(
        ctx.metadata.get("processing_time"),
        Some(&serde_json::json!(95))
    );
}

// ============================================================================
// ResponseStatus Tests
// ============================================================================

// Removed test_response_status_display() - Display implementation not needed for production code

#[test]
fn test_response_status_equality() {
    assert_eq!(ResponseStatus::Success, ResponseStatus::Success);
    assert_eq!(ResponseStatus::Partial, ResponseStatus::Partial);
    assert_eq!(ResponseStatus::Cancelled, ResponseStatus::Cancelled);

    let error1 = ResponseStatus::Error {
        code: -32603,
        message: "Error".to_string(),
    };
    let error2 = ResponseStatus::Error {
        code: -32603,
        message: "Error".to_string(),
    };
    let error3 = ResponseStatus::Error {
        code: -32604,
        message: "Error".to_string(),
    };

    assert_eq!(error1, error2);
    assert_ne!(error1, error3);
    assert_ne!(ResponseStatus::Success, ResponseStatus::Cancelled);
}

#[test]
fn test_response_status_serialization() {
    // Test that ResponseStatus can be serialized/deserialized
    let statuses = vec![
        ResponseStatus::Success,
        ResponseStatus::Partial,
        ResponseStatus::Cancelled,
        ResponseStatus::Error {
            code: -32600,
            message: "Invalid Request".to_string(),
        },
    ];

    for status in statuses {
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: ResponseStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);
    }
}

// ============================================================================
// ClientId Tests
// ============================================================================

#[test]
fn test_client_id_as_str() {
    assert_eq!(ClientId::Header("test".to_string()).as_str(), "test");
    assert_eq!(ClientId::Token("token".to_string()).as_str(), "token");
    assert_eq!(ClientId::Session("sess".to_string()).as_str(), "sess");
    assert_eq!(ClientId::QueryParam("param".to_string()).as_str(), "param");
    assert_eq!(ClientId::UserAgent("ua".to_string()).as_str(), "ua");
    assert_eq!(ClientId::Anonymous.as_str(), "anonymous");
}

#[test]
fn test_client_id_is_authenticated() {
    assert!(!ClientId::Header("test".to_string()).is_authenticated());
    assert!(ClientId::Token("token".to_string()).is_authenticated());
    assert!(ClientId::Session("sess".to_string()).is_authenticated());
    assert!(!ClientId::QueryParam("param".to_string()).is_authenticated());
    assert!(!ClientId::UserAgent("ua".to_string()).is_authenticated());
    assert!(!ClientId::Anonymous.is_authenticated());
}

#[test]
fn test_client_id_auth_method() {
    assert_eq!(ClientId::Header("test".to_string()).auth_method(), "header");
    assert_eq!(
        ClientId::Token("token".to_string()).auth_method(),
        "bearer_token"
    );
    assert_eq!(
        ClientId::Session("sess".to_string()).auth_method(),
        "session_cookie"
    );
    assert_eq!(
        ClientId::QueryParam("param".to_string()).auth_method(),
        "query_param"
    );
    assert_eq!(
        ClientId::UserAgent("ua".to_string()).auth_method(),
        "user_agent"
    );
    assert_eq!(ClientId::Anonymous.auth_method(), "anonymous");
}

#[test]
fn test_client_id_equality() {
    assert_eq!(
        ClientId::Header("test".to_string()),
        ClientId::Header("test".to_string())
    );
    assert_ne!(
        ClientId::Header("test1".to_string()),
        ClientId::Header("test2".to_string())
    );
    assert_ne!(
        ClientId::Header("test".to_string()),
        ClientId::Token("test".to_string())
    );
    assert_eq!(ClientId::Anonymous, ClientId::Anonymous);
}

#[test]
fn test_client_id_serialization() {
    let client_ids = vec![
        ClientId::Header("header-id".to_string()),
        ClientId::Token("token-123".to_string()),
        ClientId::Session("session-456".to_string()),
        ClientId::QueryParam("param-789".to_string()),
        ClientId::UserAgent("ua-abc".to_string()),
        ClientId::Anonymous,
    ];

    for client_id in client_ids {
        let json = serde_json::to_string(&client_id).unwrap();
        let deserialized: ClientId = serde_json::from_str(&json).unwrap();
        assert_eq!(client_id, deserialized);
    }
}

// ============================================================================
// ClientSession Tests
// ============================================================================

#[test]
fn test_client_session_new() {
    let session = ClientSession::new("client-123".to_string(), "websocket".to_string());

    assert_eq!(session.client_id, "client-123");
    assert_eq!(session.transport_type, "websocket");
    assert!(session.client_name.is_none());
    assert!(!session.authenticated);
    assert_eq!(session.request_count, 0);
    assert!(session.capabilities.is_none());
    assert!(session.metadata.is_empty());
}

#[test]
fn test_client_session_update_activity() {
    let mut session = ClientSession::new("client-123".to_string(), "http".to_string());
    let initial_count = session.request_count;
    let initial_activity = session.last_activity;

    std::thread::sleep(Duration::from_millis(10));
    session.update_activity();

    assert_eq!(session.request_count, initial_count + 1);
    assert!(session.last_activity > initial_activity);
}

#[test]
fn test_client_session_authenticate() {
    let mut session = ClientSession::new("client-123".to_string(), "http".to_string());
    assert!(!session.authenticated);
    assert!(session.client_name.is_none());

    session.authenticate(Some("Test Client".to_string()));
    assert!(session.authenticated);
    assert_eq!(session.client_name, Some("Test Client".to_string()));

    // Test without client name
    let mut session2 = ClientSession::new("client-456".to_string(), "stdio".to_string());
    session2.authenticate(None);
    assert!(session2.authenticated);
    assert!(session2.client_name.is_none());
}

#[test]
fn test_client_session_set_capabilities() {
    let mut session = ClientSession::new("client-123".to_string(), "http".to_string());
    assert!(session.capabilities.is_none());

    let caps = serde_json::json!({
        "tools": {"listChanged": true},
        "resources": {"subscribe": true}
    });

    session.set_capabilities(caps.clone());
    assert_eq!(session.capabilities, Some(caps));
}

#[test]
fn test_client_session_session_duration() {
    let mut session = ClientSession::new("client-123".to_string(), "http".to_string());

    std::thread::sleep(Duration::from_millis(50));
    session.update_activity();

    let duration = session.session_duration();
    assert!(duration.num_milliseconds() >= 50);
}

#[test]
fn test_client_session_is_idle() {
    let session = ClientSession::new("client-123".to_string(), "http".to_string());

    // Should not be idle immediately after creation
    assert!(!session.is_idle(chrono::Duration::seconds(1)));

    // Simulate old session by creating a session with past timestamps
    let old_time = chrono::Utc::now() - chrono::Duration::hours(2);
    let mut old_session = ClientSession {
        client_id: "client-old".to_string(),
        client_name: None,
        connected_at: old_time,
        last_activity: old_time,
        request_count: 5,
        transport_type: "http".to_string(),
        authenticated: false,
        capabilities: None,
        metadata: HashMap::new(),
    };

    // Should be idle with 1 hour threshold
    assert!(old_session.is_idle(chrono::Duration::hours(1)));

    // Should not be idle after updating activity
    old_session.update_activity();
    assert!(!old_session.is_idle(chrono::Duration::hours(1)));
}

// ============================================================================
// RequestInfo Tests
// ============================================================================

#[test]
fn test_request_info_new() {
    let params = serde_json::json!({"key": "value"});
    let info = RequestInfo::new(
        "client-123".to_string(),
        "test_method".to_string(),
        params.clone(),
    );

    assert_eq!(info.client_id, "client-123");
    assert_eq!(info.method_name, "test_method");
    assert_eq!(info.parameters, params);
    assert!(!info.success);
    assert!(info.response_time_ms.is_none());
    assert!(info.error_message.is_none());
    assert!(info.status_code.is_none());
    assert!(info.metadata.is_empty());
}

#[test]
fn test_request_info_complete_success() {
    let params = serde_json::json!({"param": "value"});
    let info = RequestInfo::new("client-123".to_string(), "method".to_string(), params);

    let completed = info.complete_success(250);

    assert!(completed.success);
    assert_eq!(completed.response_time_ms, Some(250));
    assert_eq!(completed.status_code, Some(200));
    assert!(completed.error_message.is_none());
}

#[test]
fn test_request_info_complete_error() {
    let params = serde_json::json!({});
    let info = RequestInfo::new("client-456".to_string(), "method".to_string(), params);

    let completed = info.complete_error(150, "Something went wrong".to_string());

    assert!(!completed.success);
    assert_eq!(completed.response_time_ms, Some(150));
    assert_eq!(completed.status_code, Some(500));
    assert_eq!(
        completed.error_message,
        Some("Something went wrong".to_string())
    );
}

#[test]
fn test_request_info_with_status_code() {
    let params = serde_json::json!({});
    let info = RequestInfo::new("client-789".to_string(), "method".to_string(), params)
        .with_status_code(404);

    assert_eq!(info.status_code, Some(404));
}

#[test]
fn test_request_info_with_metadata() {
    let params = serde_json::json!({});
    let info = RequestInfo::new("client-abc".to_string(), "method".to_string(), params)
        .with_metadata("custom_key".to_string(), serde_json::json!("custom_value"))
        .with_metadata("cache_hit".to_string(), serde_json::json!(true));

    assert_eq!(
        info.metadata.get("custom_key"),
        Some(&serde_json::json!("custom_value"))
    );
    assert_eq!(
        info.metadata.get("cache_hit"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn test_request_info_chaining() {
    let params = serde_json::json!({"input": "test"});
    let info = RequestInfo::new(
        "client-def".to_string(),
        "chained_method".to_string(),
        params,
    )
    .with_status_code(201)
    .with_metadata("version".to_string(), serde_json::json!("1.2.3"))
    .complete_success(100)
    .with_metadata("processed".to_string(), serde_json::json!(true));

    assert!(info.success);
    assert_eq!(info.response_time_ms, Some(100));
    assert_eq!(info.status_code, Some(200)); // complete_success overwrites status
    assert_eq!(
        info.metadata.get("version"),
        Some(&serde_json::json!("1.2.3"))
    );
    assert_eq!(
        info.metadata.get("processed"),
        Some(&serde_json::json!(true))
    );
}

// ============================================================================
// ClientIdExtractor Tests
// ============================================================================

#[test]
fn test_client_id_extractor_new() {
    let extractor = ClientIdExtractor::new();
    assert_eq!(extractor.list_tokens().len(), 0);
}

#[test]
fn test_client_id_extractor_default() {
    let extractor = ClientIdExtractor::default();
    assert_eq!(extractor.list_tokens().len(), 0);
}

#[test]
fn test_client_id_extractor_register_revoke_tokens() {
    let extractor = ClientIdExtractor::new();

    extractor.register_token("token123".to_string(), "client1".to_string());
    extractor.register_token("token456".to_string(), "client2".to_string());

    let tokens = extractor.list_tokens();
    assert_eq!(tokens.len(), 2);
    assert!(tokens.contains(&("token123".to_string(), "client1".to_string())));
    assert!(tokens.contains(&("token456".to_string(), "client2".to_string())));

    extractor.revoke_token("token123");
    let remaining_tokens = extractor.list_tokens();
    assert_eq!(remaining_tokens.len(), 1);
    assert!(remaining_tokens.contains(&("token456".to_string(), "client2".to_string())));
}

#[test]
fn test_client_id_extractor_header_extraction() {
    let extractor = ClientIdExtractor::new();

    // Test explicit client ID header
    let mut headers = HashMap::new();
    headers.insert("x-client-id".to_string(), "explicit-client".to_string());

    let client_id = extractor.extract_from_http_headers(&headers);
    assert_eq!(client_id, ClientId::Header("explicit-client".to_string()));
}

#[test]
fn test_client_id_extractor_bearer_token() {
    let extractor = ClientIdExtractor::new();
    extractor.register_token("known-token".to_string(), "mapped-client".to_string());

    // Test registered token
    let mut headers = HashMap::new();
    headers.insert(
        "authorization".to_string(),
        "Bearer known-token".to_string(),
    );

    let client_id = extractor.extract_from_http_headers(&headers);
    assert_eq!(client_id, ClientId::Token("mapped-client".to_string()));

    // Test unregistered token
    headers.insert(
        "authorization".to_string(),
        "Bearer unknown-token".to_string(),
    );
    let unknown_client_id = extractor.extract_from_http_headers(&headers);
    assert_eq!(
        unknown_client_id,
        ClientId::Token("unknown-token".to_string())
    );

    // Test malformed authorization header
    headers.insert("authorization".to_string(), "Basic user:pass".to_string());
    let malformed_client_id = extractor.extract_from_http_headers(&headers);
    // Should fall back to anonymous since no other headers
    assert_eq!(malformed_client_id, ClientId::Anonymous);
}

#[test]
fn test_client_id_extractor_session_cookie() {
    let extractor = ClientIdExtractor::new();

    // Test session_id cookie
    let mut headers = HashMap::new();
    headers.insert(
        "cookie".to_string(),
        "session_id=sess123; other=value".to_string(),
    );

    let client_id = extractor.extract_from_http_headers(&headers);
    assert_eq!(client_id, ClientId::Session("sess123".to_string()));

    // Test sessionid cookie (alternative name)
    headers.insert(
        "cookie".to_string(),
        "sessionid=sess456; other=value".to_string(),
    );
    let client_id2 = extractor.extract_from_http_headers(&headers);
    assert_eq!(client_id2, ClientId::Session("sess456".to_string()));

    // Test multiple cookies
    headers.insert(
        "cookie".to_string(),
        "first=1; session_id=sess789; last=end".to_string(),
    );
    let client_id3 = extractor.extract_from_http_headers(&headers);
    assert_eq!(client_id3, ClientId::Session("sess789".to_string()));

    // Test malformed cookie
    headers.insert(
        "cookie".to_string(),
        "malformed_cookie_no_equals".to_string(),
    );
    let malformed_client_id = extractor.extract_from_http_headers(&headers);
    assert_eq!(malformed_client_id, ClientId::Anonymous);
}

#[test]
fn test_client_id_extractor_user_agent_fallback() {
    let extractor = ClientIdExtractor::new();

    let mut headers = HashMap::new();
    headers.insert("user-agent".to_string(), "TestAgent/1.0 (OS)".to_string());

    let client_id = extractor.extract_from_http_headers(&headers);
    if let ClientId::UserAgent(ref id) = client_id {
        assert!(id.starts_with("ua_"));
        assert_eq!(id.len(), 19); // "ua_" + 16 hex chars
    } else {
        panic!("Expected UserAgent ClientId, got {client_id:?}");
    }

    // Same user agent should produce same ID
    let client_id2 = extractor.extract_from_http_headers(&headers);
    assert_eq!(client_id, client_id2);
}

#[test]
fn test_client_id_extractor_anonymous_fallback() {
    let extractor = ClientIdExtractor::new();

    // Empty headers should return anonymous
    let headers = HashMap::new();
    let client_id = extractor.extract_from_http_headers(&headers);
    assert_eq!(client_id, ClientId::Anonymous);
}

#[test]
fn test_client_id_extractor_header_priority() {
    let extractor = ClientIdExtractor::new();
    extractor.register_token("token123".to_string(), "token-client".to_string());

    let mut headers = HashMap::new();
    // Add multiple possible sources - x-client-id should have highest priority
    headers.insert("x-client-id".to_string(), "explicit-client".to_string());
    headers.insert("authorization".to_string(), "Bearer token123".to_string());
    headers.insert("cookie".to_string(), "session_id=sess123".to_string());
    headers.insert("user-agent".to_string(), "TestAgent/1.0".to_string());

    let client_id = extractor.extract_from_http_headers(&headers);
    assert_eq!(client_id, ClientId::Header("explicit-client".to_string()));
}

#[test]
fn test_client_id_extractor_query_params() {
    let extractor = ClientIdExtractor::new();

    let mut params = HashMap::new();
    params.insert("client_id".to_string(), "query-client".to_string());

    let client_id = extractor.extract_from_query(&params);
    assert_eq!(
        client_id,
        Some(ClientId::QueryParam("query-client".to_string()))
    );

    // No client_id parameter
    let empty_params = HashMap::new();
    let no_client_id = extractor.extract_from_query(&empty_params);
    assert_eq!(no_client_id, None);
}

#[test]
fn test_client_id_extractor_extract_client_id_priority() {
    let extractor = ClientIdExtractor::new();

    let mut headers = HashMap::new();
    headers.insert("x-client-id".to_string(), "header-client".to_string());

    let mut query_params = HashMap::new();
    query_params.insert("client_id".to_string(), "query-client".to_string());

    // Query params should have higher priority
    let client_id = extractor.extract_client_id(Some(&headers), Some(&query_params));
    assert_eq!(client_id, ClientId::QueryParam("query-client".to_string()));

    // Headers only
    let client_id2 = extractor.extract_client_id(Some(&headers), None);
    assert_eq!(client_id2, ClientId::Header("header-client".to_string()));

    // Neither provided
    let client_id3 = extractor.extract_client_id(None, None);
    assert_eq!(client_id3, ClientId::Anonymous);
}

// ============================================================================
// RequestContextExt Tests
// ============================================================================

#[test]
fn test_request_context_ext_with_enhanced_client_id() {
    let client_id = ClientId::Token("bearer-token-123".to_string());
    let ctx = RequestContext::new().with_enhanced_client_id(client_id);

    assert_eq!(ctx.client_id, Some("bearer-token-123".to_string()));
    assert_eq!(
        ctx.get_metadata("client_id_method"),
        Some(&serde_json::json!("bearer_token"))
    );
    assert_eq!(
        ctx.get_metadata("client_authenticated"),
        Some(&serde_json::json!(true))
    );
}

#[test]
fn test_request_context_ext_extract_client_id() {
    let extractor = ClientIdExtractor::new();

    let mut headers = HashMap::new();
    headers.insert("x-client-id".to_string(), "header-client-123".to_string());

    let ctx = RequestContext::new().extract_client_id(&extractor, Some(&headers), None);

    assert_eq!(ctx.client_id, Some("header-client-123".to_string()));
    assert_eq!(
        ctx.get_metadata("client_id_method"),
        Some(&serde_json::json!("header"))
    );
    assert_eq!(
        ctx.get_metadata("client_authenticated"),
        Some(&serde_json::json!(false))
    );
}

#[test]
fn test_request_context_ext_get_enhanced_client_id() {
    // Test all client ID methods
    let test_cases = vec![
        ("header", ClientId::Header("test".to_string())),
        ("bearer_token", ClientId::Token("test".to_string())),
        ("session_cookie", ClientId::Session("test".to_string())),
        ("query_param", ClientId::QueryParam("test".to_string())),
        ("user_agent", ClientId::UserAgent("test".to_string())),
        ("anonymous", ClientId::Anonymous),
        ("unknown_method", ClientId::Header("test".to_string())), // fallback
    ];

    for (method, expected) in test_cases {
        let ctx = RequestContext::new()
            .with_client_id("test")
            .with_metadata("client_id_method", method);

        let enhanced_id = ctx.get_enhanced_client_id();
        if method == "anonymous" {
            assert_eq!(enhanced_id, Some(ClientId::Anonymous));
        } else {
            assert_eq!(enhanced_id, Some(expected));
        }
    }

    // Test context without client ID
    let empty_ctx = RequestContext::new();
    assert_eq!(empty_ctx.get_enhanced_client_id(), None);
}

// ============================================================================
// Integration and Complex Scenarios Tests
// ============================================================================

#[test]
fn test_context_integration_full_workflow() {
    let extractor = ClientIdExtractor::new();
    extractor.register_token(
        "api-token-456".to_string(),
        "client-production-001".to_string(),
    );

    let mut headers = HashMap::new();
    headers.insert(
        "authorization".to_string(),
        "Bearer api-token-456".to_string(),
    );
    headers.insert("user-agent".to_string(), "MyApp/2.1 (Linux)".to_string());

    // Create request context
    let request_ctx = RequestContext::with_id("req-workflow-001")
        .with_user_id("alice")
        .extract_client_id(&extractor, Some(&headers), None)
        .with_metadata("authenticated", true)
        .with_metadata("auth", serde_json::json!({"roles": ["admin", "user"]}))
        .with_metadata("api_version", "v2");

    // Verify the context is properly set up
    assert_eq!(request_ctx.request_id, "req-workflow-001");
    assert_eq!(request_ctx.user_id.as_deref(), Some("alice"));
    assert_eq!(
        request_ctx.client_id,
        Some("client-production-001".to_string())
    );
    // Check authentication via enhanced client ID
    let enhanced_client_id = request_ctx.get_enhanced_client_id().unwrap();
    assert!(enhanced_client_id.is_authenticated());
    // Check roles via metadata
    let auth_metadata = request_ctx.get_metadata("auth").unwrap();
    let roles = auth_metadata.get("roles").unwrap().as_array().unwrap();
    assert!(roles.contains(&serde_json::Value::String("admin".to_string())));
    assert_eq!(
        request_ctx.get_enhanced_client_id(),
        Some(ClientId::Token("client-production-001".to_string()))
    );

    // Simulate request processing and create response
    std::thread::sleep(Duration::from_millis(25));
    let processing_duration = request_ctx.elapsed();

    let mut response_ctx = ResponseContext::success(&request_ctx.request_id, processing_duration);
    // Add metadata manually
    std::sync::Arc::make_mut(&mut response_ctx.metadata)
        .insert("cache_hit".to_string(), serde_json::json!(false));
    std::sync::Arc::make_mut(&mut response_ctx.metadata).insert(
        "processed_by".to_string(),
        serde_json::json!("server-node-3"),
    );

    assert_eq!(response_ctx.request_id, request_ctx.request_id);
    assert_eq!(response_ctx.status, ResponseStatus::Success);
    assert!(response_ctx.duration >= Duration::from_millis(25));

    // Create request analytics
    let request_info = RequestInfo::new(
        request_ctx.client_id.unwrap(),
        "user.profile.get".to_string(),
        serde_json::json!({"user_id": "alice"}),
    )
    .complete_success(response_ctx.duration.as_millis() as u64)
    .with_metadata("user_tier".to_string(), serde_json::json!("premium"));

    assert!(request_info.success);
    assert_eq!(request_info.client_id, "client-production-001");
    assert_eq!(request_info.method_name, "user.profile.get");
}

#[test]
fn test_context_error_handling_workflow() {
    let request_ctx = RequestContext::new()
        .with_user_id("bob")
        .with_metadata("authenticated", true)
        .with_metadata("auth", serde_json::json!({"roles": ["viewer"]}));

    // Simulate request that fails due to insufficient permissions
    // Check if user has admin or editor role via metadata
    let user_roles = request_ctx
        .get_metadata("auth")
        .and_then(|auth| auth.get("roles"))
        .and_then(|roles| roles.as_array())
        .unwrap();
    let required_roles = &["admin", "editor"];
    let has_required_role = required_roles
        .iter()
        .any(|required| user_roles.contains(&serde_json::Value::String(required.to_string())));
    assert!(!has_required_role); // User should NOT have admin/editor role - this tests insufficient permissions

    let error_response = ResponseContext::error(
        &request_ctx.request_id,
        Duration::from_millis(10),
        -32000,
        "Insufficient permissions",
    );

    // Check status directly instead of convenience methods
    match &error_response.status {
        ResponseStatus::Success => panic!("Expected error status"),
        ResponseStatus::Error { code, message } => {
            assert_eq!(*code, -32000);
            assert_eq!(message, "Insufficient permissions");
        }
        _ => panic!("Unexpected status"),
    }

    let error_info = RequestInfo::new(
        request_ctx.client_id.unwrap_or("anonymous".to_string()),
        "admin.users.delete".to_string(),
        serde_json::json!({"user_id": "target-user"}),
    )
    .complete_error(10, "Access denied".to_string());

    assert!(!error_info.success);
    assert_eq!(error_info.error_message, Some("Access denied".to_string()));
}

#[test]
fn test_context_cancellation_workflow() {
    let cancellation_token = Arc::new(CancellationToken::new());

    let mut request_ctx = RequestContext::new().with_user_id("charlie");
    request_ctx.cancellation_token = Some(cancellation_token.clone());

    assert!(!request_ctx.is_cancelled());

    // Simulate cancellation during processing
    cancellation_token.cancel();
    assert!(request_ctx.is_cancelled());

    // Create cancelled response context manually
    let mut cancelled_response =
        ResponseContext::success(request_ctx.request_id.clone(), Duration::from_millis(50));
    cancelled_response.status = ResponseStatus::Cancelled;

    assert_eq!(cancelled_response.status, ResponseStatus::Cancelled);
    assert!(matches!(
        cancelled_response.status,
        ResponseStatus::Cancelled
    ));
}

#[test]
fn test_client_session_management_workflow() {
    let mut session =
        ClientSession::new("client-long-running".to_string(), "websocket".to_string());

    // Initial state
    assert!(!session.authenticated);
    assert_eq!(session.request_count, 0);

    // Authenticate the client
    session.authenticate(Some("WebSocket Client v1.2".to_string()));
    assert!(session.authenticated);

    // Set capabilities
    let capabilities = serde_json::json!({
        "tools": {"listChanged": true, "callTool": true},
        "resources": {"subscribe": true, "listChanged": true},
        "prompts": {"listChanged": false}
    });
    session.set_capabilities(capabilities.clone());
    assert_eq!(session.capabilities, Some(capabilities));

    // Simulate multiple requests
    for _ in 0..5 {
        session.update_activity();
    }
    assert_eq!(session.request_count, 5);

    // Check session duration and idle status
    let duration = session.session_duration();
    assert!(duration.num_milliseconds() >= 0);
    assert!(!session.is_idle(chrono::Duration::minutes(5)));
}

#[test]
fn test_metadata_sharing_between_contexts() {
    // Create parent context with shared metadata
    let parent_ctx = RequestContext::new()
        .with_metadata("shared_key", "shared_value")
        .with_metadata("request_metadata", "parent_specific");

    // Create child context manually (replacing derive() convenience method)
    let mut child_ctx = RequestContext::new();
    child_ctx.user_id = parent_ctx.user_id.clone();
    child_ctx.session_id = parent_ctx.session_id.clone();
    child_ctx.client_id = parent_ctx.client_id.clone();
    child_ctx.metadata = parent_ctx.metadata.clone();
    child_ctx.cancellation_token = parent_ctx.cancellation_token.clone();

    // Both should see the shared metadata (Arc sharing)
    assert_eq!(
        parent_ctx.get_metadata("shared_key"),
        child_ctx.get_metadata("shared_key")
    );

    // Modify metadata in child - this should create a new copy due to Arc::make_mut
    let modified_child = child_ctx.with_metadata("child_specific", "child_value");

    // Child should have both shared and its own metadata
    assert_eq!(
        modified_child.get_metadata("shared_key"),
        Some(&serde_json::json!("shared_value"))
    );
    assert_eq!(
        modified_child.get_metadata("child_specific"),
        Some(&serde_json::json!("child_value"))
    );

    // Parent should not have child-specific metadata
    assert!(parent_ctx.get_metadata("child_specific").is_none());
}
