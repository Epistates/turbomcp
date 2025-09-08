//! Streaming response support infrastructure

use crate::llm::core::{LLMError, LLMMessage, LLMResult, TokenUsage};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures::Stream;
use serde::{Deserialize, Serialize};
use std::pin::Pin;

/// A chunk of a streaming response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamChunk {
    /// Chunk content
    pub content: String,
    /// Whether this is the final chunk
    pub is_final: bool,
    /// Chunk index
    pub index: usize,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Token usage (if available)
    pub usage: Option<TokenUsage>,
}

/// Streaming response handler
#[async_trait]
pub trait StreamingHandler: Send + Sync {
    /// Handle a stream chunk
    async fn handle_chunk(&self, chunk: StreamChunk) -> LLMResult<()>;

    /// Handle stream completion
    async fn handle_completion(
        &self,
        final_message: LLMMessage,
        usage: TokenUsage,
    ) -> LLMResult<()>;

    /// Handle stream error
    async fn handle_error(&self, error: LLMError) -> LLMResult<()>;
}

/// A streaming response from an LLM
pub type StreamingResponse = Pin<Box<dyn Stream<Item = LLMResult<StreamChunk>> + Send>>;

/// Default streaming handler that collects chunks
#[derive(Debug, Default)]
pub struct CollectingStreamHandler {
    chunks: std::sync::Mutex<Vec<StreamChunk>>,
}

impl CollectingStreamHandler {
    /// Create a new collecting handler
    pub fn new() -> Self {
        Self::default()
    }

    /// Get collected chunks
    pub fn get_chunks(&self) -> Vec<StreamChunk> {
        self.chunks.lock().unwrap().clone()
    }

    /// Get complete text
    pub fn get_complete_text(&self) -> String {
        self.chunks
            .lock()
            .unwrap()
            .iter()
            .map(|chunk| chunk.content.as_str())
            .collect::<Vec<_>>()
            .join("")
    }
}

#[async_trait]
impl StreamingHandler for CollectingStreamHandler {
    async fn handle_chunk(&self, chunk: StreamChunk) -> LLMResult<()> {
        self.chunks.lock().unwrap().push(chunk);
        Ok(())
    }

    async fn handle_completion(
        &self,
        _final_message: LLMMessage,
        _usage: TokenUsage,
    ) -> LLMResult<()> {
        // Nothing to do for collecting handler
        Ok(())
    }

    async fn handle_error(&self, _error: LLMError) -> LLMResult<()> {
        // Could store error for later retrieval
        Ok(())
    }
}
