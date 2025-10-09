//! Comprehensive tests for the handler registry module

use crate::handlers::HandlerMetadata;
use crate::registry::{HandlerRegistry, RegistryConfig, RegistryEvent};
use chrono::Utc;

#[test]
fn test_handler_registry_creation() {
    let registry = HandlerRegistry::new();

    assert_eq!(registry.tools.len(), 0);
    assert_eq!(registry.prompts.len(), 0);
    assert_eq!(registry.resources.len(), 0);
    assert_eq!(registry.sampling.len(), 0);
    assert_eq!(registry.logging.len(), 0);
}

#[test]
fn test_handler_registry_debug() {
    let registry = HandlerRegistry::new();
    let debug_str = format!("{registry:?}");

    assert!(debug_str.contains("HandlerRegistry"));
    assert!(debug_str.contains("tools_count"));
    assert!(debug_str.contains("prompts_count"));
    assert!(debug_str.contains("resources_count"));
    assert!(debug_str.contains("sampling_count"));
    assert!(debug_str.contains("logging_count"));
}

#[test]
fn test_registry_config_default() {
    let config = RegistryConfig::default();
    // Test default config values
    assert_eq!(config.max_handlers_per_type, 1000);
    assert!(config.enable_metrics);
    assert!(config.enable_validation);
    assert_eq!(config.handler_timeout_ms, 30_000);
    assert!(!config.enable_hot_reload);
    assert!(config.event_listeners.is_empty());
}

#[test]
fn test_registry_config_clone() {
    let config = RegistryConfig::default();
    let cloned = config.clone();

    assert_eq!(config.max_handlers_per_type, cloned.max_handlers_per_type);
    assert_eq!(config.enable_metrics, cloned.enable_metrics);
    assert_eq!(config.enable_validation, cloned.enable_validation);
    assert_eq!(config.handler_timeout_ms, cloned.handler_timeout_ms);
    assert_eq!(config.enable_hot_reload, cloned.enable_hot_reload);
    assert_eq!(config.event_listeners, cloned.event_listeners);
}

#[test]
fn test_registry_config_debug() {
    let config = RegistryConfig::default();
    let debug_str = format!("{config:?}");

    assert!(debug_str.contains("RegistryConfig"));
    assert!(debug_str.contains("max_handlers_per_type"));
    assert!(debug_str.contains("enable_metrics"));
    assert!(debug_str.contains("enable_validation"));
    assert!(debug_str.contains("handler_timeout_ms"));
    assert!(debug_str.contains("enable_hot_reload"));
    assert!(debug_str.contains("event_listeners"));
}

#[test]
fn test_registry_config_custom() {
    let config = RegistryConfig {
        max_handlers_per_type: 500,
        enable_metrics: false,
        enable_validation: false,
        handler_timeout_ms: 60_000,
        enable_hot_reload: true,
        event_listeners: vec!["listener1".to_string(), "listener2".to_string()],
    };

    assert_eq!(config.max_handlers_per_type, 500);
    assert!(!config.enable_metrics);
    assert!(!config.enable_validation);
    assert_eq!(config.handler_timeout_ms, 60_000);
    assert!(config.enable_hot_reload);
    assert_eq!(config.event_listeners.len(), 2);
    assert!(config.event_listeners.contains(&"listener1".to_string()));
    assert!(config.event_listeners.contains(&"listener2".to_string()));
}

#[test]
fn test_registry_event_handler_registered() {
    let timestamp = Utc::now();
    let event = RegistryEvent::HandlerRegistered {
        handler_type: "tool".to_string(),
        name: "test_tool".to_string(),
        timestamp,
    };

    match event {
        RegistryEvent::HandlerRegistered {
            handler_type,
            name,
            timestamp: ts,
        } => {
            assert_eq!(handler_type, "tool");
            assert_eq!(name, "test_tool");
            assert_eq!(ts, timestamp);
        }
        _ => panic!("Expected HandlerRegistered event"),
    }
}

#[test]
fn test_registry_event_handler_unregistered() {
    let timestamp = Utc::now();
    let event = RegistryEvent::HandlerUnregistered {
        handler_type: "prompt".to_string(),
        name: "test_prompt".to_string(),
        timestamp,
    };

    match event {
        RegistryEvent::HandlerUnregistered {
            handler_type,
            name,
            timestamp: ts,
        } => {
            assert_eq!(handler_type, "prompt");
            assert_eq!(name, "test_prompt");
            assert_eq!(ts, timestamp);
        }
        _ => panic!("Expected HandlerUnregistered event"),
    }
}

#[test]
fn test_registry_event_handler_updated() {
    let timestamp = Utc::now();
    let event = RegistryEvent::HandlerUpdated {
        handler_type: "resource".to_string(),
        name: "test_resource".to_string(),
        timestamp,
    };

    match event {
        RegistryEvent::HandlerUpdated {
            handler_type,
            name,
            timestamp: ts,
        } => {
            assert_eq!(handler_type, "resource");
            assert_eq!(name, "test_resource");
            assert_eq!(ts, timestamp);
        }
        _ => panic!("Expected HandlerUpdated event"),
    }
}

#[test]
fn test_registry_event_clone() {
    let timestamp = Utc::now();
    let event = RegistryEvent::HandlerRegistered {
        handler_type: "sampling".to_string(),
        name: "test_sampling".to_string(),
        timestamp,
    };

    let cloned = event.clone();

    match (event, cloned) {
        (
            RegistryEvent::HandlerRegistered {
                handler_type: ht1,
                name: n1,
                timestamp: ts1,
            },
            RegistryEvent::HandlerRegistered {
                handler_type: ht2,
                name: n2,
                timestamp: ts2,
            },
        ) => {
            assert_eq!(ht1, ht2);
            assert_eq!(n1, n2);
            assert_eq!(ts1, ts2);
        }
        _ => panic!("Event types should match after clone"),
    }
}

#[test]
fn test_registry_event_debug() {
    let timestamp = Utc::now();
    let event = RegistryEvent::HandlerRegistered {
        handler_type: "logging".to_string(),
        name: "test_logging".to_string(),
        timestamp,
    };

    let debug_str = format!("{event:?}");
    assert!(debug_str.contains("HandlerRegistered"));
    assert!(debug_str.contains("handler_type"));
    assert!(debug_str.contains("name"));
    assert!(debug_str.contains("timestamp"));
    assert!(debug_str.contains("logging"));
    assert!(debug_str.contains("test_logging"));
}

#[test]
fn test_registry_event_types() {
    let timestamp = Utc::now();

    let events = [
        RegistryEvent::HandlerRegistered {
            handler_type: "tool".to_string(),
            name: "tool1".to_string(),
            timestamp,
        },
        RegistryEvent::HandlerUnregistered {
            handler_type: "prompt".to_string(),
            name: "prompt1".to_string(),
            timestamp,
        },
        RegistryEvent::HandlerUpdated {
            handler_type: "resource".to_string(),
            name: "resource1".to_string(),
            timestamp,
        },
    ];

    assert_eq!(events.len(), 3);

    // Test each event type
    match &events[0] {
        RegistryEvent::HandlerRegistered { handler_type, .. } => {
            assert_eq!(handler_type, "tool");
        }
        _ => panic!("Expected HandlerRegistered"),
    }

    match &events[1] {
        RegistryEvent::HandlerUnregistered { handler_type, .. } => {
            assert_eq!(handler_type, "prompt");
        }
        _ => panic!("Expected HandlerUnregistered"),
    }

    match &events[2] {
        RegistryEvent::HandlerUpdated { handler_type, .. } => {
            assert_eq!(handler_type, "resource");
        }
        _ => panic!("Expected HandlerUpdated"),
    }
}

#[test]
fn test_handler_metadata_creation() {
    let metadata = HandlerMetadata {
        name: "test_handler".to_string(),
        description: Some("A test handler".to_string()),
        version: "1.0.0".to_string(),
        allowed_roles: Some(vec!["admin".to_string(), "user".to_string()]),
        tags: Vec::new(),
        created_at: chrono::Utc::now(),
        config: std::collections::HashMap::new(),
        metrics_enabled: true,
        rate_limit: None,
    };

    assert_eq!(metadata.name, "test_handler");
    assert_eq!(metadata.description, Some("A test handler".to_string()));
    assert_eq!(metadata.version, "1.0.0");
    assert!(metadata.allowed_roles.is_some());
    if let Some(roles) = &metadata.allowed_roles {
        assert_eq!(roles.len(), 2);
        assert!(roles.contains(&"admin".to_string()));
        assert!(roles.contains(&"user".to_string()));
    }
}

#[test]
fn test_handler_metadata_default() {
    let metadata = HandlerMetadata::default();

    assert_eq!(metadata.name, "unnamed");
    assert_eq!(metadata.description, None);
    assert_eq!(metadata.version, "1.0.0");
    assert_eq!(metadata.allowed_roles, None);
    assert!(metadata.tags.is_empty());
    assert!(metadata.config.is_empty());
    assert!(metadata.metrics_enabled);
    assert!(metadata.rate_limit.is_none());
}

#[test]
fn test_handler_metadata_clone() {
    let metadata = HandlerMetadata {
        name: "clone_test".to_string(),
        description: Some("Test cloning".to_string()),
        version: "2.0.0".to_string(),
        allowed_roles: Some(vec!["tester".to_string()]),
        tags: vec!["test".to_string()],
        created_at: chrono::Utc::now(),
        config: std::collections::HashMap::new(),
        metrics_enabled: false,
        rate_limit: Some(10),
    };

    let cloned = metadata.clone();

    assert_eq!(metadata.name, cloned.name);
    assert_eq!(metadata.description, cloned.description);
    assert_eq!(metadata.version, cloned.version);
    assert_eq!(metadata.allowed_roles, cloned.allowed_roles);
    assert_eq!(metadata.tags, cloned.tags);
    assert_eq!(metadata.created_at, cloned.created_at);
    assert_eq!(metadata.config, cloned.config);
    assert_eq!(metadata.metrics_enabled, cloned.metrics_enabled);
    assert_eq!(metadata.rate_limit, cloned.rate_limit);
}

#[test]
fn test_handler_metadata_debug() {
    let metadata = HandlerMetadata {
        name: "debug_test".to_string(),
        description: Some("Debug test handler".to_string()),
        version: "3.0.0".to_string(),
        allowed_roles: Some(vec!["developer".to_string()]),
        tags: vec!["debug".to_string(), "testing".to_string()],
        created_at: chrono::Utc::now(),
        config: std::collections::HashMap::new(),
        metrics_enabled: true,
        rate_limit: None,
    };

    let debug_str = format!("{metadata:?}");

    assert!(debug_str.contains("HandlerMetadata"));
    assert!(debug_str.contains("debug_test"));
    assert!(debug_str.contains("Debug test handler"));
    assert!(debug_str.contains("3.0.0"));
    assert!(debug_str.contains("developer"));
}

#[test]
fn test_registry_collections_empty() {
    let registry = HandlerRegistry::new();

    // Test all collections are empty on creation
    assert!(registry.tools.is_empty());
    assert!(registry.prompts.is_empty());
    assert!(registry.resources.is_empty());
    assert!(registry.sampling.is_empty());
    assert!(registry.logging.is_empty());
}

#[test]
fn test_registry_collections_thread_safety() {
    use std::sync::Arc;
    use std::thread;

    let registry = Arc::new(HandlerRegistry::new());
    let mut handles = vec![];

    // Spawn threads that access collections concurrently
    for i in 0..5 {
        let registry_clone = Arc::clone(&registry);
        let handle = thread::spawn(move || {
            // Read operations should be safe
            let _tools_len = registry_clone.tools.len();
            let _prompts_len = registry_clone.prompts.len();
            let _resources_len = registry_clone.resources.len();
            let _sampling_len = registry_clone.sampling.len();
            let _logging_len = registry_clone.logging.len();
            i
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        let result = handle.join().unwrap();
        assert!(result < 5);
    }
}

#[test]
fn test_registry_config_boundaries() {
    let config = RegistryConfig {
        max_handlers_per_type: 0,
        enable_metrics: true,
        enable_validation: true,
        handler_timeout_ms: 0,
        enable_hot_reload: false,
        event_listeners: vec![],
    };

    // Test boundary values
    assert_eq!(config.max_handlers_per_type, 0);
    assert_eq!(config.handler_timeout_ms, 0);

    let high_config = RegistryConfig {
        max_handlers_per_type: usize::MAX,
        enable_metrics: true,
        enable_validation: true,
        handler_timeout_ms: u64::MAX,
        enable_hot_reload: true,
        event_listeners: vec!["listener".to_string(); 100],
    };

    assert_eq!(high_config.max_handlers_per_type, usize::MAX);
    assert_eq!(high_config.handler_timeout_ms, u64::MAX);
    assert_eq!(high_config.event_listeners.len(), 100);
}

#[test]
fn test_registry_event_timestamp_ordering() {
    let now = Utc::now();
    let earlier = now - chrono::Duration::seconds(10);
    let later = now + chrono::Duration::seconds(10);

    let events = [
        RegistryEvent::HandlerRegistered {
            handler_type: "tool".to_string(),
            name: "tool1".to_string(),
            timestamp: earlier,
        },
        RegistryEvent::HandlerUpdated {
            handler_type: "tool".to_string(),
            name: "tool1".to_string(),
            timestamp: now,
        },
        RegistryEvent::HandlerUnregistered {
            handler_type: "tool".to_string(),
            name: "tool1".to_string(),
            timestamp: later,
        },
    ];

    // Verify timestamp ordering
    match (&events[0], &events[1], &events[2]) {
        (
            RegistryEvent::HandlerRegistered { timestamp: ts1, .. },
            RegistryEvent::HandlerUpdated { timestamp: ts2, .. },
            RegistryEvent::HandlerUnregistered { timestamp: ts3, .. },
        ) => {
            assert!(ts1 < ts2);
            assert!(ts2 < ts3);
        }
        _ => panic!("Event types don't match expected pattern"),
    }
}

#[test]
fn test_handler_metadata_optional_fields() {
    let minimal_metadata = HandlerMetadata {
        name: "minimal".to_string(),
        description: None,
        version: "1.0.0".to_string(),
        allowed_roles: None,
        tags: Vec::new(),
        created_at: chrono::Utc::now(),
        config: std::collections::HashMap::new(),
        metrics_enabled: true,
        rate_limit: None,
    };

    let full_metadata = HandlerMetadata {
        name: "full".to_string(),
        description: Some("Full metadata".to_string()),
        version: "2.0.0".to_string(),
        allowed_roles: Some(vec![
            "admin".to_string(),
            "user".to_string(),
            "guest".to_string(),
        ]),
        tags: vec!["full".to_string(), "complete".to_string()],
        created_at: chrono::Utc::now(),
        config: {
            let mut c = std::collections::HashMap::new();
            c.insert("key".to_string(), serde_json::json!("value"));
            c
        },
        metrics_enabled: false,
        rate_limit: Some(100),
    };

    // Test minimal metadata
    assert!(minimal_metadata.description.is_none());
    assert!(minimal_metadata.allowed_roles.is_none());

    // Test full metadata
    assert!(full_metadata.description.is_some());
    assert!(full_metadata.allowed_roles.is_some());
    if let Some(roles) = &full_metadata.allowed_roles {
        assert_eq!(roles.len(), 3);
    }
}

#[test]
fn test_registry_debug_with_empty_collections() {
    let registry = HandlerRegistry::new();
    let debug_output = format!("{registry:?}");

    // Should show zero counts for all collections
    assert!(debug_output.contains("tools_count: 0"));
    assert!(debug_output.contains("prompts_count: 0"));
    assert!(debug_output.contains("resources_count: 0"));
    assert!(debug_output.contains("sampling_count: 0"));
    assert!(debug_output.contains("logging_count: 0"));
}

#[test]
fn test_registry_config_event_listeners_management() {
    let mut config = RegistryConfig::default();
    assert!(config.event_listeners.is_empty());

    // Add some listeners
    config.event_listeners.push("logger".to_string());
    config.event_listeners.push("metrics".to_string());
    config.event_listeners.push("auditor".to_string());

    assert_eq!(config.event_listeners.len(), 3);
    assert!(config.event_listeners.contains(&"logger".to_string()));
    assert!(config.event_listeners.contains(&"metrics".to_string()));
    assert!(config.event_listeners.contains(&"auditor".to_string()));

    // Remove a listener
    config.event_listeners.retain(|l| l != "metrics");
    assert_eq!(config.event_listeners.len(), 2);
    assert!(!config.event_listeners.contains(&"metrics".to_string()));
}

#[test]
fn test_handler_metadata_version_patterns() {
    let version_patterns = vec![
        "1.0.0",
        "2.1.3",
        "0.0.1",
        "10.20.30",
        "1.0.0-alpha",
        "2.0.0-beta.1",
        "3.0.0-rc.1+build.1",
        "v1.0.0",
        "1.0",
        "1",
    ];

    for version in version_patterns {
        let metadata = HandlerMetadata {
            name: "version_test".to_string(),
            description: None,
            version: version.to_string(),
            allowed_roles: None,
            tags: Vec::new(),
            created_at: chrono::Utc::now(),
            config: std::collections::HashMap::new(),
            metrics_enabled: true,
            rate_limit: None,
        };

        assert_eq!(metadata.version, version);
        assert!(!metadata.version.is_empty());
    }
}

#[test]
fn test_registry_creation_consistency() {
    // Create multiple registries and ensure they're all initialized consistently
    let registries: Vec<HandlerRegistry> = (0..10).map(|_| HandlerRegistry::new()).collect();

    for registry in &registries {
        assert_eq!(registry.tools.len(), 0);
        assert_eq!(registry.prompts.len(), 0);
        assert_eq!(registry.resources.len(), 0);
        assert_eq!(registry.sampling.len(), 0);
        assert_eq!(registry.logging.len(), 0);
    }
}
