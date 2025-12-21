//! Structured Audit Logging for Authentication Events
//!
//! This module provides comprehensive audit logging for all authentication-related
//! events, supporting compliance requirements (SOC2, GDPR, HIPAA) and security monitoring.
//!
//! ## Features
//!
//! - **Structured Events** - All events are structured for easy parsing and analysis
//! - **Correlation IDs** - Events include correlation IDs for request tracing
//! - **Privacy-Aware** - Sensitive data is redacted or hashed by default
//! - **Tracing Integration** - Uses the `tracing` ecosystem for flexible output
//!
//! ## Event Types
//!
//! - [`AuthEvent::LoginAttempt`] - User login attempt (success or failure)
//! - [`AuthEvent::LoginSuccess`] - Successful authentication
//! - [`AuthEvent::LoginFailure`] - Failed authentication attempt
//! - [`AuthEvent::TokenIssued`] - New token issued
//! - [`AuthEvent::TokenRefreshed`] - Token refreshed
//! - [`AuthEvent::TokenRevoked`] - Token revoked
//! - [`AuthEvent::TokenExpired`] - Token expired
//! - [`AuthEvent::PermissionDenied`] - Authorization failure
//! - [`AuthEvent::SessionCreated`] - New session started
//! - [`AuthEvent::SessionTerminated`] - Session ended
//!
//! ## Usage
//!
//! ```rust
//! use turbomcp_auth::audit::{AuditLogger, AuthEvent, EventOutcome};
//!
//! let logger = AuditLogger::new("my-service");
//!
//! // Log a successful login
//! logger.log(AuthEvent::LoginSuccess {
//!     user_id: "user123".to_string(),
//!     provider: "oauth2-google".to_string(),
//!     ip_address: Some("192.168.1.1".to_string()),
//!     user_agent: Some("Mozilla/5.0...".to_string()),
//! });
//!
//! // Log a failed login attempt
//! logger.log(AuthEvent::LoginFailure {
//!     attempted_user: Some("alice@example.com".to_string()),
//!     provider: "api-key".to_string(),
//!     reason: "Invalid API key".to_string(),
//!     ip_address: Some("10.0.0.1".to_string()),
//!     user_agent: None,
//! });
//!
//! // Log a permission denied event
//! logger.log(AuthEvent::PermissionDenied {
//!     user_id: "user123".to_string(),
//!     resource: "admin/settings".to_string(),
//!     action: "write".to_string(),
//!     required_permission: "admin:write".to_string(),
//! });
//! ```
//!
//! ## Compliance Notes
//!
//! This module is designed to support:
//! - **SOC 2**: Audit trail requirements for access control
//! - **GDPR**: Right to access logs, data minimization
//! - **HIPAA**: Access logging for protected health information
//! - **PCI DSS**: Cardholder data access tracking
//!
//! Configure log retention and access according to your compliance requirements.

use serde::{Deserialize, Serialize};
use std::time::SystemTime;
use tracing::{error, info, warn};
use uuid::Uuid;

/// Audit logger for authentication events
///
/// Provides structured logging with service identification and correlation support.
#[derive(Debug, Clone)]
pub struct AuditLogger {
    /// Service name for event attribution
    service_name: String,
    /// Whether to include IP addresses in logs (privacy consideration)
    include_ip: bool,
    /// Whether to hash sensitive identifiers
    hash_identifiers: bool,
}

impl AuditLogger {
    /// Create a new audit logger with the given service name
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            include_ip: true,
            hash_identifiers: false,
        }
    }

    /// Create a privacy-focused audit logger that hashes identifiers and excludes IPs
    pub fn privacy_focused(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            include_ip: false,
            hash_identifiers: true,
        }
    }

    /// Builder method to configure IP inclusion
    pub fn with_ip_logging(mut self, include: bool) -> Self {
        self.include_ip = include;
        self
    }

    /// Builder method to configure identifier hashing
    pub fn with_identifier_hashing(mut self, hash: bool) -> Self {
        self.hash_identifiers = hash;
        self
    }

    /// Log an authentication event
    pub fn log(&self, event: AuthEvent) {
        let record = AuditRecord {
            id: Uuid::now_v7(),
            timestamp: SystemTime::now(),
            service: self.service_name.clone(),
            event: self.maybe_redact(event),
        };

        match &record.event {
            AuthEvent::LoginSuccess {
                user_id, provider, ..
            } => {
                info!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "login_success",
                    user_id = %self.maybe_hash(user_id),
                    provider = %provider,
                    service = %self.service_name,
                    "Authentication successful"
                );
            }
            AuthEvent::LoginFailure {
                attempted_user,
                provider,
                reason,
                ip_address,
                ..
            } => {
                warn!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "login_failure",
                    attempted_user = ?attempted_user.as_ref().map(|u| self.maybe_hash(u)),
                    provider = %provider,
                    reason = %reason,
                    ip_address = ?self.maybe_include_ip(ip_address.as_deref()),
                    service = %self.service_name,
                    "Authentication failed"
                );
            }
            AuthEvent::LoginAttempt {
                user_identifier,
                provider,
                ..
            } => {
                info!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "login_attempt",
                    user_identifier = %self.maybe_hash(user_identifier),
                    provider = %provider,
                    service = %self.service_name,
                    "Login attempt initiated"
                );
            }
            AuthEvent::TokenIssued {
                user_id,
                token_type,
                expires_in,
                ..
            } => {
                info!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "token_issued",
                    user_id = %self.maybe_hash(user_id),
                    token_type = %token_type,
                    expires_in_secs = ?expires_in,
                    service = %self.service_name,
                    "Token issued"
                );
            }
            AuthEvent::TokenRefreshed {
                user_id, token_id, ..
            } => {
                info!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "token_refreshed",
                    user_id = %self.maybe_hash(user_id),
                    token_id = %self.maybe_hash(token_id),
                    service = %self.service_name,
                    "Token refreshed"
                );
            }
            AuthEvent::TokenRevoked {
                user_id,
                token_id,
                reason,
                ..
            } => {
                info!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "token_revoked",
                    user_id = %self.maybe_hash(user_id),
                    token_id = %self.maybe_hash(token_id),
                    reason = %reason,
                    service = %self.service_name,
                    "Token revoked"
                );
            }
            AuthEvent::TokenExpired { user_id, token_id } => {
                info!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "token_expired",
                    user_id = %self.maybe_hash(user_id),
                    token_id = %self.maybe_hash(token_id),
                    service = %self.service_name,
                    "Token expired"
                );
            }
            AuthEvent::PermissionDenied {
                user_id,
                resource,
                action,
                required_permission,
            } => {
                warn!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "permission_denied",
                    user_id = %self.maybe_hash(user_id),
                    resource = %resource,
                    action = %action,
                    required_permission = %required_permission,
                    service = %self.service_name,
                    "Permission denied"
                );
            }
            AuthEvent::SessionCreated {
                user_id,
                session_id,
                ..
            } => {
                info!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "session_created",
                    user_id = %self.maybe_hash(user_id),
                    session_id = %self.maybe_hash(session_id),
                    service = %self.service_name,
                    "Session created"
                );
            }
            AuthEvent::SessionTerminated {
                user_id,
                session_id,
                reason,
            } => {
                info!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "session_terminated",
                    user_id = %self.maybe_hash(user_id),
                    session_id = %self.maybe_hash(session_id),
                    reason = %reason,
                    service = %self.service_name,
                    "Session terminated"
                );
            }
            AuthEvent::RateLimited {
                identifier,
                endpoint,
                limit,
                window_secs,
            } => {
                warn!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "rate_limited",
                    identifier = %self.maybe_hash(identifier),
                    endpoint = %endpoint,
                    limit = %limit,
                    window_secs = %window_secs,
                    service = %self.service_name,
                    "Rate limit exceeded"
                );
            }
            AuthEvent::SuspiciousActivity {
                user_id,
                activity_type,
                details,
                severity,
            } => {
                error!(
                    target: "audit::auth",
                    audit_id = %record.id,
                    event_type = "suspicious_activity",
                    user_id = ?user_id.as_ref().map(|u| self.maybe_hash(u)),
                    activity_type = %activity_type,
                    details = %details,
                    severity = %severity,
                    service = %self.service_name,
                    "Suspicious activity detected"
                );
            }
        }
    }

    /// Log an event with a correlation ID for request tracing
    pub fn log_with_correlation(&self, event: AuthEvent, correlation_id: &str) {
        let record = AuditRecord {
            id: Uuid::now_v7(),
            timestamp: SystemTime::now(),
            service: self.service_name.clone(),
            event: self.maybe_redact(event),
        };

        // Log with correlation ID in span
        let span = tracing::info_span!(
            "audit",
            correlation_id = %correlation_id,
            audit_id = %record.id
        );
        let _guard = span.enter();

        self.log(record.event);
    }

    fn maybe_hash(&self, value: &str) -> String {
        if self.hash_identifiers {
            // Use BLAKE3 for fast, secure hashing
            let hash = blake3::hash(value.as_bytes());
            format!("sha3:{}", &hash.to_hex()[..16])
        } else {
            value.to_string()
        }
    }

    fn maybe_include_ip(&self, ip: Option<&str>) -> Option<String> {
        if self.include_ip {
            ip.map(String::from)
        } else {
            ip.map(|_| "[REDACTED]".to_string())
        }
    }

    fn maybe_redact(&self, event: AuthEvent) -> AuthEvent {
        if !self.include_ip {
            match event {
                AuthEvent::LoginSuccess {
                    user_id,
                    provider,
                    user_agent,
                    ..
                } => AuthEvent::LoginSuccess {
                    user_id,
                    provider,
                    ip_address: Some("[REDACTED]".to_string()),
                    user_agent,
                },
                AuthEvent::LoginFailure {
                    attempted_user,
                    provider,
                    reason,
                    user_agent,
                    ..
                } => AuthEvent::LoginFailure {
                    attempted_user,
                    provider,
                    reason,
                    ip_address: Some("[REDACTED]".to_string()),
                    user_agent,
                },
                AuthEvent::SessionCreated {
                    user_id,
                    session_id,
                    user_agent,
                    ..
                } => AuthEvent::SessionCreated {
                    user_id,
                    session_id,
                    ip_address: Some("[REDACTED]".to_string()),
                    user_agent,
                },
                other => other,
            }
        } else {
            event
        }
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new("turbomcp")
    }
}

/// Authentication event types for audit logging
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AuthEvent {
    /// Login attempt initiated (before verification)
    LoginAttempt {
        /// User identifier (email, username, etc.)
        user_identifier: String,
        /// Authentication provider
        provider: String,
        /// Client IP address
        ip_address: Option<String>,
        /// User agent string
        user_agent: Option<String>,
    },

    /// Successful authentication
    LoginSuccess {
        /// Authenticated user ID
        user_id: String,
        /// Authentication provider used
        provider: String,
        /// Client IP address
        ip_address: Option<String>,
        /// User agent string
        user_agent: Option<String>,
    },

    /// Failed authentication attempt
    LoginFailure {
        /// Attempted user identifier (may be None for invalid requests)
        attempted_user: Option<String>,
        /// Authentication provider
        provider: String,
        /// Failure reason
        reason: String,
        /// Client IP address
        ip_address: Option<String>,
        /// User agent string
        user_agent: Option<String>,
    },

    /// New token issued
    TokenIssued {
        /// User the token was issued for
        user_id: String,
        /// Type of token (access, refresh, api_key)
        token_type: String,
        /// Token expiration in seconds
        expires_in: Option<u64>,
        /// Scopes granted
        scopes: Vec<String>,
    },

    /// Token refreshed
    TokenRefreshed {
        /// User ID
        user_id: String,
        /// Token identifier (not the token itself)
        token_id: String,
        /// New expiration in seconds
        new_expires_in: Option<u64>,
    },

    /// Token revoked
    TokenRevoked {
        /// User ID
        user_id: String,
        /// Token identifier
        token_id: String,
        /// Reason for revocation
        reason: String,
        /// Who initiated the revocation
        revoked_by: Option<String>,
    },

    /// Token expired
    TokenExpired {
        /// User ID
        user_id: String,
        /// Token identifier
        token_id: String,
    },

    /// Authorization failure (permission denied)
    PermissionDenied {
        /// User who was denied
        user_id: String,
        /// Resource being accessed
        resource: String,
        /// Action attempted
        action: String,
        /// Permission that was required
        required_permission: String,
    },

    /// Session created
    SessionCreated {
        /// User ID
        user_id: String,
        /// Session identifier
        session_id: String,
        /// Client IP address
        ip_address: Option<String>,
        /// User agent
        user_agent: Option<String>,
    },

    /// Session terminated
    SessionTerminated {
        /// User ID
        user_id: String,
        /// Session identifier
        session_id: String,
        /// Reason (logout, timeout, revoked, etc.)
        reason: String,
    },

    /// Rate limit exceeded
    RateLimited {
        /// Identifier being rate limited (IP, user ID, API key prefix)
        identifier: String,
        /// Endpoint or action being limited
        endpoint: String,
        /// Limit value
        limit: u32,
        /// Time window in seconds
        window_secs: u32,
    },

    /// Suspicious activity detected
    SuspiciousActivity {
        /// User ID if known
        user_id: Option<String>,
        /// Type of suspicious activity
        activity_type: String,
        /// Details about the activity
        details: String,
        /// Severity (low, medium, high, critical)
        severity: String,
    },
}

/// Audit record wrapping an event with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditRecord {
    /// Unique audit record ID
    pub id: Uuid,
    /// Timestamp of the event
    #[serde(with = "system_time_serde")]
    pub timestamp: SystemTime,
    /// Service that generated the event
    pub service: String,
    /// The audit event
    pub event: AuthEvent,
}

/// Event outcome for metrics
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventOutcome {
    /// Operation succeeded
    Success,
    /// Operation failed
    Failure,
    /// Operation was denied (authorization)
    Denied,
    /// Operation was rate limited
    RateLimited,
}

mod system_time_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    pub fn serialize<S>(time: &SystemTime, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let duration = time.duration_since(UNIX_EPOCH).unwrap_or(Duration::ZERO);
        duration.as_secs().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<SystemTime, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = u64::deserialize(deserializer)?;
        Ok(UNIX_EPOCH + Duration::from_secs(secs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_logger_creation() {
        let logger = AuditLogger::new("test-service");
        assert_eq!(logger.service_name, "test-service");
        assert!(logger.include_ip);
        assert!(!logger.hash_identifiers);
    }

    #[test]
    fn test_privacy_focused_logger() {
        let logger = AuditLogger::privacy_focused("secure-service");
        assert!(!logger.include_ip);
        assert!(logger.hash_identifiers);
    }

    #[test]
    fn test_identifier_hashing() {
        let logger = AuditLogger::new("test").with_identifier_hashing(true);
        let hashed = logger.maybe_hash("user123");
        assert!(hashed.starts_with("sha3:"));
        assert_eq!(hashed.len(), 21); // "sha3:" + 16 hex chars
    }

    #[test]
    fn test_ip_redaction() {
        let logger = AuditLogger::new("test").with_ip_logging(false);
        let ip = logger.maybe_include_ip(Some("192.168.1.1"));
        assert_eq!(ip, Some("[REDACTED]".to_string()));
    }

    #[test]
    fn test_event_serialization() {
        let event = AuthEvent::LoginSuccess {
            user_id: "user123".to_string(),
            provider: "oauth2".to_string(),
            ip_address: Some("10.0.0.1".to_string()),
            user_agent: Some("Mozilla/5.0".to_string()),
        };

        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"type\":\"login_success\""));
        assert!(json.contains("\"user_id\":\"user123\""));
    }

    #[test]
    fn test_audit_record_serialization() {
        let record = AuditRecord {
            id: Uuid::nil(),
            timestamp: std::time::UNIX_EPOCH,
            service: "test".to_string(),
            event: AuthEvent::TokenExpired {
                user_id: "user".to_string(),
                token_id: "token".to_string(),
            },
        };

        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("\"service\":\"test\""));
        assert!(json.contains("\"timestamp\":0"));
    }
}
