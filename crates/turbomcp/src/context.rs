//! Context injection system with automatic dependency resolution

use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;

use async_trait::async_trait;
// use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{McpError, McpErrorConstructors, McpResult};

/// Service provider trait for dependency injection
#[async_trait]
pub trait ServiceProvider: Send + Sync {
    /// The type this provider creates
    type Output: Send + Sync + 'static;

    /// Create an instance of the service
    async fn provide(&self, container: &Container) -> McpResult<Self::Output>;

    /// Check if this provider has dependencies
    fn dependencies(&self) -> Vec<String> {
        vec![]
    }
}

/// Factory function provider
pub struct FactoryProvider<F, T>
where
    F: Fn() -> T + Send + Sync,
    T: Send + Sync + 'static,
{
    factory: F,
    _phantom: PhantomData<T>,
}

impl<F, T> FactoryProvider<F, T>
where
    F: Fn() -> T + Send + Sync,
    T: Send + Sync + 'static,
{
    /// Create a new factory provider
    pub const fn new(factory: F) -> Self {
        Self {
            factory,
            _phantom: PhantomData,
        }
    }
}

#[async_trait]
impl<F, T> ServiceProvider for FactoryProvider<F, T>
where
    F: Fn() -> T + Send + Sync,
    T: Send + Sync + 'static,
{
    type Output = T;

    async fn provide(&self, _container: &Container) -> McpResult<Self::Output> {
        Ok((self.factory)())
    }
}

/// Singleton provider that caches instances
pub struct SingletonProvider<T: Clone + Send + Sync + 'static> {
    instance: Arc<RwLock<Option<T>>>,
    provider: Box<dyn ServiceProvider<Output = T> + Send + Sync>,
}

impl<T: Clone + Send + Sync + 'static> SingletonProvider<T> {
    /// Create a new singleton provider
    pub fn new(provider: Box<dyn ServiceProvider<Output = T> + Send + Sync>) -> Self {
        Self {
            instance: Arc::new(RwLock::new(None)),
            provider,
        }
    }
}

#[async_trait]
impl<T: Clone + Send + Sync + 'static> ServiceProvider for SingletonProvider<T> {
    type Output = T;

    async fn provide(&self, container: &Container) -> McpResult<Self::Output> {
        // Check if instance already exists
        {
            let instance = self.instance.read().await;
            if let Some(cached) = instance.as_ref() {
                return Ok(cached.clone());
            }
        }

        // Create new instance
        let new_instance = self.provider.provide(container).await?;

        // Cache it
        {
            let mut instance = self.instance.write().await;
            if instance.is_none() {
                *instance = Some(new_instance.clone());
            }
        }

        Ok(new_instance)
    }

    fn dependencies(&self) -> Vec<String> {
        self.provider.dependencies()
    }
}

/// Service registration info
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    /// Service name
    pub name: String,
    /// Service type name
    pub service_type: String,
    /// Service dependencies
    pub dependencies: Vec<String>,
    /// Whether this is a singleton service
    pub is_singleton: bool,
}

/// Type alias for the service registry
type ServiceRegistry = Arc<RwLock<HashMap<String, Box<dyn std::any::Any + Send + Sync>>>>;

/// Type alias for the provider registry  
type ProviderRegistry = Arc<RwLock<HashMap<String, BoxedServiceProvider>>>;

/// Type alias for boxed service provider
type BoxedServiceProvider =
    Box<dyn ServiceProvider<Output = Box<dyn std::any::Any + Send + Sync>> + Send + Sync>;

/// Type alias for service information registry
type ServiceInfoRegistry = Arc<RwLock<HashMap<String, ServiceInfo>>>;

/// Enhanced dependency injection container
#[derive(Clone)]
pub struct Container {
    services: ServiceRegistry,
    providers: ProviderRegistry,
    service_info: ServiceInfoRegistry,
}

impl Container {
    /// Create a new container
    #[must_use]
    pub fn new() -> Self {
        Self {
            services: Arc::new(RwLock::new(HashMap::new())),
            providers: Arc::new(RwLock::new(HashMap::new())),
            service_info: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a service instance directly
    pub async fn register<T: 'static + Send + Sync>(&self, name: &str, service: T) {
        self.services
            .write()
            .await
            .insert(name.to_string(), Box::new(service));

        self.service_info.write().await.insert(
            name.to_string(),
            ServiceInfo {
                name: name.to_string(),
                service_type: std::any::type_name::<T>().to_string(),
                dependencies: vec![],
                is_singleton: false,
            },
        );
    }

    /// Register a service with a factory function
    pub async fn register_factory<F, T>(&self, name: &str, factory: F)
    where
        F: Fn() -> T + Send + Sync + 'static,
        T: Send + Sync + Clone + 'static,
    {
        let provider = FactoryProvider::new(factory);
        self.register_provider(name, Box::new(provider)).await;
    }

    /// Register a singleton service
    pub async fn register_singleton<F, T>(&self, name: &str, factory: F)
    where
        F: Fn() -> T + Send + Sync + 'static,
        T: Send + Sync + Clone + 'static,
    {
        let factory_provider = FactoryProvider::new(factory);
        let singleton_provider = SingletonProvider::new(Box::new(factory_provider));
        self.register_provider_singleton(name, Box::new(singleton_provider))
            .await;
    }

    /// Register a service provider
    async fn register_provider<T: Send + Sync + Clone + 'static>(
        &self,
        name: &str,
        provider: Box<dyn ServiceProvider<Output = T> + Send + Sync>,
    ) {
        let dependencies = provider.dependencies();

        // Wrap the provider to work with Any
        let wrapper = AnyServiceProvider::new(provider);
        self.providers
            .write()
            .await
            .insert(name.to_string(), Box::new(wrapper));

        self.service_info.write().await.insert(
            name.to_string(),
            ServiceInfo {
                name: name.to_string(),
                service_type: std::any::type_name::<T>().to_string(),
                dependencies,
                is_singleton: false,
            },
        );
    }

    /// Register a singleton service provider
    async fn register_provider_singleton<T: Send + Sync + Clone + 'static>(
        &self,
        name: &str,
        provider: Box<dyn ServiceProvider<Output = T> + Send + Sync>,
    ) {
        let dependencies = provider.dependencies();

        // Wrap the provider to work with Any
        let wrapper = AnyServiceProvider::new(provider);
        self.providers
            .write()
            .await
            .insert(name.to_string(), Box::new(wrapper));

        self.service_info.write().await.insert(
            name.to_string(),
            ServiceInfo {
                name: name.to_string(),
                service_type: std::any::type_name::<T>().to_string(),
                dependencies,
                is_singleton: true,
            },
        );
    }

    /// Resolve a service
    pub async fn resolve<T: 'static + Clone>(&self, name: &str) -> McpResult<T> {
        // First check if service is already instantiated
        {
            let services = self.services.read().await;
            if let Some(service) = services.get(name) {
                return service.downcast_ref::<T>().cloned().ok_or_else(|| {
                    McpError::context("Type mismatch in service resolution".to_string())
                });
            }
        }

        // Check if we have a provider for this service
        {
            let providers = self.providers.read().await;
            if let Some(provider) = providers.get(name) {
                let service_any = provider.provide(self).await?;
                return service_any.downcast_ref::<T>().cloned().ok_or_else(|| {
                    McpError::context("Type mismatch in provider resolution".to_string())
                });
            }
        }

        Err(McpError::context(format!("Service '{name}' not found")))
    }

    /// Resolve with dependency injection
    pub async fn resolve_with_dependencies<T: 'static + Clone>(&self, name: &str) -> McpResult<T> {
        self.check_circular_dependencies(name, &mut vec![]).await?;
        self.resolve(name).await
    }

    /// Check for circular dependencies
    async fn check_circular_dependencies(
        &self,
        name: &str,
        chain: &mut Vec<String>,
    ) -> McpResult<()> {
        if chain.contains(&name.to_string()) {
            return Err(McpError::context(format!(
                "Circular dependency detected: {} -> {}",
                chain.join(" -> "),
                name
            )));
        }

        chain.push(name.to_string());

        let service_info = self.service_info.read().await;
        if let Some(info) = service_info.get(name) {
            let dependencies = info.dependencies.clone();
            drop(service_info); // Release the lock before recursion

            for dep in &dependencies {
                Box::pin(self.check_circular_dependencies(dep, chain)).await?;
            }
        }

        chain.pop();
        Ok(())
    }

    /// Get service information
    pub async fn get_service_info(&self, name: &str) -> Option<ServiceInfo> {
        self.service_info.read().await.get(name).cloned()
    }

    /// List all registered services
    pub async fn list_services(&self) -> Vec<ServiceInfo> {
        self.service_info.read().await.values().cloned().collect()
    }

    /// Check if service is registered
    pub async fn has_service(&self, name: &str) -> bool {
        let services = self.services.read().await;
        let providers = self.providers.read().await;
        services.contains_key(name) || providers.contains_key(name)
    }
}

/// Wrapper to make any `ServiceProvider` work with Any
struct AnyServiceProvider<T: Send + Sync + Clone + 'static> {
    provider: Box<dyn ServiceProvider<Output = T> + Send + Sync>,
}

impl<T: Send + Sync + Clone + 'static> AnyServiceProvider<T> {
    fn new(provider: Box<dyn ServiceProvider<Output = T> + Send + Sync>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl<T: Send + Sync + Clone + 'static> ServiceProvider for AnyServiceProvider<T> {
    type Output = Box<dyn std::any::Any + Send + Sync>;

    async fn provide(&self, container: &Container) -> McpResult<Self::Output> {
        let service = self.provider.provide(container).await?;
        Ok(Box::new(service))
    }

    fn dependencies(&self) -> Vec<String> {
        self.provider.dependencies()
    }
}

impl Default for Container {
    fn default() -> Self {
        Self::new()
    }
}
