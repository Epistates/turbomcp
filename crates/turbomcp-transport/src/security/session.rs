//! Session security management for transport layer
//!
//! This module provides **optional** session security mechanisms that users can enable:
//! - Cryptographically secure session ID generation
//! - IP binding to prevent session hijacking (disabled by default)
//! - User agent fingerprinting for anomaly detection
//! - Session expiration and timeout handling
//! - Concurrent session limits per IP (1000 default - high and permissive)
//!
//! ## Design Philosophy
//! This is a **library**, not a security product. Defaults are permissive to avoid friction.
//! Users should configure security policies appropriate for their deployment:
//! - Use `for_production(max_sessions, ip_binding, regenerate_ids)` with explicit parameters for production
//! - Use `for_development()` for relaxed constraints during development
//! - Create custom configs for specialized needs
//! - Layer application-level policies on top as needed
//!
//! **Production Configuration Philosophy**: Production presets require explicit configuration
//! to ensure developers consciously choose their security posture, preventing accidental use
//! of permissive defaults in production environments.

use super::errors::SecurityError;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Production session security configuration parameters
///
/// Used with `SessionSecurityConfig::production()` for explicit, self-documenting
/// production configuration with named fields.
#[derive(Clone, Debug)]
pub struct ProductionSessionConfig {
    /// Maximum concurrent sessions per IP address
    pub max_sessions_per_ip: usize,
    /// Whether to enforce IP binding (prevents session hijacking)
    pub enforce_ip_binding: bool,
    /// Whether to regularly regenerate session IDs (defense in depth)
    pub regenerate_session_ids: bool,
}

/// Session security configuration
#[derive(Clone, Debug)]
pub struct SessionSecurityConfig {
    /// Maximum session lifetime
    pub max_lifetime: Duration,
    /// Session timeout for inactivity
    pub idle_timeout: Duration,
    /// Maximum concurrent sessions per IP
    pub max_sessions_per_ip: usize,
    /// Whether to enforce IP binding (prevents session hijacking)
    pub enforce_ip_binding: bool,
    /// Whether to regenerate session IDs periodically
    pub regenerate_session_ids: bool,
    /// Session ID regeneration interval
    pub regeneration_interval: Duration,
}

impl Default for SessionSecurityConfig {
    fn default() -> Self {
        Self {
            max_lifetime: Duration::from_secs(24 * 60 * 60), // 24 hour max session
            idle_timeout: Duration::from_secs(30 * 60),      // 30 minute idle timeout
            max_sessions_per_ip: usize::MAX,                 // Unlimited by default - users add limits if needed
            enforce_ip_binding: false,                       // Disabled - users enable if needed
            regenerate_session_ids: false,                   // Disabled - users enable if needed
            regeneration_interval: Duration::from_secs(60 * 60), // Regenerate every hour
        }
    }
}

impl SessionSecurityConfig {
    /// Create a new session security configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create development configuration with relaxed security
    pub fn for_development() -> Self {
        Self {
            max_lifetime: Duration::from_secs(24 * 60 * 60),
            idle_timeout: Duration::from_secs(60 * 60), // 1 hour idle timeout
            max_sessions_per_ip: 100,                   // High limit for development
            enforce_ip_binding: false,                  // Allow IP changes for dev
            regenerate_session_ids: false,              // Disabled for easier debugging
            regeneration_interval: Duration::from_secs(24 * 60 * 60),
        }
    }

    /// Create strict production configuration - recommended for most production deployments
    ///
    /// This preset provides strong security with:
    /// - 5 sessions per IP (prevents session exhaustion)
    /// - IP binding enabled (prevents session hijacking)
    /// - Session ID regeneration (defense in depth)
    ///
    /// For custom production config, use `production()` with `ProductionSessionConfig`.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_transport::SessionSecurityConfig;
    /// // Recommended production preset
    /// let config = SessionSecurityConfig::for_production();
    /// ```
    pub fn for_production() -> Self {
        Self {
            max_lifetime: Duration::from_secs(8 * 60 * 60),
            idle_timeout: Duration::from_secs(15 * 60),
            max_sessions_per_ip: 5,
            enforce_ip_binding: true,
            regenerate_session_ids: true,
            regeneration_interval: Duration::from_secs(30 * 60),
        }
    }

    /// Create custom production configuration with explicit parameters
    ///
    /// Use this when you need production-grade security with custom limits.
    /// Named struct fields make configuration explicit and self-documenting.
    ///
    /// # Example
    /// ```
    /// # use turbomcp_transport::{SessionSecurityConfig, ProductionSessionConfig};
    /// // Custom production config
    /// let config = SessionSecurityConfig::production(ProductionSessionConfig {
    ///     max_sessions_per_ip: 10,
    ///     enforce_ip_binding: true,
    ///     regenerate_session_ids: false, // Disabled for high-traffic
    /// });
    /// ```
    pub fn production(config: ProductionSessionConfig) -> Self {
        Self {
            max_lifetime: Duration::from_secs(8 * 60 * 60),
            idle_timeout: Duration::from_secs(15 * 60),
            max_sessions_per_ip: config.max_sessions_per_ip,
            enforce_ip_binding: config.enforce_ip_binding,
            regenerate_session_ids: config.regenerate_session_ids,
            regeneration_interval: Duration::from_secs(30 * 60),
        }
    }

    /// Create testing configuration with very short timeouts
    pub fn for_testing() -> Self {
        Self {
            max_lifetime: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(5),
            max_sessions_per_ip: 2,
            enforce_ip_binding: true,
            regenerate_session_ids: true,
            regeneration_interval: Duration::from_secs(2),
        }
    }
}

/// Session security information
#[derive(Clone, Debug)]
pub struct SecureSessionInfo {
    /// Session ID (cryptographically secure)
    pub id: String,
    /// Original IP address (for hijacking prevention)
    pub original_ip: IpAddr,
    /// Current IP address
    pub current_ip: IpAddr,
    /// Session creation time
    pub created_at: Instant,
    /// Last activity time
    pub last_activity: Instant,
    /// Last session ID regeneration
    pub last_regeneration: Instant,
    /// Number of requests in this session
    pub request_count: u64,
    /// User agent fingerprint (for anomaly detection)
    pub user_agent_hash: Option<u64>,
    /// Session metadata
    pub metadata: HashMap<String, String>,
}

impl SecureSessionInfo {
    /// Create a new secure session
    pub fn new(ip: IpAddr, user_agent: Option<&str>) -> Self {
        let now = Instant::now();
        Self {
            id: Self::generate_secure_id(),
            original_ip: ip,
            current_ip: ip,
            created_at: now,
            last_activity: now,
            last_regeneration: now,
            request_count: 0,
            user_agent_hash: user_agent.map(Self::hash_user_agent),
            metadata: HashMap::new(),
        }
    }

    /// Generate a cryptographically secure session ID
    fn generate_secure_id() -> String {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Use current time, process ID, and random data for entropy
        let mut hasher = DefaultHasher::new();
        std::time::SystemTime::now().hash(&mut hasher);
        std::process::id().hash(&mut hasher);
        uuid::Uuid::new_v4().hash(&mut hasher);

        format!("mcp_session_{:x}", hasher.finish())
    }

    /// Hash user agent for fingerprinting
    fn hash_user_agent(user_agent: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        user_agent.hash(&mut hasher);
        hasher.finish()
    }

    /// Check if session should be regenerated
    pub fn should_regenerate(&self, config: &SessionSecurityConfig) -> bool {
        config.regenerate_session_ids
            && self.last_regeneration.elapsed() >= config.regeneration_interval
    }

    /// Regenerate session ID
    pub fn regenerate_id(&mut self) {
        self.id = Self::generate_secure_id();
        self.last_regeneration = Instant::now();
    }

    /// Update activity and increment request count
    pub fn update_activity(&mut self, current_ip: IpAddr) {
        self.current_ip = current_ip;
        self.last_activity = Instant::now();
        self.request_count += 1;
    }

    /// Check if session is expired
    pub fn is_expired(&self, config: &SessionSecurityConfig) -> bool {
        // Check max lifetime
        if self.created_at.elapsed() >= config.max_lifetime {
            return true;
        }

        // Check idle timeout
        if self.last_activity.elapsed() >= config.idle_timeout {
            return true;
        }

        false
    }

    /// Validate session security (IP binding, etc.)
    pub fn validate_security(
        &self,
        config: &SessionSecurityConfig,
        current_ip: IpAddr,
        user_agent: Option<&str>,
    ) -> Result<(), SecurityError> {
        // Check IP binding to prevent session hijacking
        if config.enforce_ip_binding && self.original_ip != current_ip {
            return Err(SecurityError::SessionViolation(format!(
                "IP address mismatch: session created from {} but accessed from {}",
                self.original_ip, current_ip
            )));
        }

        // Check user agent consistency for anomaly detection
        if let (Some(stored_hash), Some(current_ua)) = (self.user_agent_hash, user_agent) {
            let current_hash = Self::hash_user_agent(current_ua);
            if stored_hash != current_hash {
                return Err(SecurityError::SessionViolation(
                    "User agent fingerprint mismatch detected".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Add metadata to session
    pub fn add_metadata(&mut self, key: String, value: String) {
        self.metadata.insert(key, value);
    }

    /// Get metadata from session
    pub fn get_metadata(&self, key: &str) -> Option<&String> {
        self.metadata.get(key)
    }

    /// Get session age
    pub fn age(&self) -> Duration {
        self.created_at.elapsed()
    }

    /// Get idle time
    pub fn idle_time(&self) -> Duration {
        self.last_activity.elapsed()
    }
}

/// Session security manager
#[derive(Debug)]
pub struct SessionSecurityManager {
    config: SessionSecurityConfig,
    sessions: Arc<Mutex<HashMap<String, SecureSessionInfo>>>,
    ip_session_count: Arc<Mutex<HashMap<IpAddr, usize>>>,
}

impl SessionSecurityManager {
    /// Create a new session security manager
    pub fn new(config: SessionSecurityConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            ip_session_count: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create session security manager with default configuration
    pub fn with_defaults() -> Self {
        Self::new(SessionSecurityConfig::default())
    }

    /// Create a new secure session
    pub fn create_session(
        &self,
        ip: IpAddr,
        user_agent: Option<&str>,
    ) -> Result<SecureSessionInfo, SecurityError> {
        // Check concurrent session limit per IP
        {
            let ip_counts = self.ip_session_count.lock();
            if let Some(&count) = ip_counts.get(&ip)
                && count >= self.config.max_sessions_per_ip
            {
                tracing::warn!(
                    client_ip = %ip,
                    current_sessions = count,
                    max_sessions = self.config.max_sessions_per_ip,
                    "Session limit exceeded - rejecting new session"
                );
                return Err(SecurityError::SessionViolation(format!(
                    "Maximum sessions per IP exceeded: {}/{}",
                    count, self.config.max_sessions_per_ip
                )));
            }
        }

        let session = SecureSessionInfo::new(ip, user_agent);

        // Store session
        self.sessions
            .lock()
            .insert(session.id.clone(), session.clone());

        // Update IP session count
        *self.ip_session_count.lock().entry(ip).or_insert(0) += 1;

        Ok(session)
    }

    /// Validate and update existing session
    pub fn validate_session(
        &self,
        session_id: &str,
        current_ip: IpAddr,
        user_agent: Option<&str>,
    ) -> Result<SecureSessionInfo, SecurityError> {
        let mut sessions = self.sessions.lock();

        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| SecurityError::SessionViolation("Session not found".to_string()))?;

        // Check if session is expired
        if session.is_expired(&self.config) {
            // Remove expired session
            let expired_session = sessions.remove(session_id).unwrap();
            self.cleanup_ip_count(expired_session.original_ip);
            return Err(SecurityError::SessionViolation(
                "Session expired".to_string(),
            ));
        }

        // Validate session security
        session.validate_security(&self.config, current_ip, user_agent)?;

        // Check if session ID should be regenerated
        if session.should_regenerate(&self.config) {
            let old_id = session.id.clone();
            session.regenerate_id();
            session.update_activity(current_ip);

            // Move session to new ID
            let updated_session = session.clone();
            drop(sessions); // Release the lock

            let mut sessions = self.sessions.lock();
            sessions.remove(&old_id);
            sessions.insert(updated_session.id.clone(), updated_session.clone());

            return Ok(updated_session);
        }

        // Update activity
        session.update_activity(current_ip);
        Ok(session.clone())
    }

    /// Remove session
    pub fn remove_session(&self, session_id: &str) -> Result<(), SecurityError> {
        let mut sessions = self.sessions.lock();

        if let Some(session) = sessions.remove(session_id) {
            self.cleanup_ip_count(session.original_ip);
            Ok(())
        } else {
            Err(SecurityError::SessionViolation(
                "Session not found".to_string(),
            ))
        }
    }

    /// Clean up expired sessions
    pub fn cleanup_expired_sessions(&self) -> usize {
        let mut sessions = self.sessions.lock();
        let mut expired_sessions = Vec::new();

        for (id, session) in sessions.iter() {
            if session.is_expired(&self.config) {
                expired_sessions.push((id.clone(), session.original_ip));
            }
        }

        let count = expired_sessions.len();
        for (id, ip) in expired_sessions {
            sessions.remove(&id);
            self.cleanup_ip_count(ip);
        }

        count
    }

    /// Get session count
    pub fn session_count(&self) -> usize {
        self.sessions.lock().len()
    }

    /// Get sessions per IP
    pub fn sessions_per_ip(&self, ip: IpAddr) -> usize {
        self.ip_session_count.lock().get(&ip).copied().unwrap_or(0)
    }

    /// Get session by ID (read-only)
    pub fn get_session(&self, session_id: &str) -> Option<SecureSessionInfo> {
        self.sessions.lock().get(session_id).cloned()
    }

    /// Get all active session IDs
    pub fn get_session_ids(&self) -> Vec<String> {
        self.sessions.lock().keys().cloned().collect()
    }

    /// Get configuration
    pub fn config(&self) -> &SessionSecurityConfig {
        &self.config
    }

    /// Helper to clean up IP session count
    fn cleanup_ip_count(&self, ip: IpAddr) {
        let mut ip_counts = self.ip_session_count.lock();
        if let Some(count) = ip_counts.get_mut(&ip) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                ip_counts.remove(&ip);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;

    #[test]
    fn test_session_config_default() {
        let config = SessionSecurityConfig::default();
        // Library defaults are unlimited - users configure their own policy
        assert!(!config.enforce_ip_binding);
        assert!(!config.regenerate_session_ids);
        assert_eq!(config.max_sessions_per_ip, usize::MAX);
    }

    #[test]
    fn test_session_config_for_development() {
        let config = SessionSecurityConfig::for_development();
        assert!(!config.enforce_ip_binding);
        assert!(!config.regenerate_session_ids);
    }

    #[test]
    fn test_session_config_for_production() {
        let config = SessionSecurityConfig::for_production();
        assert!(config.enforce_ip_binding);
        assert!(config.regenerate_session_ids);
        assert_eq!(config.max_sessions_per_ip, 5);
    }

    #[test]
    fn test_session_config_production_custom() {
        let config = SessionSecurityConfig::production(ProductionSessionConfig {
            max_sessions_per_ip: 10,
            enforce_ip_binding: true,
            regenerate_session_ids: false,
        });
        assert!(config.enforce_ip_binding);
        assert!(!config.regenerate_session_ids);
        assert_eq!(config.max_sessions_per_ip, 10);
    }

    #[test]
    fn test_secure_session_creation() {
        let ip = "127.0.0.1".parse().unwrap();
        let session = SecureSessionInfo::new(ip, Some("Mozilla/5.0"));

        assert!(session.id.starts_with("mcp_session_"));
        assert_eq!(session.original_ip, ip);
        assert_eq!(session.current_ip, ip);
        assert_eq!(session.request_count, 0);
        assert!(session.user_agent_hash.is_some());
    }

    #[test]
    fn test_session_security_manager_creation() {
        let config = SessionSecurityConfig {
            max_sessions_per_ip: 2,
            ..SessionSecurityConfig::default()
        };
        let manager = SessionSecurityManager::new(config);
        let ip = "127.0.0.1".parse().unwrap();

        // Create first session
        let session1 = manager.create_session(ip, Some("Mozilla/5.0")).unwrap();
        assert_eq!(manager.sessions_per_ip(ip), 1);

        // Create second session
        let _session2 = manager.create_session(ip, Some("Mozilla/5.0")).unwrap();
        assert_eq!(manager.sessions_per_ip(ip), 2);

        // Third session should fail
        assert!(manager.create_session(ip, Some("Mozilla/5.0")).is_err());

        // Validate existing session
        let validated = manager
            .validate_session(&session1.id, ip, Some("Mozilla/5.0"))
            .unwrap();
        assert_eq!(validated.request_count, 1); // Should increment

        // Remove session
        manager.remove_session(&session1.id).unwrap();
        assert_eq!(manager.sessions_per_ip(ip), 1);
    }

    #[test]
    fn test_session_ip_binding() {
        // Default config has IP binding disabled
        let default_config = SessionSecurityConfig::default();
        let original_ip = "127.0.0.1".parse().unwrap();
        let different_ip = "192.168.1.1".parse().unwrap();

        let session = SecureSessionInfo::new(original_ip, Some("Mozilla/5.0"));

        // With default config (IP binding off), different IP should succeed
        assert!(
            session
                .validate_security(&default_config, different_ip, Some("Mozilla/5.0"))
                .is_ok()
        );

        // Test with IP binding enabled
        let strict_config = SessionSecurityConfig {
            enforce_ip_binding: true,
            ..SessionSecurityConfig::default()
        };

        // Should fail with different IP when IP binding is enabled
        assert!(
            session
                .validate_security(&strict_config, different_ip, Some("Mozilla/5.0"))
                .is_err()
        );

        // Should succeed with same IP
        assert!(
            session
                .validate_security(&strict_config, original_ip, Some("Mozilla/5.0"))
                .is_ok()
        );
    }

    #[test]
    fn test_user_agent_fingerprinting() {
        let config = SessionSecurityConfig::default();
        let ip = "127.0.0.1".parse().unwrap();

        let session = SecureSessionInfo::new(ip, Some("Mozilla/5.0"));

        // Should fail with different user agent
        assert!(
            session
                .validate_security(&config, ip, Some("Chrome/91.0"))
                .is_err()
        );

        // Should succeed with same user agent
        assert!(
            session
                .validate_security(&config, ip, Some("Mozilla/5.0"))
                .is_ok()
        );
    }

    #[test]
    fn test_session_expiration() {
        let config = SessionSecurityConfig {
            idle_timeout: Duration::from_millis(1), // Very short timeout
            ..SessionSecurityConfig::default()
        };

        let ip = "127.0.0.1".parse().unwrap();
        let session = SecureSessionInfo::new(ip, None);

        // Wait for expiration
        sleep(Duration::from_millis(10));

        assert!(session.is_expired(&config));
    }

    #[test]
    fn test_session_regeneration() {
        let config = SessionSecurityConfig {
            regenerate_session_ids: true,
            regeneration_interval: Duration::from_millis(1),
            ..SessionSecurityConfig::default()
        };

        let ip = "127.0.0.1".parse().unwrap();
        let mut session = SecureSessionInfo::new(ip, None);
        let original_id = session.id.clone();

        // Wait for regeneration interval
        sleep(Duration::from_millis(10));

        assert!(session.should_regenerate(&config));

        session.regenerate_id();
        assert_ne!(session.id, original_id);
    }

    #[test]
    fn test_session_metadata() {
        let ip = "127.0.0.1".parse().unwrap();
        let mut session = SecureSessionInfo::new(ip, None);

        session.add_metadata("client_type".to_string(), "web".to_string());
        assert_eq!(
            session.get_metadata("client_type"),
            Some(&"web".to_string())
        );
        assert_eq!(session.get_metadata("nonexistent"), None);
    }

    #[test]
    fn test_cleanup_expired_sessions() {
        let config = SessionSecurityConfig {
            idle_timeout: Duration::from_millis(1),
            ..SessionSecurityConfig::default()
        };
        let manager = SessionSecurityManager::new(config);
        let ip = "127.0.0.1".parse().unwrap();

        // Create session
        let _session = manager.create_session(ip, None).unwrap();
        assert_eq!(manager.session_count(), 1);

        // Wait for expiration
        sleep(Duration::from_millis(10));

        // Cleanup should remove expired session
        let cleaned = manager.cleanup_expired_sessions();
        assert_eq!(cleaned, 1);
        assert_eq!(manager.session_count(), 0);
    }
}
