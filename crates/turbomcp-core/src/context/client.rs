//! Client-related context types for MCP client session management.
//!
//! This module contains types for managing client sessions, capabilities,
//! and identification across different transport mechanisms.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Client capabilities for server-initiated requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// Supports sampling/message creation
    pub sampling: bool,
    /// Supports roots listing
    pub roots: bool,
    /// Supports elicitation
    pub elicitation: bool,
    /// Maximum concurrent server requests
    pub max_concurrent_requests: usize,
    /// Supported experimental features
    pub experimental: HashMap<String, bool>,
}

/// Client identifier types for authentication and tracking
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClientId {
    /// Explicit client ID from header
    Header(String),
    /// Bearer token from Authorization header
    Token(String),
    /// Session cookie
    Session(String),
    /// Query parameter
    QueryParam(String),
    /// Hash of User-Agent (fallback)
    UserAgent(String),
    /// Anonymous client
    Anonymous,
}

impl ClientId {
    /// Get the string representation of the client ID
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Header(id)
            | Self::Token(id)
            | Self::Session(id)
            | Self::QueryParam(id)
            | Self::UserAgent(id) => id,
            Self::Anonymous => "anonymous",
        }
    }

    /// Check if the client is authenticated
    #[must_use]
    pub const fn is_authenticated(&self) -> bool {
        matches!(self, Self::Token(_) | Self::Session(_))
    }

    /// Get the authentication method
    #[must_use]
    pub const fn auth_method(&self) -> &'static str {
        match self {
            Self::Header(_) => "header",
            Self::Token(_) => "bearer_token",
            Self::Session(_) => "session_cookie",
            Self::QueryParam(_) => "query_param",
            Self::UserAgent(_) => "user_agent",
            Self::Anonymous => "anonymous",
        }
    }
}

/// Client session information for tracking and analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSession {
    /// Unique client identifier
    pub client_id: String,
    /// Client name (optional, human-readable)
    pub client_name: Option<String>,
    /// When the client connected
    pub connected_at: DateTime<Utc>,
    /// Last activity timestamp
    pub last_activity: DateTime<Utc>,
    /// Number of requests made
    pub request_count: usize,
    /// Transport type (stdio, http, websocket, etc.)
    pub transport_type: String,
    /// Authentication status
    pub authenticated: bool,
    /// Client capabilities (optional)
    pub capabilities: Option<serde_json::Value>,
    /// Additional metadata
    pub metadata: HashMap<String, serde_json::Value>,
}

impl ClientSession {
    /// Create a new client session
    #[must_use]
    pub fn new(client_id: String, transport_type: String) -> Self {
        let now = Utc::now();
        Self {
            client_id,
            client_name: None,
            connected_at: now,
            last_activity: now,
            request_count: 0,
            transport_type,
            authenticated: false,
            capabilities: None,
            metadata: HashMap::new(),
        }
    }

    /// Update activity timestamp and increment request count
    pub fn update_activity(&mut self) {
        self.last_activity = Utc::now();
        self.request_count += 1;
    }

    /// Set authentication status and client info
    pub fn authenticate(&mut self, client_name: Option<String>) {
        self.authenticated = true;
        self.client_name = client_name;
    }

    /// Set client capabilities
    pub fn set_capabilities(&mut self, capabilities: serde_json::Value) {
        self.capabilities = Some(capabilities);
    }

    /// Get session duration
    #[must_use]
    pub fn session_duration(&self) -> chrono::Duration {
        self.last_activity - self.connected_at
    }

    /// Check if session is idle (no activity for specified duration)
    #[must_use]
    pub fn is_idle(&self, idle_threshold: chrono::Duration) -> bool {
        Utc::now() - self.last_activity > idle_threshold
    }
}

/// Client ID extractor for authentication across different transports
#[derive(Debug)]
pub struct ClientIdExtractor {
    /// Authentication tokens mapping token -> `client_id`
    auth_tokens: Arc<dashmap::DashMap<String, String>>,
}

impl ClientIdExtractor {
    /// Create a new client ID extractor
    #[must_use]
    pub fn new() -> Self {
        Self {
            auth_tokens: Arc::new(dashmap::DashMap::new()),
        }
    }

    /// Register an authentication token for a client
    pub fn register_token(&self, token: String, client_id: String) {
        self.auth_tokens.insert(token, client_id);
    }

    /// Remove an authentication token
    pub fn revoke_token(&self, token: &str) {
        self.auth_tokens.remove(token);
    }

    /// List all registered tokens (for admin purposes)
    #[must_use]
    pub fn list_tokens(&self) -> Vec<(String, String)> {
        self.auth_tokens
            .iter()
            .map(|entry| (entry.key().clone(), entry.value().clone()))
            .collect()
    }

    /// Extract client ID from HTTP headers
    #[must_use]
    #[allow(clippy::significant_drop_tightening)]
    pub fn extract_from_http_headers(&self, headers: &HashMap<String, String>) -> ClientId {
        // 1. Check for explicit client ID header
        if let Some(client_id) = headers.get("x-client-id") {
            return ClientId::Header(client_id.clone());
        }

        // 2. Check for Authorization header with Bearer token
        if let Some(auth) = headers.get("authorization")
            && let Some(token) = auth.strip_prefix("Bearer ")
        {
            // Look up client ID from token
            let token_lookup = self.auth_tokens.iter().find(|e| e.key() == token);
            if let Some(entry) = token_lookup {
                let client_id = entry.value().clone();
                drop(entry); // Explicitly drop the lock guard early
                return ClientId::Token(client_id);
            }
            // Token not found - return the token itself as identifier
            return ClientId::Token(token.to_string());
        }

        // 3. Check for session cookie
        if let Some(cookie) = headers.get("cookie") {
            for cookie_part in cookie.split(';') {
                let parts: Vec<&str> = cookie_part.trim().splitn(2, '=').collect();
                if parts.len() == 2 && (parts[0] == "session_id" || parts[0] == "sessionid") {
                    return ClientId::Session(parts[1].to_string());
                }
            }
        }

        // 4. Use User-Agent hash as fallback
        if let Some(user_agent) = headers.get("user-agent") {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            user_agent.hash(&mut hasher);
            return ClientId::UserAgent(format!("ua_{:x}", hasher.finish()));
        }

        ClientId::Anonymous
    }

    /// Extract client ID from query parameters
    #[must_use]
    pub fn extract_from_query(&self, query_params: &HashMap<String, String>) -> Option<ClientId> {
        query_params
            .get("client_id")
            .map(|client_id| ClientId::QueryParam(client_id.clone()))
    }

    /// Extract client ID from multiple sources (with priority)
    #[must_use]
    pub fn extract_client_id(
        &self,
        headers: Option<&HashMap<String, String>>,
        query_params: Option<&HashMap<String, String>>,
    ) -> ClientId {
        // Try query parameters first (highest priority)
        if let Some(params) = query_params
            && let Some(client_id) = self.extract_from_query(params)
        {
            return client_id;
        }

        // Try HTTP headers
        if let Some(headers) = headers {
            return self.extract_from_http_headers(headers);
        }

        ClientId::Anonymous
    }
}

impl Default for ClientIdExtractor {
    fn default() -> Self {
        Self::new()
    }
}
