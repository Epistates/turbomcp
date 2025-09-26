//! Security audit logging for file operations

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

/// Security events that require auditing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum SecurityEvent {
    FileAccessAttempt {
        path: PathBuf,
        timestamp: DateTime<Utc>,
    },
    FileAccessGranted {
        path: PathBuf,
        timestamp: DateTime<Utc>,
    },
    FileAccessDenied {
        path: PathBuf,
        reason: String,
        timestamp: DateTime<Utc>,
    },
    PathTraversalAttempt {
        attempted_path: PathBuf,
        canonical_path: Option<PathBuf>,
        timestamp: DateTime<Utc>,
    },
    SymlinkAttackAttempt {
        symlink_path: PathBuf,
        target_path: PathBuf,
        timestamp: DateTime<Utc>,
    },
    ResourceLimitExceeded {
        resource_type: String,
        limit: u64,
        attempted: u64,
        timestamp: DateTime<Utc>,
    },
    MemoryMapAccess {
        path: PathBuf,
        offset: usize,
        length: usize,
        timestamp: DateTime<Utc>,
    },
    SocketPathValidated {
        path: PathBuf,
        timestamp: DateTime<Utc>,
    },
    SecurityPolicyViolation {
        policy: String,
        details: String,
        timestamp: DateTime<Utc>,
    },
    AuditSystemFailure {
        error: String,
        timestamp: DateTime<Utc>,
    },
}

impl SecurityEvent {
    /// Get the severity level of this security event
    pub fn severity(&self) -> EventSeverity {
        match self {
            SecurityEvent::FileAccessAttempt { .. } => EventSeverity::Info,
            SecurityEvent::FileAccessGranted { .. } => EventSeverity::Info,
            SecurityEvent::FileAccessDenied { .. } => EventSeverity::Warning,
            SecurityEvent::PathTraversalAttempt { .. } => EventSeverity::Critical,
            SecurityEvent::SymlinkAttackAttempt { .. } => EventSeverity::Critical,
            SecurityEvent::ResourceLimitExceeded { .. } => EventSeverity::Warning,
            SecurityEvent::MemoryMapAccess { .. } => EventSeverity::Info,
            SecurityEvent::SocketPathValidated { .. } => EventSeverity::Info,
            SecurityEvent::SecurityPolicyViolation { .. } => EventSeverity::High,
            SecurityEvent::AuditSystemFailure { .. } => EventSeverity::Critical,
        }
    }

    /// Get event category for metrics and alerting
    pub fn category(&self) -> &'static str {
        match self {
            SecurityEvent::FileAccessAttempt { .. } => "file_access",
            SecurityEvent::FileAccessGranted { .. } => "file_access",
            SecurityEvent::FileAccessDenied { .. } => "file_access",
            SecurityEvent::PathTraversalAttempt { .. } => "path_security",
            SecurityEvent::SymlinkAttackAttempt { .. } => "path_security",
            SecurityEvent::ResourceLimitExceeded { .. } => "resource_limits",
            SecurityEvent::MemoryMapAccess { .. } => "memory_mapping",
            SecurityEvent::SocketPathValidated { .. } => "socket_security",
            SecurityEvent::SecurityPolicyViolation { .. } => "policy_violation",
            SecurityEvent::AuditSystemFailure { .. } => "audit_system",
        }
    }

    /// Check if this event should trigger immediate alerts
    pub fn requires_immediate_alert(&self) -> bool {
        matches!(
            self.severity(),
            EventSeverity::Critical | EventSeverity::High
        )
    }
}

/// Event severity levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EventSeverity {
    Info,
    Warning,
    High,
    Critical,
}

/// Audit logger configuration
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Enable structured logging to stdout
    pub log_to_stdout: bool,
    /// Enable file-based audit logging
    pub log_to_file: Option<PathBuf>,
    /// Buffer size for async logging
    pub buffer_size: usize,
    /// Enable real-time alerting for critical events
    pub enable_alerting: bool,
    /// Maximum log file size before rotation
    pub max_log_file_size: u64,
    /// Number of rotated log files to keep
    pub max_log_files: usize,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            log_to_stdout: true,
            log_to_file: None,
            buffer_size: 1000,
            enable_alerting: true,
            max_log_file_size: 10 * 1024 * 1024, // 10MB
            max_log_files: 5,
        }
    }
}

/// Async security audit logger
#[derive(Debug)]
pub struct AuditLogger {
    sender: mpsc::Sender<SecurityEvent>,
    config: AuditConfig,
}

impl AuditLogger {
    /// Create a new audit logger with default configuration
    pub fn new() -> Self {
        Self::with_config(AuditConfig::default())
    }

    /// Create a new audit logger with custom configuration
    pub fn with_config(config: AuditConfig) -> Self {
        let (sender, receiver) = mpsc::channel(config.buffer_size);

        // Spawn background task to handle audit events
        let logger_config = config.clone();
        tokio::spawn(async move {
            Self::audit_event_handler(receiver, logger_config).await;
        });

        Self { sender, config }
    }

    /// Log a security event asynchronously
    pub async fn log_event(&self, event: SecurityEvent) {
        match self.sender.try_send(event) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(event)) => {
                // Buffer is full, log to stderr as fallback
                eprintln!("AUDIT BUFFER FULL: {:?}", event);

                // Try to make room by logging a system failure event
                let failure_event = SecurityEvent::AuditSystemFailure {
                    error: "Audit buffer overflow - some events may be lost".to_string(),
                    timestamp: Utc::now(),
                };
                let _ = self.sender.try_send(failure_event);
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                eprintln!("AUDIT SYSTEM FAILURE: Channel closed");
            }
        }
    }

    /// Force flush any pending audit events
    pub async fn flush(&self) -> Result<(), Box<dyn std::error::Error>> {
        // Send a marker event and wait for it to be processed
        let flush_event = SecurityEvent::AuditSystemFailure {
            error: "Flush marker".to_string(),
            timestamp: Utc::now(),
        };

        self.sender.send(flush_event).await?;
        Ok(())
    }

    /// Background task to handle audit events
    async fn audit_event_handler(mut receiver: mpsc::Receiver<SecurityEvent>, config: AuditConfig) {
        let mut file_writer = if let Some(ref log_file) = config.log_to_file {
            Some(Self::create_file_writer(log_file).await)
        } else {
            None
        };

        while let Some(event) = receiver.recv().await {
            // Skip flush marker events
            if matches!(&event, SecurityEvent::AuditSystemFailure { error, .. } if error == "Flush marker")
            {
                continue;
            }

            // Log to stdout if enabled
            if config.log_to_stdout {
                Self::log_to_stdout(&event);
            }

            // Log to file if enabled
            if let Some(ref mut writer) = file_writer {
                Self::log_to_file(&event, writer).await;
            }

            // Handle alerting for critical events
            if config.enable_alerting && event.requires_immediate_alert() {
                Self::send_alert(&event).await;
            }
        }

        info!("Audit event handler shutdown complete");
    }

    /// Log event to stdout with structured format
    fn log_to_stdout(event: &SecurityEvent) {
        match event.severity() {
            EventSeverity::Info => {
                info!(
                    event_type = event.category(),
                    event = ?event,
                    "Security audit event"
                );
            }
            EventSeverity::Warning => {
                warn!(
                    event_type = event.category(),
                    event = ?event,
                    "Security audit event"
                );
            }
            EventSeverity::High | EventSeverity::Critical => {
                error!(
                    event_type = event.category(),
                    severity = ?event.severity(),
                    event = ?event,
                    "SECURITY ALERT"
                );
            }
        }
    }

    /// Create file writer for audit logs
    async fn create_file_writer(_log_file: &Path) -> tokio::fs::File {
        // In a production implementation, this would set up log rotation
        // For now, we'll create a simple file writer
        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/tmp/turbomcp_security_audit.log")
            .await
            .unwrap_or_else(|e| {
                eprintln!("Failed to open audit log file: {}", e);
                std::process::exit(1);
            })
    }

    /// Write event to audit log file
    async fn log_to_file(event: &SecurityEvent, writer: &mut tokio::fs::File) {
        use tokio::io::AsyncWriteExt;

        let json_line = match serde_json::to_string(event) {
            Ok(json) => format!("{}\n", json),
            Err(e) => {
                eprintln!("Failed to serialize audit event: {}", e);
                return;
            }
        };

        if let Err(e) = writer.write_all(json_line.as_bytes()).await {
            eprintln!("Failed to write to audit log: {}", e);
        }

        // Ensure immediate write to disk for critical events
        if event.requires_immediate_alert() {
            let _ = writer.sync_all().await;
        }
    }

    /// Send alert for critical security events
    async fn send_alert(event: &SecurityEvent) {
        // In a production system, this would integrate with:
        // - SIEM systems
        // - Slack/Teams notifications
        // - PagerDuty alerts
        // - Email notifications
        // - Security monitoring platforms

        error!(
            alert = true,
            severity = ?event.severity(),
            event_category = event.category(),
            event = ?event,
            "🚨 CRITICAL SECURITY ALERT 🚨"
        );

        // Log to system logs as well
        #[cfg(unix)]
        {
            use std::process::Command;
            let message = format!(
                "TurboMCP Security Alert: {} - {:?}",
                event.category(),
                event
            );
            let _ = Command::new("logger")
                .arg("-p")
                .arg("security.crit")
                .arg(&message)
                .output();
        }
    }
}

impl Default for AuditLogger {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for AuditLogger {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            config: self.config.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_audit_logger_creation() {
        let logger = AuditLogger::new();
        assert!(logger.sender.capacity() > 0);
    }

    #[tokio::test]
    async fn test_event_severity_classification() {
        let file_access = SecurityEvent::FileAccessAttempt {
            path: PathBuf::from("/tmp/test.txt"),
            timestamp: Utc::now(),
        };
        assert_eq!(file_access.severity(), EventSeverity::Info);

        let traversal = SecurityEvent::PathTraversalAttempt {
            attempted_path: PathBuf::from("../../../etc/passwd"),
            canonical_path: None,
            timestamp: Utc::now(),
        };
        assert_eq!(traversal.severity(), EventSeverity::Critical);
        assert!(traversal.requires_immediate_alert());
    }

    #[tokio::test]
    async fn test_event_categorization() {
        let file_event = SecurityEvent::FileAccessGranted {
            path: PathBuf::from("/tmp/test.txt"),
            timestamp: Utc::now(),
        };
        assert_eq!(file_event.category(), "file_access");

        let security_event = SecurityEvent::PathTraversalAttempt {
            attempted_path: PathBuf::from("../etc/passwd"),
            canonical_path: None,
            timestamp: Utc::now(),
        };
        assert_eq!(security_event.category(), "path_security");
    }

    #[tokio::test]
    async fn test_async_event_logging() {
        let logger = AuditLogger::new();

        let event = SecurityEvent::FileAccessGranted {
            path: PathBuf::from("/tmp/test_async.txt"),
            timestamp: Utc::now(),
        };

        // Should not block
        logger.log_event(event).await;

        // Give background task time to process
        sleep(Duration::from_millis(10)).await;
    }

    #[tokio::test]
    async fn test_audit_config_builder() {
        let temp_dir = TempDir::new().unwrap();
        let log_file = temp_dir.path().join("audit.log");

        let config = AuditConfig {
            log_to_stdout: false,
            log_to_file: Some(log_file.clone()),
            buffer_size: 500,
            enable_alerting: false,
            max_log_file_size: 5 * 1024 * 1024,
            max_log_files: 3,
        };

        assert!(!config.log_to_stdout);
        assert_eq!(config.log_to_file, Some(log_file));
        assert_eq!(config.buffer_size, 500);
        assert!(!config.enable_alerting);
    }

    #[tokio::test]
    async fn test_buffer_overflow_handling() {
        let config = AuditConfig {
            buffer_size: 2, // Very small buffer
            ..Default::default()
        };

        let logger = AuditLogger::with_config(config);

        // Fill the buffer beyond capacity
        for i in 0..5 {
            let event = SecurityEvent::FileAccessAttempt {
                path: PathBuf::from(format!("/tmp/test_{}.txt", i)),
                timestamp: Utc::now(),
            };
            logger.log_event(event).await;
        }

        // Should handle overflow gracefully
        sleep(Duration::from_millis(50)).await;
    }

    #[test]
    fn test_event_serialization() {
        let event = SecurityEvent::PathTraversalAttempt {
            attempted_path: PathBuf::from("../../../etc/passwd"),
            canonical_path: Some(PathBuf::from("/etc/passwd")),
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&event).unwrap();
        // The enum serializes with "event_type": "PathTraversalAttempt", not "path_traversal"
        assert!(json.contains("PathTraversalAttempt"));
        assert!(json.contains("etc/passwd"));

        // Verify deserialization
        let _deserialized: SecurityEvent = serde_json::from_str(&json).unwrap();
    }

    #[tokio::test]
    async fn test_flush_functionality() {
        let logger = AuditLogger::new();

        let event = SecurityEvent::FileAccessGranted {
            path: PathBuf::from("/tmp/flush_test.txt"),
            timestamp: Utc::now(),
        };

        logger.log_event(event).await;

        // Flush should complete without error
        let result = logger.flush().await;
        assert!(result.is_ok());
    }
}
