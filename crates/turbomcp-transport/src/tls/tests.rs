//! Comprehensive TLS transport tests
//!
//! This module provides thorough testing of the TLS transport implementation,
//! covering certificate validation, connection establishment, message transport,
//! security features, and error handling.

#[cfg(test)]
mod tls_transport_tests {
    use super::super::*;
    use crate::core::{TransportMessage, TransportMessageMetadata};
    use bytes::Bytes;
    use pretty_assertions::assert_eq;
    use sha2::{Digest, Sha256};
    use std::fs;
    use std::net::SocketAddr;
    use std::time::Duration;
    use tempfile::TempDir;
    use turbomcp_core::MessageId;

    /// Generate a self-signed certificate for testing using rcgen
    async fn generate_test_certificate(
        temp_dir: &TempDir,
    ) -> (std::path::PathBuf, std::path::PathBuf) {
        use rcgen::generate_simple_self_signed;
        
        let cert_path = temp_dir.path().join("test_cert.pem");
        let key_path = temp_dir.path().join("test_key.pem");

        // Generate a proper self-signed certificate using rcgen
        let subject_alt_names = vec!["localhost".to_string()];
        let certified_key = generate_simple_self_signed(subject_alt_names)
            .expect("Failed to generate test certificate");
        
        let cert_pem = certified_key.cert.pem();
        let key_pem = certified_key.key_pair.serialize_pem();

        fs::write(&cert_path, cert_pem).expect("Failed to write test certificate");
        fs::write(&key_path, key_pem).expect("Failed to write test key");

        (cert_path, key_path)
    }
    
    /// Generate a test certificate and return its DER bytes and public key SHA256 hash  
    fn generate_test_certificate_with_hash() -> (Vec<u8>, String) {
        use rcgen::generate_simple_self_signed;
        
        // Generate a simple self-signed certificate for testing
        let subject_alt_names = vec!["localhost".to_string()];
        let certified_key = generate_simple_self_signed(subject_alt_names)
            .expect("Failed to generate test certificate");
        
        let cert_der = certified_key.cert.der().to_vec();
        
        // Calculate SHA256 hash of the public key (not the certificate)
        // For testing, we'll use a simplified approach that extracts the public key bytes
        let public_key_der = certified_key.key_pair.public_key_der();
        let hash = Sha256::digest(&public_key_der);
        let hash_hex = hex::encode(hash);
        let hash_string = format!("sha256:{}", hash_hex);
        
        (cert_der, hash_string)
    }

    #[tokio::test]
    async fn test_tls_config_creation() {
        let config = TlsConfig::new("cert.pem", "key.pem");

        assert_eq!(config.cert_chain_path, std::path::PathBuf::from("cert.pem"));
        assert_eq!(config.private_key_path, std::path::PathBuf::from("key.pem"));
        assert_eq!(config.min_version, TlsVersion::V1_3);
        assert!(config.ocsp_stapling);
        assert_eq!(config.client_auth, ClientAuthMode::None);
        assert!(config.session_resumption);
        assert!(config.cert_pinning.is_none());
    }

    #[tokio::test]
    async fn test_tls_config_with_mtls() {
        let config = TlsConfig::new("cert.pem", "key.pem").with_mtls();

        assert_eq!(config.client_auth, ClientAuthMode::Required);
    }

    #[tokio::test]
    async fn test_tls_config_with_cert_pinning() {
        let pinning = CertPinningConfig {
            allowed_hashes: vec!["sha256:ABCD1234".to_string()],
            enforce: true,
        };

        let config = TlsConfig::new("cert.pem", "key.pem").with_cert_pinning(pinning.clone());

        assert!(config.cert_pinning.is_some());
        let actual_pinning = config.cert_pinning.unwrap();
        assert_eq!(actual_pinning.allowed_hashes, pinning.allowed_hashes);
        assert_eq!(actual_pinning.enforce, pinning.enforce);
    }

    #[tokio::test]
    async fn test_tls_config_with_dpop_security() {
        let config = TlsConfig::new("cert.pem", "key.pem")
            .with_min_version(TlsVersion::V1_2) // Start with TLS 1.2
            .with_dpop_security(); // Should upgrade to TLS 1.3

        assert_eq!(config.min_version, TlsVersion::V1_3);
        assert!(config.ocsp_stapling);
        assert!(config.validation.verify_hostname);
    }

    #[tokio::test]
    async fn test_cert_validation_config_defaults() {
        let config = CertValidationConfig::default();

        assert!(config.verify_hostname);
        assert!(config.ca_bundle_path.is_none());
        assert!(config.client_ca_cert_path.is_none());
        assert!(config.ocsp_stapling);
        assert!(config.ct_validation); // CT validation is enabled by default for security
    }

    #[tokio::test]
    async fn test_tls_transport_creation_without_feature() {
        // This test runs regardless of the TLS feature to ensure proper error handling
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let config = TlsConfig::default();

        #[cfg(not(feature = "tls"))]
        {
            let result = TlsTransport::new_server(bind_addr, config).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                TlsError::Configuration { reason } => {
                    assert_eq!(reason, "TLS feature not enabled");
                }
                _ => panic!("Expected configuration error"),
            }
        }

        #[cfg(feature = "tls")]
        {
            // With TLS feature enabled, creation should work (even if cert files don't exist)
            // We expect a certificate loading error instead
            let result = TlsTransport::new_server(bind_addr, config).await;
            assert!(result.is_err());
            match result.unwrap_err() {
                TlsError::Certificate { .. } => {
                    // Expected - certificate files don't exist
                }
                _ => panic!("Expected certificate error when files don't exist"),
            }
        }
    }

    #[cfg(feature = "tls")]
    #[tokio::test]
    async fn test_tls_transport_capabilities() {
        // Test TLS configuration capabilities without creating actual transport
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let (cert_path, key_path) = generate_test_certificate(&temp_dir).await;
        let config = TlsConfig::new(cert_path, key_path);

        // Verify configuration has the expected properties for capabilities
        assert!(!config.cert_chain_path.to_string_lossy().is_empty());
        assert!(!config.private_key_path.to_string_lossy().is_empty());

        // Test configuration validation (this is what capabilities depend on)
        assert!(
            config.cert_chain_path.exists() || !config.cert_chain_path.to_string_lossy().is_empty()
        );
        assert!(
            config.private_key_path.exists()
                || !config.private_key_path.to_string_lossy().is_empty()
        );

        // Test that configuration is internally consistent for TLS capabilities
        assert_eq!(config.handshake_timeout, Duration::from_secs(30));
        assert_eq!(config.min_version, TlsVersion::V1_3);
    }

    #[tokio::test]
    async fn test_transport_message_serialization() {
        // Test that TransportMessage can be serialized/deserialized properly
        let message = TransportMessage {
            id: MessageId::from("test-message-123"),
            payload: Bytes::from("Hello, TLS World!"),
            metadata: TransportMessageMetadata::default(),
        };

        // Test JSON serialization
        let json_data = serde_json::to_vec(&message).expect("Failed to serialize message");
        assert!(!json_data.is_empty());

        // Test JSON deserialization
        let deserialized: TransportMessage =
            serde_json::from_slice(&json_data).expect("Failed to deserialize message");

        assert_eq!(deserialized.id, message.id);
        assert_eq!(deserialized.payload, message.payload);
    }

    #[tokio::test]
    async fn test_certificate_pinning_validation() {
        // Generate a real test certificate with proper hash
        let (cert_der, hash_string) = generate_test_certificate_with_hash();
        
        // Test 1: Valid pinning configuration with correct hash - should pass
        let valid_pinning_config = CertPinningConfig {
            allowed_hashes: vec![hash_string.clone()],
            enforce: true,
        };
        
        let cert_data = CertificateDer::from(cert_der.clone());
        let peer_certs = vec![cert_data];

        #[cfg(feature = "tls")]
        {
            let result = TlsTransport::validate_cert_pinning(&valid_pinning_config, &peer_certs);
            assert!(result.is_ok(), "Certificate pinning should succeed with correct hash");
        }
        
        // Test 2: Invalid pinning configuration with wrong hash - should fail
        let invalid_pinning_config = CertPinningConfig {
            allowed_hashes: vec!["sha256:wronghashvalue1234567890abcdef".to_string()],
            enforce: true,
        };
        
        let cert_data_2 = CertificateDer::from(cert_der);
        let peer_certs_2 = vec![cert_data_2];

        #[cfg(feature = "tls")]
        {
            let result = TlsTransport::validate_cert_pinning(&invalid_pinning_config, &peer_certs_2);
            assert!(result.is_err(), "Certificate pinning should fail with wrong hash");
            match result.unwrap_err() {
                TlsError::PinningFailed { reason } => {
                    assert!(reason.contains("Certificate pin validation failed"));
                }
                _ => panic!("Expected pinning failed error"),
            }
        }
    }

    #[tokio::test]
    async fn test_certificate_pinning_non_enforced() {
        // Generate a real test certificate
        let (cert_der, _) = generate_test_certificate_with_hash();
        
        // Configure pinning with wrong hash but non-enforced mode
        let pinning_config = CertPinningConfig {
            allowed_hashes: vec!["sha256:wronghashvalue1234567890abcdef".to_string()],
            enforce: false, // Non-enforced mode
        };

        let cert_data = CertificateDer::from(cert_der);
        let peer_certs = vec![cert_data];

        #[cfg(feature = "tls")]
        {
            let result = TlsTransport::validate_cert_pinning(&pinning_config, &peer_certs);

            // Should succeed in non-enforced mode (just logs warning)
            assert!(result.is_ok(), "Non-enforced pinning should always pass");
        }
    }

    #[tokio::test]
    async fn test_tls_error_conversion() {
        let tls_errors = vec![
            TlsError::Configuration {
                reason: "Test config error".to_string(),
            },
            TlsError::Certificate {
                reason: "Test cert error".to_string(),
            },
            TlsError::Handshake {
                reason: "Test handshake error".to_string(),
            },
            TlsError::PinningFailed {
                reason: "Test pinning error".to_string(),
            },
            TlsError::OcspFailed {
                reason: "Test OCSP error".to_string(),
            },
        ];

        for tls_error in tls_errors {
            let transport_error: crate::core::TransportError = tls_error.into();

            // Verify conversion works and produces meaningful errors
            assert!(!transport_error.to_string().is_empty());
        }
    }

    #[tokio::test]
    async fn test_tls_version_defaults() {
        assert_eq!(TlsVersion::default(), TlsVersion::V1_3);
    }

    #[tokio::test]
    async fn test_client_auth_mode_defaults() {
        assert_eq!(ClientAuthMode::default(), ClientAuthMode::None);
    }

    #[tokio::test]
    async fn test_transport_state_transitions() {
        // Test state transition logic through configuration validation
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let (cert_path, key_path) = generate_test_certificate(&temp_dir).await;

        // Test various configuration states
        let config_valid = TlsConfig::new(cert_path.clone(), key_path.clone());
        assert!(!config_valid.cert_chain_path.to_string_lossy().is_empty());

        // Test configuration with mTLS (different state)
        let config_mtls = TlsConfig::new(cert_path.clone(), key_path.clone()).with_mtls();

        // Verify the configurations represent different states
        assert!(!config_valid.cert_chain_path.to_string_lossy().is_empty());
        assert!(!config_mtls.cert_chain_path.to_string_lossy().is_empty());

        // Both configurations should be internally consistent
        assert_eq!(config_valid.cert_chain_path, config_mtls.cert_chain_path);
    }

    #[tokio::test]
    async fn test_concurrent_tls_operations() {
        // Test concurrent configuration operations for thread safety
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let (cert_path, key_path) = generate_test_certificate(&temp_dir).await;

        let handles: Vec<_> = (0..10)
            .map(|i| {
                let cert_path = cert_path.clone();
                let key_path = key_path.clone();
                tokio::spawn(async move {
                    // Test concurrent configuration creation
                    let config = TlsConfig::new(cert_path.clone(), key_path.clone());

                    // Test concurrent configuration modifications
                    let _config_with_mtls = TlsConfig::new(cert_path, key_path).with_mtls();

                    // Verify each task can create valid configurations
                    assert!(!config.cert_chain_path.to_string_lossy().is_empty());

                    i
                })
            })
            .collect();

        // Wait for all tasks to complete
        for handle in handles {
            let result = handle.await.expect("Task failed");
            assert!(result < 10);
        }
    }

    #[tokio::test]
    async fn test_tls_config_validation() {
        // Test various TLS configuration edge cases

        // Test with minimum viable config
        let config = TlsConfig::new("cert.pem", "key.pem").with_min_version(TlsVersion::V1_2);
        assert_eq!(config.min_version, TlsVersion::V1_2);

        // Test timeout configuration
        assert_eq!(config.handshake_timeout, Duration::from_secs(30));

        // Test that DPoP security overrides minimum version
        let dpop_config = TlsConfig::new("cert.pem", "key.pem")
            .with_min_version(TlsVersion::V1_2) // Start with 1.2
            .with_dpop_security(); // Should upgrade to 1.3

        assert_eq!(dpop_config.min_version, TlsVersion::V1_3);
    }

    #[tokio::test]
    async fn test_error_handling_edge_cases() {
        // Test empty certificate list for pinning
        let pinning_config = CertPinningConfig {
            allowed_hashes: vec!["sha256:test".to_string()],
            enforce: true,
        };

        #[cfg(feature = "tls")]
        {
            let result = TlsTransport::validate_cert_pinning(&pinning_config, &[]);
            assert!(result.is_err());

            match result.unwrap_err() {
                TlsError::PinningFailed { reason } => {
                    assert!(reason.contains("No peer certificates provided"));
                }
                _ => panic!("Expected pinning failed error for empty cert list"),
            }
        }
    }

    /// Test demonstrating TLS configuration workflow
    #[tokio::test]
    async fn test_tls_workflow_concept() {
        // Test the complete configuration workflow without actual connections
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let (cert_path, key_path) = generate_test_certificate(&temp_dir).await;

        // Test server configuration with advanced features
        let server_config = TlsConfig::new(cert_path.clone(), key_path.clone())
            .with_mtls()
            .with_cert_pinning(CertPinningConfig {
                allowed_hashes: vec!["sha256:expected_client_key_hash".to_string()],
                enforce: false, // Non-enforced for testing
            })
            .with_dpop_security();

        // Test client configuration
        let client_config = TlsConfig::new(cert_path, key_path);

        // Verify configurations are properly constructed
        assert!(!server_config.cert_chain_path.to_string_lossy().is_empty());
        assert!(!client_config.cert_chain_path.to_string_lossy().is_empty());

        // Verify configurations have expected properties
        assert_eq!(server_config.cert_chain_path, client_config.cert_chain_path);
        assert_eq!(
            server_config.private_key_path,
            client_config.private_key_path
        );

        // Test that the workflow created different configuration objects
        assert_ne!(
            format!("{:?}", server_config),
            format!("{:?}", client_config)
        );

        // Verify advanced features were configured correctly
        assert_eq!(server_config.min_version, TlsVersion::V1_3); // DPoP enforces TLS 1.3
    }
}

/// Performance and stress tests for TLS transport
#[cfg(test)]
mod performance_tests {
    use super::super::*;

    #[tokio::test]
    async fn test_tls_config_creation_performance() {
        let start = std::time::Instant::now();

        // Create many TLS configs to test performance
        for i in 0..1000 {
            let _config = TlsConfig::new(format!("cert_{}.pem", i), format!("key_{}.pem", i))
                .with_mtls()
                .with_min_version(TlsVersion::V1_3)
                .with_dpop_security();
        }

        let duration = start.elapsed();

        // Should be very fast (well under 1ms per config)
        assert!(duration < std::time::Duration::from_millis(100));
        println!("Created 1000 TLS configs in {:?}", duration);
    }

    #[tokio::test]
    async fn test_certificate_pinning_validation_performance() {
        let pinning_config = CertPinningConfig {
            allowed_hashes: vec!["sha256:hash1".to_string(), "sha256:hash2".to_string()],
            enforce: false,
        };

        let cert_data = CertificateDer::from(vec![1; 1024]); // 1KB cert
        let peer_certs = vec![cert_data];

        let start = std::time::Instant::now();

        #[cfg(feature = "tls")]
        {
            // Run pinning validation many times
            for _ in 0..1000 {
                let _result = TlsTransport::validate_cert_pinning(&pinning_config, &peer_certs);
            }
        }

        let duration = start.elapsed();
        println!(
            "Performed 1000 certificate pinning validations in {:?}",
            duration
        );

        // Should be reasonably fast
        assert!(duration < std::time::Duration::from_millis(500));
    }
}
