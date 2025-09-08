//! LLM Registry for managing multiple providers

use crate::llm::core::{LLMError, LLMProvider, LLMResult};
use crate::llm::session::SessionConfig;
use crate::sampling::SamplingHandler;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use turbomcp_protocol::types::{CreateMessageRequest, CreateMessageResult};

/// Configuration for the LLM registry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// Default provider to use
    pub default_provider: Option<String>,
    /// Maximum number of providers
    pub max_providers: usize,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            default_provider: None,
            max_providers: 10,
        }
    }
}

/// Information about a registered provider
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    /// Provider name
    pub name: String,
    /// Provider version
    pub version: String,
    /// Whether provider is healthy
    pub healthy: bool,
    /// Number of models available
    pub model_count: usize,
}

/// Registry for managing multiple LLM providers
#[derive(Debug)]
pub struct LLMRegistry {
    providers: HashMap<String, Arc<dyn LLMProvider>>,
    config: RegistryConfig,
    session_config: Option<SessionConfig>,
}

impl LLMRegistry {
    /// Create a new LLM registry
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
            config: RegistryConfig::default(),
            session_config: None,
        }
    }

    /// Create registry with config
    pub fn with_config(config: RegistryConfig) -> Self {
        Self {
            providers: HashMap::new(),
            config,
            session_config: None,
        }
    }

    /// Register a provider
    pub async fn register_provider(
        &mut self,
        name: impl Into<String>,
        provider: Arc<dyn LLMProvider>,
    ) -> LLMResult<()> {
        let name = name.into();

        if self.providers.len() >= self.config.max_providers {
            return Err(LLMError::configuration(format!(
                "Maximum number of providers ({}) exceeded",
                self.config.max_providers
            )));
        }

        // Basic health check
        provider.health_check().await?;

        self.providers.insert(name, provider);
        Ok(())
    }

    /// Get provider by name
    pub fn get_provider(&self, name: &str) -> Option<&Arc<dyn LLMProvider>> {
        self.providers.get(name)
    }

    /// List all provider names
    pub fn list_providers(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }

    /// Get provider info
    pub async fn get_provider_info(&self, name: &str) -> LLMResult<ProviderInfo> {
        let provider = self
            .providers
            .get(name)
            .ok_or_else(|| LLMError::provider_not_found(name))?;

        let models = provider.list_models().await.unwrap_or_default();
        let healthy = provider.health_check().await.is_ok();

        Ok(ProviderInfo {
            name: name.to_string(),
            version: provider.version().to_string(),
            healthy,
            model_count: models.len(),
        })
    }

    /// Set default provider
    pub fn set_default_provider(&mut self, name: impl Into<String>) -> LLMResult<()> {
        let name = name.into();
        if !self.providers.contains_key(&name) {
            return Err(LLMError::provider_not_found(&name));
        }
        self.config.default_provider = Some(name);
        Ok(())
    }

    /// Get default provider
    pub fn get_default_provider(&self) -> Option<&Arc<dyn LLMProvider>> {
        self.config
            .default_provider
            .as_ref()
            .and_then(|name| self.providers.get(name))
    }

    /// Configure session management
    pub async fn configure_sessions(&mut self, config: SessionConfig) -> LLMResult<()> {
        self.session_config = Some(config);
        Ok(())
    }

    /// Get session configuration
    pub fn session_config(&self) -> Option<&SessionConfig> {
        self.session_config.as_ref()
    }
}

/// Implement SamplingHandler for LLMRegistry
#[async_trait]
impl SamplingHandler for LLMRegistry {
    async fn handle_create_message(
        &self,
        request: CreateMessageRequest,
    ) -> Result<CreateMessageResult, Box<dyn std::error::Error + Send + Sync>> {
        // Get the default provider or use the first available one
        let provider = self
            .get_default_provider()
            .or_else(|| self.providers.values().next())
            .ok_or_else(|| LLMError::configuration("No LLM providers registered".to_string()))?;

        // For now, delegate to the provider's handle_create_message method
        // In a more sophisticated implementation, we could route based on model preferences,
        // manage sessions, handle load balancing, etc.
        provider
            .handle_create_message(request)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }
}

impl Default for LLMRegistry {
    fn default() -> Self {
        Self::new()
    }
}
