//! Session and conversation management
//!
//! Handles conversation sessions, context strategies, and conversation history.

use crate::llm::core::{LLMError, LLMMessage, LLMResult};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Configuration for session management
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Maximum conversation history length
    pub max_history_length: usize,
    /// Default context strategy
    pub default_context_strategy: ContextStrategy,
    /// Session timeout in seconds
    pub session_timeout_seconds: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            max_history_length: 100,
            default_context_strategy: ContextStrategy::SlidingWindow { window_size: 20 },
            session_timeout_seconds: 3600, // 1 hour
        }
    }
}

/// Strategies for managing conversation context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ContextStrategy {
    /// Keep full conversation history
    FullHistory,
    /// Keep a sliding window of recent messages
    SlidingWindow { window_size: usize },
    /// Summarize old messages and keep recent ones
    Summarized {
        summary_threshold: usize,
        keep_recent: usize,
    },
    /// Smart context management based on relevance
    Smart {
        max_tokens: usize,
        relevance_threshold: f64,
    },
}

/// Metadata associated with a conversation session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// User identifier
    pub user_id: String,
    /// Session tags
    pub tags: Vec<String>,
    /// Custom metadata
    pub custom: HashMap<String, serde_json::Value>,
    /// Session priority
    pub priority: i32,
    /// Language preference
    pub language: Option<String>,
}

impl SessionMetadata {
    /// Create new session metadata
    pub fn new(user_id: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            tags: Vec::new(),
            custom: HashMap::new(),
            priority: 0,
            language: None,
        }
    }

    /// Add a tag
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Set custom metadata
    pub fn with_custom(mut self, key: String, value: serde_json::Value) -> Self {
        self.custom.insert(key, value);
        self
    }
}

/// A conversation session with history and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationSession {
    /// Unique session identifier
    pub id: String,
    /// Session metadata
    pub metadata: SessionMetadata,
    /// Conversation messages
    pub messages: Vec<LLMMessage>,
    /// Context management strategy
    pub context_strategy: ContextStrategy,
    /// Session creation time
    pub created_at: DateTime<Utc>,
    /// Last activity time
    pub last_activity: DateTime<Utc>,
    /// Session status
    pub status: SessionStatus,
}

/// Status of a conversation session
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SessionStatus {
    /// Session is active
    Active,
    /// Session is paused
    Paused,
    /// Session has expired
    Expired,
    /// Session was manually closed
    Closed,
}

impl ConversationSession {
    /// Create a new conversation session
    pub fn new(metadata: SessionMetadata, context_strategy: ContextStrategy) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            metadata,
            messages: Vec::new(),
            context_strategy,
            created_at: now,
            last_activity: now,
            status: SessionStatus::Active,
        }
    }

    /// Add a message to the session
    pub fn add_message(&mut self, message: LLMMessage) {
        self.messages.push(message);
        self.last_activity = Utc::now();
    }

    /// Get active messages based on context strategy
    pub fn get_active_messages(&self) -> Vec<LLMMessage> {
        match &self.context_strategy {
            ContextStrategy::FullHistory => self.messages.clone(),
            ContextStrategy::SlidingWindow { window_size } => {
                let start_idx = self.messages.len().saturating_sub(*window_size);
                self.messages[start_idx..].to_vec()
            }
            ContextStrategy::Summarized { keep_recent, .. } => {
                let start_idx = self.messages.len().saturating_sub(*keep_recent);
                self.messages[start_idx..].to_vec()
            }
            ContextStrategy::Smart { .. } => {
                // TODO: Implement smart context selection
                // For now, fall back to sliding window
                let window_size = 20;
                let start_idx = self.messages.len().saturating_sub(window_size);
                self.messages[start_idx..].to_vec()
            }
        }
    }

    /// Check if session has expired
    pub fn is_expired(&self, timeout_seconds: u64) -> bool {
        let now = Utc::now();
        let timeout = chrono::Duration::seconds(timeout_seconds as i64);
        now.signed_duration_since(self.last_activity) > timeout
    }

    /// Get session duration
    pub fn duration(&self) -> chrono::Duration {
        self.last_activity.signed_duration_since(self.created_at)
    }

    /// Get message count
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Pause the session
    pub fn pause(&mut self) {
        self.status = SessionStatus::Paused;
    }

    /// Resume the session
    pub fn resume(&mut self) {
        self.status = SessionStatus::Active;
        self.last_activity = Utc::now();
    }

    /// Close the session
    pub fn close(&mut self) {
        self.status = SessionStatus::Closed;
    }
}

/// Session manager for handling multiple conversation sessions
#[derive(Debug)]
pub struct SessionManager {
    sessions: HashMap<String, ConversationSession>,
    config: SessionConfig,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(config: SessionConfig) -> Self {
        Self {
            sessions: HashMap::new(),
            config,
        }
    }

    /// Create a new session
    pub fn create_session(
        &mut self,
        metadata: SessionMetadata,
        context_strategy: Option<ContextStrategy>,
    ) -> String {
        let strategy = context_strategy.unwrap_or(self.config.default_context_strategy.clone());
        let session = ConversationSession::new(metadata, strategy);
        let session_id = session.id.clone();

        self.sessions.insert(session_id.clone(), session);
        session_id
    }

    /// Get a session by ID
    pub fn get_session(&self, session_id: &str) -> Option<&ConversationSession> {
        self.sessions.get(session_id)
    }

    /// Get a mutable session by ID
    pub fn get_session_mut(&mut self, session_id: &str) -> Option<&mut ConversationSession> {
        self.sessions.get_mut(session_id)
    }

    /// Add a message to a session
    pub fn add_message(&mut self, session_id: &str, message: LLMMessage) -> LLMResult<()> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| LLMError::session(format!("Session not found: {}", session_id)))?;

        if session.status != SessionStatus::Active {
            return Err(LLMError::session("Session is not active"));
        }

        session.add_message(message);

        // Trim history if needed
        if session.messages.len() > self.config.max_history_length {
            let excess = session.messages.len() - self.config.max_history_length;
            session.messages.drain(0..excess);
        }

        Ok(())
    }

    /// Get active messages for a session
    pub fn get_active_messages(&self, session_id: &str) -> LLMResult<Vec<LLMMessage>> {
        let session = self
            .sessions
            .get(session_id)
            .ok_or_else(|| LLMError::session(format!("Session not found: {}", session_id)))?;

        Ok(session.get_active_messages())
    }

    /// List all session IDs
    pub fn list_sessions(&self) -> Vec<String> {
        self.sessions.keys().cloned().collect()
    }

    /// Get sessions by user ID
    pub fn get_user_sessions(&self, user_id: &str) -> Vec<String> {
        self.sessions
            .iter()
            .filter_map(|(id, session)| {
                if session.metadata.user_id == user_id {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    /// Clean up expired sessions
    pub fn cleanup_expired(&mut self) -> usize {
        let timeout = self.config.session_timeout_seconds;
        let expired_ids: Vec<_> = self
            .sessions
            .iter()
            .filter_map(|(id, session)| {
                if session.is_expired(timeout) {
                    Some(id.clone())
                } else {
                    None
                }
            })
            .collect();

        let count = expired_ids.len();
        for id in expired_ids {
            if let Some(mut session) = self.sessions.remove(&id) {
                session.status = SessionStatus::Expired;
            }
        }

        count
    }

    /// Pause a session
    pub fn pause_session(&mut self, session_id: &str) -> LLMResult<()> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| LLMError::session(format!("Session not found: {}", session_id)))?;

        session.pause();
        Ok(())
    }

    /// Resume a session
    pub fn resume_session(&mut self, session_id: &str) -> LLMResult<()> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| LLMError::session(format!("Session not found: {}", session_id)))?;

        session.resume();
        Ok(())
    }

    /// Close a session
    pub fn close_session(&mut self, session_id: &str) -> LLMResult<()> {
        let session = self
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| LLMError::session(format!("Session not found: {}", session_id)))?;

        session.close();
        Ok(())
    }

    /// Remove a session completely
    pub fn remove_session(&mut self, session_id: &str) -> Option<ConversationSession> {
        self.sessions.remove(session_id)
    }

    /// Get session statistics
    pub fn get_stats(&self) -> SessionStats {
        let total_sessions = self.sessions.len();
        let active_sessions = self
            .sessions
            .values()
            .filter(|s| s.status == SessionStatus::Active)
            .count();
        let paused_sessions = self
            .sessions
            .values()
            .filter(|s| s.status == SessionStatus::Paused)
            .count();
        let total_messages: usize = self.sessions.values().map(|s| s.message_count()).sum();

        SessionStats {
            total_sessions,
            active_sessions,
            paused_sessions,
            total_messages,
        }
    }
}

/// Statistics about session manager
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    /// Total number of sessions
    pub total_sessions: usize,
    /// Number of active sessions
    pub active_sessions: usize,
    /// Number of paused sessions
    pub paused_sessions: usize,
    /// Total messages across all sessions
    pub total_messages: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::core::LLMMessage;

    #[test]
    fn test_session_creation() {
        let metadata = SessionMetadata::new("user123")
            .with_tag("test")
            .with_custom("priority".to_string(), serde_json::json!(1));

        let session =
            ConversationSession::new(metadata, ContextStrategy::SlidingWindow { window_size: 10 });

        assert!(!session.id.is_empty());
        assert_eq!(session.metadata.user_id, "user123");
        assert!(session.metadata.tags.contains(&"test".to_string()));
        assert_eq!(session.status, SessionStatus::Active);
        assert_eq!(session.message_count(), 0);
    }

    #[test]
    fn test_session_messages() {
        let metadata = SessionMetadata::new("user123");
        let mut session = ConversationSession::new(metadata, ContextStrategy::FullHistory);

        session.add_message(LLMMessage::user("Hello"));
        session.add_message(LLMMessage::assistant("Hi there!"));

        assert_eq!(session.message_count(), 2);

        let active_messages = session.get_active_messages();
        assert_eq!(active_messages.len(), 2);
    }

    #[test]
    fn test_sliding_window_context() {
        let metadata = SessionMetadata::new("user123");
        let mut session =
            ConversationSession::new(metadata, ContextStrategy::SlidingWindow { window_size: 2 });

        session.add_message(LLMMessage::user("Message 1"));
        session.add_message(LLMMessage::assistant("Response 1"));
        session.add_message(LLMMessage::user("Message 2"));
        session.add_message(LLMMessage::assistant("Response 2"));

        let active_messages = session.get_active_messages();
        assert_eq!(active_messages.len(), 2); // Only last 2 messages
        assert_eq!(active_messages[0].content.as_text(), Some("Message 2"));
        assert_eq!(active_messages[1].content.as_text(), Some("Response 2"));
    }

    #[test]
    fn test_session_manager() {
        let config = SessionConfig::default();
        let mut manager = SessionManager::new(config);

        let metadata = SessionMetadata::new("user123");
        let session_id = manager.create_session(metadata, None);

        assert!(manager.get_session(&session_id).is_some());
        assert_eq!(manager.list_sessions().len(), 1);

        manager
            .add_message(&session_id, LLMMessage::user("Hello"))
            .unwrap();

        let active_messages = manager.get_active_messages(&session_id).unwrap();
        assert_eq!(active_messages.len(), 1);

        let stats = manager.get_stats();
        assert_eq!(stats.total_sessions, 1);
        assert_eq!(stats.active_sessions, 1);
        assert_eq!(stats.total_messages, 1);
    }

    #[test]
    fn test_session_status_management() {
        let config = SessionConfig::default();
        let mut manager = SessionManager::new(config);

        let metadata = SessionMetadata::new("user123");
        let session_id = manager.create_session(metadata, None);

        manager.pause_session(&session_id).unwrap();
        let session = manager.get_session(&session_id).unwrap();
        assert_eq!(session.status, SessionStatus::Paused);

        manager.resume_session(&session_id).unwrap();
        let session = manager.get_session(&session_id).unwrap();
        assert_eq!(session.status, SessionStatus::Active);

        manager.close_session(&session_id).unwrap();
        let session = manager.get_session(&session_id).unwrap();
        assert_eq!(session.status, SessionStatus::Closed);
    }
}
