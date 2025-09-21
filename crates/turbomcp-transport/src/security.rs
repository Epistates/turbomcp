//! Security module for transport layer
//!
//! Implements critical security validations required by MCP specification:
//! - Origin header validation to prevent DNS rebinding attacks
//! - Authentication framework with API key support
//! - Rate limiting to prevent abuse
//! - Session security improvements (hijacking prevention, secure ID generation, timeout enforcement)

use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Generic header representation for cross-framework compatibility
pub type HeaderValue = String;
/// Security headers type for HTTP requests
pub type SecurityHeaders = HashMap<String, HeaderValue>;

/// Security-related errors
#[derive(Error, Debug)]
pub enum SecurityError {
    /// Origin header validation failed
    #[error("Origin header validation failed: {0}")]
    InvalidOrigin(String),

    /// Authentication failed
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Rate limit exceeded for client
    #[error("Rate limit exceeded for {client}: {current}/{limit} requests")]
    RateLimitExceeded {
        /// Client identifier
        client: String,
        /// Current request count
        current: usize,
        /// Rate limit threshold
        limit: usize,
    },

    /// Session security violation
    #[error("Session security violation: {0}")]
    SessionViolation(String),

    /// Message too large
    #[error("Message too large: {size} bytes exceeds limit of {limit} bytes")]
    MessageTooLarge {
        /// Message size in bytes
        size: usize,
        /// Size limit in bytes
        limit: usize,
    },
}

/// Origin validation configuration
#[derive(Clone, Debug)]
pub struct OriginConfig {
    /// Allowed origins for CORS
    pub allowed_origins: HashSet<String>,
    /// Whether to allow localhost origins (for development)
    pub allow_localhost: bool,
    /// Whether to allow any origin (DANGEROUS - only for testing)
    pub allow_any: bool,
}

impl Default for OriginConfig {
    fn default() -> Self {
        Self {
            allowed_origins: HashSet::new(),
            allow_localhost: true,
            allow_any: false,
        }
    }
}

/// Authentication configuration
#[derive(Clone, Debug)]
pub struct AuthConfig {
    /// Whether authentication is required
    pub require_auth: bool,
    /// Valid API keys for authentication
    pub api_keys: HashSet<String>,
    /// Authentication method
    pub method: AuthMethod,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            require_auth: false,
            api_keys: HashSet::new(),
            method: AuthMethod::Bearer,
        }
    }
}

/// Authentication methods
#[derive(Clone, Debug)]
pub enum AuthMethod {
    /// Bearer token authentication
    Bearer,
    /// API key in Authorization header
    ApiKey,
    /// Custom header authentication
    Custom(String),
}

/// Rate limiting configuration
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    /// Maximum requests per window
    pub max_requests: usize,
    /// Time window for rate limiting
    pub window: Duration,
    /// Whether rate limiting is enabled
    pub enabled: bool,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window: Duration::from_secs(60),
            enabled: true,
        }
    }
}

/// Rate limiter state
#[derive(Debug)]
struct RateLimiterState {
    requests: std::collections::HashMap<IpAddr, Vec<Instant>>,
}

/// Rate limiter implementation
#[derive(Debug)]
pub struct RateLimiter {
    config: RateLimitConfig,
    state: Arc<Mutex<RateLimiterState>>,
}

impl RateLimiter {
    /// Create a new rate limiter with the given configuration
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(RateLimiterState {
                requests: std::collections::HashMap::new(),
            })),
        }
    }

    /// Check if request is within rate limits
    pub fn check_rate_limit(&self, client_ip: IpAddr) -> Result<(), SecurityError> {
        if !self.config.enabled {
            return Ok(());
        }

        let mut state = self.state.lock();
        let now = Instant::now();

        let requests = state.requests.entry(client_ip).or_default();

        // Remove old requests outside the window
        requests.retain(|&time| now.duration_since(time) < self.config.window);

        if requests.len() >= self.config.max_requests {
            return Err(SecurityError::RateLimitExceeded {
                client: client_ip.to_string(),
                current: requests.len(),
                limit: self.config.max_requests,
            });
        }

        requests.push(now);
        Ok(())
    }
}

/// Security validator for HTTP requests
#[derive(Debug)]
pub struct SecurityValidator {
    origin_config: OriginConfig,
    auth_config: AuthConfig,
    rate_limiter: Option<RateLimiter>,
}

impl SecurityValidator {
    /// Create a new security validator
    pub fn new(
        origin_config: OriginConfig,
        auth_config: AuthConfig,
        rate_limit_config: Option<RateLimitConfig>,
    ) -> Self {
        let rate_limiter = rate_limit_config.map(RateLimiter::new);

        Self {
            origin_config,
            auth_config,
            rate_limiter,
        }
    }

    /// Validate Origin header to prevent DNS rebinding attacks
    ///
    /// Per MCP 2025-06-18 specification:
    /// "Servers MUST validate the Origin header on all incoming connections
    /// to prevent DNS rebinding attacks"
    pub fn validate_origin(&self, headers: &SecurityHeaders) -> Result<(), SecurityError> {
        if self.origin_config.allow_any {
            return Ok(());
        }

        let origin = headers
            .get("Origin")
            .ok_or_else(|| SecurityError::InvalidOrigin("Missing Origin header".to_string()))?;

        // Allow explicitly configured origins
        if self.origin_config.allowed_origins.contains(origin) {
            return Ok(());
        }

        // Allow localhost origins for development
        if self.origin_config.allow_localhost {
            let localhost_patterns = [
                "http://localhost",
                "https://localhost",
                "http://127.0.0.1",
                "https://127.0.0.1",
            ];

            if localhost_patterns
                .iter()
                .any(|&pattern| origin.starts_with(pattern))
            {
                return Ok(());
            }
        }

        Err(SecurityError::InvalidOrigin(format!(
            "Origin '{}' not allowed",
            origin
        )))
    }

    /// Validate authentication credentials
    pub fn validate_authentication(&self, headers: &SecurityHeaders) -> Result<(), SecurityError> {
        if !self.auth_config.require_auth {
            return Ok(());
        }

        let auth_header = headers.get("Authorization").ok_or_else(|| {
            SecurityError::AuthenticationFailed("Missing Authorization header".to_string())
        })?;

        match self.auth_config.method {
            AuthMethod::Bearer => {
                if !auth_header.starts_with("Bearer ") {
                    return Err(SecurityError::AuthenticationFailed(
                        "Invalid Authorization format, expected Bearer token".to_string(),
                    ));
                }

                let token = &auth_header[7..];
                if !self.auth_config.api_keys.contains(token) {
                    return Err(SecurityError::AuthenticationFailed(
                        "Invalid bearer token".to_string(),
                    ));
                }
            }
            AuthMethod::ApiKey => {
                if !auth_header.starts_with("ApiKey ") {
                    return Err(SecurityError::AuthenticationFailed(
                        "Invalid Authorization format, expected ApiKey".to_string(),
                    ));
                }

                let key = &auth_header[7..];
                if !self.auth_config.api_keys.contains(key) {
                    return Err(SecurityError::AuthenticationFailed(
                        "Invalid API key".to_string(),
                    ));
                }
            }
            AuthMethod::Custom(ref header_name) => {
                let custom_value = headers.get(header_name).ok_or_else(|| {
                    SecurityError::AuthenticationFailed(format!("Missing {} header", header_name))
                })?;

                if !self.auth_config.api_keys.contains(custom_value) {
                    return Err(SecurityError::AuthenticationFailed(format!(
                        "Invalid {} value",
                        header_name
                    )));
                }
            }
        }

        Ok(())
    }

    /// Check rate limits for a client IP
    pub fn check_rate_limit(&self, client_ip: IpAddr) -> Result<(), SecurityError> {
        if let Some(ref rate_limiter) = self.rate_limiter {
            rate_limiter.check_rate_limit(client_ip)?;
        }
        Ok(())
    }

    /// Comprehensive security validation for HTTP requests
    pub fn validate_request(
        &self,
        headers: &SecurityHeaders,
        client_ip: IpAddr,
    ) -> Result<(), SecurityError> {
        // 1. Validate Origin header (DNS rebinding protection)
        self.validate_origin(headers)?;

        // 2. Validate authentication
        self.validate_authentication(headers)?;

        // 3. Check rate limits
        self.check_rate_limit(client_ip)?;

        Ok(())
    }
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
            max_sessions_per_ip: 10,                         // Max 10 sessions per IP
            enforce_ip_binding: true,                        // Prevent session hijacking
            regenerate_session_ids: true,                    // Prevent session fixation
            regeneration_interval: Duration::from_secs(60 * 60), // Regenerate every hour
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
}

/// Session security manager
#[derive(Debug)]
pub struct SessionSecurityManager {
    config: SessionSecurityConfig,
    sessions: Arc<Mutex<HashMap<String, SecureSessionInfo>>>,
    ip_session_count: Arc<Mutex<HashMap<IpAddr, usize>>>,
}

impl SessionSecurityManager {
    /// Create a new session security validator
    pub fn new(config: SessionSecurityConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            ip_session_count: Arc::new(Mutex::new(HashMap::new())),
        }
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

/// Convert SecurityError to appropriate HTTP status code (for HTTP transports)
impl SecurityError {
    /// Convert security error to HTTP status code
    pub fn to_http_status(&self) -> u16 {
        match self {
            SecurityError::InvalidOrigin(_) => 403,         // Forbidden
            SecurityError::AuthenticationFailed(_) => 401,  // Unauthorized
            SecurityError::RateLimitExceeded { .. } => 429, // Too Many Requests
            SecurityError::SessionViolation(_) => 403,      // Forbidden
            SecurityError::MessageTooLarge { .. } => 413,   // Payload Too Large
        }
    }
}

/// Message size validation
pub fn validate_message_size(data: &[u8], max_size: usize) -> Result<(), SecurityError> {
    if data.len() > max_size {
        return Err(SecurityError::MessageTooLarge {
            size: data.len(),
            limit: max_size,
        });
    }
    Ok(())
}

/// Security configuration builder
#[derive(Debug)]
pub struct SecurityConfigBuilder {
    origin_config: OriginConfig,
    auth_config: AuthConfig,
    rate_limit_config: Option<RateLimitConfig>,
}

impl Default for SecurityConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl SecurityConfigBuilder {
    /// Create a new security configuration builder
    pub fn new() -> Self {
        Self {
            origin_config: OriginConfig::default(),
            auth_config: AuthConfig::default(),
            rate_limit_config: Some(RateLimitConfig::default()),
        }
    }

    /// Set allowed origins for CORS
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.origin_config.allowed_origins = origins.into_iter().collect();
        self
    }

    /// Allow localhost origins (localhost and 127.0.0.1)
    pub fn allow_localhost(mut self, allow: bool) -> Self {
        self.origin_config.allow_localhost = allow;
        self
    }

    /// Allow any origin (wildcard '*' - use with caution in production)
    pub fn allow_any_origin(mut self, allow: bool) -> Self {
        self.origin_config.allow_any = allow;
        self
    }

    /// Require authentication
    pub fn require_authentication(mut self, require: bool) -> Self {
        self.auth_config.require_auth = require;
        self
    }

    /// Set API keys for authentication
    pub fn with_api_keys(mut self, keys: Vec<String>) -> Self {
        self.auth_config.api_keys = keys.into_iter().collect();
        self
    }

    /// Set authentication method
    pub fn with_auth_method(mut self, method: AuthMethod) -> Self {
        self.auth_config.method = method;
        self
    }

    /// Set rate limiting parameters
    pub fn with_rate_limit(mut self, max_requests: usize, window: Duration) -> Self {
        self.rate_limit_config = Some(RateLimitConfig {
            max_requests,
            window,
            enabled: true,
        });
        self
    }

    /// Disable rate limiting
    pub fn disable_rate_limiting(mut self) -> Self {
        self.rate_limit_config = None;
        self
    }

    /// Build the security validator
    pub fn build(self) -> SecurityValidator {
        SecurityValidator::new(self.origin_config, self.auth_config, self.rate_limit_config)
    }
}

/// Enhanced security configuration builder for session security
#[derive(Debug)]
pub struct EnhancedSecurityConfigBuilder {
    security_config: SecurityConfigBuilder,
    session_config: SessionSecurityConfig,
}

impl Default for EnhancedSecurityConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl EnhancedSecurityConfigBuilder {
    /// Create a new enhanced security configuration builder
    pub fn new() -> Self {
        Self {
            security_config: SecurityConfigBuilder::new(),
            session_config: SessionSecurityConfig::default(),
        }
    }

    /// Configure basic security settings
    /// Set allowed origins for CORS
    pub fn with_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.security_config = self.security_config.with_allowed_origins(origins);
        self
    }

    /// Allow localhost origins (localhost and 127.0.0.1)
    pub fn allow_localhost(mut self, allow: bool) -> Self {
        self.security_config = self.security_config.allow_localhost(allow);
        self
    }

    /// Allow any origin (wildcard '*' - use with caution in production)
    pub fn allow_any_origin(mut self, allow: bool) -> Self {
        self.security_config = self.security_config.allow_any_origin(allow);
        self
    }

    /// Require authentication
    pub fn require_authentication(mut self, require: bool) -> Self {
        self.security_config = self.security_config.require_authentication(require);
        self
    }

    /// Set API keys for authentication
    pub fn with_api_keys(mut self, keys: Vec<String>) -> Self {
        self.security_config = self.security_config.with_api_keys(keys);
        self
    }

    /// Set rate limiting parameters
    pub fn with_rate_limit(mut self, max_requests: usize, window: Duration) -> Self {
        self.security_config = self.security_config.with_rate_limit(max_requests, window);
        self
    }

    /// Configure session security settings
    pub fn with_session_max_lifetime(mut self, lifetime: Duration) -> Self {
        self.session_config.max_lifetime = lifetime;
        self
    }

    /// Set session idle timeout
    pub fn with_session_idle_timeout(mut self, timeout: Duration) -> Self {
        self.session_config.idle_timeout = timeout;
        self
    }

    /// Set maximum sessions per IP
    pub fn with_max_sessions_per_ip(mut self, max_sessions: usize) -> Self {
        self.session_config.max_sessions_per_ip = max_sessions;
        self
    }

    /// Enforce IP binding for sessions
    pub fn enforce_ip_binding(mut self, enforce: bool) -> Self {
        self.session_config.enforce_ip_binding = enforce;
        self
    }

    /// Enable session ID regeneration
    pub fn enable_session_id_regeneration(mut self, enable: bool, interval: Duration) -> Self {
        self.session_config.regenerate_session_ids = enable;
        self.session_config.regeneration_interval = interval;
        self
    }

    /// Build enhanced security configuration
    pub fn build(self) -> (SecurityValidator, SessionSecurityManager) {
        let validator = self.security_config.build();
        let session_manager = SessionSecurityManager::new(self.session_config);
        (validator, session_manager)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_origin_validation_allows_localhost() {
        let validator = SecurityConfigBuilder::new().allow_localhost(true).build();

        let mut headers = SecurityHeaders::new();
        headers.insert("Origin".to_string(), "http://localhost:3000".to_string());

        assert!(validator.validate_origin(&headers).is_ok());
    }

    #[test]
    fn test_origin_validation_blocks_evil_origin() {
        let validator = SecurityConfigBuilder::new().allow_localhost(true).build();

        let mut headers = SecurityHeaders::new();
        headers.insert("Origin".to_string(), "http://evil.com".to_string());

        assert!(validator.validate_origin(&headers).is_err());
    }

    #[test]
    fn test_bearer_authentication() {
        let validator = SecurityConfigBuilder::new()
            .require_authentication(true)
            .with_api_keys(vec!["secret123".to_string()])
            .with_auth_method(AuthMethod::Bearer)
            .build();

        let mut headers = SecurityHeaders::new();
        headers.insert("Authorization".to_string(), "Bearer secret123".to_string());

        assert!(validator.validate_authentication(&headers).is_ok());
    }

    #[test]
    fn test_rate_limiting() {
        let rate_limiter = RateLimiter::new(RateLimitConfig {
            max_requests: 2,
            window: Duration::from_secs(60),
            enabled: true,
        });

        let client_ip = "127.0.0.1".parse().unwrap();

        // First two requests should succeed
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());
        assert!(rate_limiter.check_rate_limit(client_ip).is_ok());

        // Third request should fail
        assert!(rate_limiter.check_rate_limit(client_ip).is_err());
    }

    #[test]
    fn test_message_size_validation() {
        let small_message = b"small";
        let large_message = vec![0u8; 2000]; // 2KB message

        assert!(validate_message_size(small_message, 1024 * 1024).is_ok()); // 1MB limit
        assert!(validate_message_size(&large_message, 1024).is_err()); // 1KB limit
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
    fn test_session_security_manager() {
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
        let config = SessionSecurityConfig::default();
        let original_ip = "127.0.0.1".parse().unwrap();
        let different_ip = "192.168.1.1".parse().unwrap();

        let session = SecureSessionInfo::new(original_ip, Some("Mozilla/5.0"));

        // Should fail with different IP
        assert!(
            session
                .validate_security(&config, different_ip, Some("Mozilla/5.0"))
                .is_err()
        );

        // Should succeed with same IP
        assert!(
            session
                .validate_security(&config, original_ip, Some("Mozilla/5.0"))
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
        std::thread::sleep(Duration::from_millis(10));

        assert!(session.is_expired(&config));
    }

    #[test]
    fn test_enhanced_security_builder() {
        let (validator, session_manager) = EnhancedSecurityConfigBuilder::new()
            .allow_localhost(true)
            .with_max_sessions_per_ip(5)
            .with_session_idle_timeout(Duration::from_secs(15 * 60))
            .enforce_ip_binding(true)
            .build();

        let ip = "127.0.0.1".parse().unwrap();

        // Test session creation
        let session = session_manager
            .create_session(ip, Some("test-agent"))
            .unwrap();
        assert!(session.id.starts_with("mcp_session_"));

        // Test validation
        let mut headers = crate::security::SecurityHeaders::new();
        headers.insert("Origin".to_string(), "http://localhost:3000".to_string());
        assert!(validator.validate_origin(&headers).is_ok());
    }
}
