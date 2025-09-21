//! Comprehensive tests for TCP transport implementation

#[cfg(feature = "tcp")]
mod tcp_tests {
    use std::net::SocketAddr;
    use std::str::FromStr;
    use turbomcp_transport::core::{Transport, TransportState, TransportType};
    use turbomcp_transport::tcp::{TcpConfig, TcpTransport, TcpTransportBuilder};

    #[test]
    fn test_tcp_config_default() {
        let config = TcpConfig::default();
        assert_eq!(config.bind_addr.to_string(), "127.0.0.1:8080");
        assert_eq!(config.remote_addr, None);
        assert_eq!(config.connect_timeout_ms, 5000);
        assert!(config.keep_alive);
        assert_eq!(config.buffer_size, 8192);
    }

    #[test]
    fn test_tcp_config_debug() {
        let config = TcpConfig::default();
        let debug_str = format!("{config:?}");
        assert!(debug_str.contains("TcpConfig"));
        assert!(debug_str.contains("bind_addr"));
        assert!(debug_str.contains("127.0.0.1:8080"));
    }

    #[test]
    fn test_tcp_config_clone() {
        let original = TcpConfig::default();
        let cloned = original.clone();
        assert_eq!(original.bind_addr, cloned.bind_addr);
        assert_eq!(original.connect_timeout_ms, cloned.connect_timeout_ms);
        assert_eq!(original.keep_alive, cloned.keep_alive);
    }

    #[test]
    fn test_tcp_config_custom_values() {
        let bind_addr: SocketAddr = "192.168.1.100:9090".parse().unwrap();
        let remote_addr: SocketAddr = "192.168.1.101:9091".parse().unwrap();

        let config = TcpConfig {
            bind_addr,
            remote_addr: Some(remote_addr),
            connect_timeout_ms: 10000,
            keep_alive: false,
            buffer_size: 16384,
        };

        assert_eq!(config.bind_addr, bind_addr);
        assert_eq!(config.remote_addr, Some(remote_addr));
        assert_eq!(config.connect_timeout_ms, 10000);
        assert!(!config.keep_alive);
        assert_eq!(config.buffer_size, 16384);
    }

    #[test]
    fn test_tcp_transport_builder_new() {
        let builder = TcpTransportBuilder::new();
        // Should not panic and create successfully
        let transport = builder.build();
        assert_eq!(transport.transport_type(), TransportType::Tcp);
    }

    #[test]
    fn test_tcp_transport_builder_default() {
        let builder = TcpTransportBuilder::default();
        let transport = builder.build();
        assert_eq!(transport.transport_type(), TransportType::Tcp);
    }

    #[test]
    fn test_tcp_transport_builder_bind_addr() {
        let addr: SocketAddr = "127.0.0.1:9000".parse().unwrap();
        let transport = TcpTransportBuilder::new().bind_addr(addr).build();

        // bind_addr is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("9000"));
    }

    #[test]
    fn test_tcp_transport_builder_remote_addr() {
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let remote_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

        let transport = TcpTransportBuilder::new()
            .bind_addr(bind_addr)
            .remote_addr(remote_addr)
            .build();

        // remote_addr is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("8080"));
    }

    #[test]
    fn test_tcp_transport_builder_timeout() {
        let transport = TcpTransportBuilder::new().connect_timeout_ms(15000).build();

        // Timeout is used internally, we can't directly access it from the transport
        // but we can verify the builder doesn't panic
        assert_eq!(transport.transport_type(), TransportType::Tcp);
    }

    #[test]
    fn test_tcp_transport_builder_keep_alive() {
        let transport = TcpTransportBuilder::new().keep_alive(false).build();

        assert_eq!(transport.transport_type(), TransportType::Tcp);
    }

    #[test]
    fn test_tcp_transport_builder_buffer_size() {
        let transport = TcpTransportBuilder::new().buffer_size(4096).build();

        assert_eq!(transport.transport_type(), TransportType::Tcp);
    }

    #[test]
    fn test_tcp_transport_builder_chaining() {
        let bind_addr: SocketAddr = "127.0.0.1:7777".parse().unwrap();
        let remote_addr: SocketAddr = "127.0.0.1:8888".parse().unwrap();

        let transport = TcpTransportBuilder::new()
            .bind_addr(bind_addr)
            .remote_addr(remote_addr)
            .connect_timeout_ms(20000)
            .keep_alive(true)
            .buffer_size(32768)
            .build();

        // bind_addr is private, test via endpoint instead
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("127.0.0.1"));
        // remote_addr is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("8888"));
    }

    #[test]
    fn test_tcp_transport_new_server() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let transport = TcpTransport::new_server(addr);

        // bind_addr is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("8080"));
        // remote_addr is private, we test behavior instead
        assert_eq!(transport.transport_type(), TransportType::Tcp);
    }

    #[test]
    fn test_tcp_transport_new_client() {
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let remote_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();

        let transport = TcpTransport::new_client(bind_addr, remote_addr);

        // bind_addr is private, test via endpoint instead
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("127.0.0.1"));
        // remote_addr is private, but we can verify through endpoint
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("8080"));
    }

    #[test]
    fn test_tcp_transport_debug() {
        let transport = TcpTransport::new_server("127.0.0.1:8080".parse().unwrap());
        let debug_str = format!("{transport:?}");
        assert!(debug_str.contains("TcpTransport"));
        assert!(debug_str.contains("127.0.0.1:8080"));
    }

    #[tokio::test]
    async fn test_tcp_transport_state() {
        let transport = TcpTransport::new_server("127.0.0.1:8080".parse().unwrap());

        let state = transport.state().await;
        assert_eq!(state, TransportState::Disconnected);
    }

    #[test]
    fn test_tcp_transport_transport_type() {
        let transport = TcpTransport::new_server("127.0.0.1:8080".parse().unwrap());
        assert_eq!(transport.transport_type(), TransportType::Tcp);
    }

    #[test]
    fn test_tcp_transport_capabilities() {
        let transport = TcpTransport::new_server("127.0.0.1:8080".parse().unwrap());
        let caps = transport.capabilities();

        assert!(caps.supports_bidirectional);
        assert!(caps.supports_streaming);
        assert_eq!(caps.max_message_size, Some(1024 * 1024)); // 1MB per turbomcp_core::MAX_MESSAGE_SIZE
    }

    #[test]
    fn test_tcp_transport_endpoint_server() {
        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let transport = TcpTransport::new_server(addr);

        let endpoint = transport.endpoint();
        assert_eq!(endpoint, Some("tcp://127.0.0.1:8080".to_string()));
    }

    #[test]
    fn test_tcp_transport_endpoint_client() {
        let bind_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        let remote_addr: SocketAddr = "127.0.0.1:9090".parse().unwrap();
        let transport = TcpTransport::new_client(bind_addr, remote_addr);

        let endpoint = transport.endpoint();
        assert_eq!(endpoint, Some("tcp://127.0.0.1:9090".to_string()));
    }

    #[tokio::test]
    async fn test_tcp_transport_metrics() {
        let transport = TcpTransport::new_server("127.0.0.1:8080".parse().unwrap());
        let metrics = transport.metrics().await;

        assert_eq!(metrics.messages_sent, 0);
        assert_eq!(metrics.messages_received, 0);
        assert_eq!(metrics.bytes_sent, 0);
        assert_eq!(metrics.bytes_received, 0);
    }

    #[tokio::test]
    async fn test_tcp_transport_send_when_disconnected() {
        use bytes::Bytes;
        use turbomcp_core::MessageId;
        use turbomcp_transport::core::TransportMessage;

        let mut transport = TcpTransport::new_server("127.0.0.1:8080".parse().unwrap());
        let message = TransportMessage::new(
            MessageId::String("test".to_string()),
            Bytes::from("test message"),
        );

        let result = transport.send(message).await;
        assert!(result.is_err());

        if let Err(err) = result {
            let error_msg = format!("{err}");
            assert!(
                error_msg.contains("not connected")
                    || error_msg.contains("ConnectionFailed")
                    || error_msg.contains("TCP transport not connected")
                    || error_msg.contains("No active TCP connections")
            );
        }
    }

    #[tokio::test]
    async fn test_tcp_transport_receive_when_disconnected() {
        let mut transport = TcpTransport::new_server("127.0.0.1:8080".parse().unwrap());

        let result = transport.receive().await;
        assert!(result.is_err());

        if let Err(err) = result {
            let error_msg = format!("{err}");
            assert!(
                error_msg.contains("not connected")
                    || error_msg.contains("ConnectionFailed")
                    || error_msg.contains("TCP transport not connected")
                    || error_msg.contains("No active TCP connections")
            );
        }
    }

    #[tokio::test]
    async fn test_tcp_transport_disconnect() {
        let mut transport = TcpTransport::new_server("127.0.0.1:8080".parse().unwrap());

        let result = transport.disconnect().await;
        assert!(result.is_ok());

        let state = transport.state().await;
        assert_eq!(state, TransportState::Disconnected);
    }

    #[test]
    fn test_socket_addr_parsing_ipv4() {
        let addr_str = "192.168.1.1:8080";
        let addr: SocketAddr = addr_str.parse().unwrap();
        assert_eq!(addr.to_string(), addr_str);
    }

    #[test]
    fn test_socket_addr_parsing_localhost() {
        let addr: SocketAddr = "localhost:8080"
            .parse()
            .unwrap_or_else(|_| "127.0.0.1:8080".parse().unwrap());
        assert_eq!(addr.port(), 8080);
    }

    #[test]
    fn test_socket_addr_parsing_zero_port() {
        let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
        assert_eq!(addr.port(), 0);
    }

    #[test]
    fn test_socket_addr_parsing_high_port() {
        let addr: SocketAddr = "127.0.0.1:65535".parse().unwrap();
        assert_eq!(addr.port(), 65535);
    }

    #[test]
    fn test_socket_addr_from_str() {
        let addr = SocketAddr::from_str("127.0.0.1:8080").unwrap();
        assert!(addr.is_ipv4());
        assert_eq!(addr.port(), 8080);
    }

    #[test]
    fn test_invalid_socket_addr() {
        let result = "invalid:addr".parse::<SocketAddr>();
        assert!(result.is_err());
    }

    #[test]
    fn test_builder_server_mode() {
        let _transport = TcpTransportBuilder::new()
            .bind_addr("127.0.0.1:8080".parse().unwrap())
            .build();

        // Server mode when no remote_addr is set
        // remote_addr is private, we test behavior instead
    }

    #[test]
    fn test_builder_client_mode() {
        let transport = TcpTransportBuilder::new()
            .bind_addr("127.0.0.1:0".parse().unwrap())
            .remote_addr("127.0.0.1:8080".parse().unwrap())
            .build();

        // Client mode when remote_addr is set
        // remote_addr is private, test via endpoint instead
        let endpoint = transport.endpoint().unwrap();
        assert!(endpoint.contains("8080"));
    }

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

    #[test]
    fn test_transport_type_equality() {
        let tcp1 = TransportType::Tcp;
        let tcp2 = TransportType::Tcp;
        assert_eq!(tcp1, tcp2);
    }

    // Test edge cases and boundary conditions
    #[test]
    fn test_tcp_config_timeout_boundaries() {
        // Test minimum timeout
        let config1 = TcpConfig {
            connect_timeout_ms: 0,
            ..Default::default()
        };
        assert_eq!(config1.connect_timeout_ms, 0);

        // Test maximum reasonable timeout
        let config2 = TcpConfig {
            connect_timeout_ms: u64::MAX,
            ..Default::default()
        };
        assert_eq!(config2.connect_timeout_ms, u64::MAX);
    }

    #[test]
    fn test_tcp_config_buffer_size_boundaries() {
        // Test minimum buffer size
        let config1 = TcpConfig {
            buffer_size: 1,
            ..Default::default()
        };
        assert_eq!(config1.buffer_size, 1);

        // Test large buffer size
        let config2 = TcpConfig {
            buffer_size: 1024 * 1024, // 1MB
            ..Default::default()
        };
        assert_eq!(config2.buffer_size, 1024 * 1024);
    }
}

// Tests that work without the tcp feature
#[test]
fn test_tcp_module_accessible() {
    // Module accessibility validated by successful compilation
}
