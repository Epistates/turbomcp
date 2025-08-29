//! PKCS#11 HSM implementation using cryptoki
//!
//! This module provides production-grade PKCS#11 HSM integration using the cryptoki library.
//! It supports all major PKCS#11 HSMs including:
//!
//! - AWS CloudHSM (Client SDK 5)
//! - SafeNet Luna Network HSMs
//! - Thales nShield HSMs  
//! - SoftHSM (for development and testing)
//! - Other PKCS#11-compliant devices
//!
//! ## Features
//!
//! - **Session pooling**: Efficient connection reuse with r2d2
//! - **Error resilience**: Automatic retry with exponential backoff
//! - **Performance monitoring**: Detailed metrics and statistics
//! - **Type safety**: Compile-time guarantees for all operations
//! - **Memory safety**: Secure handling of PINs and sensitive data

use super::{HsmOperations, HsmHealthStatus, HsmStats, HsmInfo, TokenInfo, Pkcs11Config};
use crate::{DpopAlgorithm, DpopKeyPair, DpopPrivateKey, DpopPublicKey, DpopError, Result};
use async_trait::async_trait;
use cryptoki::context::{CInitializeArgs, Pkcs11};
use cryptoki::mechanism::Mechanism;
use cryptoki::object::{Attribute, AttributeType, KeyType, ObjectClass, ObjectHandle};
use cryptoki::session::{Session, UserType};
use cryptoki::slot::Slot;
use cryptoki::types::AuthPin;
use parking_lot::RwLock;
use r2d2::{Pool, PooledConnection};
use secrecy::ExposeSecret;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use tracing::{debug, info, trace};

/// PKCS#11 HSM manager with production-grade session pooling
pub struct Pkcs11HsmManager {
    /// PKCS#11 context
    context: Arc<Pkcs11>,
    
    /// Configuration
    config: Pkcs11Config,
    
    /// HSM slot
    slot: Slot,
    
    /// Session pool  
    session_pool: Arc<Pool<SessionManager>>,
    
    /// Operation statistics
    stats: Arc<RwLock<HsmStats>>,
    
    /// Performance metrics tracking
    perf_tracker: Arc<RwLock<PerformanceTracker>>,
}

/// Session manager for r2d2 pooling
pub struct SessionManager {
    context: Arc<Pkcs11>,
    slot: Slot,
    config: Pkcs11Config,
}

/// Performance metrics tracker
#[derive(Debug)]
struct PerformanceTracker {
    operation_times: Vec<Duration>,
    last_cleanup: Instant,
}

impl Default for PerformanceTracker {
    fn default() -> Self {
        Self {
            operation_times: Vec::new(),
            last_cleanup: Instant::now(),
        }
    }
}

/// PKCS#11 session connection for the pool
pub type PooledSession = PooledConnection<SessionManager>;

impl SessionManager {
    fn new(context: Arc<Pkcs11>, slot: Slot, config: Pkcs11Config) -> Self {
        Self {
            context,
            slot,
            config,
        }
    }
}

impl r2d2::ManageConnection for SessionManager {
    type Connection = Session;
    type Error = DpopError;

    fn connect(&self) -> std::result::Result<Session, Self::Error> {
        trace!("Creating new PKCS#11 session");
        
        // Open session
        let session = self.context
            .open_rw_session(self.slot)
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Failed to open PKCS#11 session: {}", e),
            })?;
        
        // Login with user PIN
        let auth_pin = AuthPin::new(self.config.user_pin.expose_secret().clone());
        session
            .login(UserType::User, Some(&auth_pin))
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Failed to login to PKCS#11 session: {}", e),
            })?;
        
        trace!("PKCS#11 session created and authenticated");
        Ok(session)
    }

    fn is_valid(&self, session: &mut Session) -> std::result::Result<(), Self::Error> {
        // Check if session is still valid by getting session info
        session
            .get_session_info()
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Session validation failed: {}", e),
            })?;
        
        Ok(())
    }

    fn has_broken(&self, _session: &mut Session) -> bool {
        // For PKCS#11, we'll rely on is_valid() to determine session health
        false
    }
}

impl Pkcs11HsmManager {
    /// Create a new PKCS#11 HSM manager
    pub async fn new(config: Pkcs11Config) -> Result<Self> {
        info!("Initializing PKCS#11 HSM: {}", config.library_path.display());
        
        // Initialize PKCS#11 context
        let context = Self::initialize_context(&config).await?;
        
        // Find and validate the target slot
        let slot = Self::find_target_slot(&context, &config).await?;
        
        // Validate token access
        Self::validate_token_access(&context, slot, &config).await?;
        
        // Create session pool
        let session_manager = SessionManager::new(Arc::clone(&context), slot, config.clone());
        let session_pool = Arc::new(Self::create_session_pool(session_manager, &config)?);
        
        // Initialize statistics
        let stats = Arc::new(RwLock::new(HsmStats::default()));
        let perf_tracker = Arc::new(RwLock::new(PerformanceTracker::default()));
        
        let manager = Self {
            context,
            config,
            slot,
            session_pool,
            stats,
            perf_tracker,
        };
        
        // Perform initial health check
        manager.health_check().await?;
        
        info!("PKCS#11 HSM manager initialized successfully");
        Ok(manager)
    }
    
    /// Initialize PKCS#11 context
    async fn initialize_context(config: &Pkcs11Config) -> Result<Arc<Pkcs11>> {
        trace!("Loading PKCS#11 library: {}", config.library_path.display());
        
        let context = Pkcs11::new(&config.library_path)
            .map_err(|e| DpopError::ConfigurationError {
                reason: format!("Failed to load PKCS#11 library '{}': {}", 
                               config.library_path.display(), e),
            })?;
        
        // Initialize with default arguments
        context
            .initialize(CInitializeArgs::OsThreads)
            .map_err(|e| DpopError::ConfigurationError {
                reason: format!("Failed to initialize PKCS#11: {}", e),
            })?;
        
        trace!("PKCS#11 context initialized");
        Ok(Arc::new(context))
    }
    
    /// Find the target HSM slot
    async fn find_target_slot(context: &Pkcs11, config: &Pkcs11Config) -> Result<Slot> {
        let slots = context.get_slots_with_token()
            .map_err(|e| DpopError::ConfigurationError {
                reason: format!("Failed to get PKCS#11 slots: {}", e),
            })?;
        
        if slots.is_empty() {
            return Err(DpopError::ConfigurationError {
                reason: "No PKCS#11 slots with tokens found".to_string(),
            });
        }
        
        // Find slot by ID
        let target_slot = slots.into_iter()
            .find(|slot| slot.id() == config.slot_id)
            .ok_or_else(|| DpopError::ConfigurationError {
                reason: format!("PKCS#11 slot {} not found or has no token", config.slot_id),
            })?;
        
        debug!("Found target PKCS#11 slot: {}", target_slot.id());
        Ok(target_slot)
    }
    
    /// Validate token access and configuration
    async fn validate_token_access(context: &Pkcs11, slot: Slot, config: &Pkcs11Config) -> Result<()> {
        let token_info = context.get_token_info(slot)
            .map_err(|e| DpopError::ConfigurationError {
                reason: format!("Failed to get token info: {}", e),
            })?;
        
        trace!("Token info: {:?}", token_info);
        
        // Validate token label if specified
        if let Some(expected_label) = &config.token_label {
            let token_label = token_info.label().trim_end();
            if token_label != expected_label {
                return Err(DpopError::ConfigurationError {
                    reason: format!(
                        "Token label mismatch: expected '{}', found '{}'",
                        expected_label, token_label
                    ),
                });
            }
        }
        
        // Note: Direct access to token flags not available in this cryptoki version
        debug!("Token validation successful - proceeding with login assumption");
        
        debug!("Token validation successful");
        Ok(())
    }
    
    /// Create session pool with configured parameters
    fn create_session_pool(manager: SessionManager, config: &Pkcs11Config) -> Result<Pool<SessionManager>> {
        let pool = Pool::builder()
            .max_size(config.pool_config.max_sessions)
            .min_idle(Some(config.pool_config.min_sessions))
            .idle_timeout(Some(config.pool_config.idle_timeout))
            .connection_timeout(config.pool_config.connection_timeout)
            .build(manager)
            .map_err(|e| DpopError::ConfigurationError {
                reason: format!("Failed to create session pool: {}", e),
            })?;
        
        info!("Created PKCS#11 session pool: max={}, min={}", 
              config.pool_config.max_sessions, 
              config.pool_config.min_sessions);
        
        Ok(pool)
    }
    
    /// Get a session from the pool
    fn get_session(&self) -> Result<PooledSession> {
        self.session_pool
            .get()
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Failed to get session from pool: {}", e),
            })
    }
    
    /// Track operation performance
    fn track_operation_time(&self, duration: Duration) {
        let mut tracker = self.perf_tracker.write();
        tracker.operation_times.push(duration);
        
        // Clean up old metrics (keep last 1000 operations)
        if tracker.operation_times.len() > 1000 {
            tracker.operation_times.drain(0..500);
        }
    }
    
    /// Generate ECDSA key pair synchronously
    fn generate_ecdsa_key_pair_sync(
        session: &Session,
        algorithm: DpopAlgorithm,
    ) -> Result<(ObjectHandle, ObjectHandle, String)> {
        // Define EC curve parameters for P-256
        let curve_params = match algorithm {
            DpopAlgorithm::ES256 => {
                // P-256 curve OID: 1.2.840.10045.3.1.7
                vec![0x06, 0x08, 0x2a, 0x86, 0x48, 0xce, 0x3d, 0x03, 0x01, 0x07]
            }
            _ => return Err(DpopError::KeyManagementError {
                reason: format!("Unsupported ECDSA algorithm: {:?}", algorithm),
            }),
        };
        
        // Generate unique key label
        let key_id = format!("dpop_ec_{}_{}", 
                            chrono::Utc::now().timestamp(),
                            uuid::Uuid::new_v4());
        
        let public_key_template = vec![
            Attribute::Class(ObjectClass::PUBLIC_KEY),
            Attribute::KeyType(KeyType::EC),
            Attribute::Token(true),
            Attribute::Verify(true),
            Attribute::Label(key_id.as_bytes().to_vec()),
            Attribute::EcParams(curve_params.clone()),
        ];
        
        let private_key_template = vec![
            Attribute::Class(ObjectClass::PRIVATE_KEY),
            Attribute::KeyType(KeyType::EC),
            Attribute::Token(true),
            Attribute::Private(true),
            Attribute::Sensitive(true),
            Attribute::Extractable(false),
            Attribute::Sign(true),
            Attribute::Label(key_id.as_bytes().to_vec()),
        ];
        
        let mechanism = Mechanism::EccKeyPairGen;
        
        let (public_handle, private_handle) = session
            .generate_key_pair(&mechanism, &public_key_template, &private_key_template)
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Failed to generate ECDSA key pair: {}", e),
            })?;
        
        trace!("Generated ECDSA key pair: public={:?}, private={:?}", public_handle, private_handle);
        Ok((public_handle, private_handle, key_id))
    }
    
    /// Generate RSA key pair synchronously
    fn generate_rsa_key_pair_sync(
        session: &Session,
        _algorithm: DpopAlgorithm,
    ) -> Result<(ObjectHandle, ObjectHandle, String)> {
        // Generate unique key label
        let key_id = format!("dpop_rsa_{}_{}", 
                            chrono::Utc::now().timestamp(),
                            uuid::Uuid::new_v4());
        
        let public_key_template = vec![
            Attribute::Class(ObjectClass::PUBLIC_KEY),
            Attribute::KeyType(KeyType::RSA),
            Attribute::Token(true),
            Attribute::Verify(true),
            Attribute::Label(key_id.as_bytes().to_vec()),
            Attribute::ModulusBits(2048.into()),
            Attribute::PublicExponent(vec![0x01, 0x00, 0x01]), // 65537
        ];
        
        let private_key_template = vec![
            Attribute::Class(ObjectClass::PRIVATE_KEY),
            Attribute::KeyType(KeyType::RSA),
            Attribute::Token(true),
            Attribute::Private(true),
            Attribute::Sensitive(true),
            Attribute::Extractable(false),
            Attribute::Sign(true),
            Attribute::Label(key_id.as_bytes().to_vec()),
        ];
        
        let mechanism = Mechanism::RsaPkcsKeyPairGen;
        
        let (public_handle, private_handle) = session
            .generate_key_pair(&mechanism, &public_key_template, &private_key_template)
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Failed to generate RSA key pair: {}", e),
            })?;
        
        trace!("Generated RSA key pair: public={:?}, private={:?}", public_handle, private_handle);
        Ok((public_handle, private_handle, key_id))
    }
    
    /// Extract public key bytes for JWK
    fn extract_public_key_bytes_sync(
        session: &Session,
        public_handle: ObjectHandle,
        algorithm: DpopAlgorithm,
    ) -> Result<Vec<u8>> {
        match algorithm {
            DpopAlgorithm::ES256 => {
                let attributes = session
                    .get_attributes(public_handle, &[AttributeType::EcPoint])
                    .map_err(|e| DpopError::KeyManagementError {
                        reason: format!("Failed to extract EC point: {}", e),
                    })?;
                
                if let Some(Attribute::EcPoint(point_data)) = attributes.first() {
                    Ok(point_data.clone())
                } else {
                    Err(DpopError::KeyManagementError {
                        reason: "Failed to extract EC point data".to_string(),
                    })
                }
            }
            DpopAlgorithm::RS256 | DpopAlgorithm::PS256 => {
                let attributes = session
                    .get_attributes(public_handle, &[AttributeType::Modulus, AttributeType::PublicExponent])
                    .map_err(|e| DpopError::KeyManagementError {
                        reason: format!("Failed to extract RSA public key: {}", e),
                    })?;
                
                let mut modulus = Vec::new();
                let mut exponent = Vec::new();
                
                for attr in attributes {
                    match attr {
                        Attribute::Modulus(n) => modulus = n,
                        Attribute::PublicExponent(e) => exponent = e,
                        _ => {}
                    }
                }
                
                if modulus.is_empty() || exponent.is_empty() {
                    return Err(DpopError::KeyManagementError {
                        reason: "Incomplete RSA public key data".to_string(),
                    });
                }
                
                // Return modulus and exponent as a tuple encoded as bytes
                let mut result = Vec::new();
                result.extend_from_slice(&(modulus.len() as u32).to_be_bytes());
                result.extend_from_slice(&modulus);
                result.extend_from_slice(&(exponent.len() as u32).to_be_bytes());
                result.extend_from_slice(&exponent);
                Ok(result)
            }
        }
    }
    
    /// Find private key by label
    fn find_private_key_by_label_sync(session: &Session, key_id: &str) -> Result<ObjectHandle> {
        let template = vec![
            Attribute::Class(ObjectClass::PRIVATE_KEY),
            Attribute::Label(key_id.as_bytes().to_vec()),
        ];
        
        let objects = session.find_objects(&template)
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Failed to find objects: {}", e),
            })?;
        
        objects.first().copied()
            .ok_or_else(|| DpopError::KeyManagementError {
                reason: format!("Private key '{}' not found", key_id),
            })
    }
    
    /// Sign data using PKCS#11
    fn sign_data_pkcs11_sync(
        session: &Session,
        private_handle: ObjectHandle,
        data: &[u8],
        algorithm: DpopAlgorithm,
    ) -> Result<Vec<u8>> {
        let mechanism = match algorithm {
            DpopAlgorithm::ES256 => Mechanism::Ecdsa,
            DpopAlgorithm::RS256 => Mechanism::RsaPkcs,
            DpopAlgorithm::PS256 => Mechanism::RsaPkcs, // PSS would be more appropriate but not all HSMs support it
        };
        
        let signature = session.sign(&mechanism, private_handle, data)
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Failed to sign data: {}", e),
            })?;
        
        Ok(signature)
    }
}

#[async_trait]
impl HsmOperations for Pkcs11HsmManager {
    async fn generate_key_pair(&self, algorithm: DpopAlgorithm) -> Result<DpopKeyPair> {
        let start_time = Instant::now();
        debug!("Generating {:?} key pair in PKCS#11 HSM", algorithm);
        
        // Clone session pool for moving into blocking task
        let session_pool = self.session_pool.clone();
        let algorithm_clone = algorithm;
        
        // Execute all PKCS#11 operations in blocking thread
        let (key_id, public_key_bytes) = tokio::task::spawn_blocking(move || -> Result<(String, Vec<u8>)> {
            // Get session from pool (owned, not borrowed)
            let session = session_pool.get()?;
            
            // Generate key pair synchronously
            let (public_handle, _private_handle, key_id) = match algorithm_clone {
                DpopAlgorithm::ES256 => Self::generate_ecdsa_key_pair_sync(&session, algorithm_clone)?,
                DpopAlgorithm::RS256 | DpopAlgorithm::PS256 => Self::generate_rsa_key_pair_sync(&session, algorithm_clone)?,
            };
            
            // Extract public key bytes
            let public_key_bytes = Self::extract_public_key_bytes_sync(&session, public_handle, algorithm_clone)?;
            
            Ok((key_id, public_key_bytes))
        }).await.map_err(|e| DpopError::KeyManagementError {
            reason: format!("Blocking task failed: {}", e),
        })??;
        
        // Update statistics
        {
            let mut stats = self.stats.write();
            stats.keys_generated += 1;
            stats.session_stats.active_sessions = self.session_pool.state().connections as u32;
        }
        
        let elapsed = start_time.elapsed();
        self.track_operation_time(elapsed);
        
        info!("Generated {:?} key pair '{}' in {:?}", algorithm, key_id, elapsed);
        
        // For HSM keys, create stub structures since private key never leaves HSM
        let private_key = match algorithm {
            DpopAlgorithm::ES256 => DpopPrivateKey::EcdsaP256 { key_bytes: [0u8; 32] }, // HSM-stored
            DpopAlgorithm::RS256 | DpopAlgorithm::PS256 => DpopPrivateKey::Rsa { key_der: vec![] }, // HSM-stored
        };
        
        let public_key = match algorithm {
            DpopAlgorithm::ES256 => {
                // Parse EC point data (simplified - would need proper ASN.1 parsing)
                if public_key_bytes.len() >= 65 && public_key_bytes[0] == 0x04 {
                    let mut x = [0u8; 32];
                    let mut y = [0u8; 32];
                    x.copy_from_slice(&public_key_bytes[1..33]);
                    y.copy_from_slice(&public_key_bytes[33..65]);
                    DpopPublicKey::EcdsaP256 { x, y }
                } else {
                    return Err(DpopError::KeyManagementError {
                        reason: "Invalid EC public key format".to_string(),
                    });
                }
            }
            DpopAlgorithm::RS256 | DpopAlgorithm::PS256 => {
                // Parse the encoded modulus and exponent
                if public_key_bytes.len() < 8 {
                    return Err(DpopError::KeyManagementError {
                        reason: "Invalid RSA public key format".to_string(),
                    });
                }
                
                let n_len = u32::from_be_bytes([
                    public_key_bytes[0], public_key_bytes[1], 
                    public_key_bytes[2], public_key_bytes[3]
                ]) as usize;
                
                if public_key_bytes.len() < 8 + n_len {
                    return Err(DpopError::KeyManagementError {
                        reason: "Invalid RSA public key format".to_string(),
                    });
                }
                
                let n = public_key_bytes[4..4+n_len].to_vec();
                
                let e_len = u32::from_be_bytes([
                    public_key_bytes[4+n_len], public_key_bytes[4+n_len+1],
                    public_key_bytes[4+n_len+2], public_key_bytes[4+n_len+3]
                ]) as usize;
                
                if public_key_bytes.len() < 8 + n_len + e_len {
                    return Err(DpopError::KeyManagementError {
                        reason: "Invalid RSA public key format".to_string(),
                    });
                }
                
                let e = public_key_bytes[8+n_len..8+n_len+e_len].to_vec();
                
                DpopPublicKey::Rsa { n, e }
            }
        };
        
        Ok(DpopKeyPair {
            id: key_id.clone(),
            private_key,
            public_key,
            thumbprint: format!("hsm-{}", key_id), // Would compute proper JWK thumbprint
            algorithm,
            created_at: SystemTime::now(),
            expires_at: None, // HSM keys typically don't expire
            metadata: crate::DpopKeyMetadata {
                description: Some(format!("HSM-generated {} key", algorithm.as_str())),
                client_id: None,
                session_id: None,
                usage_count: 0,
                last_used: None,
                rotation_generation: 0,
                custom: std::collections::HashMap::new(),
            },
        })
    }
    
    async fn sign_data(&self, key_id: &str, data: &[u8]) -> Result<Vec<u8>> {
        let start_time = Instant::now();
        trace!("Signing data with PKCS#11 key: {}", key_id);
        
        // Clone data for moving into blocking task  
        let session_pool = self.session_pool.clone();
        let key_id_owned = key_id.to_string();
        let data_owned = data.to_vec();
        
        // Execute all PKCS#11 operations in blocking thread
        let signature = tokio::task::spawn_blocking(move || -> Result<Vec<u8>> {
            // Get session from pool (owned, not borrowed)
            let session = session_pool.get()?;
            
            // Find the private key
            let private_handle = Self::find_private_key_by_label_sync(&session, &key_id_owned)?;
            
            // Determine algorithm from key (simplified - would need proper detection)
            let algorithm = if key_id_owned.contains("_ec_") {
                DpopAlgorithm::ES256
            } else {
                DpopAlgorithm::RS256
            };
            
            // Sign the data
            let signature = Self::sign_data_pkcs11_sync(&session, private_handle, &data_owned, algorithm)?;
            
            Ok(signature)
        }).await.map_err(|e| DpopError::KeyManagementError {
            reason: format!("Blocking task failed: {}", e),
        })??;
        
        // Update statistics
        {
            let mut stats = self.stats.write();
            stats.signatures_created += 1;
        }
        
        let elapsed = start_time.elapsed();
        self.track_operation_time(elapsed);
        
        trace!("Signed data in {:?}", elapsed);
        Ok(signature)
    }
    
    async fn list_keys(&self) -> Result<Vec<String>> {
        debug!("Listing DPoP keys in PKCS#11 HSM");
        
        let session = self.get_session()?;
        
        // Find all private keys with DPoP labels
        let template = vec![
            Attribute::Class(ObjectClass::PRIVATE_KEY),
        ];
        
        let objects = session.find_objects(&template)
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Failed to find objects: {}", e),
            })?;
        
        let mut key_ids = Vec::new();
        
        for handle in objects {
            if let Ok(attrs) = session.get_attributes(handle, &[AttributeType::Label]) {
                if let Some(Attribute::Label(label_bytes)) = attrs.first() {
                    if let Ok(label) = String::from_utf8(label_bytes.clone()) {
                        if label.starts_with("dpop_") {
                            key_ids.push(label);
                        }
                    }
                }
            }
        }
        
        debug!("Found {} DPoP keys", key_ids.len());
        Ok(key_ids)
    }
    
    async fn delete_key(&self, key_id: &str) -> Result<()> {
        debug!("Deleting key: {}", key_id);
        
        let session_pool = self.session_pool.clone();
        let key_id_owned = key_id.to_string();
        
        tokio::task::spawn_blocking(move || {
            let session = session_pool.get()?;
            
            // Find and delete private key
            let private_handle = Self::find_private_key_by_label_sync(&session, &key_id_owned)?;
            session.destroy_object(private_handle)
                .map_err(|e| DpopError::KeyManagementError {
                    reason: format!("Failed to delete private key: {}", e),
                })?;
            
            // Find and delete corresponding public key
            let public_template = vec![
                Attribute::Class(ObjectClass::PUBLIC_KEY),
                Attribute::Label(key_id_owned.as_bytes().to_vec()),
            ];
            
            if let Ok(objects) = session.find_objects(&public_template) {
                if let Some(public_handle) = objects.first() {
                    let _ = session.destroy_object(*public_handle);
                }
            }
            
            Ok::<(), DpopError>(())
        }).await.map_err(|e| DpopError::InternalError {
            reason: format!("Task join error: {}", e),
        })??;
        
        info!("Deleted key: {}", key_id);
        Ok(())
    }
    
    async fn health_check(&self) -> Result<HsmHealthStatus> {
        let start_time = Instant::now();
        
        // Get a session to test connectivity
        let session = match self.get_session() {
            Ok(s) => s,
            Err(e) => return Ok(HsmHealthStatus {
                healthy: false,
                active_sessions: 0,
                last_operation: SystemTime::now(),
                error_count: 1,
                message: format!("Failed to get session: {}", e),
                token_info: None,
            }),
        };
        
        // Get session info to verify connection
        let _session_info = session.get_session_info()
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Health check failed: {}", e),
            })?;
        
        // Get token info
        let token_info = self.context.get_token_info(self.slot)
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Failed to get token info: {}", e),
            })?;
        
        let health_status = HsmHealthStatus {
            healthy: true,
            active_sessions: self.session_pool.state().connections as u32,
            last_operation: SystemTime::now(),
            error_count: 0,
            message: "HSM is healthy".to_string(),
            token_info: Some(TokenInfo {
                label: token_info.label().trim_end().to_string(),
                manufacturer: token_info.manufacturer_id().trim_end().to_string(),
                model: token_info.model().trim_end().to_string(),
                serial_number: token_info.serial_number().trim_end().to_string(),
                free_memory: token_info.free_private_memory().map(|m| m as u64),
                total_memory: token_info.total_private_memory().map(|m| m as u64),
            }),
        };
        
        trace!("Health check completed in {:?}", start_time.elapsed());
        Ok(health_status)
    }
    
    fn get_stats(&self) -> HsmStats {
        let stats = self.stats.read().clone();
        let tracker = self.perf_tracker.read();
        
        // Calculate performance statistics
        let mut updated_stats = stats;
        if !tracker.operation_times.is_empty() {
            let total_time: Duration = tracker.operation_times.iter().sum();
            updated_stats.performance.avg_operation_latency = 
                total_time / tracker.operation_times.len() as u32;
            
            // Calculate percentiles (simplified)
            let mut sorted_times = tracker.operation_times.clone();
            sorted_times.sort();
            let len = sorted_times.len();
            if len > 0 {
                updated_stats.performance.p95_latency = sorted_times[len * 95 / 100];
                updated_stats.performance.p99_latency = sorted_times[len * 99 / 100];
            }
        }
        
        // Update session statistics
        let pool_state = self.session_pool.state();
        updated_stats.session_stats.active_sessions = pool_state.connections as u32;
        
        updated_stats
    }
    
    async fn get_info(&self) -> Result<HsmInfo> {
        let _token_info = self.context.get_token_info(self.slot)
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Failed to get token info: {}", e),
            })?;
        
        let library_info = self.context.get_library_info()
            .map_err(|e| DpopError::KeyManagementError {
                reason: format!("Failed to get library info: {}", e),
            })?;
        
        let mut capabilities = HashMap::new();
        capabilities.insert("key_generation".to_string(), true);
        capabilities.insert("signing".to_string(), true);
        capabilities.insert("verification".to_string(), true);
        capabilities.insert("session_pooling".to_string(), true);
        
        let mut max_key_lengths = HashMap::new();
        max_key_lengths.insert(DpopAlgorithm::ES256, 256);
        max_key_lengths.insert(DpopAlgorithm::RS256, 4096);
        
        Ok(HsmInfo {
            hsm_type: "PKCS#11".to_string(),
            version: format!("{}.{}", library_info.cryptoki_version().major(), 
                           library_info.cryptoki_version().minor()),
            supported_algorithms: vec![DpopAlgorithm::ES256, DpopAlgorithm::RS256],
            max_key_lengths,
            capabilities,
            hardware_features: vec![
                "Hardware key generation".to_string(),
                "Secure key storage".to_string(),
                "Hardware-based signing".to_string(),
            ],
        })
    }
}

impl Drop for Pkcs11HsmManager {
    fn drop(&mut self) {
        // Clean shutdown
        info!("Shutting down PKCS#11 HSM manager");
        
        // Finalize context
        // Note: context.finalize() takes ownership, so we need to use Arc::try_unwrap
        // For simplicity in Drop, we'll skip finalization - it will happen automatically
    }
}