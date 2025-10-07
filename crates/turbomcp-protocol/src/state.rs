//! State management utilities for MCP servers

use crate::{Error, ErrorKind, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Thread-safe state manager for MCP servers
#[derive(Debug, Clone)]
pub struct StateManager {
    /// Internal state storage
    state: Arc<RwLock<HashMap<String, Value>>>,
}

impl StateManager {
    /// Create a new state manager
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set a value in the state
    pub fn set(&self, key: String, value: Value) {
        if let Ok(mut state) = self.state.write() {
            state.insert(key, value);
        }
    }

    /// Get a value from the state
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Value> {
        self.state.read().ok()?.get(key).cloned()
    }

    /// Remove a value from the state
    #[must_use]
    pub fn remove(&self, key: &str) -> Option<Value> {
        self.state.write().ok()?.remove(key)
    }

    /// Check if a key exists in the state
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        self.state.read().is_ok_and(|state| state.contains_key(key))
    }

    /// Get the number of entries in the state
    #[must_use]
    pub fn size(&self) -> usize {
        self.state.read().map_or(0, |state| state.len())
    }

    /// List all keys in the state
    #[must_use]
    pub fn list_keys(&self) -> Vec<String> {
        self.state
            .read()
            .map_or_else(|_| Vec::new(), |state| state.keys().cloned().collect())
    }

    /// Clear all entries from the state
    pub fn clear(&self) {
        if let Ok(mut state) = self.state.write() {
            state.clear();
        }
    }

    /// Export the state as JSON
    #[must_use]
    pub fn export(&self) -> Value {
        self.state.read().map_or_else(
            |_| Value::Object(serde_json::Map::new()),
            |state| Value::Object(state.iter().map(|(k, v)| (k.clone(), v.clone())).collect()),
        )
    }

    /// Import state from JSON
    pub fn import(&self, data: Value) -> Result<()> {
        match data {
            Value::Object(obj) => self.state.write().map_or_else(
                |_| {
                    Err(Error::new(
                        ErrorKind::Internal,
                        "Failed to acquire write lock",
                    ))
                },
                |mut state| {
                    for (key, value) in obj {
                        state.insert(key, value);
                    }
                    Ok(())
                },
            ),
            _ => Err(Error::new(
                ErrorKind::Configuration,
                "Import data must be a JSON object",
            )),
        }
    }
}

impl Default for StateManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_state_operations() {
        let state = StateManager::new();

        // Test set and get
        state.set("key1".to_string(), json!("value1"));
        assert_eq!(state.get("key1"), Some(json!("value1")));

        // Test contains
        assert!(state.contains("key1"));
        assert!(!state.contains("key2"));

        // Test size
        assert_eq!(state.size(), 1);

        // Test remove
        assert_eq!(state.remove("key1"), Some(json!("value1")));
        assert!(!state.contains("key1"));
        assert_eq!(state.size(), 0);
    }

    #[test]
    fn test_export_import() {
        let state1 = StateManager::new();
        state1.set("key1".to_string(), json!("value1"));
        state1.set("key2".to_string(), json!(42));

        let exported = state1.export();

        let state2 = StateManager::new();
        assert!(state2.import(exported).is_ok());

        assert_eq!(state2.get("key1"), Some(json!("value1")));
        assert_eq!(state2.get("key2"), Some(json!(42)));
        assert_eq!(state2.size(), 2);
    }

    #[test]
    fn test_list_keys() {
        let state = StateManager::new();
        state.set("a".to_string(), json!(1));
        state.set("b".to_string(), json!(2));
        state.set("c".to_string(), json!(3));

        let mut keys = state.list_keys();
        keys.sort();
        assert_eq!(keys, vec!["a", "b", "c"]);
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
}
