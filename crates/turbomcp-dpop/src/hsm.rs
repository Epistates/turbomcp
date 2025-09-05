//! Hardware Security Module (HSM) integration for DPoP key management
//!
//! This module provides enterprise-grade HSM-backed key storage and cryptographic operations
//! for production-grade security. It supports multiple HSM backends including:
//!
//! - **PKCS#11 HSMs**: SafeNet Luna, Thales nShield, AWS CloudHSM, and other PKCS#11 devices
//! - **YubiHSM 2**: Direct integration with Yubico's hardware security modules
//! - **SoftHSM**: For development and testing
//!
//! ## Features
//!
//! - **Zero-copy operations**: Private keys never leave the HSM
//! - **Session management**: Efficient connection pooling and session reuse
//! - **Enterprise monitoring**: Comprehensive metrics and audit logging
//! - **Type safety**: Compile-time guarantees for HSM operations
//! - **Async throughout**: Full tokio compatibility for high performance
//!
//! ## Usage
//!
//! ```rust,no_run
//! # #[cfg(feature = "hsm-pkcs11")]
//! # {
//! use turbomcp_dpop::hsm::{HsmManager, HsmConfig};
//! use turbomcp_dpop::DpopAlgorithm;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Configure PKCS#11 HSM
//!     let config = HsmConfig::pkcs11()
//!         .library_path("/opt/cloudhsm/lib/libcloudhsm_pkcs11.so")
//!         .slot_id(0)
//!         .user_pin("your-pin")
//!         .build()?;
//!
//!     let hsm = HsmManager::new(config).await?;
//!     
//!     // Generate DPoP key pair
//!     let key_pair = hsm.generate_key_pair(DpopAlgorithm::ES256).await?;
//!     
//!     // Sign DPoP proof
//!     let signature = hsm.sign_data(&key_pair.id, b"data").await?;
//!     
//!     Ok(())
//! }
//! # }
//! ```
//!
//! ## Configuration
//!
//! Enable HSM support with feature flags:
//!
//! ```toml
//! [dependencies]
//! turbomcp-dpop = { version = "1.1.0-exp.3", features = ["hsm-pkcs11", "hsm-yubico"] }
//! ```

#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
use crate::{DpopAlgorithm, DpopError, DpopKeyPair, Result};
#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
use std::time::SystemTime;

/// Core HSM operations trait
///
/// This trait defines the interface for all HSM implementations, ensuring
/// consistent behavior across different HSM types and vendors.
#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
#[async_trait]
pub trait HsmOperations: Send + Sync {
    /// Generate a DPoP key pair in the HSM
    ///
    /// The private key never leaves the HSM, ensuring maximum security.
    async fn generate_key_pair(&self, algorithm: DpopAlgorithm) -> Result<DpopKeyPair>;

    /// Sign data using an HSM-stored private key
    ///
    /// This performs the cryptographic signing operation entirely within the HSM.
    async fn sign_data(&self, key_id: &str, data: &[u8]) -> Result<Vec<u8>>;

    /// List all DPoP keys stored in the HSM
    async fn list_keys(&self) -> Result<Vec<String>>;

    /// Delete a key from the HSM
    async fn delete_key(&self, key_id: &str) -> Result<()>;

    /// Check HSM connection and health status
    async fn health_check(&self) -> Result<HsmHealthStatus>;

    /// Get HSM operation statistics
    fn get_stats(&self) -> HsmStats;

    /// Get HSM information and capabilities
    async fn get_info(&self) -> Result<HsmInfo>;
}

/// HSM configuration with support for multiple backends
#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum HsmConfig {
    /// PKCS#11 HSM configuration (SafeNet Luna, Thales, AWS CloudHSM, etc.)
    #[cfg(feature = "hsm-pkcs11")]
    Pkcs11(Pkcs11Config),
    /// YubiHSM 2 configuration
    #[cfg(feature = "hsm-yubico")]
    YubiHsm(YubiHsmConfig),
}

/// PKCS#11 HSM configuration
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(not(feature = "hsm-pkcs11"), derive(Deserialize))]
pub struct Pkcs11Config {
    /// Path to PKCS#11 library (e.g., "/opt/cloudhsm/lib/libcloudhsm_pkcs11.so")
    pub library_path: PathBuf,

    /// HSM slot number
    pub slot_id: u64,

    /// HSM token label (optional, used for validation)
    pub token_label: Option<String>,

    /// User PIN for HSM authentication
    #[cfg(feature = "hsm-pkcs11")]
    #[serde(skip)]
    pub user_pin: secrecy::SecretString,

    /// User PIN as string (for deserialization)
    #[cfg(not(feature = "hsm-pkcs11"))]
    pub user_pin: String,

    /// SO PIN for administrative operations (optional)
    #[cfg(feature = "hsm-pkcs11")]
    #[serde(skip)]
    pub so_pin: Option<secrecy::SecretString>,

    /// SO PIN as string (for deserialization)
    #[cfg(not(feature = "hsm-pkcs11"))]
    pub so_pin: Option<String>,

    /// Session pool configuration
    pub pool_config: PoolConfig,

    /// Operation timeouts
    pub timeouts: TimeoutConfig,

    /// Retry configuration
    pub retry_config: RetryConfig,

    /// Vendor-specific configuration
    pub vendor_config: HashMap<String, String>,
}

/// YubiHSM configuration
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(not(feature = "hsm-yubico"), derive(Deserialize))]
pub struct YubiHsmConfig {
    /// Connection type (HTTP or USB)
    pub connector: YubiHsmConnector,

    /// Authentication key ID
    pub auth_key_id: u16,

    /// Authentication password
    #[cfg(feature = "hsm-yubico")]
    #[serde(skip)]
    pub password: secrecy::SecretString,

    /// Password as string (for deserialization)
    #[cfg(not(feature = "hsm-yubico"))]
    pub password: String,

    /// Operation timeouts
    pub timeouts: TimeoutConfig,

    /// Retry configuration  
    pub retry_config: RetryConfig,
}

/// YubiHSM connector configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum YubiHsmConnector {
    /// HTTP connector
    Http {
        /// HTTP URL for YubiHSM connector service
        url: String,
    },
    /// USB connector (default port)
    Usb,
}

// Custom Deserialize implementations for HSM configs with secret fields
#[cfg(feature = "hsm-pkcs11")]
impl<'de> serde::Deserialize<'de> for Pkcs11Config {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        #[derive(serde::Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            LibraryPath,
            SlotId,
            TokenLabel,
            UserPin,
            SoPin,
            PoolConfig,
            TimeoutConfig,
            RetryConfig,
            VendorConfig,
        }

        struct Pkcs11ConfigVisitor;

        impl<'de> Visitor<'de> for Pkcs11ConfigVisitor {
            type Value = Pkcs11Config;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("struct Pkcs11Config")
            }

            fn visit_map<V>(self, mut map: V) -> std::result::Result<Pkcs11Config, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut library_path = None;
                let mut slot_id = None;
                let mut token_label = None;
                let mut user_pin_str: Option<String> = None;
                let mut so_pin_str: Option<String> = None;
                let mut pool_config = None;
                let mut timeout_config = None;
                let mut retry_config = None;
                let mut vendor_config = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::LibraryPath => {
                            if library_path.is_some() {
                                return Err(de::Error::duplicate_field("library_path"));
                            }
                            library_path = Some(map.next_value()?);
                        }
                        Field::SlotId => {
                            if slot_id.is_some() {
                                return Err(de::Error::duplicate_field("slot_id"));
                            }
                            slot_id = Some(map.next_value()?);
                        }
                        Field::TokenLabel => {
                            if token_label.is_some() {
                                return Err(de::Error::duplicate_field("token_label"));
                            }
                            token_label = map.next_value()?;
                        }
                        Field::UserPin => {
                            if user_pin_str.is_some() {
                                return Err(de::Error::duplicate_field("user_pin"));
                            }
                            user_pin_str = Some(map.next_value()?);
                        }
                        Field::SoPin => {
                            if so_pin_str.is_some() {
                                return Err(de::Error::duplicate_field("so_pin"));
                            }
                            so_pin_str = map.next_value()?;
                        }
                        Field::PoolConfig => {
                            if pool_config.is_some() {
                                return Err(de::Error::duplicate_field("pool_config"));
                            }
                            pool_config = Some(map.next_value()?);
                        }
                        Field::TimeoutConfig => {
                            if timeout_config.is_some() {
                                return Err(de::Error::duplicate_field("timeout_config"));
                            }
                            timeout_config = Some(map.next_value()?);
                        }
                        Field::RetryConfig => {
                            if retry_config.is_some() {
                                return Err(de::Error::duplicate_field("retry_config"));
                            }
                            retry_config = Some(map.next_value()?);
                        }
                        Field::VendorConfig => {
                            if vendor_config.is_some() {
                                return Err(de::Error::duplicate_field("vendor_config"));
                            }
                            vendor_config = Some(map.next_value()?);
                        }
                    }
                }

                let library_path =
                    library_path.ok_or_else(|| de::Error::missing_field("library_path"))?;
                let slot_id = slot_id.ok_or_else(|| de::Error::missing_field("slot_id"))?;
                let user_pin_str =
                    user_pin_str.ok_or_else(|| de::Error::missing_field("user_pin"))?;
                let pool_config = pool_config.unwrap_or_default();
                let timeout_config = timeout_config.unwrap_or_default();
                let retry_config = retry_config.unwrap_or_default();
                let vendor_config = vendor_config.unwrap_or_default();

                Ok(Pkcs11Config {
                    library_path,
                    slot_id,
                    token_label,
                    user_pin: secrecy::SecretString::new(user_pin_str),
                    so_pin: so_pin_str.map(secrecy::SecretString::new),
                    pool_config,
                    timeouts: timeout_config,
                    retry_config,
                    vendor_config,
                })
            }
        }

        deserializer.deserialize_struct(
            "Pkcs11Config",
            &[
                "library_path",
                "slot_id",
                "token_label",
                "user_pin",
                "so_pin",
                "pool_config",
                "timeout_config",
                "retry_config",
                "vendor_config",
            ],
            Pkcs11ConfigVisitor,
        )
    }
}

#[cfg(feature = "hsm-yubico")]
impl<'de> serde::Deserialize<'de> for YubiHsmConfig {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{self, MapAccess, Visitor};
        use std::fmt;

        #[derive(serde::Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Connector,
            AuthKeyId,
            Password,
            TimeoutConfig,
            RetryConfig,
        }

        struct YubiHsmConfigVisitor;

        impl<'de> Visitor<'de> for YubiHsmConfigVisitor {
            type Value = YubiHsmConfig;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("struct YubiHsmConfig")
            }

            fn visit_map<V>(self, mut map: V) -> std::result::Result<YubiHsmConfig, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut connector = None;
                let mut auth_key_id = None;
                let mut password_str: Option<String> = None;
                let mut timeout_config = None;
                let mut retry_config = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Connector => {
                            if connector.is_some() {
                                return Err(de::Error::duplicate_field("connector"));
                            }
                            connector = Some(map.next_value()?);
                        }
                        Field::AuthKeyId => {
                            if auth_key_id.is_some() {
                                return Err(de::Error::duplicate_field("auth_key_id"));
                            }
                            auth_key_id = Some(map.next_value()?);
                        }
                        Field::Password => {
                            if password_str.is_some() {
                                return Err(de::Error::duplicate_field("password"));
                            }
                            password_str = Some(map.next_value()?);
                        }
                        Field::TimeoutConfig => {
                            if timeout_config.is_some() {
                                return Err(de::Error::duplicate_field("timeout_config"));
                            }
                            timeout_config = Some(map.next_value()?);
                        }
                        Field::RetryConfig => {
                            if retry_config.is_some() {
                                return Err(de::Error::duplicate_field("retry_config"));
                            }
                            retry_config = Some(map.next_value()?);
                        }
                    }
                }

                let connector = connector.ok_or_else(|| de::Error::missing_field("connector"))?;
                let auth_key_id =
                    auth_key_id.ok_or_else(|| de::Error::missing_field("auth_key_id"))?;
                let password_str =
                    password_str.ok_or_else(|| de::Error::missing_field("password"))?;
                let timeout_config = timeout_config.unwrap_or_default();
                let retry_config = retry_config.unwrap_or_default();

                Ok(YubiHsmConfig {
                    connector,
                    auth_key_id,
                    password: secrecy::SecretString::new(password_str),
                    timeouts: timeout_config,
                    retry_config,
                })
            }
        }

        deserializer.deserialize_struct(
            "YubiHsmConfig",
            &[
                "connector",
                "auth_key_id",
                "password",
                "timeouts",
                "retry_config",
            ],
            YubiHsmConfigVisitor,
        )
    }
}

/// Session pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolConfig {
    /// Maximum number of concurrent sessions
    pub max_sessions: u32,

    /// Minimum number of sessions to keep alive
    pub min_sessions: u32,

    /// Session idle timeout
    pub idle_timeout: Duration,

    /// Maximum time to wait for an available session
    pub connection_timeout: Duration,
}

/// Timeout configuration for HSM operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    /// Connection establishment timeout
    pub connect_timeout: Duration,

    /// Individual operation timeout
    pub operation_timeout: Duration,

    /// Health check timeout
    pub health_check_timeout: Duration,
}

/// Retry configuration for HSM operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,

    /// Base delay between retries
    pub base_delay: Duration,

    /// Maximum delay between retries
    pub max_delay: Duration,

    /// Exponential backoff multiplier
    pub backoff_multiplier: f64,
}

/// HSM health status information
#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HsmHealthStatus {
    /// Overall health status
    pub healthy: bool,

    /// Number of active sessions
    pub active_sessions: u32,

    /// Last successful operation timestamp
    pub last_operation: SystemTime,

    /// Error count in the last period
    pub error_count: u64,

    /// Detailed status message
    pub message: String,

    /// Token-specific information
    pub token_info: Option<TokenInfo>,
}

/// Token information from HSM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Token label
    pub label: String,

    /// Manufacturer ID
    pub manufacturer: String,

    /// Model name
    pub model: String,

    /// Serial number
    pub serial_number: String,

    /// Available storage space
    pub free_memory: Option<u64>,

    /// Total storage space
    pub total_memory: Option<u64>,
}

/// HSM information and capabilities
#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HsmInfo {
    /// HSM type and backend
    pub hsm_type: String,

    /// HSM version information
    pub version: String,

    /// Supported algorithms
    pub supported_algorithms: Vec<DpopAlgorithm>,

    /// Maximum key length for each algorithm
    pub max_key_lengths: HashMap<DpopAlgorithm, u32>,

    /// Additional capabilities
    pub capabilities: HashMap<String, bool>,

    /// Hardware features
    pub hardware_features: Vec<String>,
}

/// HSM operation statistics
#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct HsmStats {
    /// Total keys generated
    pub keys_generated: u64,

    /// Total signature operations
    pub signatures_created: u64,

    /// Total verification operations  
    pub verifications_performed: u64,

    /// Total failed operations
    pub failed_operations: u64,

    /// Session statistics
    pub session_stats: SessionStats,

    /// Performance metrics
    pub performance: PerformanceStats,

    /// Error statistics
    pub error_stats: HashMap<String, u64>,
}

/// Session management statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    /// Total sessions created
    pub sessions_created: u64,

    /// Currently active sessions
    pub active_sessions: u32,

    /// Sessions closed due to timeout
    pub timed_out_sessions: u64,

    /// Sessions closed due to errors
    pub error_sessions: u64,

    /// Average session lifetime
    pub avg_session_lifetime: Duration,
}

/// Performance statistics
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct PerformanceStats {
    /// Average operation latency
    pub avg_operation_latency: Duration,

    /// 95th percentile latency
    pub p95_latency: Duration,

    /// 99th percentile latency
    pub p99_latency: Duration,

    /// Operations per second
    pub ops_per_second: f64,

    /// Cache hit rate (0.0 - 1.0)
    pub cache_hit_rate: f64,
}

/// Unified HSM manager for all supported HSM types
#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
pub struct HsmManager {
    inner: Box<dyn HsmOperations>,
    config: HsmConfig,
}

#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
impl std::fmt::Debug for HsmManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HsmManager")
            .field("config", &self.config)
            .field("inner", &"Box<dyn HsmOperations>")
            .finish()
    }
}

#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
impl HsmManager {
    /// Create a new HSM manager with the specified configuration
    pub async fn new(config: HsmConfig) -> Result<Self> {
        use tracing::info;
        info!("Initializing HSM manager with config: {:?}", config);

        let inner: Box<dyn HsmOperations> = match &config {
            #[cfg(feature = "hsm-pkcs11")]
            HsmConfig::Pkcs11(pkcs11_config) => {
                Box::new(crate::hsm::pkcs11::Pkcs11HsmManager::new(pkcs11_config.clone()).await?)
            }
            #[cfg(feature = "hsm-yubico")]
            HsmConfig::YubiHsm(yubi_config) => {
                Box::new(crate::hsm::yubihsm::YubiHsmManager::new(yubi_config.clone()).await?)
            }
        };

        Ok(Self { inner, config })
    }

    /// Generate a DPoP key pair in the HSM
    pub async fn generate_key_pair(&self, algorithm: DpopAlgorithm) -> Result<DpopKeyPair> {
        self.inner.generate_key_pair(algorithm).await
    }

    /// Sign data using an HSM-stored private key
    pub async fn sign_data(&self, key_id: &str, data: &[u8]) -> Result<Vec<u8>> {
        self.inner.sign_data(key_id, data).await
    }

    /// List all DPoP keys stored in the HSM
    pub async fn list_keys(&self) -> Result<Vec<String>> {
        self.inner.list_keys().await
    }

    /// Delete a key from the HSM
    pub async fn delete_key(&self, key_id: &str) -> Result<()> {
        self.inner.delete_key(key_id).await
    }

    /// Check HSM connection and health status
    pub async fn health_check(&self) -> Result<HsmHealthStatus> {
        self.inner.health_check().await
    }

    /// Get HSM operation statistics
    pub fn get_stats(&self) -> HsmStats {
        self.inner.get_stats()
    }

    /// Get HSM information and capabilities
    pub async fn get_info(&self) -> Result<HsmInfo> {
        self.inner.get_info().await
    }

    /// Get the HSM configuration
    pub fn config(&self) -> &HsmConfig {
        &self.config
    }
}

// Default implementations

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_sessions: 10,
            min_sessions: 2,
            idle_timeout: Duration::from_secs(300),
            connection_timeout: Duration::from_secs(30),
        }
    }
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(30),
            operation_timeout: Duration::from_secs(60),
            health_check_timeout: Duration::from_secs(10),
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            backoff_multiplier: 2.0,
        }
    }
}

#[cfg(any(feature = "hsm-pkcs11", feature = "hsm-yubico"))]
impl HsmConfig {
    /// Create a new PKCS#11 configuration builder
    #[cfg(feature = "hsm-pkcs11")]
    pub fn pkcs11() -> Pkcs11ConfigBuilder {
        Pkcs11ConfigBuilder::default()
    }

    /// Create a new YubiHSM configuration builder
    #[cfg(feature = "hsm-yubico")]
    pub fn yubihsm() -> YubiHsmConfigBuilder {
        YubiHsmConfigBuilder::default()
    }
}

// Configuration builders

#[cfg(feature = "hsm-pkcs11")]
#[derive(Debug, Default)]
/// Builder for PKCS#11 HSM configuration
pub struct Pkcs11ConfigBuilder {
    library_path: Option<PathBuf>,
    slot_id: u64,
    token_label: Option<String>,
    user_pin: Option<secrecy::SecretString>,
    so_pin: Option<secrecy::SecretString>,
    pool_config: PoolConfig,
    timeouts: TimeoutConfig,
    retry_config: RetryConfig,
    vendor_config: HashMap<String, String>,
}

#[cfg(feature = "hsm-pkcs11")]
impl Pkcs11ConfigBuilder {
    /// Set the PKCS#11 library path
    pub fn library_path<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.library_path = Some(path.into());
        self
    }

    /// Set the PKCS#11 slot ID for the token
    pub fn slot_id(mut self, slot_id: u64) -> Self {
        self.slot_id = slot_id;
        self
    }

    /// Set the PKCS#11 token label for identification
    pub fn token_label<S: Into<String>>(mut self, label: S) -> Self {
        self.token_label = Some(label.into());
        self
    }

    /// Set the user PIN for PKCS#11 token authentication
    pub fn user_pin<S: Into<String>>(mut self, pin: S) -> Self {
        self.user_pin = Some(secrecy::SecretString::new(pin.into()));
        self
    }

    /// Set the security officer (SO) PIN for PKCS#11 token administration
    pub fn so_pin<S: Into<String>>(mut self, pin: S) -> Self {
        self.so_pin = Some(secrecy::SecretString::new(pin.into()));
        self
    }

    /// Configure the connection pool settings for PKCS#11 sessions
    pub fn pool_config(mut self, config: PoolConfig) -> Self {
        self.pool_config = config;
        self
    }

    /// Set operation timeouts
    pub fn timeouts(mut self, timeouts: TimeoutConfig) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Set retry configuration
    pub fn retry_config(mut self, retry_config: RetryConfig) -> Self {
        self.retry_config = retry_config;
        self
    }

    /// Set vendor-specific configuration
    pub fn vendor_config(mut self, config: HashMap<String, String>) -> Self {
        self.vendor_config = config;
        self
    }

    /// Build the PKCS#11 configuration
    pub fn build(self) -> Result<HsmConfig> {
        let library_path = self
            .library_path
            .ok_or_else(|| DpopError::ConfigurationError {
                reason: "PKCS#11 library path is required".to_string(),
            })?;

        let user_pin = self.user_pin.ok_or_else(|| DpopError::ConfigurationError {
            reason: "User PIN is required".to_string(),
        })?;

        Ok(HsmConfig::Pkcs11(Pkcs11Config {
            library_path,
            slot_id: self.slot_id,
            token_label: self.token_label,
            user_pin,
            so_pin: self.so_pin,
            pool_config: self.pool_config,
            timeouts: self.timeouts,
            retry_config: self.retry_config,
            vendor_config: self.vendor_config,
        }))
    }
}

#[cfg(feature = "hsm-yubico")]
#[derive(Debug, Default)]
/// Builder for YubiHSM configuration
pub struct YubiHsmConfigBuilder {
    connector: Option<YubiHsmConnector>,
    auth_key_id: u16,
    password: Option<secrecy::SecretString>,
    timeouts: TimeoutConfig,
    retry_config: RetryConfig,
}

#[cfg(feature = "hsm-yubico")]
impl YubiHsmConfigBuilder {
    /// Set HTTP connector with URL
    pub fn http_connector<S: Into<String>>(mut self, url: S) -> Self {
        self.connector = Some(YubiHsmConnector::Http { url: url.into() });
        self
    }

    /// Set USB connector
    pub fn usb_connector(mut self) -> Self {
        self.connector = Some(YubiHsmConnector::Usb);
        self
    }

    /// Set authentication key ID
    pub fn auth_key_id(mut self, key_id: u16) -> Self {
        self.auth_key_id = key_id;
        self
    }

    /// Set authentication password
    pub fn password<S: Into<String>>(mut self, password: S) -> Self {
        self.password = Some(secrecy::SecretString::new(password.into()));
        self
    }

    /// Set operation timeouts
    pub fn timeouts(mut self, timeouts: TimeoutConfig) -> Self {
        self.timeouts = timeouts;
        self
    }

    /// Set retry configuration
    pub fn retry_config(mut self, retry_config: RetryConfig) -> Self {
        self.retry_config = retry_config;
        self
    }

    /// Build the YubiHSM configuration
    pub fn build(self) -> Result<HsmConfig> {
        let connector = self.connector.unwrap_or(YubiHsmConnector::Usb);
        let password = self.password.ok_or_else(|| DpopError::ConfigurationError {
            reason: "Password is required".to_string(),
        })?;

        Ok(HsmConfig::YubiHsm(YubiHsmConfig {
            connector,
            auth_key_id: self.auth_key_id,
            password,
            timeouts: self.timeouts,
            retry_config: self.retry_config,
        }))
    }
}

// HSM backend modules
pub mod common;

#[cfg(feature = "hsm-pkcs11")]
pub mod pkcs11;

#[cfg(feature = "hsm-yubico")]
pub mod yubihsm;

// Note: HsmManager only exists when HSM features are enabled
// This is intentional - HSM is opt-in and should fail at compile time if used without features
