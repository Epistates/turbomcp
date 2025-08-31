//! Tests for enhanced session management with elicitations and completions

use chrono::Duration;
use serde_json::json;
use std::time::Duration as StdDuration;
use turbomcp_core::context::{
    CompletionContext, CompletionReference, ElicitationContext, ElicitationState,
};
use turbomcp_core::session::{SessionConfig, SessionManager};

#[test]
fn test_session_manager_with_elicitations() {
    let config = SessionConfig {
        max_sessions: 10,
        session_timeout: Duration::hours(1),
        max_request_history: 100,
        max_requests_per_session: Some(1000),
        cleanup_interval: StdDuration::from_secs(60),
        enable_analytics: true,
    };

    let manager = SessionManager::new(config);

    // Create a session
    let session = manager.get_or_create_session("client-001".to_string(), "websocket".to_string());
    assert_eq!(session.client_id, "client-001");

    // Add elicitation
    let elicitation = ElicitationContext::new(
        "Please provide your API key".to_string(),
        json!({
            "type": "object",
            "properties": {
                "api_key": {"type": "string"}
            },
            "required": ["api_key"]
        }),
    );

    manager.add_pending_elicitation("client-001".to_string(), elicitation.clone());

    // Check pending elicitations
    let pending = manager.get_pending_elicitations("client-001");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].message, "Please provide your API key");
    assert_eq!(pending[0].state, ElicitationState::Pending);
}

#[test]
fn test_elicitation_state_management() {
    let manager = SessionManager::new(SessionConfig::default());

    // Create elicitation
    let elicitation =
        ElicitationContext::new("Enter username".to_string(), json!({"type": "string"}));
    let elicitation_id = elicitation.elicitation_id.clone();

    // Add elicitation
    manager.add_pending_elicitation("client-002".to_string(), elicitation);

    // Update state to accepted
    let updated =
        manager.update_elicitation_state("client-002", &elicitation_id, ElicitationState::Accepted);
    assert!(updated);

    // Verify state was updated
    let pending = manager.get_pending_elicitations("client-002");
    assert_eq!(pending[0].state, ElicitationState::Accepted);
    assert!(pending[0].is_complete());

    // Remove completed elicitations
    manager.remove_completed_elicitations("client-002");
    let pending_after = manager.get_pending_elicitations("client-002");
    assert_eq!(pending_after.len(), 0);
}

#[test]
fn test_completion_management() {
    let manager = SessionManager::new(SessionConfig::default());

    // Create completion context
    let completion = CompletionContext::new(CompletionReference::Tool {
        name: "file_search".to_string(),
        argument: "path".to_string(),
    });
    let completion_id = completion.completion_id.clone();

    // Add completion
    manager.add_active_completion("client-003".to_string(), completion);

    // Check active completions
    let active = manager.get_active_completions("client-003");
    assert_eq!(active.len(), 1);

    // Remove specific completion
    let removed = manager.remove_completion("client-003", &completion_id);
    assert!(removed);

    // Verify removed
    let active_after = manager.get_active_completions("client-003");
    assert_eq!(active_after.len(), 0);
}

#[test]
fn test_session_termination_clears_contexts() {
    let manager = SessionManager::new(SessionConfig::default());

    // Create session and add contexts
    let _ = manager.get_or_create_session("client-004".to_string(), "http".to_string());

    // Add elicitation
    let elicitation = ElicitationContext::new("Test".to_string(), json!({}));
    manager.add_pending_elicitation("client-004".to_string(), elicitation);

    // Add completion
    let completion = CompletionContext::new(CompletionReference::Prompt {
        name: "test".to_string(),
        argument: "arg".to_string(),
    });
    manager.add_active_completion("client-004".to_string(), completion);

    // Verify contexts exist
    assert_eq!(manager.get_pending_elicitations("client-004").len(), 1);
    assert_eq!(manager.get_active_completions("client-004").len(), 1);

    // Terminate session
    let terminated = manager.terminate_session("client-004");
    assert!(terminated);

    // Verify contexts are cleared
    assert_eq!(manager.get_pending_elicitations("client-004").len(), 0);
    assert_eq!(manager.get_active_completions("client-004").len(), 0);
}

#[test]
fn test_multiple_elicitations_per_client() {
    let manager = SessionManager::new(SessionConfig::default());

    // Add multiple elicitations
    for i in 0..5 {
        let elicitation = ElicitationContext::new(format!("Question {}", i), json!({"index": i}));
        manager.add_pending_elicitation("client-005".to_string(), elicitation);
    }

    // Check all are present
    let pending = manager.get_pending_elicitations("client-005");
    assert_eq!(pending.len(), 5);

    // Clear all elicitations
    manager.clear_elicitations("client-005");
    let pending_after = manager.get_pending_elicitations("client-005");
    assert_eq!(pending_after.len(), 0);
}

#[test]
fn test_enhanced_analytics() {
    let manager = SessionManager::new(SessionConfig::default());

    // Create sessions
    for i in 0..3 {
        let client_id = format!("client-{:03}", i);
        let _ = manager.get_or_create_session(client_id.clone(), "websocket".to_string());

        // Add elicitations
        let elicitation = ElicitationContext::new("Test".to_string(), json!({}));
        manager.add_pending_elicitation(client_id.clone(), elicitation);

        // Add completions
        let completion = CompletionContext::new(CompletionReference::Tool {
            name: "test".to_string(),
            argument: "arg".to_string(),
        });
        manager.add_active_completion(client_id, completion);
    }

    // Get enhanced analytics
    let analytics = manager.get_enhanced_analytics();
    assert_eq!(analytics.active_sessions, 3);

    // The counts are tracked internally even if not exposed in the struct yet
    // This test verifies the method executes without panicking
}
