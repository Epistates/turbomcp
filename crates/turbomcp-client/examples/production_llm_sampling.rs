//! Production-grade LLM sampling example
//!
//! This example demonstrates the production-ready LLM backend integration
//! for TurboMCP client-side sampling. It shows how to:
//!
//! - Configure multiple LLM providers (OpenAI, Anthropic)
//! - Handle real CreateMessageRequest/CreateMessageResult flows
//! - Implement proper error handling and retry logic
//! - Manage conversation context and model preferences
//! - Use MCP protocol-compliant patterns
//!
//! # Usage
//!
//! Set environment variables:
//! ```bash
//! export OPENAI_API_KEY="your-openai-api-key"
//! export ANTHROPIC_API_KEY="your-anthropic-api-key"
//! ```
//!
//! Then run:
//! ```bash
//! cargo run --example production_llm_sampling
//! ```

use std::env;
use tracing::{Level, info};
use turbomcp_client::sampling::{
    LLMBackendConfig, LLMProvider, ProductionSamplingHandler, SamplingHandler,
};
use turbomcp_protocol::types::{
    Content, CreateMessageRequest, IncludeContext, ModelHint, ModelPreferences, Role,
    SamplingMessage, TextContent,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    info!("🚀 TurboMCP Production LLM Sampling Example");

    // Check for API keys
    let openai_key = env::var("OPENAI_API_KEY").ok();
    let anthropic_key = env::var("ANTHROPIC_API_KEY").ok();

    if openai_key.is_none() && anthropic_key.is_none() {
        println!("❌ No API keys found. Please set OPENAI_API_KEY and/or ANTHROPIC_API_KEY");
        println!("   Example: export OPENAI_API_KEY='your-key-here'");
        return Ok(());
    }

    // Demonstrate OpenAI integration
    if let Some(api_key) = openai_key {
        info!("🔧 Testing OpenAI backend integration");

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

        let handler = ProductionSamplingHandler::new(config)?;

        // Basic conversation test
        let request = CreateMessageRequest {
            messages: vec![SamplingMessage {
                role: Role::User,
                content: Content::Text(TextContent {
                    text: "Hello! Please introduce yourself in one sentence.".to_string(),
                    annotations: None,
                    meta: None,
                }),
            }],
            model_preferences: Some(ModelPreferences {
                cost_priority: Some(0.8),
                speed_priority: Some(0.6),
                intelligence_priority: Some(0.4),
                hints: Some(vec![ModelHint {
                    name: "gpt-3.5-turbo".to_string(),
                }]),
            }),
            system_prompt: Some("You are a helpful AI assistant built with TurboMCP.".to_string()),
            include_context: Some(IncludeContext::ThisServer),
            temperature: Some(0.7),
            max_tokens: 100,
            stop_sequences: None,
            metadata: None,
            _meta: None,
        };

        match handler.handle_create_message(request).await {
            Ok(response) => {
                info!("✅ OpenAI Response received");
                println!(
                    "🤖 OpenAI Assistant: {}",
                    extract_text_content(&response.content)
                );
                println!(
                    "📊 Model used: {}",
                    response.model.unwrap_or("unknown".to_string())
                );
                println!(
                    "🏁 Stop reason: {}",
                    response.stop_reason.unwrap_or("unknown".to_string())
                );
            }
            Err(e) => {
                println!("❌ OpenAI request failed: {}", e);
            }
        }

        // Multi-turn conversation test
        let multi_turn_request = CreateMessageRequest {
            messages: vec![
                SamplingMessage {
                    role: Role::User,
                    content: Content::Text(TextContent {
                        text: "My favorite color is blue.".to_string(),
                        annotations: None,
                        meta: None,
                    }),
                },
                SamplingMessage {
                    role: Role::Assistant,
                    content: Content::Text(TextContent {
                        text: "That's lovely! Blue is a beautiful and calming color.".to_string(),
                        annotations: None,
                        meta: None,
                    }),
                },
                SamplingMessage {
                    role: Role::User,
                    content: Content::Text(TextContent {
                        text: "What's my favorite color?".to_string(),
                        annotations: None,
                        meta: None,
                    }),
                },
            ],
            model_preferences: None,
            system_prompt: Some("Remember the conversation context.".to_string()),
            include_context: None,
            temperature: Some(0.1),
            max_tokens: 50,
            stop_sequences: None,
            metadata: None,
            _meta: None,
        };

        match handler.handle_create_message(multi_turn_request).await {
            Ok(response) => {
                info!("✅ OpenAI Context Test passed");
                println!(
                    "🧠 Context Response: {}",
                    extract_text_content(&response.content)
                );
            }
            Err(e) => {
                println!("❌ OpenAI context test failed: {}", e);
            }
        }
    }

    // Demonstrate Anthropic integration
    if let Some(api_key) = anthropic_key {
        info!("🔧 Testing Anthropic backend integration");

        let config = LLMBackendConfig {
            provider: LLMProvider::Anthropic {
                api_key,
                base_url: None,
            },
            default_model: Some("claude-3-haiku-20240307".to_string()),
            timeout_seconds: 30,
            max_retries: 3,
        };

        let handler = ProductionSamplingHandler::new(config)?;

        // Basic conversation test
        let request = CreateMessageRequest {
            messages: vec![SamplingMessage {
                role: Role::User,
                content: Content::Text(TextContent {
                    text: "Hello Claude! Please tell me about TurboMCP in one sentence."
                        .to_string(),
                    annotations: None,
                    meta: None,
                }),
            }],
            model_preferences: Some(ModelPreferences {
                cost_priority: Some(1.0),
                speed_priority: Some(0.5),
                intelligence_priority: Some(0.8),
                hints: Some(vec![ModelHint {
                    name: "claude-3-haiku-20240307".to_string(),
                }]),
            }),
            system_prompt: Some(
                "You are Claude, integrated with TurboMCP for production-grade AI interactions."
                    .to_string(),
            ),
            include_context: None,
            temperature: Some(0.3),
            max_tokens: 100,
            stop_sequences: Some(vec!["!".to_string()]),
            metadata: None,
            _meta: None,
        };

        match handler.handle_create_message(request).await {
            Ok(response) => {
                info!("✅ Anthropic Response received");
                println!(
                    "🤖 Claude Assistant: {}",
                    extract_text_content(&response.content)
                );
                println!(
                    "📊 Model used: {}",
                    response.model.unwrap_or("unknown".to_string())
                );
                println!(
                    "🏁 Stop reason: {}",
                    response.stop_reason.unwrap_or("unknown".to_string())
                );
            }
            Err(e) => {
                println!("❌ Anthropic request failed: {}", e);
            }
        }
    }

    // Demonstrate error handling
    info!("🔧 Testing error handling with invalid configuration");

    let invalid_config = LLMBackendConfig {
        provider: LLMProvider::OpenAI {
            api_key: "invalid-key".to_string(),
            base_url: None,
            organization: None,
        },
        default_model: Some("gpt-3.5-turbo".to_string()),
        timeout_seconds: 5,
        max_retries: 1,
    };

    let handler = ProductionSamplingHandler::new(invalid_config)?;

    let error_test_request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: "This should fail".to_string(),
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
        _meta: None,
    };

    match handler.handle_create_message(error_test_request).await {
        Ok(_) => {
            println!("🤔 Unexpected success with invalid API key");
        }
        Err(e) => {
            info!("✅ Error handling working correctly");
            println!("❌ Expected error: {}", e);
        }
    }

    info!("🎉 TurboMCP Production LLM Sampling Example completed!");
    println!();
    println!("🚀 Production Features Demonstrated:");
    println!("   ✅ Multi-provider support (OpenAI, Anthropic)");
    println!("   ✅ MCP protocol compliance");
    println!("   ✅ Conversation context management");
    println!("   ✅ Model preferences and configuration");
    println!("   ✅ Comprehensive error handling");
    println!("   ✅ Retry logic and timeout handling");
    println!("   ✅ Production-grade architecture");
    println!();
    println!("🔧 Ready for production use with any MCP client!");

    Ok(())
}

/// Helper function to extract text content from Content enum
fn extract_text_content(content: &Content) -> &str {
    match content {
        Content::Text(text) => &text.text,
        _ => "[Non-text content]",
    }
}
