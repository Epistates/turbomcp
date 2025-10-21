/// Integration tests for configurable concurrency limits
///
/// These tests verify that the client's max_concurrent_handlers configuration
/// is properly initialized and can be customized.
use turbomcp_client::ClientCapabilities;

#[test]
fn test_client_capabilities_default_concurrency() {
    // Default should be 100
    let caps = ClientCapabilities::default();
    assert_eq!(caps.max_concurrent_handlers, 100);
}

#[test]
fn test_client_capabilities_all_includes_concurrency() {
    let caps = ClientCapabilities::all();
    assert_eq!(caps.max_concurrent_handlers, 100);
}

#[test]
fn test_client_capabilities_core_includes_concurrency() {
    let caps = ClientCapabilities::core();
    assert_eq!(caps.max_concurrent_handlers, 100);
}

#[test]
fn test_client_capabilities_minimal_includes_concurrency() {
    let caps = ClientCapabilities::minimal();
    assert_eq!(caps.max_concurrent_handlers, 100);
}

#[test]
fn test_client_capabilities_only_resources_includes_concurrency() {
    let caps = ClientCapabilities::only_resources();
    assert_eq!(caps.max_concurrent_handlers, 100);
}

#[test]
fn test_client_capabilities_only_prompts_includes_concurrency() {
    let caps = ClientCapabilities::only_prompts();
    assert_eq!(caps.max_concurrent_handlers, 100);
}

#[test]
fn test_client_capabilities_only_sampling_includes_concurrency() {
    let caps = ClientCapabilities::only_sampling();
    assert_eq!(caps.max_concurrent_handlers, 100);
}

#[test]
fn test_client_capabilities_custom_concurrency_low() {
    let caps = ClientCapabilities {
        tools: true,
        prompts: false,
        resources: false,
        sampling: false,
        max_concurrent_handlers: 50,
    };
    assert_eq!(caps.max_concurrent_handlers, 50);
}

#[test]
fn test_client_capabilities_custom_concurrency_standard() {
    let caps = ClientCapabilities {
        tools: true,
        prompts: false,
        resources: false,
        sampling: false,
        max_concurrent_handlers: 100,
    };
    assert_eq!(caps.max_concurrent_handlers, 100);
}

#[test]
fn test_client_capabilities_custom_concurrency_high() {
    let caps = ClientCapabilities {
        tools: true,
        prompts: true,
        resources: true,
        sampling: false,
        max_concurrent_handlers: 500,
    };
    assert_eq!(caps.max_concurrent_handlers, 500);
}

#[test]
fn test_client_capabilities_custom_concurrency_max() {
    let caps = ClientCapabilities {
        tools: true,
        prompts: true,
        resources: true,
        sampling: true,
        max_concurrent_handlers: 1000,
    };
    assert_eq!(caps.max_concurrent_handlers, 1000);
}

#[test]
fn test_client_capabilities_clone() {
    let caps = ClientCapabilities {
        tools: true,
        prompts: false,
        resources: true,
        sampling: false,
        max_concurrent_handlers: 200,
    };

    let cloned = caps.clone();
    assert_eq!(cloned.max_concurrent_handlers, 200);
    assert!(cloned.tools);
    assert!(!cloned.prompts);
    assert!(cloned.resources);
    assert!(!cloned.sampling);
}
