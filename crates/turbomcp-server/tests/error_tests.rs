//! Tests for server error types and handling

use turbomcp_server::ServerError;
use turbomcp_server::error::*;

#[test]
fn test_server_error_display() {
    let error = ServerError::handler("test error");
    assert!(format!("{error}").contains("Handler error: test error"));

    let error = ServerError::configuration("config error");
    assert!(format!("{error}").contains("Configuration error: config error"));

    let error = ServerError::authentication("auth failed");
    assert!(format!("{error}").contains("Authentication error: auth failed"));

    let error = ServerError::authorization("access denied");
    assert!(format!("{error}").contains("Authorization error: access denied"));

    let error = ServerError::rate_limit("too many requests");
    assert!(format!("{error}").contains("Rate limit exceeded: too many requests"));

    let error = ServerError::Lifecycle("startup failed".to_string());
    assert!(format!("{error}").contains("Lifecycle error: startup failed"));

    let error = ServerError::Shutdown("shutdown failed".to_string());
    assert!(format!("{error}").contains("Shutdown error: shutdown failed"));

    let error = ServerError::middleware("auth", "failed");
    assert!(format!("{error}").contains("Middleware error: auth: failed"));

    let error = ServerError::Registry("registry error".to_string());
    assert!(format!("{error}").contains("Registry error: registry error"));

    let error = ServerError::routing("route not found");
    assert!(format!("{error}").contains("Routing error: route not found"));

    let error = ServerError::not_found("resource");
    assert!(format!("{error}").contains("Resource not found: resource"));

    let error = ServerError::Internal("internal error".to_string());
    assert!(format!("{error}").contains("Internal server error: internal error"));

    let error = ServerError::timeout("operation", 5000);
    assert!(format!("{error}").contains("Timeout error: operation timed out after 5000ms"));

    let error = ServerError::resource_exhausted("memory");
    assert!(format!("{error}").contains("Resource exhausted: memory"));
}

#[test]
fn test_server_error_constructors() {
    // Test basic constructors
    let error = ServerError::handler("handler failed");
    if let ServerError::Handler { message, context } = error {
        assert_eq!(message, "handler failed");
        assert!(context.is_none());
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::handler_with_context("handler failed", "context info");
    if let ServerError::Handler { message, context } = error {
        assert_eq!(message, "handler failed");
        assert_eq!(context, Some("context info".to_string()));
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::configuration("invalid config");
    if let ServerError::Configuration { message, key } = error {
        assert_eq!(message, "invalid config");
        assert!(key.is_none());
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::configuration_with_key("invalid config", "timeout");
    if let ServerError::Configuration { message, key } = error {
        assert_eq!(message, "invalid config");
        assert_eq!(key, Some("timeout".to_string()));
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::authentication("auth failed");
    if let ServerError::Authentication { message, method } = error {
        assert_eq!(message, "auth failed");
        assert!(method.is_none());
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::authentication_with_method("auth failed", "bearer");
    if let ServerError::Authentication { message, method } = error {
        assert_eq!(message, "auth failed");
        assert_eq!(method, Some("bearer".to_string()));
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::authorization("access denied");
    if let ServerError::Authorization { message, resource } = error {
        assert_eq!(message, "access denied");
        assert!(resource.is_none());
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::authorization_with_resource("access denied", "tools");
    if let ServerError::Authorization { message, resource } = error {
        assert_eq!(message, "access denied");
        assert_eq!(resource, Some("tools".to_string()));
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::rate_limit("too many requests");
    if let ServerError::RateLimit {
        message,
        retry_after,
    } = error
    {
        assert_eq!(message, "too many requests");
        assert!(retry_after.is_none());
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::rate_limit_with_retry("too many requests", 60);
    if let ServerError::RateLimit {
        message,
        retry_after,
    } = error
    {
        assert_eq!(message, "too many requests");
        assert_eq!(retry_after, Some(60));
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::middleware("auth", "failed");
    if let ServerError::Middleware { name, message } = error {
        assert_eq!(name, "auth");
        assert_eq!(message, "failed");
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::routing("route not found");
    if let ServerError::Routing { message, method } = error {
        assert_eq!(message, "route not found");
        assert!(method.is_none());
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::routing_with_method("route not found", "POST");
    if let ServerError::Routing { message, method } = error {
        assert_eq!(message, "route not found");
        assert_eq!(method, Some("POST".to_string()));
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::not_found("resource");
    if let ServerError::NotFound { resource } = error {
        assert_eq!(resource, "resource");
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::timeout("operation", 5000);
    if let ServerError::Timeout {
        operation,
        timeout_ms,
    } = error
    {
        assert_eq!(operation, "operation");
        assert_eq!(timeout_ms, 5000);
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::resource_exhausted("memory");
    if let ServerError::ResourceExhausted {
        resource,
        current,
        max,
    } = error
    {
        assert_eq!(resource, "memory");
        assert!(current.is_none());
        assert!(max.is_none());
    } else {
        panic!("Wrong error variant");
    }

    let error = ServerError::resource_exhausted_with_usage("memory", 100, 200);
    if let ServerError::ResourceExhausted {
        resource,
        current,
        max,
    } = error
    {
        assert_eq!(resource, "memory");
        assert_eq!(current, Some(100));
        assert_eq!(max, Some(200));
    } else {
        panic!("Wrong error variant");
    }
}

#[test]
fn test_server_error_retryable() {
    assert!(ServerError::timeout("op", 1000).is_retryable());
    assert!(ServerError::resource_exhausted("memory").is_retryable());
    assert!(ServerError::rate_limit("too many").is_retryable());

    assert!(!ServerError::handler("failed").is_retryable());
    assert!(!ServerError::authentication("failed").is_retryable());
    assert!(!ServerError::authorization("denied").is_retryable());
    assert!(!ServerError::not_found("resource").is_retryable());
    assert!(!ServerError::Internal("error".to_string()).is_retryable());
}

#[test]
fn test_server_error_fatal() {
    assert!(ServerError::Lifecycle("failed".to_string()).is_fatal());
    assert!(ServerError::Shutdown("failed".to_string()).is_fatal());
    assert!(ServerError::Internal("error".to_string()).is_fatal());

    assert!(!ServerError::handler("failed").is_fatal());
    assert!(!ServerError::timeout("op", 1000).is_fatal());
    assert!(!ServerError::rate_limit("too many").is_fatal());
    assert!(!ServerError::not_found("resource").is_fatal());
}

#[test]
fn test_server_error_codes() {
    assert_eq!(ServerError::not_found("resource").error_code(), -32004);
    assert_eq!(ServerError::authentication("failed").error_code(), -32008);
    assert_eq!(ServerError::authorization("denied").error_code(), -32005);
    assert_eq!(ServerError::rate_limit("too many").error_code(), -32009);
    assert_eq!(
        ServerError::resource_exhausted("memory").error_code(),
        -32010
    );
    assert_eq!(ServerError::timeout("op", 1000).error_code(), -32603);
    assert_eq!(ServerError::handler("failed").error_code(), -32002);

    // Default error code for other variants
    assert_eq!(
        ServerError::Internal("error".to_string()).error_code(),
        -32603
    );
    assert_eq!(
        ServerError::Lifecycle("error".to_string()).error_code(),
        -32603
    );
    assert_eq!(ServerError::middleware("m", "error").error_code(), -32603);
}

#[test]
fn test_error_recovery_enum() {
    let recovery = ErrorRecovery::Retry;
    assert_eq!(recovery, ErrorRecovery::Retry);
    assert_ne!(recovery, ErrorRecovery::Skip);

    let recovery = ErrorRecovery::Skip;
    assert_eq!(recovery, ErrorRecovery::Skip);

    let recovery = ErrorRecovery::Fail;
    assert_eq!(recovery, ErrorRecovery::Fail);

    let recovery = ErrorRecovery::Degrade;
    assert_eq!(recovery, ErrorRecovery::Degrade);

    // Test Debug formatting
    assert_eq!(format!("{:?}", ErrorRecovery::Retry), "Retry");
    assert_eq!(format!("{:?}", ErrorRecovery::Skip), "Skip");
    assert_eq!(format!("{:?}", ErrorRecovery::Fail), "Fail");
    assert_eq!(format!("{:?}", ErrorRecovery::Degrade), "Degrade");
}

#[test]
fn test_error_context_creation() {
    let context = ErrorContext::new("handler", "execute");
    assert_eq!(context.category, "handler");
    assert_eq!(context.operation, "execute");
    assert!(context.request_id.is_none());
    assert!(context.client_id.is_none());
    assert!(context.metadata.is_empty());
}

#[test]
fn test_error_context_builder_pattern() {
    let context = ErrorContext::new("handler", "execute")
        .with_request_id("req-123")
        .with_client_id("client-456")
        .with_metadata("key1", "value1")
        .with_metadata("key2", "value2");

    assert_eq!(context.category, "handler");
    assert_eq!(context.operation, "execute");
    assert_eq!(context.request_id, Some("req-123".to_string()));
    assert_eq!(context.client_id, Some("client-456".to_string()));
    assert_eq!(context.metadata.len(), 2);
    assert_eq!(context.metadata.get("key1"), Some(&"value1".to_string()));
    assert_eq!(context.metadata.get("key2"), Some(&"value2".to_string()));
}

#[test]
fn test_error_context_debug_formatting() {
    let context = ErrorContext::new("test", "operation");
    let debug_str = format!("{context:?}");
    assert!(debug_str.contains("ErrorContext"));
    assert!(debug_str.contains("category: \"test\""));
    assert!(debug_str.contains("operation: \"operation\""));
}

#[test]
fn test_error_context_clone() {
    let original = ErrorContext::new("test", "op")
        .with_request_id("123")
        .with_metadata("key", "value");

    let cloned = original.clone();
    assert_eq!(original.category, cloned.category);
    assert_eq!(original.operation, cloned.operation);
    assert_eq!(original.request_id, cloned.request_id);
    assert_eq!(original.metadata, cloned.metadata);
}

#[test]
fn test_server_error_from_io_error() {
    let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
    let server_error: ServerError = io_error.into();

    if let ServerError::Io(inner) = server_error {
        assert_eq!(inner.kind(), std::io::ErrorKind::NotFound);
        assert_eq!(inner.to_string(), "file not found");
    } else {
        panic!("Expected Io error variant");
    }
}

#[test]
fn test_server_error_from_serde_error() {
    let json_error = serde_json::from_str::<i32>("invalid json").unwrap_err();
    let server_error: ServerError = json_error.into();

    match server_error {
        ServerError::Serialization(_) => {
            // Success - the error was converted correctly
        }
        _ => panic!("Expected Serialization error variant"),
    }
}

#[test]
fn test_server_result_type() {
    fn returns_ok() -> ServerResult<i32> {
        Ok(42)
    }

    fn returns_error() -> ServerResult<i32> {
        Err(ServerError::handler("test error"))
    }

    assert!(returns_ok().is_ok());
    assert_eq!(returns_ok().unwrap(), 42);

    assert!(returns_error().is_err());
    let error = returns_error().unwrap_err();
    assert!(format!("{error}").contains("Handler error"));
}

#[test]
fn test_all_error_variants_coverage() {
    // This test ensures we cover all error variants for completeness

    // Core error (would need actual registry error)
    // let core_err = ServerError::Core(turbomcp_protocol::registry::RegistryError::...);

    // Transport error (would need actual transport error)
    // let transport_err = ServerError::Transport(...);

    let handler_err = ServerError::Handler {
        message: "test".to_string(),
        context: Some("context".to_string()),
    };
    assert!(format!("{handler_err}").contains("Handler error"));

    let config_err = ServerError::Configuration {
        message: "test".to_string(),
        key: Some("key".to_string()),
    };
    assert!(format!("{config_err}").contains("Configuration error"));

    let auth_err = ServerError::Authentication {
        message: "test".to_string(),
        method: Some("bearer".to_string()),
    };
    assert!(format!("{auth_err}").contains("Authentication error"));

    let authz_err = ServerError::Authorization {
        message: "test".to_string(),
        resource: Some("tools".to_string()),
    };
    assert!(format!("{authz_err}").contains("Authorization error"));

    let rate_err = ServerError::RateLimit {
        message: "test".to_string(),
        retry_after: Some(60),
    };
    assert!(format!("{rate_err}").contains("Rate limit exceeded"));

    let lifecycle_err = ServerError::Lifecycle("test".to_string());
    assert!(format!("{lifecycle_err}").contains("Lifecycle error"));

    let shutdown_err = ServerError::Shutdown("test".to_string());
    assert!(format!("{shutdown_err}").contains("Shutdown error"));

    let middleware_err = ServerError::Middleware {
        name: "auth".to_string(),
        message: "failed".to_string(),
    };
    assert!(format!("{middleware_err}").contains("Middleware error"));

    let registry_err = ServerError::Registry("test".to_string());
    assert!(format!("{registry_err}").contains("Registry error"));

    let routing_err = ServerError::Routing {
        message: "test".to_string(),
        method: Some("POST".to_string()),
    };
    assert!(format!("{routing_err}").contains("Routing error"));

    let not_found_err = ServerError::NotFound {
        resource: "test".to_string(),
    };
    assert!(format!("{not_found_err}").contains("Resource not found"));

    let internal_err = ServerError::Internal("test".to_string());
    assert!(format!("{internal_err}").contains("Internal server error"));

    let timeout_err = ServerError::Timeout {
        operation: "test".to_string(),
        timeout_ms: 5000,
    };
    assert!(format!("{timeout_err}").contains("Timeout error"));

    let exhausted_err = ServerError::ResourceExhausted {
        resource: "memory".to_string(),
        current: Some(100),
        max: Some(200),
    };
    assert!(format!("{exhausted_err}").contains("Resource exhausted"));
}
