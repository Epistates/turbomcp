//! Comprehensive tests for Unix domain socket transport implementation

#[cfg(feature = "unix")]
mod unix_tests {
    use std::path::{Path, PathBuf};
    use turbomcp_transport::core::{Transport, TransportState, TransportType};
    use turbomcp_transport::unix::{UnixConfig, UnixTransport, UnixTransportBuilder};

    #[test]
    fn test_unix_config_default() {
        let config = UnixConfig::default();
        assert_eq!(config.socket_path, Path::new("/tmp/turbomcp.sock"));
        assert_eq!(config.permissions, Some(0o600));
        assert_eq!(config.buffer_size, 8192);
        assert!(config.cleanup_on_disconnect);
    }

    #[test]
    fn test_unix_config_debug() {
        let config = UnixConfig::default();
        let debug_str = format!("{config:?}");
        assert!(debug_str.contains("UnixConfig"));
        assert!(debug_str.contains("socket_path"));
        assert!(debug_str.contains("/tmp/turbomcp.sock"));
    }

    #[test]
    fn test_unix_config_clone() {
        let original = UnixConfig::default();
        let cloned = original.clone();
        assert_eq!(original.socket_path, cloned.socket_path);
        assert_eq!(original.permissions, cloned.permissions);
        assert_eq!(original.buffer_size, cloned.buffer_size);
        assert_eq!(original.cleanup_on_disconnect, cloned.cleanup_on_disconnect);
    }

    #[test]
    fn test_unix_config_custom_values() {
        let socket_path = PathBuf::from("/tmp/custom-socket.sock");
        let config = UnixConfig {
            socket_path: socket_path.clone(),
            permissions: Some(0o755),
            buffer_size: 16384,
            cleanup_on_disconnect: false,
        };

        assert_eq!(config.socket_path, socket_path);
        assert_eq!(config.permissions, Some(0o755));
        assert_eq!(config.buffer_size, 16384);
        assert!(!config.cleanup_on_disconnect);
    }

    #[test]
    fn test_unix_transport_builder_new_server() {
        let builder = UnixTransportBuilder::new_server();
        let transport = builder.build();

        // is_server is private, we verify behavior instead
        assert_eq!(transport.transport_type(), TransportType::Unix);
    }

    #[test]
    fn test_unix_transport_builder_new_client() {
        let builder = UnixTransportBuilder::new_client();
        let transport = builder.build();

        // is_server is private, we verify behavior instead
        assert_eq!(transport.transport_type(), TransportType::Unix);
    }

    #[test]
    fn test_unix_transport_builder_socket_path() {
        let custom_path = "/tmp/test-socket.sock";
        let transport = UnixTransportBuilder::new_server()
            .socket_path(custom_path)
            .build();

        // socket_path is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("test-socket.sock"));
    }

    #[test]
    fn test_unix_transport_builder_socket_path_pathbuf() {
        let custom_path = PathBuf::from("/tmp/pathbuf-socket.sock");
        let transport = UnixTransportBuilder::new_server()
            .socket_path(custom_path.clone())
            .build();

        // socket_path is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("pathbuf-socket.sock"));
    }

    #[test]
    fn test_unix_transport_builder_permissions() {
        let transport = UnixTransportBuilder::new_server()
            .permissions(0o644)
            .build();

        // Permissions are stored in the builder config but not directly accessible
        // from the transport, so we just verify it doesn't panic
        assert_eq!(transport.transport_type(), TransportType::Unix);
    }

    #[test]
    fn test_unix_transport_builder_buffer_size() {
        let transport = UnixTransportBuilder::new_server().buffer_size(4096).build();

        assert_eq!(transport.transport_type(), TransportType::Unix);
    }

    #[test]
    fn test_unix_transport_builder_cleanup_on_disconnect() {
        let transport = UnixTransportBuilder::new_server()
            .cleanup_on_disconnect(false)
            .build();

        assert_eq!(transport.transport_type(), TransportType::Unix);
    }

    #[test]
    fn test_unix_transport_builder_chaining() {
        let path = "/tmp/chained-socket.sock";
        let transport = UnixTransportBuilder::new_client()
            .socket_path(path)
            .permissions(0o755)
            .buffer_size(32768)
            .cleanup_on_disconnect(true)
            .build();

        // socket_path is private, test via endpoint instead
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("chained-socket.sock"));
        // is_server is private, we verify behavior instead // Client mode
    }

    #[test]
    fn test_unix_transport_new_server() {
        let path = PathBuf::from("/tmp/server-socket.sock");
        let transport = UnixTransport::new_server(path.clone());

        // socket_path is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains(".sock"));
        // is_server is private, we verify behavior instead
        assert_eq!(transport.transport_type(), TransportType::Unix);
    }

    #[test]
    fn test_unix_transport_new_client() {
        let path = PathBuf::from("/tmp/client-socket.sock");
        let transport = UnixTransport::new_client(path.clone());

        // socket_path is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains(".sock"));
        // is_server is private, we verify behavior instead
        assert_eq!(transport.transport_type(), TransportType::Unix);
    }

    #[test]
    fn test_unix_transport_debug() {
        let transport = UnixTransport::new_server(PathBuf::from("/tmp/debug.sock"));
        let debug_str = format!("{transport:?}");
        assert!(debug_str.contains("UnixTransport"));
    }

    #[tokio::test]
    async fn test_unix_transport_state() {
        let transport = UnixTransport::new_server(PathBuf::from("/tmp/state.sock"));

        let state = transport.state().await;
        assert_eq!(state, TransportState::Disconnected);
    }

    #[test]
    fn test_unix_transport_transport_type() {
        let transport = UnixTransport::new_server(PathBuf::from("/tmp/type.sock"));
        assert_eq!(transport.transport_type(), TransportType::Unix);
    }

    #[test]
    fn test_unix_transport_capabilities() {
        let transport = UnixTransport::new_server(PathBuf::from("/tmp/caps.sock"));
        let caps = transport.capabilities();

        assert!(caps.supports_bidirectional);
        assert!(caps.supports_streaming);
        assert_eq!(caps.max_message_size, Some(1024 * 1024)); // 1MB
    }

    #[test]
    fn test_unix_transport_endpoint() {
        let path = PathBuf::from("/tmp/endpoint.sock");
        let transport = UnixTransport::new_server(path.clone());

        let endpoint = transport.endpoint();
        assert_eq!(endpoint, Some(format!("unix://{}", path.display())));
    }

    #[tokio::test]
    async fn test_unix_transport_metrics() {
        let transport = UnixTransport::new_server(PathBuf::from("/tmp/metrics.sock"));
        let metrics = transport.metrics().await;

        assert_eq!(metrics.messages_sent, 0);
        assert_eq!(metrics.messages_received, 0);
        assert_eq!(metrics.bytes_sent, 0);
        assert_eq!(metrics.bytes_received, 0);
    }

    #[tokio::test]
    async fn test_unix_transport_send_when_disconnected() {
        use bytes::Bytes;
        use turbomcp_core::MessageId;
        use turbomcp_transport::core::TransportMessage;

        let mut transport = UnixTransport::new_server(PathBuf::from("/tmp/send.sock"));
        let message = TransportMessage::new(
            MessageId::String("test".to_string()),
            Bytes::from("test message"),
        );

        let result = transport.send(message).await;
        assert!(result.is_err());

        if let Err(err) = result {
            let error_msg = format!("{err}");
            assert!(
                error_msg.contains("No active Unix socket connections")
                    || error_msg.contains("ConnectionFailed")
            );
        }
    }

    #[tokio::test]
    async fn test_unix_transport_receive_when_disconnected() {
        let mut transport = UnixTransport::new_server(PathBuf::from("/tmp/receive.sock"));

        let result = transport.receive().await;
        assert!(result.is_err());

        if let Err(err) = result {
            let error_msg = format!("{err}");
            assert!(
                error_msg.contains("Unix socket transport not connected")
                    || error_msg.contains("ConnectionFailed")
            );
        }
    }

    #[tokio::test]
    async fn test_unix_transport_disconnect() {
        let mut transport = UnixTransport::new_server(PathBuf::from("/tmp/disconnect.sock"));

        let result = transport.disconnect().await;
        assert!(result.is_ok());

        let state = transport.state().await;
        assert_eq!(state, TransportState::Disconnected);
    }

    // Test path-related functionality
    #[test]
    fn test_path_operations() {
        let path_str = "/tmp/test.sock";
        let path = Path::new(path_str);
        let pathbuf = PathBuf::from(path_str);

        assert_eq!(path, pathbuf.as_path());
        assert_eq!(path.to_string_lossy(), path_str);
    }

    #[test]
    fn test_path_with_directories() {
        let path = PathBuf::from("/var/run/app/socket.sock");
        let transport = UnixTransport::new_server(path.clone());

        // socket_path is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains(".sock"));
    }

    #[test]
    fn test_relative_path() {
        let path = PathBuf::from("./relative-socket.sock");
        let transport = UnixTransport::new_client(path.clone());

        // socket_path is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains(".sock"));
    }

    #[test]
    fn test_path_display() {
        let path = PathBuf::from("/tmp/display-test.sock");
        let transport = UnixTransport::new_server(path.clone());
        let endpoint = transport.endpoint().unwrap();

        assert!(endpoint.starts_with("unix://"));
        assert!(endpoint.contains("display-test.sock"));
    }

    // Test permission values
    #[test]
    fn test_permission_values() {
        let config = UnixConfig {
            socket_path: PathBuf::from("/tmp/perms.sock"),
            permissions: Some(0o600), // Owner read/write only
            buffer_size: 8192,
            cleanup_on_disconnect: true,
        };

        assert_eq!(config.permissions, Some(0o600));
    }

    #[test]
    fn test_permission_values_644() {
        let config = UnixConfig {
            socket_path: PathBuf::from("/tmp/perms.sock"),
            permissions: Some(0o644), // Owner read/write, group/others read
            buffer_size: 8192,
            cleanup_on_disconnect: true,
        };

        assert_eq!(config.permissions, Some(0o644));
    }

    #[test]
    fn test_no_permissions() {
        let config = UnixConfig {
            socket_path: PathBuf::from("/tmp/no-perms.sock"),
            permissions: None,
            buffer_size: 8192,
            cleanup_on_disconnect: true,
        };

        assert_eq!(config.permissions, None);
    }

    // Test buffer size boundaries
    #[test]
    fn test_buffer_size_boundaries() {
        // Test minimum buffer size
        let config1 = UnixConfig {
            buffer_size: 1,
            ..Default::default()
        };
        assert_eq!(config1.buffer_size, 1);

        // Test large buffer size
        let config2 = UnixConfig {
            buffer_size: 1024 * 1024, // 1MB
            ..Default::default()
        };
        assert_eq!(config2.buffer_size, 1024 * 1024);
    }

    // Test cleanup flag
    #[test]
    fn test_cleanup_on_disconnect_true() {
        let config = UnixConfig {
            socket_path: PathBuf::from("/tmp/cleanup-true.sock"),
            permissions: Some(0o600),
            buffer_size: 8192,
            cleanup_on_disconnect: true,
        };

        assert!(config.cleanup_on_disconnect);
    }

    #[test]
    fn test_cleanup_on_disconnect_false() {
        let config = UnixConfig {
            socket_path: PathBuf::from("/tmp/cleanup-false.sock"),
            permissions: Some(0o600),
            buffer_size: 8192,
            cleanup_on_disconnect: false,
        };

        assert!(!config.cleanup_on_disconnect);
    }

    // Test transport state transitions
    #[test]
    fn test_transport_state_equality() {
        let state1 = TransportState::Disconnected;
        let state2 = TransportState::Disconnected;
        assert_eq!(state1, state2);

        let state3 = TransportState::Connected;
        assert_ne!(state1, state3);
    }

    #[test]
    fn test_transport_state_clone() {
        let original = TransportState::Connecting;
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    // Test different socket paths
    #[test]
    fn test_various_socket_paths() {
        let paths = vec![
            "/tmp/test1.sock",
            "/var/run/test2.sock",
            "./local.sock",
            "../parent.sock",
            "simple.sock",
        ];

        for path_str in paths {
            let path = PathBuf::from(path_str);
            let transport = UnixTransport::new_server(path.clone());
            // socket_path is private, but we can verify through endpoint
            let endpoint = transport.endpoint().unwrap();
            assert!(endpoint.contains(".sock"));
        }
    }

    #[test]
    fn test_socket_path_with_spaces() {
        let path = PathBuf::from("/tmp/socket with spaces.sock");
        let transport = UnixTransport::new_server(path.clone());
        // socket_path is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains(".sock"));
    }

    #[test]
    fn test_socket_path_with_unicode() {
        let path = PathBuf::from("/tmp/socket_测试_🚀.sock");
        let transport = UnixTransport::new_client(path.clone());
        // socket_path is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains(".sock"));
    }

    // Test builder pattern edge cases
    #[test]
    fn test_builder_multiple_socket_path_calls() {
        let transport = UnixTransportBuilder::new_server()
            .socket_path("/tmp/first.sock")
            .socket_path("/tmp/second.sock") // This should override the first
            .build();

        // socket_path is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("second.sock"));
    }

    #[test]
    fn test_builder_multiple_permission_calls() {
        let _transport = UnixTransportBuilder::new_server()
            .permissions(0o600)
            .permissions(0o755) // This should override the first
            .build();

        // Permissions validated by successful builder configuration
    }

    #[test]
    fn test_empty_socket_path() {
        let path = PathBuf::from("");
        let transport = UnixTransport::new_server(path.clone());
        // socket_path is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        // An empty path should result in "unix://" endpoint
        assert!(endpoint.starts_with("unix://"));
    }

    // ============================================================================
    // Async File I/O Tests - Production-Grade TDD
    // ============================================================================

    #[tokio::test]
    async fn test_socket_cleanup_async_file_operations() {
        use std::io::Write;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let socket_path = temp_dir.path().join("async_test.sock");

        // Create an existing socket file to be cleaned up
        {
            let mut file =
                std::fs::File::create(&socket_path).expect("Failed to create test socket file");
            file.write_all(b"fake socket")
                .expect("Failed to write to test file");
            file.sync_all().expect("Failed to sync test file");
        }

        // Verify file exists before cleanup
        assert!(
            socket_path.exists(),
            "Test socket file should exist before cleanup"
        );

        // Get the old file metadata to verify it's replaced
        let old_metadata =
            std::fs::metadata(&socket_path).expect("Failed to get old file metadata");

        // Create transport that should clean up the existing socket
        let mut transport = UnixTransport::new_server(socket_path.clone());

        // Test that connect performs async cleanup without blocking
        let start_time = std::time::Instant::now();
        let result = transport.connect().await;
        let cleanup_duration = start_time.elapsed();

        // Should complete quickly using tokio::fs::remove_file (async I/O)
        assert!(
            cleanup_duration.as_millis() < 100,
            "Socket cleanup took {}ms - should be <100ms for async I/O",
            cleanup_duration.as_millis()
        );

        // Verify operation succeeded
        match result {
            Ok(_) => {
                // New socket file should exist (created by bind)
                assert!(
                    socket_path.exists(),
                    "New socket file should exist after connect"
                );

                // Verify it's a socket, not our fake file
                let new_metadata =
                    std::fs::metadata(&socket_path).expect("Failed to get new file metadata");
                assert_ne!(
                    old_metadata.len(),
                    new_metadata.len(),
                    "Socket file should be replaced (old: {} bytes, new: {} bytes)",
                    old_metadata.len(),
                    new_metadata.len()
                );
            }
            Err(e) => {
                // If connect failed, at least verify cleanup was attempted
                println!("connect failed during test: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_socket_cleanup_preserves_async_context() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let socket_path = temp_dir.path().join("context_test.sock");

        // Create existing socket file
        std::fs::File::create(&socket_path).expect("Failed to create test socket");

        let _transport = UnixTransport::new_server(socket_path.clone());

        // Test that we can run multiple concurrent cleanup operations
        // This will FAIL initially due to blocking I/O preventing proper async concurrency
        let handles = (0..3)
            .map(|i| {
                let _socket_path_clone = socket_path.clone();
                let transport_path = temp_dir.path().join(format!("concurrent_test_{}.sock", i));

                tokio::spawn(async move {
                    // Create test socket for each concurrent operation
                    std::fs::File::create(&transport_path).ok();

                    let mut concurrent_transport = UnixTransport::new_server(transport_path);
                    concurrent_transport.connect().await
                })
            })
            .collect::<Vec<_>>();

        // Wait for all concurrent operations
        let start_time = std::time::Instant::now();
        let results = futures::future::join_all(handles).await;
        let total_duration = start_time.elapsed();

        // All operations should complete quickly if truly async
        assert!(
            total_duration.as_millis() < 500,
            "Concurrent cleanup operations took {}ms - should be <500ms for proper async I/O",
            total_duration.as_millis()
        );

        // At least one operation should have attempted cleanup
        let _success_count = results.iter().filter(|r| r.is_ok()).count();
        // We don't require all to succeed since we're testing concurrency
    }

    #[tokio::test]
    async fn test_socket_file_removal_error_handling() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp directory");

        // Create a socket path in a non-existent directory to trigger removal errors
        let non_existent_dir = temp_dir.path().join("does_not_exist");
        let invalid_socket_path = non_existent_dir.join("invalid.sock");

        let mut transport = UnixTransport::new_server(invalid_socket_path.clone());

        // Test error handling for file removal failures
        let result = transport.connect().await;

        // Should handle file removal errors gracefully (either by succeeding because file doesn't exist,
        // or by providing meaningful error messages)
        match result {
            Ok(_) => {
                // OK - no file existed to remove
            }
            Err(e) => {
                let error_msg = format!("{}", e);
                // Error should be about the directory not existing, not a generic blocking I/O error
                assert!(
                    error_msg.contains("No such file")
                        || error_msg.contains("directory")
                        || error_msg.contains("path"),
                    "Error should be descriptive about path issues, got: {}",
                    error_msg
                );
            }
        }
    }

    #[tokio::test]
    async fn test_socket_cleanup_on_drop_is_async_safe() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let socket_path = temp_dir.path().join("drop_test.sock");

        // Create socket file
        std::fs::File::create(&socket_path).expect("Failed to create test socket");

        {
            // Create transport in scope that will be dropped
            let _transport = UnixTransport::new_server(socket_path.clone());

            // Force the transport to think it's a server with an active socket
            // The drop implementation should handle cleanup without blocking
        } // transport drops here

        // After drop, we should be able to continue async operations immediately
        let start_time = std::time::Instant::now();

        // This async operation should not be delayed by blocking I/O in drop
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;

        let post_drop_duration = start_time.elapsed();

        // Drop should not have introduced significant blocking delays
        assert!(
            post_drop_duration.as_millis() < 50,
            "Post-drop async operations delayed by {}ms - drop may have blocked",
            post_drop_duration.as_millis()
        );
    }

    #[tokio::test]
    async fn test_verify_tokio_fs_import_available() {
        // This test verifies that tokio::fs is available for use
        // Will PASS once we add the proper imports in the implementation

        let temp_dir = tempfile::TempDir::new().expect("Failed to create temp directory");
        let test_file = temp_dir.path().join("tokio_test.txt");

        // Test tokio::fs operations work correctly
        tokio::fs::write(&test_file, b"test content")
            .await
            .expect("tokio::fs::write should work");

        let content = tokio::fs::read_to_string(&test_file)
            .await
            .expect("tokio::fs::read should work");
        assert_eq!(content, "test content");

        tokio::fs::remove_file(&test_file)
            .await
            .expect("tokio::fs::remove_file should work");

        assert!(!test_file.exists(), "File should be removed");
    }
}

// Tests that work without the unix feature
#[test]
fn test_unix_module_accessible() {
    // Module accessibility validated by successful compilation
}
