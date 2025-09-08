//! Enhanced LLM Integration System
//!
//! This module provides a comprehensive LLM integration system that builds on the existing
//! SamplingHandler foundation with advanced features:
//!
//! - **Provider abstraction**: Generic LLMProvider trait for multi-provider support
//! - **Token management**: Token counting and context window management
//! - **Session management**: Conversation tracking with history and metadata
//! - **Streaming support**: Infrastructure for streaming responses
//! - **Smart routing**: Intelligent provider selection based on request type
//! - **Registry system**: Centralized management of multiple LLM providers
//!
//! ## Architecture
//!
//! ```text
//! LLMRegistry
//!     ├── LLMProvider (OpenAI, Anthropic, Custom)
//!     ├── SessionManager
//!     │   ├── ConversationSession
//!     │   └── ContextStrategy
//!     ├── TokenCounter
//!     └── RequestRouter
//! ```
//!
//! ## Usage
//!
//! ```rust,no_run
//! use turbomcp_client::llm::{LLMRegistry, OpenAIProvider, LLMProviderConfig};
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut registry = LLMRegistry::new();
//!
//! // Register providers
//! let openai = Arc::new(OpenAIProvider::new(LLMProviderConfig {
//!     api_key: std::env::var("OPENAI_API_KEY")?,
//!     model: "gpt-4".to_string(),
//!     ..Default::default()
//! })?);
//! registry.register_provider("openai", openai).await?;
//!
//! // Set as default provider
//! registry.set_default_provider("openai")?;
//!
//! // List available providers
//! let providers = registry.list_providers();
//! println!("Available providers: {:?}", providers);
//! # Ok(())
//! # }
//! ```

pub mod core;
pub mod providers;
pub mod registry;
pub mod routing;
pub mod session;
pub mod streaming;
pub mod tokens;

// Re-export public API
pub use core::{
    LLMCapabilities, LLMError, LLMProvider, LLMProviderConfig, LLMRequest, LLMResponse, LLMResult,
    ModelInfo,
};

pub use providers::{AnthropicProvider, OllamaProvider, OpenAIProvider};

pub use session::{
    ContextStrategy, ConversationSession, SessionConfig, SessionManager, SessionMetadata,
};

pub use tokens::{ContextWindow, TokenCounter, TokenUsage};

pub use registry::{LLMRegistry, ProviderInfo, RegistryConfig};

pub use streaming::{StreamChunk, StreamingHandler, StreamingResponse};

pub use routing::{RequestRouter, RouteRule, RoutingStrategy};
