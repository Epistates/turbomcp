//! Enhanced registry with support for MCP protocol handlers
//!
//! This module extends the base registry with specialized support for
//! elicitation, completion, resource templates, and ping handlers.

use dashmap::DashMap;
use std::sync::Arc;

use crate::handlers::{
    CompletionProvider, ElicitationHandler, HandlerCapabilities, PingHandler,
    ResourceTemplateHandler,
};
use crate::registry::{Registry, RegistryError};

/// Enhanced registry with handler support
pub struct EnhancedRegistry {
    /// Base registry for general components
    base: Registry,

    /// Elicitation handlers
    elicitation_handlers: Arc<DashMap<String, Arc<dyn ElicitationHandler>>>,

    /// Completion providers
    completion_providers: Arc<DashMap<String, Arc<dyn CompletionProvider>>>,

    /// Resource template handlers
    template_handlers: Arc<DashMap<String, Arc<dyn ResourceTemplateHandler>>>,

    /// Ping handlers
    ping_handlers: Arc<DashMap<String, Arc<dyn PingHandler>>>,

    /// Handler capabilities
    capabilities: Arc<DashMap<String, HandlerCapabilities>>,
}

impl EnhancedRegistry {
    /// Create a new enhanced registry
    pub fn new() -> Self {
        Self {
            base: Registry::new(),
            elicitation_handlers: Arc::new(DashMap::new()),
            completion_providers: Arc::new(DashMap::new()),
            template_handlers: Arc::new(DashMap::new()),
            ping_handlers: Arc::new(DashMap::new()),
            capabilities: Arc::new(DashMap::new()),
        }
    }

    /// Register an elicitation handler
    pub fn register_elicitation_handler(
        &self,
        name: impl Into<String>,
        handler: Arc<dyn ElicitationHandler>,
    ) -> Result<(), RegistryError> {
        let name = name.into();
        if self.elicitation_handlers.contains_key(&name) {
            return Err(RegistryError::AlreadyExists(name));
        }

        self.elicitation_handlers.insert(name.clone(), handler);

        // Update capabilities
        self.capabilities
            .entry(name.clone())
            .or_default()
            .elicitation = true;

        Ok(())
    }

    /// Get an elicitation handler
    pub fn get_elicitation_handler(&self, name: &str) -> Option<Arc<dyn ElicitationHandler>> {
        self.elicitation_handlers.get(name).map(|h| h.clone())
    }

    /// List all elicitation handlers
    pub fn list_elicitation_handlers(&self) -> Vec<String> {
        self.elicitation_handlers
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Register a completion provider
    pub fn register_completion_provider(
        &self,
        name: impl Into<String>,
        provider: Arc<dyn CompletionProvider>,
    ) -> Result<(), RegistryError> {
        let name = name.into();
        if self.completion_providers.contains_key(&name) {
            return Err(RegistryError::AlreadyExists(name));
        }

        self.completion_providers.insert(name.clone(), provider);

        // Update capabilities
        self.capabilities
            .entry(name.clone())
            .or_default()
            .completion = true;

        Ok(())
    }

    /// Get a completion provider
    pub fn get_completion_provider(&self, name: &str) -> Option<Arc<dyn CompletionProvider>> {
        self.completion_providers.get(name).map(|p| p.clone())
    }

    /// Get all completion providers that can handle a context
    pub fn get_matching_completion_providers(
        &self,
        context: &crate::context::CompletionContext,
    ) -> Vec<Arc<dyn CompletionProvider>> {
        let mut providers: Vec<_> = self
            .completion_providers
            .iter()
            .filter_map(|entry| {
                let provider = entry.value();
                if provider.can_provide(context) {
                    Some(provider.clone())
                } else {
                    None
                }
            })
            .collect();

        // Sort by priority (descending)
        providers.sort_by_key(|p| -p.priority());
        providers
    }

    /// Register a resource template handler
    pub fn register_template_handler(
        &self,
        name: impl Into<String>,
        handler: Arc<dyn ResourceTemplateHandler>,
    ) -> Result<(), RegistryError> {
        let name = name.into();
        if self.template_handlers.contains_key(&name) {
            return Err(RegistryError::AlreadyExists(name));
        }

        self.template_handlers.insert(name.clone(), handler);

        // Update capabilities
        self.capabilities.entry(name.clone()).or_default().templates = true;

        Ok(())
    }

    /// Get a resource template handler
    pub fn get_template_handler(&self, name: &str) -> Option<Arc<dyn ResourceTemplateHandler>> {
        self.template_handlers.get(name).map(|h| h.clone())
    }

    /// Register a ping handler
    pub fn register_ping_handler(
        &self,
        name: impl Into<String>,
        handler: Arc<dyn PingHandler>,
    ) -> Result<(), RegistryError> {
        let name = name.into();
        if self.ping_handlers.contains_key(&name) {
            return Err(RegistryError::AlreadyExists(name));
        }

        self.ping_handlers.insert(name.clone(), handler);

        // Update capabilities
        self.capabilities.entry(name.clone()).or_default().ping = true;

        Ok(())
    }

    /// Get a ping handler
    pub fn get_ping_handler(&self, name: &str) -> Option<Arc<dyn PingHandler>> {
        self.ping_handlers.get(name).map(|h| h.clone())
    }

    /// Get handler capabilities for a component
    pub fn get_capabilities(&self, name: &str) -> Option<HandlerCapabilities> {
        self.capabilities.get(name).map(|c| c.clone())
    }

    /// Get all components with specific capabilities
    pub fn find_by_capabilities(
        &self,
        filter: impl Fn(&HandlerCapabilities) -> bool,
    ) -> Vec<String> {
        self.capabilities
            .iter()
            .filter(|entry| filter(entry.value()))
            .map(|entry| entry.key().clone())
            .collect()
    }

    /// Clear all handlers
    pub fn clear_handlers(&self) {
        self.elicitation_handlers.clear();
        self.completion_providers.clear();
        self.template_handlers.clear();
        self.ping_handlers.clear();
        self.capabilities.clear();
    }

    /// Get handler statistics
    pub fn handler_stats(&self) -> HandlerStats {
        HandlerStats {
            elicitation_handlers: self.elicitation_handlers.len(),
            completion_providers: self.completion_providers.len(),
            template_handlers: self.template_handlers.len(),
            ping_handlers: self.ping_handlers.len(),
            total_components: self.capabilities.len(),
        }
    }

    /// Access the base registry
    pub fn base(&self) -> &Registry {
        &self.base
    }
}

impl Default for EnhancedRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for EnhancedRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnhancedRegistry")
            .field("base", &self.base)
            .field(
                "elicitation_handlers_count",
                &self.elicitation_handlers.len(),
            )
            .field(
                "completion_providers_count",
                &self.completion_providers.len(),
            )
            .field("template_handlers_count", &self.template_handlers.len())
            .field("ping_handlers_count", &self.ping_handlers.len())
            .field("capabilities_count", &self.capabilities.len())
            .finish()
    }
}

/// Statistics about registered handlers
#[derive(Debug, Clone)]
pub struct HandlerStats {
    /// Number of elicitation handlers
    pub elicitation_handlers: usize,
    /// Number of completion providers
    pub completion_providers: usize,
    /// Number of template handlers
    pub template_handlers: usize,
    /// Number of ping handlers
    pub ping_handlers: usize,
    /// Total number of components with capabilities
    pub total_components: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{CompletionContext, ElicitationContext};
    use crate::handlers::{CompletionItem, ElicitationResponse};
    use async_trait::async_trait;

    struct TestElicitationHandler;

    #[async_trait]
    impl ElicitationHandler for TestElicitationHandler {
        async fn handle_elicitation(
            &self,
            _context: &ElicitationContext,
        ) -> crate::error::Result<ElicitationResponse> {
            Ok(ElicitationResponse {
                accepted: true,
                content: None,
                decline_reason: None,
            })
        }

        fn can_handle(&self, _context: &ElicitationContext) -> bool {
            true
        }
    }

    struct TestCompletionProvider;

    #[async_trait]
    impl CompletionProvider for TestCompletionProvider {
        async fn provide_completions(
            &self,
            _context: &CompletionContext,
        ) -> crate::error::Result<Vec<CompletionItem>> {
            Ok(vec![])
        }

        fn can_provide(&self, _context: &CompletionContext) -> bool {
            true
        }

        fn priority(&self) -> i32 {
            10
        }
    }

    #[test]
    fn test_enhanced_registry() {
        let registry = EnhancedRegistry::new();

        // Register elicitation handler
        let handler = Arc::new(TestElicitationHandler);
        registry
            .register_elicitation_handler("test_handler", handler)
            .unwrap();

        // Verify registration
        assert!(registry.get_elicitation_handler("test_handler").is_some());
        assert_eq!(registry.list_elicitation_handlers(), vec!["test_handler"]);

        // Check capabilities
        let caps = registry.get_capabilities("test_handler").unwrap();
        assert!(caps.elicitation);
        assert!(!caps.completion);
    }

    #[test]
    fn test_completion_provider_priority() {
        let registry = EnhancedRegistry::new();

        // Register provider
        let provider = Arc::new(TestCompletionProvider);
        registry
            .register_completion_provider("test_provider", provider)
            .unwrap();

        // Create a dummy context
        use crate::context::CompletionReference;
        let context = CompletionContext::new(CompletionReference::Tool {
            name: "test".to_string(),
            argument: "arg".to_string(),
        });

        // Get matching providers
        let providers = registry.get_matching_completion_providers(&context);
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].priority(), 10);
    }

    #[test]
    fn test_handler_stats() {
        let registry = EnhancedRegistry::new();

        // Register various handlers
        registry
            .register_elicitation_handler("elicit1", Arc::new(TestElicitationHandler))
            .unwrap();
        registry
            .register_completion_provider("comp1", Arc::new(TestCompletionProvider))
            .unwrap();

        let stats = registry.handler_stats();
        assert_eq!(stats.elicitation_handlers, 1);
        assert_eq!(stats.completion_providers, 1);
        assert_eq!(stats.template_handlers, 0);
        assert_eq!(stats.ping_handlers, 0);
        assert_eq!(stats.total_components, 2);
    }
}
