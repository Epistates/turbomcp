//! Tower Layer implementation for authentication

use std::sync::Arc;
use tower::Layer;

use crate::AuthProvider;

use super::AuthLayerConfig;
use super::service::AuthService;

/// Tower Layer that adds authentication to services
///
/// This layer wraps inner services with [`AuthService`], which performs
/// token extraction and validation before forwarding requests.
///
/// # Example
///
/// ```rust,ignore
/// use tower::ServiceBuilder;
/// use turbomcp_auth::tower::AuthLayer;
///
/// let auth_layer = AuthLayer::new(auth_provider);
///
/// let service = ServiceBuilder::new()
///     .layer(auth_layer)
///     .service(my_inner_service);
/// ```
#[derive(Debug, Clone)]
pub struct AuthLayer<P> {
    /// The authentication provider
    provider: Arc<P>,
    /// Layer configuration
    config: AuthLayerConfig,
}

impl<P> AuthLayer<P>
where
    P: AuthProvider,
{
    /// Create a new auth layer with default configuration
    pub fn new(provider: P) -> Self {
        Self {
            provider: Arc::new(provider),
            config: AuthLayerConfig::default(),
        }
    }

    /// Create a new auth layer with custom configuration
    pub fn with_config(provider: P, config: AuthLayerConfig) -> Self {
        Self {
            provider: Arc::new(provider),
            config,
        }
    }

    /// Create a new auth layer from an Arc'd provider
    pub fn from_arc(provider: Arc<P>) -> Self {
        Self {
            provider,
            config: AuthLayerConfig::default(),
        }
    }

    /// Create a new auth layer from an Arc'd provider with custom configuration
    pub fn from_arc_with_config(provider: Arc<P>, config: AuthLayerConfig) -> Self {
        Self { provider, config }
    }

    /// Set the configuration for this layer
    #[must_use]
    pub fn config(mut self, config: AuthLayerConfig) -> Self {
        self.config = config;
        self
    }

    /// Allow anonymous requests to pass through
    #[must_use]
    pub fn allow_anonymous(mut self) -> Self {
        self.config.allow_anonymous = true;
        self
    }

    /// Add a method to bypass authentication
    #[must_use]
    pub fn bypass_method(mut self, method: impl Into<String>) -> Self {
        self.config.bypass_methods.push(method.into());
        self
    }
}

impl<S, P> Layer<S> for AuthLayer<P>
where
    P: AuthProvider + Clone,
{
    type Service = AuthService<S, P>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthService::new(inner, Arc::clone(&self.provider), self.config.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ApiKeyProvider;

    #[test]
    fn test_layer_creation() {
        let provider = ApiKeyProvider::new("test-provider".to_string());
        let layer = AuthLayer::new(provider);
        assert!(!layer.config.allow_anonymous);
    }

    #[test]
    fn test_layer_with_config() {
        let provider = ApiKeyProvider::new("test-provider".to_string());
        let config = AuthLayerConfig::allow_anonymous();
        let layer = AuthLayer::with_config(provider, config);
        assert!(layer.config.allow_anonymous);
    }

    #[test]
    fn test_layer_builder_pattern() {
        let provider = ApiKeyProvider::new("test-provider".to_string());
        let layer = AuthLayer::new(provider)
            .allow_anonymous()
            .bypass_method("custom/method");
        assert!(layer.config.allow_anonymous);
        assert!(layer.config.should_bypass("custom/method"));
    }

    #[test]
    fn test_layer_from_arc() {
        let provider = Arc::new(ApiKeyProvider::new("test-provider".to_string()));
        let layer = AuthLayer::from_arc(Arc::clone(&provider));
        assert!(!layer.config.allow_anonymous);
    }
}
