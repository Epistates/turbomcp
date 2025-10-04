//! Component registry system for extensible architecture.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use thiserror::Error;

/// Errors that can occur in the registry
///
/// These errors are domain-specific for registry operations and are converted
/// to the main [`Error`](crate::Error) type when crossing API boundaries.
#[derive(Error, Debug, Clone)]
pub enum RegistryError {
    /// Component not found
    #[error("Component {0} not found")]
    NotFound(String),

    /// Component already exists
    #[error("Component {0} already exists")]
    AlreadyExists(String),

    /// Type mismatch
    #[error("Type mismatch for component {0}")]
    TypeMismatch(String),
}

// Conversion to main Error type for API boundary crossing
impl From<RegistryError> for Box<crate::error::Error> {
    fn from(err: RegistryError) -> Self {
        use crate::error::Error;
        match err {
            RegistryError::NotFound(name) => {
                Error::not_found(format!("Component '{}' not found in registry", name))
                    .with_component("registry")
            }
            RegistryError::AlreadyExists(name) => {
                Error::validation(format!("Component '{}' already exists in registry", name))
                    .with_component("registry")
            }
            RegistryError::TypeMismatch(name) => Error::internal(format!(
                "Type mismatch when accessing component '{}' in registry",
                name
            ))
            .with_component("registry"),
        }
    }
}

/// Component registry for dependency injection and service location
#[derive(Debug)]
pub struct Registry {
    /// Map of component names to trait objects
    components: RwLock<HashMap<String, Arc<dyn Any + Send + Sync>>>,

    /// Type mapping for better error messages
    type_map: RwLock<HashMap<String, TypeId>>,
}

/// Registry builder for fluent configuration
#[derive(Debug)]
pub struct RegistryBuilder {
    registry: Registry,
}

impl Registry {
    /// Create a new empty registry
    #[must_use]
    pub fn new() -> Self {
        Self {
            components: RwLock::new(HashMap::new()),
            type_map: RwLock::new(HashMap::new()),
        }
    }

    /// Create a registry builder
    #[must_use]
    pub fn builder() -> RegistryBuilder {
        RegistryBuilder {
            registry: Self::new(),
        }
    }

    /// Register a component with the given name
    pub fn register<T>(&self, name: impl Into<String>, component: T) -> Result<(), RegistryError>
    where
        T: 'static + Send + Sync,
    {
        let name = name.into();
        let type_id = TypeId::of::<T>();

        {
            let mut components = self.components.write();
            if components.contains_key(&name) {
                return Err(RegistryError::AlreadyExists(name));
            }
            components.insert(name.clone(), Arc::new(component));
        }

        {
            let mut type_map = self.type_map.write();
            type_map.insert(name, type_id);
        }

        Ok(())
    }

    /// Get a component by name and type
    pub fn get<T>(&self, name: &str) -> Result<Arc<T>, RegistryError>
    where
        T: 'static + Send + Sync,
    {
        let component = {
            let components = self.components.read();
            components
                .get(name)
                .ok_or_else(|| RegistryError::NotFound(name.to_string()))?
                .clone()
        }; // Lock is dropped here

        component
            .downcast::<T>()
            .map_err(|_| RegistryError::TypeMismatch(name.to_string()))
    }

    /// Check if a component exists
    pub fn contains(&self, name: &str) -> bool {
        self.components.read().contains_key(name)
    }

    /// Get all registered component names
    pub fn component_names(&self) -> Vec<String> {
        self.components.read().keys().cloned().collect()
    }

    /// Remove a component
    pub fn remove(&self, name: &str) -> Option<Arc<dyn Any + Send + Sync>> {
        {
            let mut type_map = self.type_map.write();
            type_map.remove(name);
        } // Drop type_map lock early

        let mut components = self.components.write();
        components.remove(name)
    }

    /// Clear all components
    pub fn clear(&self) {
        self.components.write().clear();
        self.type_map.write().clear();
    }

    /// Get component count
    pub fn len(&self) -> usize {
        self.components.read().len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.components.read().is_empty()
    }
}

impl RegistryBuilder {
    /// Register a component
    pub fn register<T>(self, name: impl Into<String>, component: T) -> Result<Self, RegistryError>
    where
        T: 'static + Send + Sync,
    {
        self.registry.register(name, component)?;
        Ok(self)
    }

    /// Build the final registry
    pub fn build(self) -> Registry {
        self.registry
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience trait for components that can register themselves
pub trait Component: 'static + Send + Sync {
    /// Component name
    fn name(&self) -> &'static str;

    /// Register this component in the registry
    fn register_in(self, registry: &Registry) -> Result<(), RegistryError>
    where
        Self: Sized,
    {
        registry.register(self.name(), self)
    }
}

/// Macro for easier component registration
#[macro_export]
macro_rules! register_component {
    ($registry:expr, $name:expr, $component:expr) => {
        $registry.register($name, $component)
    };
    ($registry:expr, $($name:expr => $component:expr),+ $(,)?) => {
        {
            $(
                $registry.register($name, $component)?;
            )+
            Ok::<(), $crate::registry::RegistryError>(())
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[derive(Debug)]
    struct TestService {
        id: u32,
        counter: AtomicU32,
    }

    impl TestService {
        fn new(id: u32) -> Self {
            Self {
                id,
                counter: AtomicU32::new(0),
            }
        }

        fn increment(&self) -> u32 {
            self.counter.fetch_add(1, Ordering::SeqCst) + 1
        }

        fn get_id(&self) -> u32 {
            self.id
        }
    }

    impl Component for TestService {
        fn name(&self) -> &'static str {
            "test_service"
        }
    }

    #[test]
    fn test_registry_basic_operations() {
        let registry = Registry::new();
        let service = TestService::new(42);

        // Register component
        assert!(registry.register("test", service).is_ok());

        // Check existence
        assert!(registry.contains("test"));
        assert!(!registry.contains("nonexistent"));

        // Get component
        let retrieved: Arc<TestService> = registry.get("test").unwrap();
        assert_eq!(retrieved.get_id(), 42);

        // Test functionality
        assert_eq!(retrieved.increment(), 1);
        assert_eq!(retrieved.increment(), 2);

        // Component count
        assert_eq!(registry.len(), 1);
        assert!(!registry.is_empty());
    }

    #[test]
    fn test_registry_errors() {
        let registry = Registry::new();

        // Test not found error
        let result: Result<Arc<TestService>, _> = registry.get("nonexistent");
        assert!(matches!(result, Err(RegistryError::NotFound(_))));

        // Test duplicate registration
        let service1 = TestService::new(1);
        let service2 = TestService::new(2);

        assert!(registry.register("duplicate", service1).is_ok());
        let result = registry.register("duplicate", service2);
        assert!(matches!(result, Err(RegistryError::AlreadyExists(_))));
    }

    #[test]
    fn test_registry_builder() {
        let registry = Registry::builder()
            .register("service1", TestService::new(1))
            .unwrap()
            .register("service2", TestService::new(2))
            .unwrap()
            .build();

        assert_eq!(registry.len(), 2);

        let service1: Arc<TestService> = registry.get("service1").unwrap();
        let service2: Arc<TestService> = registry.get("service2").unwrap();

        assert_eq!(service1.get_id(), 1);
        assert_eq!(service2.get_id(), 2);
    }

    #[test]
    fn test_component_trait() {
        let registry = Registry::new();
        let service = TestService::new(123);

        // Register using the Component trait
        assert!(service.register_in(&registry).is_ok());

        let retrieved: Arc<TestService> = registry.get("test_service").unwrap();
        assert_eq!(retrieved.get_id(), 123);
    }

    #[test]
    fn test_registry_removal() {
        let registry = Registry::new();
        let service = TestService::new(42);

        registry.register("test", service).unwrap();
        assert!(registry.contains("test"));

        let removed = registry.remove("test");
        assert!(removed.is_some());
        assert!(!registry.contains("test"));

        // Removing non-existent component returns None
        let removed = registry.remove("nonexistent");
        assert!(removed.is_none());
    }

    #[test]
    fn test_registry_clear() {
        let registry = Registry::new();

        registry.register("service1", TestService::new(1)).unwrap();
        registry.register("service2", TestService::new(2)).unwrap();

        assert_eq!(registry.len(), 2);

        registry.clear();

        assert_eq!(registry.len(), 0);
        assert!(registry.is_empty());
    }

    #[test]
    fn test_component_names() {
        let registry = Registry::new();

        registry.register("alpha", TestService::new(1)).unwrap();
        registry.register("beta", TestService::new(2)).unwrap();

        let mut names = registry.component_names();
        names.sort();

        assert_eq!(names, vec!["alpha", "beta"]);
    }
}
