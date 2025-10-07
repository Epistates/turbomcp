//! Property-based tests for TurboMCP

use serde_json::json;
use turbomcp_protocol::{MessageId, StateManager};

// Note: Property-based testing requires proptest crate to be added to Cargo.toml
// For now, we'll use regular randomized tests until proptest is added

#[cfg(test)]
mod property_tests {
    use super::*;

    #[test]
    fn test_state_manager_consistency() {
        let state = StateManager::new();

        // Test setting and getting values
        let key = "test_key";
        let value = json!("test_value");

        state.set(key.to_string(), value.clone());
        assert_eq!(state.get(key), Some(value));
    }

    #[test]
    fn test_state_manager_removal() {
        let state = StateManager::new();
        let key = "test_key";
        let value = json!(42);

        state.set(key.to_string(), value);
        assert!(state.contains(key));

        let removed = state.remove(key);
        assert_eq!(removed, Some(json!(42)));
        assert!(!state.contains(key));

        // Second removal should return None
        assert_eq!(state.remove(key), None);
    }

    #[test]
    fn test_state_manager_clear() {
        let state = StateManager::new();

        // Add multiple entries
        for i in 0..10 {
            state.set(format!("key{}", i), json!(i));
        }

        assert_eq!(state.size(), 10);

        state.clear();
        assert_eq!(state.size(), 0);

        // Verify all keys are gone
        for i in 0..10 {
            assert!(!state.contains(&format!("key{}", i)));
        }
    }

    #[test]
    fn test_message_id_generation() {
        use std::collections::HashSet;

        let mut ids = HashSet::new();

        // Generate multiple message IDs and ensure they're unique
        for _ in 0..1000 {
            let id = MessageId::from(fastrand::i64(..) as i64);
            assert!(ids.insert(id), "Duplicate message ID generated");
        }
    }

    #[test]
    fn test_concurrent_state_access() {
        use std::sync::Arc;
        use std::thread;

        let state = Arc::new(StateManager::new());
        let mut handles = vec![];

        // Spawn multiple threads that access state concurrently
        for thread_id in 0..10 {
            let state_clone = Arc::clone(&state);
            let handle = thread::spawn(move || {
                for i in 0..100 {
                    let key = format!("thread{}_key{}", thread_id, i);
                    let value = json!(format!("value_{}", i));

                    state_clone.set(key.clone(), value.clone());
                    assert_eq!(state_clone.get(&key), Some(value));
                }
            });
            handles.push(handle);
        }

        // Wait for all threads to complete
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify final state size
        assert_eq!(state.size(), 1000); // 10 threads * 100 keys each
    }

    #[test]
    fn test_export_import_roundtrip() {
        let state1 = StateManager::new();

        // Populate with test data
        let test_data = vec![
            ("key1", json!("string_value")),
            ("key2", json!(42)),
            ("key3", json!(true)),
            ("key4", json!({"nested": "object"})),
            ("key5", json!([1, 2, 3, 4, 5])),
        ];

        for (key, value) in &test_data {
            state1.set(key.to_string(), value.clone());
        }

        // Export and import to new state
        let exported = state1.export();
        let state2 = StateManager::new();
        state2.import(exported).expect("Import should succeed");

        // Verify all data was preserved
        for (key, expected_value) in &test_data {
            assert_eq!(state2.get(key), Some(expected_value.clone()));
        }

        assert_eq!(state1.size(), state2.size());
    }
}

// Performance and stress tests
#[cfg(test)]
mod stress_tests {
    use super::*;

    #[test]
    fn test_large_state_operations() {
        let state = StateManager::new();

        // Insert a large number of entries
        let num_entries = 10_000;
        for i in 0..num_entries {
            state.set(format!("large_key_{}", i), json!(i));
        }

        assert_eq!(state.size(), num_entries);

        // Verify random access
        for i in (0..num_entries).step_by(100) {
            assert_eq!(state.get(&format!("large_key_{}", i)), Some(json!(i)));
        }

        // Remove half the entries
        for i in (0..num_entries).step_by(2) {
            state.remove(&format!("large_key_{}", i));
        }

        assert_eq!(state.size(), num_entries / 2);
    }

    #[test]
    fn test_memory_efficiency() {
        let state = StateManager::new();

        // Add and remove entries to test memory cleanup
        for cycle in 0..100 {
            // Add entries
            for i in 0..100 {
                state.set(
                    format!("cycle_{}_key_{}", cycle, i),
                    json!(format!("value_{}", i)),
                );
            }

            // Remove all entries from this cycle
            for i in 0..100 {
                state.remove(&format!("cycle_{}_key_{}", cycle, i));
            }
        }

        assert_eq!(state.size(), 0);
    }
}
