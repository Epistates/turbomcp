//! Comprehensive tests for the StateManager

use serde_json::json;
use turbomcp_protocol::StateManager;

#[test]
fn test_key_value_store() {
    let state = StateManager::new();

    // Test set and get
    state.set("key1".to_string(), json!("value1"));
    assert_eq!(state.get("key1"), Some(json!("value1")));

    // Test overwrite
    state.set("key1".to_string(), json!("value2"));
    assert_eq!(state.get("key1"), Some(json!("value2")));

    // Test complex values
    let complex_value = json!({
        "name": "test",
        "count": 42,
        "nested": {
            "field": "value"
        }
    });
    state.set("complex".to_string(), complex_value.clone());
    assert_eq!(state.get("complex"), Some(complex_value));

    // Test non-existent key
    assert_eq!(state.get("nonexistent"), None);
}

#[test]
fn test_remove() {
    let state = StateManager::new();

    state.set("key1".to_string(), json!("value1"));
    state.set("key2".to_string(), json!("value2"));

    // Remove existing key
    let removed = state.remove("key1");
    assert_eq!(removed, Some(json!("value1")));
    assert_eq!(state.get("key1"), None);

    // Remove non-existent key
    let removed = state.remove("nonexistent");
    assert_eq!(removed, None);

    // Verify key2 still exists
    assert_eq!(state.get("key2"), Some(json!("value2")));
}

#[test]
fn test_contains() {
    let state = StateManager::new();

    state.set("key1".to_string(), json!("value1"));

    assert!(state.contains("key1"));
    assert!(!state.contains("key2"));
}

#[test]
fn test_list_keys() {
    let state = StateManager::new();

    state.set("key1".to_string(), json!("value1"));
    state.set("key2".to_string(), json!("value2"));
    state.set("key3".to_string(), json!("value3"));

    let mut keys = state.list_keys();
    keys.sort();

    assert_eq!(keys, vec!["key1", "key2", "key3"]);
}

#[test]
fn test_clear() {
    let state = StateManager::new();

    state.set("key1".to_string(), json!("value1"));
    state.set("key2".to_string(), json!("value2"));

    assert_eq!(state.size(), 2);

    state.clear();

    assert_eq!(state.size(), 0);
    assert!(!state.contains("key1"));
    assert!(!state.contains("key2"));
}

#[test]
fn test_export_import() {
    let state1 = StateManager::new();

    // Populate state
    state1.set("key1".to_string(), json!("value1"));
    state1.set("key2".to_string(), json!({"nested": "value"}));

    // Export
    let exported = state1.export();

    // Create new state and import
    let state2 = StateManager::new();
    let import_result = state2.import(exported);
    assert!(import_result.is_ok());

    // Verify imported data
    assert_eq!(state2.get("key1"), Some(json!("value1")));
    assert_eq!(state2.get("key2"), Some(json!({"nested": "value"})));
}

#[test]
fn test_import_invalid_data() {
    let state = StateManager::new();

    // Try importing non-object data
    let result = state.import(json!("not an object"));
    assert!(result.is_err());

    // Try importing empty object (should succeed)
    let result = state.import(json!({}));
    assert!(result.is_ok());
}

#[test]
fn test_concurrent_access() {
    use std::sync::Arc;
    use std::thread;

    let state = Arc::new(StateManager::new());
    let mut handles = vec![];

    // Spawn multiple threads that modify the state
    for i in 0..10 {
        let state_clone = Arc::clone(&state);
        let handle = thread::spawn(move || {
            let key = format!("key{}", i);
            state_clone.set(key.clone(), json!(i));
        });
        handles.push(handle);
    }

    // Wait for all threads to complete
    for handle in handles {
        handle.join().unwrap();
    }

    // Verify results
    assert_eq!(state.size(), 10);
}

#[test]
fn test_default_trait() {
    let state = StateManager::default();
    assert_eq!(state.size(), 0);
}

#[test]
fn test_state_persistence() {
    let state = StateManager::new();

    // Add various data types
    state.set("string".to_string(), json!("test"));
    state.set("number".to_string(), json!(42));
    state.set("bool".to_string(), json!(true));
    state.set("array".to_string(), json!([1, 2, 3]));
    state.set("object".to_string(), json!({"key": "value"}));

    assert_eq!(state.size(), 5);

    // Verify all types are stored correctly
    assert_eq!(state.get("string"), Some(json!("test")));
    assert_eq!(state.get("number"), Some(json!(42)));
    assert_eq!(state.get("bool"), Some(json!(true)));
    assert_eq!(state.get("array"), Some(json!([1, 2, 3])));
    assert_eq!(state.get("object"), Some(json!({"key": "value"})));
}

#[test]
fn test_state_memory_safety() {
    let state = StateManager::new();

    // Test with large data to ensure memory safety
    let large_string = "x".repeat(10000);
    state.set("large".to_string(), json!(large_string));

    // Verify it's stored correctly
    if let Some(value) = state.get("large") {
        if let Some(s) = value.as_str() {
            assert_eq!(s.len(), 10000);
        }
    }

    // Clear should free memory
    state.clear();
    assert_eq!(state.size(), 0);
}
