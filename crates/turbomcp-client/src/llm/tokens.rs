//! Token counting and context management utilities
//!
//! Provides utilities for counting tokens, managing context windows, and optimizing
//! prompt length for different LLM providers.

use crate::llm::core::{LLMError, LLMMessage, LLMResult, MessageRole};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Token usage information with detailed breakdown
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenUsage {
    /// Input/prompt tokens
    pub prompt_tokens: usize,

    /// Output/completion tokens
    pub completion_tokens: usize,

    /// Total tokens used
    pub total_tokens: usize,

    /// Cached tokens (if applicable)
    pub cached_tokens: Option<usize>,

    /// Tokens from images (if applicable)
    pub image_tokens: Option<usize>,
}

impl TokenUsage {
    /// Create new token usage
    pub fn new(prompt_tokens: usize, completion_tokens: usize) -> Self {
        Self {
            prompt_tokens,
            completion_tokens,
            total_tokens: prompt_tokens + completion_tokens,
            cached_tokens: None,
            image_tokens: None,
        }
    }

    /// Create empty token usage
    pub fn empty() -> Self {
        Self::new(0, 0)
    }

    /// Add cached tokens
    pub fn with_cached_tokens(mut self, cached_tokens: usize) -> Self {
        self.cached_tokens = Some(cached_tokens);
        self
    }

    /// Add image tokens
    pub fn with_image_tokens(mut self, image_tokens: usize) -> Self {
        self.image_tokens = Some(image_tokens);
        self
    }

    /// Add to existing usage
    pub fn add(&mut self, other: &TokenUsage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.total_tokens += other.total_tokens;

        if let Some(other_cached) = other.cached_tokens {
            self.cached_tokens = Some(self.cached_tokens.unwrap_or(0) + other_cached);
        }

        if let Some(other_image) = other.image_tokens {
            self.image_tokens = Some(self.image_tokens.unwrap_or(0) + other_image);
        }
    }

    /// Calculate cost estimate (in USD)
    pub fn estimate_cost(&self, input_cost_per_1k: f64, output_cost_per_1k: f64) -> f64 {
        let prompt_cost = (self.prompt_tokens as f64 / 1000.0) * input_cost_per_1k;
        let completion_cost = (self.completion_tokens as f64 / 1000.0) * output_cost_per_1k;
        prompt_cost + completion_cost
    }
}

/// Context window configuration and management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextWindow {
    /// Maximum tokens in context window
    pub max_tokens: usize,

    /// Reserved tokens for the response
    pub response_reserve: usize,

    /// Reserved tokens for system message
    pub system_reserve: usize,

    /// Minimum tokens to keep from conversation history
    pub history_minimum: usize,
}

impl ContextWindow {
    /// Create new context window configuration
    pub fn new(max_tokens: usize) -> Self {
        Self {
            max_tokens,
            response_reserve: max_tokens / 4, // Reserve 25% for response
            system_reserve: 500,              // Reserve for system message
            history_minimum: 1000,            // Keep at least 1k tokens of history
        }
    }

    /// Get available tokens for conversation history
    pub fn available_for_history(&self) -> usize {
        self.max_tokens
            .saturating_sub(self.response_reserve)
            .saturating_sub(self.system_reserve)
    }

    /// Check if token count fits in context window
    pub fn fits(&self, token_count: usize) -> bool {
        token_count <= self.available_for_history()
    }

    /// Calculate how many tokens to truncate
    pub fn tokens_to_truncate(&self, current_tokens: usize) -> usize {
        if self.fits(current_tokens) {
            0
        } else {
            current_tokens - self.available_for_history()
        }
    }
}

/// Token counter for different providers and models
#[derive(Debug)]
pub struct TokenCounter {
    /// Model-specific token estimates
    model_estimates: HashMap<String, f64>,

    /// Provider-specific multipliers
    provider_multipliers: HashMap<String, f64>,
}

impl Default for TokenCounter {
    fn default() -> Self {
        let mut model_estimates = HashMap::new();
        let mut provider_multipliers = HashMap::new();

        // OpenAI models - tokens per character
        model_estimates.insert("gpt-3.5-turbo".to_string(), 0.25);
        model_estimates.insert("gpt-4".to_string(), 0.25);
        model_estimates.insert("gpt-4-turbo".to_string(), 0.25);
        model_estimates.insert("gpt-4o".to_string(), 0.25);

        // Anthropic models - slightly different tokenization
        model_estimates.insert("claude-3-haiku-20240307".to_string(), 0.24);
        model_estimates.insert("claude-3-sonnet-20240229".to_string(), 0.24);
        model_estimates.insert("claude-3-opus-20240229".to_string(), 0.24);
        model_estimates.insert("claude-3-5-sonnet-20240620".to_string(), 0.24);

        // Provider multipliers for conversation overhead
        provider_multipliers.insert("openai".to_string(), 1.1);
        provider_multipliers.insert("anthropic".to_string(), 1.05);
        provider_multipliers.insert("ollama".to_string(), 1.0);

        Self {
            model_estimates,
            provider_multipliers,
        }
    }
}

impl TokenCounter {
    /// Create a new token counter
    pub fn new() -> Self {
        Self::default()
    }

    /// Add custom model estimate
    pub fn add_model_estimate(&mut self, model: String, tokens_per_char: f64) {
        self.model_estimates.insert(model, tokens_per_char);
    }

    /// Add provider multiplier
    pub fn add_provider_multiplier(&mut self, provider: String, multiplier: f64) {
        self.provider_multipliers.insert(provider, multiplier);
    }

    /// Estimate tokens for text
    pub fn estimate_text_tokens(&self, text: &str, model: Option<&str>) -> usize {
        let base_estimate = if let Some(model) = model {
            let tokens_per_char = self.model_estimates.get(model).copied().unwrap_or(0.25); // Default fallback
            (text.len() as f64 * tokens_per_char) as usize
        } else {
            // Simple fallback: ~4 chars per token
            text.len().div_ceil(4)
        };

        base_estimate.max(1) // At least 1 token
    }

    /// Estimate tokens for a message
    pub fn estimate_message_tokens(
        &self,
        message: &LLMMessage,
        model: Option<&str>,
        provider: Option<&str>,
    ) -> usize {
        let base_tokens = match &message.content {
            crate::llm::core::MessageContent::Text { text } => {
                self.estimate_text_tokens(text, model)
            }
            crate::llm::core::MessageContent::Image { .. } => {
                // Image tokens vary by provider and detail level
                match provider {
                    Some("openai") => 765,     // GPT-4V standard image cost
                    Some("anthropic") => 1568, // Claude 3 image cost
                    _ => 1000,                 // Conservative estimate
                }
            }
            crate::llm::core::MessageContent::ToolCall { arguments, .. } => {
                let args_str = arguments.to_string();
                self.estimate_text_tokens(&args_str, model) + 10 // Tool call overhead
            }
            crate::llm::core::MessageContent::ToolResult { result, .. } => {
                let result_str = result.to_string();
                self.estimate_text_tokens(&result_str, model) + 5 // Tool result overhead
            }
        };

        // Add message overhead (role, formatting, etc.)
        let message_overhead = match message.role {
            MessageRole::System => 10,
            MessageRole::User => 5,
            MessageRole::Assistant => 5,
            MessageRole::Function => 15,
        };

        let total_tokens = base_tokens + message_overhead;

        // Apply provider multiplier
        if let Some(provider) = provider {
            let multiplier = self
                .provider_multipliers
                .get(provider)
                .copied()
                .unwrap_or(1.0);
            (total_tokens as f64 * multiplier) as usize
        } else {
            total_tokens
        }
    }

    /// Estimate tokens for a conversation
    pub fn estimate_conversation_tokens(
        &self,
        messages: &[LLMMessage],
        model: Option<&str>,
        provider: Option<&str>,
    ) -> usize {
        let message_tokens: usize = messages
            .iter()
            .map(|msg| self.estimate_message_tokens(msg, model, provider))
            .sum();

        // Add conversation overhead
        let conversation_overhead = messages.len() * 2;

        message_tokens + conversation_overhead
    }

    /// Truncate messages to fit in context window
    pub fn truncate_to_fit(
        &self,
        messages: Vec<LLMMessage>,
        context_window: &ContextWindow,
        model: Option<&str>,
        provider: Option<&str>,
    ) -> LLMResult<Vec<LLMMessage>> {
        let total_tokens = self.estimate_conversation_tokens(&messages, model, provider);

        if context_window.fits(total_tokens) {
            return Ok(messages);
        }

        let tokens_to_remove = context_window.tokens_to_truncate(total_tokens);

        // Strategy: Keep system message, remove oldest user/assistant pairs
        let mut result = Vec::new();
        let mut removed_tokens = 0;

        // First pass: separate system messages and conversation
        let mut system_messages = Vec::new();
        let mut conversation_messages = Vec::new();

        for message in messages {
            match message.role {
                MessageRole::System => system_messages.push(message),
                _ => conversation_messages.push(message),
            }
        }

        // Keep all system messages
        result.extend(system_messages);

        // Remove messages from the beginning until we fit
        let mut skip_count = 0;
        for message in &conversation_messages {
            let message_tokens = self.estimate_message_tokens(message, model, provider);
            if removed_tokens + message_tokens >= tokens_to_remove {
                break;
            }
            removed_tokens += message_tokens;
            skip_count += 1;
        }

        // Add remaining conversation messages
        result.extend(conversation_messages.into_iter().skip(skip_count));

        // Ensure we have at least one non-system message
        if result.iter().all(|msg| msg.role == MessageRole::System) {
            return Err(LLMError::generic(
                "Cannot fit conversation in context window even after truncation",
            ));
        }

        Ok(result)
    }

    /// Get context window for a model
    pub fn get_context_window(&self, model: &str) -> ContextWindow {
        match model {
            // OpenAI models
            "gpt-3.5-turbo" => ContextWindow::new(16385),
            "gpt-4" => ContextWindow::new(8192),
            "gpt-4-turbo" | "gpt-4-turbo-preview" => ContextWindow::new(128000),
            "gpt-4o" => ContextWindow::new(128000),

            // Anthropic models
            m if m.starts_with("claude-3") => ContextWindow::new(200000),

            // Ollama models (varies)
            _ => ContextWindow::new(4096), // Conservative default
        }
    }

    /// Optimize message history for token efficiency
    pub fn optimize_messages(
        &self,
        messages: Vec<LLMMessage>,
        context_window: &ContextWindow,
        model: Option<&str>,
        provider: Option<&str>,
    ) -> LLMResult<Vec<LLMMessage>> {
        // First try: Simple truncation
        let truncated = self.truncate_to_fit(messages.clone(), context_window, model, provider)?;
        let truncated_tokens = self.estimate_conversation_tokens(&truncated, model, provider);

        if context_window.fits(truncated_tokens) {
            return Ok(truncated);
        }

        // Second try: Summarization (placeholder for future implementation)
        // For now, just use more aggressive truncation
        let aggressive_window = ContextWindow {
            max_tokens: context_window.max_tokens,
            response_reserve: context_window.response_reserve,
            system_reserve: context_window.system_reserve,
            history_minimum: context_window.history_minimum / 2, // More aggressive
        };

        self.truncate_to_fit(messages, &aggressive_window, model, provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::core::{LLMMessage, MessageRole};

    #[test]
    fn test_token_usage() {
        let mut usage = TokenUsage::new(100, 50);
        assert_eq!(usage.total_tokens, 150);

        let other = TokenUsage::new(20, 10).with_cached_tokens(5);
        usage.add(&other);

        assert_eq!(usage.prompt_tokens, 120);
        assert_eq!(usage.completion_tokens, 60);
        assert_eq!(usage.total_tokens, 180);
        assert_eq!(usage.cached_tokens, Some(5));
    }

    #[test]
    fn test_token_usage_cost() {
        let usage = TokenUsage::new(1000, 500);
        let cost = usage.estimate_cost(0.01, 0.03); // $0.01 per 1k input, $0.03 per 1k output
        assert_eq!(cost, 0.025); // (1000/1000 * 0.01) + (500/1000 * 0.03)
    }

    #[test]
    fn test_context_window() {
        let window = ContextWindow::new(4000);
        assert_eq!(window.available_for_history(), 2500); // 4000 - 1000 - 500

        assert!(window.fits(2000));
        assert!(!window.fits(3000));

        assert_eq!(window.tokens_to_truncate(3000), 500);
        assert_eq!(window.tokens_to_truncate(2000), 0);
    }

    #[test]
    fn test_token_counter_text_estimation() {
        let counter = TokenCounter::new();

        let text = "Hello, world!";
        let tokens = counter.estimate_text_tokens(text, Some("gpt-4"));
        assert!(tokens > 0);
        assert!(tokens < 20); // Reasonable estimate for short text

        let long_text = "This is a much longer text that should result in more tokens being estimated by the token counter system.";
        let long_tokens = counter.estimate_text_tokens(long_text, Some("gpt-4"));
        assert!(long_tokens > tokens);
    }

    #[test]
    fn test_message_token_estimation() {
        let counter = TokenCounter::new();

        let message = LLMMessage::user("Hello, world!");
        let tokens = counter.estimate_message_tokens(&message, Some("gpt-4"), Some("openai"));
        assert!(tokens > 0);

        let system_message = LLMMessage::system("You are a helpful assistant.");
        let system_tokens =
            counter.estimate_message_tokens(&system_message, Some("gpt-4"), Some("openai"));
        assert!(system_tokens > tokens); // System messages have more overhead
    }

    #[test]
    fn test_conversation_token_estimation() {
        let counter = TokenCounter::new();

        let messages = vec![
            LLMMessage::system("You are a helpful assistant."),
            LLMMessage::user("What's 2+2?"),
            LLMMessage::assistant("2+2 equals 4."),
        ];

        let tokens = counter.estimate_conversation_tokens(&messages, Some("gpt-4"), Some("openai"));
        assert!(tokens > 0);

        let single_message_tokens =
            counter.estimate_message_tokens(&messages[0], Some("gpt-4"), Some("openai"));
        assert!(tokens > single_message_tokens); // Should be more than just one message
    }

    #[test]
    fn test_message_truncation() {
        let counter = TokenCounter::new();
        let window = ContextWindow::new(1000); // Much larger window for testing

        let messages = vec![
            LLMMessage::system("You are a helpful assistant."),
            LLMMessage::user("First question"),
            LLMMessage::assistant("First answer"),
            LLMMessage::user("Second question"),
            LLMMessage::assistant("Second answer"),
            LLMMessage::user("Final question"),
        ];

        let truncated = counter
            .truncate_to_fit(messages.clone(), &window, Some("gpt-4"), Some("openai"))
            .unwrap();

        // Should keep system message and some conversation
        assert!(!truncated.is_empty());

        // Should preserve system message
        assert!(truncated.iter().any(|msg| msg.role == MessageRole::System));

        // Should have at least one non-system message
        assert!(truncated.iter().any(|msg| msg.role != MessageRole::System));
    }

    #[test]
    fn test_context_window_for_models() {
        let counter = TokenCounter::new();

        let gpt4_window = counter.get_context_window("gpt-4");
        assert_eq!(gpt4_window.max_tokens, 8192);

        let gpt4_turbo_window = counter.get_context_window("gpt-4-turbo");
        assert_eq!(gpt4_turbo_window.max_tokens, 128000);

        let claude_window = counter.get_context_window("claude-3-sonnet-20240229");
        assert_eq!(claude_window.max_tokens, 200000);

        let unknown_window = counter.get_context_window("unknown-model");
        assert_eq!(unknown_window.max_tokens, 4096); // Default
    }
}
