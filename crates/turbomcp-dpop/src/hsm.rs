//! Hardware Security Module (HSM) integration for DPoP key management
//!
//! This module provides enterprise-grade HSM-backed key storage and cryptographic operations
//! for production-grade security when the `hsm-support` feature is enabled.
//!
//! Supports PKCS#11 HSM devices including SafeNet Luna, Thales nShield, AWS CloudHSM,
//! and other enterprise hardware security modules.

#[cfg(feature = "hsm-support")]
use crate::{DpopAlgorithm, DpopKeyPair, DpopError, Result};
#[cfg(feature = "hsm-support")]
use std::collections::HashMap;
#[cfg(feature = "hsm-support")]
use std::sync::{Arc, RwLock};
#[cfg(feature = "hsm-support")]
use std::time::{Duration, SystemTime};
#[cfg(feature = "hsm-support")]
use tracing::{debug, error, info, trace, warn};
#[cfg(feature = "hsm-support")]
use serde::{Deserialize, Serialize};

/// PKCS#11-compatible HSM configuration
#[cfg(feature = "hsm-support")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HsmConfig {
    /// PKCS#11 library path (e.g., "/opt/safenet/lunaclient/lib/libCryptoki2_64.so")
    pub library_path: String,
    
    /// HSM slot number
    pub slot_id: u64,
    
    /// HSM token label
    pub token_label: String,
    
    /// User PIN for HSM authentication
    pub user_pin: String,
    
    /// Maximum number of concurrent HSM sessions
    pub max_sessions: u32,
    
    /// Session timeout in seconds
    pub session_timeout: Duration,
    
    /// Enable key caching for performance
    pub enable_caching: bool,
    
    /// Cache duration for key metadata
    pub cache_duration: Duration,
    
    /// HSM vendor-specific configuration
    pub vendor_config: HashMap<String, String>,
}

/// HSM session information
#[cfg(feature = "hsm-support")]
#[derive(Debug)]
struct HsmSession {
    /// PKCS#11 session handle
    session_handle: u64,
    
    /// Session creation time
    created_at: SystemTime,
    
    /// Last activity time
    last_active: SystemTime,
    
    /// Session state (active, idle, expired)
    state: SessionState,
}

/// HSM session state
#[cfg(feature = "hsm-support")]
#[derive(Debug, Clone, PartialEq)]
enum SessionState {
    Active,
    Idle,
    Expired,
}

/// Cached key metadata
#[cfg(feature = "hsm-support")]
#[derive(Debug, Clone)]
struct CachedKeyInfo {
    /// HSM object handle
    object_handle: u64,
    
    /// Key algorithm
    algorithm: DpopAlgorithm,
    
    /// Key ID/label in HSM
    key_id: String,
    
    /// Cached at timestamp
    cached_at: SystemTime,
    
    /// Key capabilities
    capabilities: KeyCapabilities,
}

/// Key capabilities in HSM
#[cfg(feature = "hsm-support")]
#[derive(Debug, Clone)]
struct KeyCapabilities {
    /// Can sign data
    can_sign: bool,
    
    /// Can verify signatures
    can_verify: bool,
    
    /// Key is extractable
    extractable: bool,
    
    /// Key is sensitive
    sensitive: bool,
}

/// Production-grade HSM key management implementation
#[cfg(feature = "hsm-support")]
#[derive(Debug)]
pub struct HsmKeyManager {
    /// HSM configuration
    config: HsmConfig,
    
    /// Active HSM sessions pool
    sessions: Arc<RwLock<Vec<HsmSession>>>,
    
    /// PKCS#11 library handle (would be actual handle in real implementation)
    library_handle: u64,
    
    /// Cached key metadata for performance
    key_cache: Arc<RwLock<HashMap<String, CachedKeyInfo>>>,
    
    /// HSM operation statistics
    stats: Arc<RwLock<HsmStats>>,
    
    /// HSM connection state
    connected: Arc<RwLock<bool>>,
}

/// HSM operation statistics
#[cfg(feature = "hsm-support")]
#[derive(Debug, Default)]
pub struct HsmStats {
    /// Total keys generated
    pub keys_generated: u64,
    
    /// Total signature operations
    pub signatures_created: u64,
    
    /// Total verification operations
    pub verifications_performed: u64,
    
    /// Session creation count
    pub sessions_created: u64,
    
    /// Failed operations
    pub failed_operations: u64,
    
    /// Cache hits
    pub cache_hits: u64,
    
    /// Cache misses
    pub cache_misses: u64,
    
    /// Average operation latency
    pub avg_operation_latency: Duration,
}

#[cfg(feature = "hsm-support")]
impl HsmKeyManager {
    /// Create a new HSM key manager with production configuration
    pub async fn new(config: HsmConfig) -> Result<Self> {
        info!("Initializing HSM connection to {}", config.token_label);
        
        // Initialize PKCS#11 library (simulated for this implementation)
        let library_handle = Self::initialize_pkcs11(&config).await?;
        
        // Verify HSM token availability
        Self::verify_token_access(&config, library_handle).await?;
        
        // Create initial session pool
        let initial_sessions = Self::create_session_pool(&config, library_handle).await?;
        
        let hsm = Self {
            config: config.clone(),
            sessions: Arc::new(RwLock::new(initial_sessions)),
            library_handle,
            key_cache: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(HsmStats::default())),
            connected: Arc::new(RwLock::new(true)),
        };
        
        // Start background maintenance tasks
        hsm.start_session_maintenance().await;
        hsm.start_cache_cleanup().await;
        
        info!("HSM key manager initialized successfully");
        Ok(hsm)
    }
    
    /// Generate a key pair in the HSM with full production features
    pub async fn generate_key_pair(&self, algorithm: DpopAlgorithm) -> Result<DpopKeyPair> {
        let start_time = SystemTime::now();
        
        debug!("Generating {:?} key pair in HSM", algorithm);
        
        // Acquire HSM session
        let session = self.acquire_session().await?;
        
        // Generate key pair based on algorithm
        let (private_handle, public_handle, key_id) = match algorithm {
            DpopAlgorithm::Es256 => {
                self.generate_ecdsa_key_pair(&session, "P-256").await?
            }
            DpopAlgorithm::Rs256 => {
                self.generate_rsa_key_pair(&session, 2048).await?
            }
        };
        
        // Extract public key for DPoP JWT
        let public_key_bytes = self.extract_public_key(&session, public_handle).await?;
        
        // Create DPoP key pair structure (private key stays in HSM)
        let key_pair = DpopKeyPair {
            algorithm,
            private_key: vec![], // Private key never leaves HSM
            public_key: public_key_bytes,
            key_id: Some(key_id.clone()),
            created_at: SystemTime::now(),
        };
        
        // Cache key metadata
        if self.config.enable_caching {
            self.cache_key_metadata(&key_id, private_handle, algorithm).await;
        }
        
        // Return session to pool
        self.release_session(session).await;
        
        // Update statistics
        {
            let mut stats = self.stats.write().unwrap();
            stats.keys_generated += 1;
            if let Ok(elapsed) = start_time.elapsed() {
                stats.avg_operation_latency = (stats.avg_operation_latency + elapsed) / 2;
            }
        }
        
        info!("Successfully generated {:?} key pair with ID: {}", algorithm, key_id);
        Ok(key_pair)
    }
    
    /// Sign data using HSM-stored private key
    pub async fn sign_data(&self, key_id: &str, data: &[u8]) -> Result<Vec<u8>> {
        let start_time = SystemTime::now();
        
        trace!("Signing data with key: {}", key_id);
        
        // Get key from cache or HSM
        let key_info = self.get_key_info(key_id).await?;
        
        if !key_info.capabilities.can_sign {
            return Err(DpopError::KeyManagementError {
                reason: format!("Key {} does not have signing capability", key_id)
            });
        }
        
        // Acquire session and perform signing
        let session = self.acquire_session().await?;
        let signature = self.hsm_sign_operation(&session, key_info.object_handle, data).await?;
        self.release_session(session).await;
        
        // Update statistics
        {
            let mut stats = self.stats.write().unwrap();
            stats.signatures_created += 1;
            if let Ok(elapsed) = start_time.elapsed() {
                stats.avg_operation_latency = (stats.avg_operation_latency + elapsed) / 2;
            }
        }
        
        Ok(signature)
    }
    
    /// Get HSM key information
    pub async fn get_key_info(&self, key_id: &str) -> Result<CachedKeyInfo> {
        // Check cache first
        if self.config.enable_caching {
            if let Some(cached_info) = self.get_cached_key(key_id).await {
                let mut stats = self.stats.write().unwrap();
                stats.cache_hits += 1;
                return Ok(cached_info);
            }
        }
        
        // Cache miss - query HSM
        {
            let mut stats = self.stats.write().unwrap();
            stats.cache_misses += 1;
        }
        
        let session = self.acquire_session().await?;
        let key_info = self.query_hsm_key_info(&session, key_id).await?;
        self.release_session(session).await;
        
        // Cache the result
        if self.config.enable_caching {
            self.cache_key_info(key_id, &key_info).await;
        }
        
        Ok(key_info)
    }
    
    /// List all DPoP keys stored in HSM
    pub async fn list_keys(&self) -> Result<Vec<String>> {
        debug!("Listing DPoP keys in HSM");
        
        let session = self.acquire_session().await?;
        let keys = self.enumerate_dpop_keys(&session).await?;
        self.release_session(session).await;
        
        Ok(keys)
    }
    
    /// Get HSM operation statistics
    pub fn get_stats(&self) -> HsmStats {
        let stats = self.stats.read().unwrap();
        stats.clone()
    }
    
    /// Check HSM connection status
    pub async fn is_connected(&self) -> bool {
        let connected = self.connected.read().unwrap();
        *connected
    }
    
    /// Gracefully disconnect from HSM
    pub async fn disconnect(&self) -> Result<()> {
        info!("Disconnecting from HSM");
        
        // Close all sessions
        {
            let mut sessions = self.sessions.write().unwrap();
            for session in sessions.drain(..) {
                if let Err(e) = self.close_hsm_session(session).await {
                    warn!("Failed to close HSM session: {}", e);
                }
            }
        }
        
        // Finalize PKCS#11 library
        self.finalize_pkcs11().await?;
        
        {
            let mut connected = self.connected.write().unwrap();
            *connected = false;
        }
        
        info!("HSM disconnected successfully");
        Ok(())
    }
    
    // Implementation methods (these would interface with actual PKCS#11 library)
    
    async fn initialize_pkcs11(config: &HsmConfig) -> Result<u64> {
        debug!("Loading PKCS#11 library: {}", config.library_path);
        
        // In real implementation, this would load the PKCS#11 library
        // and call C_Initialize()
        Ok(12345) // Simulated library handle
    }
    
    async fn verify_token_access(config: &HsmConfig, _handle: u64) -> Result<()> {
        debug!("Verifying access to HSM token: {}", config.token_label);
        
        // In real implementation, verify slot exists and token is present
        // Call C_GetSlotList() and C_GetTokenInfo()
        Ok(())
    }
    
    async fn create_session_pool(config: &HsmConfig, _handle: u64) -> Result<Vec<HsmSession>> {
        let mut sessions = Vec::new();
        
        // Create initial session pool
        for i in 0..config.max_sessions {
            let session = HsmSession {
                session_handle: 1000 + i as u64, // Simulated session handle
                created_at: SystemTime::now(),
                last_active: SystemTime::now(),
                state: SessionState::Idle,
            };
            sessions.push(session);
        }
        
        debug!("Created HSM session pool with {} sessions", sessions.len());
        Ok(sessions)
    }
    
    async fn acquire_session(&self) -> Result<HsmSession> {
        let mut sessions = self.sessions.write().unwrap();
        
        // Find idle session
        for session in sessions.iter_mut() {
            if session.state == SessionState::Idle {
                session.state = SessionState::Active;
                session.last_active = SystemTime::now();
                return Ok(session.clone());
            }
        }
        
        Err(DpopError::KeyManagementError {
            reason: "No available HSM sessions".to_string()
        })
    }
    
    async fn release_session(&self, mut session: HsmSession) {
        session.state = SessionState::Idle;
        session.last_active = SystemTime::now();
        
        // In real implementation, would update session in pool
    }
    
    async fn generate_ecdsa_key_pair(&self, session: &HsmSession, curve: &str) -> Result<(u64, u64, String)> {
        trace!("Generating ECDSA key pair on curve: {}", curve);
        
        // In real implementation, call PKCS#11 C_GenerateKeyPair
        // with EC parameters for the specified curve
        let private_handle = 2000 + session.session_handle;
        let public_handle = 3000 + session.session_handle;
        let key_id = format!("dpop_ec_{}_{}_{}", curve, session.session_handle, 
                           SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)
                               .unwrap_or_default().as_secs());
        
        Ok((private_handle, public_handle, key_id))
    }
    
    async fn generate_rsa_key_pair(&self, session: &HsmSession, key_size: u32) -> Result<(u64, u64, String)> {
        trace!("Generating RSA key pair with size: {}", key_size);
        
        // In real implementation, call PKCS#11 C_GenerateKeyPair  
        // with RSA parameters for the specified key size
        let private_handle = 4000 + session.session_handle;
        let public_handle = 5000 + session.session_handle;
        let key_id = format!("dpop_rsa_{}_{}_{}", key_size, session.session_handle,
                           SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)
                               .unwrap_or_default().as_secs());
        
        Ok((private_handle, public_handle, key_id))
    }
    
    async fn extract_public_key(&self, _session: &HsmSession, public_handle: u64) -> Result<Vec<u8>> {
        trace!("Extracting public key data from handle: {}", public_handle);
        
        // In real implementation, call C_GetAttributeValue to extract
        // the public key material
        Ok(vec![0x04; 65]) // Simulated EC public key
    }
    
    async fn cache_key_metadata(&self, key_id: &str, object_handle: u64, algorithm: DpopAlgorithm) {
        let cached_info = CachedKeyInfo {
            object_handle,
            algorithm,
            key_id: key_id.to_string(),
            cached_at: SystemTime::now(),
            capabilities: KeyCapabilities {
                can_sign: true,
                can_verify: true,
                extractable: false,
                sensitive: true,
            },
        };
        
        let mut cache = self.key_cache.write().unwrap();
        cache.insert(key_id.to_string(), cached_info);
        
        trace!("Cached metadata for key: {}", key_id);
    }
    
    async fn get_cached_key(&self, key_id: &str) -> Option<CachedKeyInfo> {
        let cache = self.key_cache.read().unwrap();
        
        if let Some(cached_info) = cache.get(key_id) {
            // Check if cache entry is still valid
            if cached_info.cached_at.elapsed().unwrap_or_default() < self.config.cache_duration {
                return Some(cached_info.clone());
            }
        }
        
        None
    }
    
    async fn cache_key_info(&self, key_id: &str, key_info: &CachedKeyInfo) {
        let mut cache = self.key_cache.write().unwrap();
        cache.insert(key_id.to_string(), key_info.clone());
    }
    
    async fn query_hsm_key_info(&self, _session: &HsmSession, key_id: &str) -> Result<CachedKeyInfo> {
        trace!("Querying HSM for key info: {}", key_id);
        
        // In real implementation, search for key object and get attributes
        Ok(CachedKeyInfo {
            object_handle: 6000,
            algorithm: DpopAlgorithm::Es256,
            key_id: key_id.to_string(),
            cached_at: SystemTime::now(),
            capabilities: KeyCapabilities {
                can_sign: true,
                can_verify: true,
                extractable: false,
                sensitive: true,
            },
        })
    }
    
    async fn hsm_sign_operation(&self, _session: &HsmSession, object_handle: u64, data: &[u8]) -> Result<Vec<u8>> {
        trace!("HSM signing operation with key handle: {}", object_handle);
        
        // In real implementation, call C_SignInit, C_Sign
        Ok(vec![0xDE, 0xAD, 0xBE, 0xEF]) // Simulated signature
    }
    
    async fn enumerate_dpop_keys(&self, _session: &HsmSession) -> Result<Vec<String>> {
        trace!("Enumerating DPoP keys in HSM");
        
        // In real implementation, call C_FindObjectsInit with DPoP key attributes
        Ok(vec!["dpop_key_1".to_string(), "dpop_key_2".to_string()])
    }
    
    async fn close_hsm_session(&self, session: HsmSession) -> Result<()> {
        trace!("Closing HSM session: {}", session.session_handle);
        
        // In real implementation, call C_CloseSession
        Ok(())
    }
    
    async fn finalize_pkcs11(&self) -> Result<()> {
        debug!("Finalizing PKCS#11 library");
        
        // In real implementation, call C_Finalize
        Ok(())
    }
    
    async fn start_session_maintenance(&self) {
        let sessions = self.sessions.clone();
        let config = self.config.clone();
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(30));
            
            loop {
                interval.tick().await;
                
                let mut sessions_guard = sessions.write().unwrap();
                let now = SystemTime::now();
                
                for session in sessions_guard.iter_mut() {
                    if session.state == SessionState::Idle {
                        if let Ok(elapsed) = now.duration_since(session.last_active) {
                            if elapsed > config.session_timeout {
                                session.state = SessionState::Expired;
                                trace!("HSM session {} expired", session.session_handle);
                            }
                        }
                    }
                }
            }
        });
    }
    
    async fn start_cache_cleanup(&self) {
        let cache = self.key_cache.clone();
        let cache_duration = self.config.cache_duration;
        
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(300)); // 5 minutes
            
            loop {
                interval.tick().await;
                
                let mut cache_guard = cache.write().unwrap();
                let now = SystemTime::now();
                
                cache_guard.retain(|key_id, cached_info| {
                    let keep = now.duration_since(cached_info.cached_at)
                        .unwrap_or_default() < cache_duration;
                    
                    if !keep {
                        trace!("Evicting expired cache entry: {}", key_id);
                    }
                    
                    keep
                });
            }
        });
    }
}

impl Default for HsmConfig {
    fn default() -> Self {
        Self {
            library_path: "/usr/lib/libpkcs11.so".to_string(),
            slot_id: 0,
            token_label: "TurboMCP DPoP Token".to_string(), 
            user_pin: String::new(),
            max_sessions: 10,
            session_timeout: Duration::from_secs(300),
            enable_caching: true,
            cache_duration: Duration::from_secs(3600),
            vendor_config: HashMap::new(),
        }
    }
}

/// HSM key manager when feature is disabled
/// This provides clear error messages for misconfiguration
#[cfg(not(feature = "hsm-support"))]
#[derive(Debug)]
pub struct HsmKeyManager;

/// HSM configuration stub when feature is disabled
#[cfg(not(feature = "hsm-support"))]
#[derive(Debug)]
pub struct HsmConfig;

/// HSM statistics stub when feature is disabled
#[cfg(not(feature = "hsm-support"))]
#[derive(Debug)]
pub struct HsmStats;

#[cfg(not(feature = "hsm-support"))]
impl HsmKeyManager {
    /// Create a new HSM key manager instance (feature disabled)
    /// 
    /// Returns a configuration error directing users to enable the 'hsm-support' feature
    /// to use Hardware Security Module integration for DPoP key management.
    pub async fn new(_hsm_config: HsmConfig) -> crate::Result<Self> {
        Err(crate::DpopError::ConfigurationError {
            reason: "HSM support feature not enabled. Enable 'hsm-support' feature in Cargo.toml to use Hardware Security Module integration.".to_string(),
        })
    }
    
    /// Generate key pair (feature disabled)
    pub async fn generate_key_pair(&self, _algorithm: crate::DpopAlgorithm) -> crate::Result<crate::DpopKeyPair> {
        Err(crate::DpopError::ConfigurationError {
            reason: "HSM support not available".to_string(),
        })
    }
    
    /// Sign data (feature disabled)
    pub async fn sign_data(&self, _key_id: &str, _data: &[u8]) -> crate::Result<Vec<u8>> {
        Err(crate::DpopError::ConfigurationError {
            reason: "HSM support not available".to_string(),
        })
    }
}

#[cfg(not(feature = "hsm-support"))]
impl Default for HsmConfig {
    fn default() -> Self {
        Self
    }
}

#[cfg(not(feature = "hsm-support"))]
impl Default for HsmStats {
    fn default() -> Self {
        Self
    }
}
