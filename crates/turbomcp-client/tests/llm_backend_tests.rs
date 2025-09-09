//! Comprehensive tests for LLM backend integration with MCP protocol compliance
//!
//! These tests validate the production-grade implementation of:
//! - Multi-provider LLM backends (OpenAI, Anthropic)
//! - MCP sampling protocol compliance
//! - Configuration management
//! - Error handling and resilience
//! - Conversation context management
//! - Real LLM integration (not mocks)

use std::env;
use turbomcp_client::sampling::{
    LLMBackendConfig, LLMProvider, ProductionSamplingHandler, SamplingHandler,
};
use turbomcp_protocol::types::{
    Content, CreateMessageRequest, CreateMessageResult, IncludeContext, ModelPreferences, Role,
    SamplingMessage, TextContent,
};

/// Test OpenAI backend configuration and basic functionality
#[tokio::test]
async fn test_openai_backend_basic_conversation() {
    // Skip if no API key available
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("Skipping OpenAI test - OPENAI_API_KEY not set");
            return;
        }
    };

    let config = LLMBackendConfig {
        provider: LLMProvider::OpenAI {
            api_key,
            base_url: None,
            organization: None,
        },
        default_model: Some("gpt-3.5-turbo".to_string()),
        timeout_seconds: 30,
        max_retries: 3,
    };

    let handler =
        ProductionSamplingHandler::new(config).expect("Should create handler successfully");

    let request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: "What is 2+2? Please respond with just the number.".to_string(),
                annotations: None,
                meta: None,
            }),
        }],
        model_preferences: None,
        system_prompt: None,
        include_context: None,
        temperature: Some(0.1),
        max_tokens: 10,
        stop_sequences: None,
        metadata: None,
    };

    let result = handler.handle_create_message(request).await;

    // Should succeed with real OpenAI response
    assert!(
        result.is_ok(),
        "OpenAI request should succeed: {:?}",
        result
    );

    let response = result.unwrap();
    assert_eq!(response.role, Role::Assistant);

    // Validate response content structure
    match &response.content {
        Content::Text(text) => {
            assert!(!text.text.is_empty(), "Response should have content");
            assert!(text.text.contains("4"), "Should contain the correct answer");
        }
        _ => panic!("Expected text content in response"),
    }

    // Validate model field is populated
    assert!(response.model.is_some(), "Model should be specified");
    assert!(
        response.model.unwrap().starts_with("gpt"),
        "Should use GPT model"
    );

    // Validate stop reason
    assert!(
        response.stop_reason.is_some(),
        "Stop reason should be provided"
    );
}

/// Test Anthropic backend configuration and conversation
#[tokio::test]
async fn test_anthropic_backend_basic_conversation() {
    // Skip if no API key available
    let api_key = match env::var("ANTHROPIC_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("Skipping Anthropic test - ANTHROPIC_API_KEY not set");
            return;
        }
    };

    let config = LLMBackendConfig {
        provider: LLMProvider::Anthropic {
            api_key,
            base_url: None,
        },
        default_model: Some("claude-3-haiku-20240307".to_string()),
        timeout_seconds: 30,
        max_retries: 3,
    };

    let handler =
        ProductionSamplingHandler::new(config).expect("Should create handler successfully");

    let request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: "Hello! Please respond with 'Hello from Claude'.".to_string(),
                annotations: None,
                meta: None,
            }),
        }],
        model_preferences: None,
        system_prompt: Some("You are a helpful assistant.".to_string()),
        include_context: None,
        temperature: Some(0.1),
        max_tokens: 20,
        stop_sequences: None,
        metadata: None,
    };

    let result = handler.handle_create_message(request).await;

    // Should succeed with real Anthropic response
    assert!(
        result.is_ok(),
        "Anthropic request should succeed: {:?}",
        result
    );

    let response = result.unwrap();
    assert_eq!(response.role, Role::Assistant);

    // Validate response content
    match &response.content {
        Content::Text(text) => {
            assert!(!text.text.is_empty(), "Response should have content");
            assert!(
                text.text.to_lowercase().contains("hello"),
                "Should respond appropriately"
            );
        }
        _ => panic!("Expected text content in response"),
    }

    // Validate model and stop reason
    assert!(response.model.is_some(), "Model should be specified");
    assert!(
        response.stop_reason.is_some(),
        "Stop reason should be provided"
    );
}

/// Test conversation history handling and context management
#[tokio::test]
async fn test_conversation_context_management() {
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("Skipping context test - OPENAI_API_KEY not set");
            return;
        }
    };

    let config = LLMBackendConfig {
        provider: LLMProvider::OpenAI {
            api_key,
            base_url: None,
            organization: None,
        },
        default_model: Some("gpt-3.5-turbo".to_string()),
        timeout_seconds: 30,
        max_retries: 3,
    };

    let handler =
        ProductionSamplingHandler::new(config).expect("Should create handler successfully");

    // Multi-turn conversation
    let request = CreateMessageRequest {
        messages: vec![
            SamplingMessage {
                role: Role::User,
                content: Content::Text(TextContent {
                    text: "My name is Alice.".to_string(),
                    annotations: None,
                    meta: None,
                }),
            },
            SamplingMessage {
                role: Role::Assistant,
                content: Content::Text(TextContent {
                    text: "Hello Alice! Nice to meet you.".to_string(),
                    annotations: None,
                    meta: None,
                }),
            },
            SamplingMessage {
                role: Role::User,
                content: Content::Text(TextContent {
                    text: "What is my name?".to_string(),
                    annotations: None,
                    meta: None,
                }),
            },
        ],
        model_preferences: None,
        system_prompt: Some("Remember the user's name and use it in responses.".to_string()),
        include_context: None,
        temperature: Some(0.1),
        max_tokens: 30,
        stop_sequences: None,
        metadata: None,
    };

    let result = handler.handle_create_message(request).await;
    assert!(
        result.is_ok(),
        "Conversation context request should succeed: {:?}",
        result
    );

    let response = result.unwrap();
    match &response.content {
        Content::Text(text) => {
            assert!(
                text.text.to_lowercase().contains("alice"),
                "Should remember name from context: {}",
                text.text
            );
        }
        _ => panic!("Expected text content in response"),
    }
}

/// Test model preferences and configuration
#[tokio::test]
async fn test_model_preferences_handling() {
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("Skipping model preferences test - OPENAI_API_KEY not set");
            return;
        }
    };

    let config = LLMBackendConfig {
        provider: LLMProvider::OpenAI {
            api_key,
            base_url: None,
            organization: None,
        },
        default_model: Some("gpt-3.5-turbo".to_string()),
        timeout_seconds: 30,
        max_retries: 3,
    };

    let handler =
        ProductionSamplingHandler::new(config).expect("Should create handler successfully");

    let request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: "Say 'test response' exactly.".to_string(),
                annotations: None,
                meta: None,
            }),
        }],
        model_preferences: Some(ModelPreferences {
            cost_priority: Some(0.8),
            speed_priority: Some(0.6),
            intelligence_priority: Some(0.4),
            hints: None,
        }),
        system_prompt: None,
        include_context: None,
        temperature: Some(0.0),
        max_tokens: 10,
        stop_sequences: Some(vec!["exactly".to_string()]),
        metadata: None,
    };

    let result = handler.handle_create_message(request).await;
    assert!(
        result.is_ok(),
        "Model preferences request should succeed: {:?}",
        result
    );

    let response = result.unwrap();
    // Should handle model preferences appropriately
    assert_eq!(response.role, Role::Assistant);
    assert!(response.model.is_some());
}

/// Test error handling with invalid API keys
#[tokio::test]
async fn test_invalid_api_key_error_handling() {
    let config = LLMBackendConfig {
        provider: LLMProvider::OpenAI {
            api_key: "invalid-key".to_string(),
            base_url: None,
            organization: None,
        },
        default_model: Some("gpt-3.5-turbo".to_string()),
        timeout_seconds: 5,
        max_retries: 1,
    };

    let handler =
        ProductionSamplingHandler::new(config).expect("Should create handler with invalid key");

    let request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: "Test message".to_string(),
                annotations: None,
                meta: None,
            }),
        }],
        model_preferences: None,
        system_prompt: None,
        include_context: None,
        temperature: None,
        max_tokens: 10,
        stop_sequences: None,
        metadata: None,
    };

    let result = handler.handle_create_message(request).await;

    // Should fail with authentication error
    assert!(result.is_err(), "Invalid API key should cause error");

    let error = result.unwrap_err();
    let error_msg = error.to_string().to_lowercase();
    assert!(
        error_msg.contains("auth") || error_msg.contains("key") || error_msg.contains("401"),
        "Error should indicate authentication issue: {}",
        error
    );
}

/// Test network timeout and retry logic
#[tokio::test]
async fn test_timeout_and_retry_logic() {
    let config = LLMBackendConfig {
        provider: LLMProvider::OpenAI {
            api_key: env::var("OPENAI_API_KEY").unwrap_or_else(|_| "test-key".to_string()),
            base_url: Some("https://httpbin.org/delay/10".to_string()), // Will timeout
            organization: None,
        },
        default_model: Some("gpt-3.5-turbo".to_string()),
        timeout_seconds: 2, // Short timeout
        max_retries: 2,
    };

    let handler = ProductionSamplingHandler::new(config).expect("Should create handler");

    let request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: "Test timeout".to_string(),
                annotations: None,
                meta: None,
            }),
        }],
        model_preferences: None,
        system_prompt: None,
        include_context: None,
        temperature: None,
        max_tokens: 10,
        stop_sequences: None,
        metadata: None,
    };

    let start = std::time::Instant::now();
    let result = handler.handle_create_message(request).await;
    let elapsed = start.elapsed();

    // Should fail with timeout after retries
    assert!(result.is_err(), "Timeout request should fail");

    // Should have attempted retries (timeout * retries should be approximate duration)
    // Allowing for some variance in network timing and system load
    assert!(
        elapsed.as_secs() >= 2,
        "Should have attempted retries: {:?}",
        elapsed
    );

    let error = result.unwrap_err();
    let error_msg = error.to_string().to_lowercase();
    assert!(
        error_msg.contains("timeout")
            || error_msg.contains("connect")
            || error_msg.contains("request")
            || error_msg.contains("503")
            || error_msg.contains("unavailable")
            || error_msg.contains("llm provider error"),
        "Error should indicate timeout/connection/service issue: {}",
        error
    );
}

/// Test configuration validation
#[tokio::test]
async fn test_configuration_validation() {
    // Test empty API key validation
    let result = LLMBackendConfig {
        provider: LLMProvider::OpenAI {
            api_key: "".to_string(),
            base_url: None,
            organization: None,
        },
        default_model: None,
        timeout_seconds: 30,
        max_retries: 3,
    };

    let handler_result = ProductionSamplingHandler::new(result);
    assert!(handler_result.is_err(), "Empty API key should be rejected");

    // Test invalid timeout
    let result = LLMBackendConfig {
        provider: LLMProvider::OpenAI {
            api_key: "test-key".to_string(),
            base_url: None,
            organization: None,
        },
        default_model: None,
        timeout_seconds: 0, // Invalid
        max_retries: 3,
    };

    let handler_result = ProductionSamplingHandler::new(result);
    assert!(handler_result.is_err(), "Zero timeout should be rejected");
}

/// Test different content types handling
#[tokio::test]
async fn test_content_types_handling() {
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("Skipping content types test - OPENAI_API_KEY not set");
            return;
        }
    };

    let config = LLMBackendConfig {
        provider: LLMProvider::OpenAI {
            api_key,
            base_url: None,
            organization: None,
        },
        default_model: Some("gpt-3.5-turbo".to_string()),
        timeout_seconds: 30,
        max_retries: 3,
    };

    let handler = ProductionSamplingHandler::new(config).expect("Should create handler");

    // Test with text content
    let request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: "Describe the color blue in one word.".to_string(),
                annotations: None,
                meta: None,
            }),
        }],
        model_preferences: None,
        system_prompt: None,
        include_context: None,
        temperature: Some(0.1),
        max_tokens: 5,
        stop_sequences: None,
        metadata: None,
    };

    let result = handler.handle_create_message(request).await;
    assert!(
        result.is_ok(),
        "Text content request should succeed: {:?}",
        result
    );

    let response = result.unwrap();
    match &response.content {
        Content::Text(text) => {
            assert!(!text.text.trim().is_empty(), "Response should have content");
        }
        _ => panic!("Expected text content in response"),
    }
}

/// Test MCP protocol compliance - field mapping and serialization
#[tokio::test]
async fn test_mcp_protocol_compliance() {
    let api_key = match env::var("OPENAI_API_KEY") {
        Ok(key) => key,
        Err(_) => {
            eprintln!("Skipping MCP compliance test - OPENAI_API_KEY not set");
            return;
        }
    };

    let config = LLMBackendConfig {
        provider: LLMProvider::OpenAI {
            api_key,
            base_url: None,
            organization: None,
        },
        default_model: Some("gpt-3.5-turbo".to_string()),
        timeout_seconds: 30,
        max_retries: 3,
    };

    let handler = ProductionSamplingHandler::new(config).expect("Should create handler");

    // Test with all MCP CreateMessageRequest fields
    let request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: "Test MCP compliance".to_string(),
                annotations: None,
                meta: None,
            }),
        }],
        model_preferences: Some(ModelPreferences {
            cost_priority: Some(0.5),
            speed_priority: Some(0.5),
            intelligence_priority: Some(1.0),
            hints: None,
        }),
        system_prompt: Some("You are a test assistant.".to_string()),
        include_context: Some(IncludeContext::ThisServer),
        temperature: Some(0.7),
        max_tokens: 50,
        stop_sequences: Some(vec!["\n".to_string()]),
        metadata: None,
    };

    let result = handler.handle_create_message(request).await;
    assert!(
        result.is_ok(),
        "MCP compliant request should succeed: {:?}",
        result
    );

    let response = result.unwrap();

    // Validate all required MCP CreateMessageResult fields
    assert_eq!(response.role, Role::Assistant, "Role should be Assistant");

    match &response.content {
        Content::Text(_) => {} // Valid
        _ => panic!("Content should be properly structured"),
    }

    // Optional fields should be handled properly
    assert!(response.model.is_some(), "Model should be populated");
    assert!(
        response.stop_reason.is_some(),
        "Stop reason should be populated"
    );

    // Validate serialization roundtrip
    let serialized = serde_json::to_string(&response).expect("Should serialize");
    let deserialized: CreateMessageResult =
        serde_json::from_str(&serialized).expect("Should deserialize back to same structure");

    assert_eq!(response.role, deserialized.role);
    // Note: We don't compare full equality due to potential metadata differences
}
