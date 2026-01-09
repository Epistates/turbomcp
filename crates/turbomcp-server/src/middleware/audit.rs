//! Audit logging middleware for security and compliance
//!
//! This middleware logs security-relevant events and access patterns
//! for compliance, monitoring, and security analysis.

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::SystemTime;

use serde_json::json;
use tower::{Layer, Service};
use tracing::{debug, info, warn};

#[cfg(feature = "auth")]
use super::auth::AuthContext;

/// Audit configuration
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Whether to log successful requests
    pub log_success: bool,
    /// Whether to log failed requests
    pub log_failures: bool,
    /// Whether to log authentication events
    pub log_auth_events: bool,
    /// Whether to log authorization events
    pub log_authz_events: bool,
    /// Log level for audit events
    pub log_level: AuditLogLevel,
}

/// Audit logging level
#[derive(Debug, Clone)]
pub enum AuditLogLevel {
    /// Debug level logging
    Debug,
    /// Info level logging
    Info,
    /// Warning level logging
    Warn,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            log_success: true,
            log_failures: true,
            log_auth_events: true,
            log_authz_events: true,
            log_level: AuditLogLevel::Info,
        }
    }
}

/// Audit event types
#[derive(Debug, Clone)]
pub enum AuditEvent {
    /// HTTP request started
    RequestStarted {
        /// HTTP method (GET, POST, etc.)
        method: String,
        /// Request path
        path: String,
        /// Authenticated user ID if available
        user_id: Option<String>,
        /// Event timestamp
        timestamp: SystemTime,
    },
    /// HTTP request completed
    RequestCompleted {
        /// HTTP method (GET, POST, etc.)
        method: String,
        /// Request path
        path: String,
        /// Authenticated user ID if available
        user_id: Option<String>,
        /// HTTP status code
        status: u16,
        /// Request duration in milliseconds
        duration_ms: u64,
        /// Event timestamp
        timestamp: SystemTime,
    },
    /// Authentication succeeded
    AuthenticationSuccess {
        /// Authenticated user ID
        user_id: String,
        /// Event timestamp
        timestamp: SystemTime,
    },
    /// Authentication failed
    AuthenticationFailure {
        /// Failure reason
        reason: String,
        /// Event timestamp
        timestamp: SystemTime,
    },
    /// Authorization denied
    AuthorizationDenied {
        /// User ID if authenticated
        user_id: Option<String>,
        /// Resource being accessed
        resource: String,
        /// Action being attempted
        action: String,
        /// Event timestamp
        timestamp: SystemTime,
    },
}

/// Audit layer
#[derive(Debug, Clone)]
pub struct AuditLayer {
    config: AuditConfig,
}

impl AuditLayer {
    /// Create new audit layer
    pub fn new(config: AuditConfig) -> Self {
        Self { config }
    }
}

impl<S> Layer<S> for AuditLayer {
    type Service = AuditService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuditService {
            inner,
            config: self.config.clone(),
        }
    }
}

/// Audit service
#[derive(Debug, Clone)]
pub struct AuditService<S> {
    inner: S,
    config: AuditConfig,
}

impl<S, ReqBody> Service<http::Request<ReqBody>> for AuditService<S>
where
    S: Service<http::Request<ReqBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        let config = self.config.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            let start_time = SystemTime::now();
            let method = req.method().to_string();
            let path = req.uri().path().to_string();

            // Extract user info if available (only when auth feature is enabled)
            #[cfg(feature = "auth")]
            let user_id = req
                .extensions()
                .get::<AuthContext>()
                .map(|ctx| ctx.sub.clone());

            #[cfg(not(feature = "auth"))]
            let user_id: Option<String> = None;

            // Log request start
            if config.log_success {
                log_audit_event(
                    &AuditEvent::RequestStarted {
                        method: method.clone(),
                        path: path.clone(),
                        user_id: user_id.clone(),
                        timestamp: start_time,
                    },
                    &config,
                );
            }

            // Process request
            let response = inner.call(req).await;

            // Log request completion
            let duration = start_time.elapsed().unwrap_or_default();

            // Note: In a real implementation, you'd extract the status code from the response
            // This is simplified for demonstration
            let status = 200; // Placeholder

            log_audit_event(
                &AuditEvent::RequestCompleted {
                    method,
                    path,
                    user_id,
                    status,
                    duration_ms: duration.as_millis() as u64,
                    timestamp: SystemTime::now(),
                },
                &config,
            );

            response
        })
    }
}

/// Log an audit event
fn log_audit_event(event: &AuditEvent, config: &AuditConfig) {
    let log_data = match event {
        AuditEvent::RequestStarted {
            method,
            path,
            user_id,
            timestamp,
        } => {
            json!({
                "event_type": "request_started",
                "method": method,
                "path": path,
                "user_id": user_id,
                "timestamp": timestamp.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
            })
        }
        AuditEvent::RequestCompleted {
            method,
            path,
            user_id,
            status,
            duration_ms,
            timestamp,
        } => {
            json!({
                "event_type": "request_completed",
                "method": method,
                "path": path,
                "user_id": user_id,
                "status": status,
                "duration_ms": duration_ms,
                "timestamp": timestamp.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
            })
        }
        AuditEvent::AuthenticationSuccess { user_id, timestamp } => {
            json!({
                "event_type": "authentication_success",
                "user_id": user_id,
                "timestamp": timestamp.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
            })
        }
        AuditEvent::AuthenticationFailure { reason, timestamp } => {
            json!({
                "event_type": "authentication_failure",
                "reason": reason,
                "timestamp": timestamp.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
            })
        }
        AuditEvent::AuthorizationDenied {
            user_id,
            resource,
            action,
            timestamp,
        } => {
            json!({
                "event_type": "authorization_denied",
                "user_id": user_id,
                "resource": resource,
                "action": action,
                "timestamp": timestamp.duration_since(SystemTime::UNIX_EPOCH).unwrap().as_secs()
            })
        }
    };

    match config.log_level {
        AuditLogLevel::Debug => debug!(audit = %log_data, "Audit event"),
        AuditLogLevel::Info => info!(audit = %log_data, "Audit event"),
        AuditLogLevel::Warn => warn!(audit = %log_data, "Audit event"),
    }
}

/// Helper function to log authentication events
pub fn log_authentication_success(user_id: &str) {
    let event = AuditEvent::AuthenticationSuccess {
        user_id: user_id.to_string(),
        timestamp: SystemTime::now(),
    };

    let config = AuditConfig::default();
    log_audit_event(&event, &config);
}

/// Helper function to log authentication failures
pub fn log_authentication_failure(reason: &str) {
    let event = AuditEvent::AuthenticationFailure {
        reason: reason.to_string(),
        timestamp: SystemTime::now(),
    };

    let config = AuditConfig::default();
    log_audit_event(&event, &config);
}

/// Helper function to log authorization denials
pub fn log_authorization_denied(user_id: Option<&str>, resource: &str, action: &str) {
    let event = AuditEvent::AuthorizationDenied {
        user_id: user_id.map(|s| s.to_string()),
        resource: resource.to_string(),
        action: action.to_string(),
        timestamp: SystemTime::now(),
    };

    let config = AuditConfig::default();
    log_audit_event(&event, &config);
}
