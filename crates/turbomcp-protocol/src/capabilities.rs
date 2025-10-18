//! # Capability Negotiation
//!
//! This module provides sophisticated capability negotiation and feature detection
//! for MCP protocol implementations.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use crate::types::{ClientCapabilities, ServerCapabilities};

/// Capability matcher for negotiating features between client and server
///
/// # Examples
///
/// ```
/// use turbomcp_protocol::capabilities::{CapabilityMatcher, CompatibilityRule};
/// use turbomcp_protocol::types::{ClientCapabilities, ServerCapabilities};
///
/// let mut matcher = CapabilityMatcher::new();
///
/// // Add custom compatibility rules
/// matcher.add_rule("custom_feature", CompatibilityRule::RequireBoth);
/// matcher.set_default("progress", true);
///
/// // Negotiate capabilities
/// let client = ClientCapabilities::default();
/// let server = ServerCapabilities::default();
/// let result = matcher.negotiate(&client, &server);
/// ```
#[derive(Debug, Clone)]
pub struct CapabilityMatcher {
    /// Feature compatibility rules
    compatibility_rules: HashMap<String, CompatibilityRule>,
    /// Default feature states
    defaults: HashMap<String, bool>,
}

/// Compatibility rule for a feature
#[derive(Debug, Clone)]
pub enum CompatibilityRule {
    /// Feature requires both client and server support
    RequireBoth,
    /// Feature requires only client support
    RequireClient,
    /// Feature requires only server support  
    RequireServer,
    /// Feature is optional (either side can enable)
    Optional,
    /// Custom compatibility function
    Custom(fn(&ClientCapabilities, &ServerCapabilities) -> bool),
}

/// Negotiated capability set
///
/// # Examples
///
/// ```
/// use turbomcp_protocol::capabilities::CapabilitySet;
///
/// let mut caps = CapabilitySet::empty();
/// caps.enable_feature("tools".to_string());
/// caps.enable_feature("prompts".to_string());
///
/// assert!(caps.has_feature("tools"));
/// assert_eq!(caps.feature_count(), 2);
///
/// let summary = caps.summary();
/// println!("{:?}", summary);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySet {
    /// Enabled features
    pub enabled_features: HashSet<String>,
    /// Negotiated client capabilities
    pub client_capabilities: ClientCapabilities,
    /// Negotiated server capabilities
    pub server_capabilities: ServerCapabilities,
    /// Additional metadata from negotiation
    pub metadata: HashMap<String, serde_json::Value>,
}

/// Capability negotiator for handling the negotiation process
#[derive(Debug, Clone)]
pub struct CapabilityNegotiator {
    /// Capability matcher
    matcher: CapabilityMatcher,
    /// Strict mode (fail on incompatible features)
    strict_mode: bool,
}

impl Default for CapabilityMatcher {
    fn default() -> Self {
        Self::new()
    }
}

impl CapabilityMatcher {
    /// Create a new capability matcher with default MCP rules
    pub fn new() -> Self {
        let mut matcher = Self {
            compatibility_rules: HashMap::new(),
            defaults: HashMap::new(),
        };

        // Set up default MCP capability rules
        matcher.add_rule("tools", CompatibilityRule::RequireServer);
        matcher.add_rule("prompts", CompatibilityRule::RequireServer);
        matcher.add_rule("resources", CompatibilityRule::RequireServer);
        matcher.add_rule("logging", CompatibilityRule::RequireServer);
        matcher.add_rule("sampling", CompatibilityRule::RequireClient);
        matcher.add_rule("roots", CompatibilityRule::RequireClient);
        matcher.add_rule("progress", CompatibilityRule::Optional);

        // Set defaults
        matcher.set_default("progress", true);

        matcher
    }

    /// Add a compatibility rule for a feature
    pub fn add_rule(&mut self, feature: &str, rule: CompatibilityRule) {
        self.compatibility_rules.insert(feature.to_string(), rule);
    }

    /// Set default state for a feature
    pub fn set_default(&mut self, feature: &str, enabled: bool) {
        self.defaults.insert(feature.to_string(), enabled);
    }

    /// Check if a feature is compatible between client and server
    pub fn is_compatible(
        &self,
        feature: &str,
        client: &ClientCapabilities,
        server: &ServerCapabilities,
    ) -> bool {
        self.compatibility_rules.get(feature).map_or_else(
            || {
                // Unknown feature - check if either side supports it
                Self::client_has_feature(feature, client)
                    || Self::server_has_feature(feature, server)
            },
            |rule| match rule {
                CompatibilityRule::RequireBoth => {
                    Self::client_has_feature(feature, client)
                        && Self::server_has_feature(feature, server)
                }
                CompatibilityRule::RequireClient => Self::client_has_feature(feature, client),
                CompatibilityRule::RequireServer => Self::server_has_feature(feature, server),
                CompatibilityRule::Optional => true,
                CompatibilityRule::Custom(func) => func(client, server),
            },
        )
    }

    /// Check if client has a specific feature
    fn client_has_feature(feature: &str, client: &ClientCapabilities) -> bool {
        match feature {
            "sampling" => client.sampling.is_some(),
            "roots" => client.roots.is_some(),
            _ => {
                // Check experimental features
                client
                    .experimental
                    .as_ref()
                    .is_some_and(|experimental| experimental.contains_key(feature))
            }
        }
    }

    /// Check if server has a specific feature
    fn server_has_feature(feature: &str, server: &ServerCapabilities) -> bool {
        match feature {
            "tools" => server.tools.is_some(),
            "prompts" => server.prompts.is_some(),
            "resources" => server.resources.is_some(),
            "logging" => server.logging.is_some(),
            _ => {
                // Check experimental features
                server
                    .experimental
                    .as_ref()
                    .is_some_and(|experimental| experimental.contains_key(feature))
            }
        }
    }

    /// Get all features from both client and server
    fn get_all_features(
        &self,
        client: &ClientCapabilities,
        server: &ServerCapabilities,
    ) -> HashSet<String> {
        let mut features = HashSet::new();

        // Standard client features
        if client.sampling.is_some() {
            features.insert("sampling".to_string());
        }
        if client.roots.is_some() {
            features.insert("roots".to_string());
        }

        // Standard server features
        if server.tools.is_some() {
            features.insert("tools".to_string());
        }
        if server.prompts.is_some() {
            features.insert("prompts".to_string());
        }
        if server.resources.is_some() {
            features.insert("resources".to_string());
        }
        if server.logging.is_some() {
            features.insert("logging".to_string());
        }

        // Experimental features
        if let Some(experimental) = &client.experimental {
            features.extend(experimental.keys().cloned());
        }
        if let Some(experimental) = &server.experimental {
            features.extend(experimental.keys().cloned());
        }

        // Add default features
        features.extend(self.defaults.keys().cloned());

        features
    }

    /// Negotiate capabilities between client and server
    ///
    /// # Errors
    ///
    /// Returns [`CapabilityError::IncompatibleFeatures`] if any features have
    /// incompatible requirements between client and server.
    pub fn negotiate(
        &self,
        client: &ClientCapabilities,
        server: &ServerCapabilities,
    ) -> Result<CapabilitySet, CapabilityError> {
        let all_features = self.get_all_features(client, server);
        let mut enabled_features = HashSet::new();
        let mut incompatible_features = Vec::new();

        for feature in &all_features {
            if self.is_compatible(feature, client, server) {
                enabled_features.insert(feature.clone());
            } else {
                incompatible_features.push(feature.clone());
            }
        }

        if !incompatible_features.is_empty() {
            return Err(CapabilityError::IncompatibleFeatures(incompatible_features));
        }

        // Apply defaults for features not explicitly enabled
        for (feature, enabled) in &self.defaults {
            if *enabled && !enabled_features.contains(feature) && all_features.contains(feature) {
                enabled_features.insert(feature.clone());
            }
        }

        Ok(CapabilitySet {
            enabled_features,
            client_capabilities: client.clone(),
            server_capabilities: server.clone(),
            metadata: HashMap::new(),
        })
    }
}

impl CapabilityNegotiator {
    /// Create a new capability negotiator
    pub const fn new(matcher: CapabilityMatcher) -> Self {
        Self {
            matcher,
            strict_mode: false,
        }
    }

    /// Enable strict mode (fail on any incompatible feature)
    pub const fn with_strict_mode(mut self) -> Self {
        self.strict_mode = true;
        self
    }

    /// Negotiate capabilities between client and server
    ///
    /// # Errors
    ///
    /// Returns [`CapabilityError`] if capability negotiation fails.
    /// In strict mode, incompatible features cause an error.
    /// In non-strict mode, incompatible features are logged and disabled.
    pub fn negotiate(
        &self,
        client: &ClientCapabilities,
        server: &ServerCapabilities,
    ) -> Result<CapabilitySet, CapabilityError> {
        match self.matcher.negotiate(client, server) {
            Ok(capability_set) => Ok(capability_set),
            Err(CapabilityError::IncompatibleFeatures(features)) if !self.strict_mode => {
                // In non-strict mode, just log the incompatible features and continue
                tracing::warn!(
                    "Some features are incompatible and will be disabled: {:?}",
                    features
                );

                // Create a capability set with only compatible features
                let all_features = self.matcher.get_all_features(client, server);
                let mut enabled_features = HashSet::new();

                for feature in &all_features {
                    if self.matcher.is_compatible(feature, client, server) {
                        enabled_features.insert(feature.clone());
                    }
                }

                Ok(CapabilitySet {
                    enabled_features,
                    client_capabilities: client.clone(),
                    server_capabilities: server.clone(),
                    metadata: HashMap::new(),
                })
            }
            Err(err) => Err(err),
        }
    }

    /// Check if a specific feature is enabled in the capability set
    pub fn is_feature_enabled(capability_set: &CapabilitySet, feature: &str) -> bool {
        capability_set.enabled_features.contains(feature)
    }

    /// Get all enabled features as a sorted vector
    pub fn get_enabled_features(capability_set: &CapabilitySet) -> Vec<String> {
        let mut features: Vec<String> = capability_set.enabled_features.iter().cloned().collect();
        features.sort();
        features
    }
}

impl Default for CapabilityNegotiator {
    fn default() -> Self {
        Self::new(CapabilityMatcher::new())
    }
}

impl CapabilitySet {
    /// Create a new empty capability set
    pub fn empty() -> Self {
        Self {
            enabled_features: HashSet::new(),
            client_capabilities: ClientCapabilities::default(),
            server_capabilities: ServerCapabilities::default(),
            metadata: HashMap::new(),
        }
    }

    /// Check if a feature is enabled
    pub fn has_feature(&self, feature: &str) -> bool {
        self.enabled_features.contains(feature)
    }

    /// Add a feature to the enabled set
    pub fn enable_feature(&mut self, feature: String) {
        self.enabled_features.insert(feature);
    }

    /// Remove a feature from the enabled set
    pub fn disable_feature(&mut self, feature: &str) {
        self.enabled_features.remove(feature);
    }

    /// Get the number of enabled features
    pub fn feature_count(&self) -> usize {
        self.enabled_features.len()
    }

    /// Add metadata
    pub fn add_metadata(&mut self, key: String, value: serde_json::Value) {
        self.metadata.insert(key, value);
    }

    /// Get metadata
    pub fn get_metadata(&self, key: &str) -> Option<&serde_json::Value> {
        self.metadata.get(key)
    }

    /// Create a summary of enabled capabilities
    pub fn summary(&self) -> CapabilitySummary {
        CapabilitySummary {
            total_features: self.enabled_features.len(),
            client_features: self.count_client_features(),
            server_features: self.count_server_features(),
            enabled_features: self.enabled_features.iter().cloned().collect(),
        }
    }

    fn count_client_features(&self) -> usize {
        let mut count = 0;
        if self.client_capabilities.sampling.is_some() {
            count += 1;
        }
        if self.client_capabilities.roots.is_some() {
            count += 1;
        }
        if let Some(experimental) = &self.client_capabilities.experimental {
            count += experimental.len();
        }
        count
    }

    fn count_server_features(&self) -> usize {
        let mut count = 0;
        if self.server_capabilities.tools.is_some() {
            count += 1;
        }
        if self.server_capabilities.prompts.is_some() {
            count += 1;
        }
        if self.server_capabilities.resources.is_some() {
            count += 1;
        }
        if self.server_capabilities.logging.is_some() {
            count += 1;
        }
        if let Some(experimental) = &self.server_capabilities.experimental {
            count += experimental.len();
        }
        count
    }
}

/// Capability negotiation errors
#[derive(Debug, Clone, thiserror::Error)]
pub enum CapabilityError {
    /// Features are incompatible between client and server
    #[error("Incompatible features: {0:?}")]
    IncompatibleFeatures(Vec<String>),
    /// Required feature is missing
    #[error("Required feature missing: {0}")]
    RequiredFeatureMissing(String),
    /// Protocol version mismatch
    #[error("Protocol version mismatch: client={client}, server={server}")]
    VersionMismatch {
        /// Client version string
        client: String,
        /// Server version string
        server: String,
    },
    /// Capability negotiation failed
    #[error("Capability negotiation failed: {0}")]
    NegotiationFailed(String),
}

/// Summary of capability negotiation results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilitySummary {
    /// Total number of enabled features
    pub total_features: usize,
    /// Number of client-side features
    pub client_features: usize,
    /// Number of server-side features
    pub server_features: usize,
    /// List of enabled features
    pub enabled_features: Vec<String>,
}

/// Utility functions for capability management
pub mod utils {
    use super::*;

    /// Create a minimal client capability set
    pub fn minimal_client_capabilities() -> ClientCapabilities {
        ClientCapabilities::default()
    }

    /// Create a minimal server capability set
    pub fn minimal_server_capabilities() -> ServerCapabilities {
        ServerCapabilities::default()
    }

    /// Create a full-featured client capability set
    pub fn full_client_capabilities() -> ClientCapabilities {
        ClientCapabilities {
            sampling: Some(Default::default()),
            roots: Some(Default::default()),
            elicitation: Some(Default::default()),
            experimental: None,
        }
    }

    /// Create a full-featured server capability set
    pub fn full_server_capabilities() -> ServerCapabilities {
        ServerCapabilities {
            tools: Some(Default::default()),
            prompts: Some(Default::default()),
            resources: Some(Default::default()),
            completions: Some(Default::default()),
            logging: Some(Default::default()),
            experimental: None,
        }
    }

    /// Check if two capability sets are compatible
    pub fn are_compatible(client: &ClientCapabilities, server: &ServerCapabilities) -> bool {
        let matcher = CapabilityMatcher::new();
        matcher.negotiate(client, server).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::*;

    #[test]
    fn test_capability_matcher() {
        let matcher = CapabilityMatcher::new();

        let client = ClientCapabilities {
            sampling: Some(SamplingCapabilities),
            roots: None,
            elicitation: None,
            experimental: None,
        };

        let server = ServerCapabilities {
            tools: Some(ToolsCapabilities::default()),
            prompts: None,
            resources: None,
            logging: None,
            completions: None,
            experimental: None,
        };

        assert!(matcher.is_compatible("sampling", &client, &server));
        assert!(matcher.is_compatible("tools", &client, &server));
        assert!(!matcher.is_compatible("roots", &client, &server));
    }

    #[test]
    fn test_capability_negotiation() {
        let negotiator = CapabilityNegotiator::default();

        let client = utils::full_client_capabilities();
        let server = utils::full_server_capabilities();

        let result = negotiator.negotiate(&client, &server);
        assert!(result.is_ok());

        let capability_set = result.unwrap();
        assert!(capability_set.has_feature("sampling"));
        assert!(capability_set.has_feature("tools"));
        assert!(capability_set.has_feature("roots"));
    }

    #[test]
    fn test_strict_mode() {
        let negotiator = CapabilityNegotiator::default().with_strict_mode();

        let client = ClientCapabilities::default();
        let server = ServerCapabilities::default();

        let result = negotiator.negotiate(&client, &server);
        assert!(result.is_ok()); // Should still work with minimal capabilities
    }

    #[test]
    fn test_capability_summary() {
        let mut capability_set = CapabilitySet::empty();
        capability_set.enable_feature("tools".to_string());
        capability_set.enable_feature("prompts".to_string());

        let summary = capability_set.summary();
        assert_eq!(summary.total_features, 2);
        assert!(summary.enabled_features.contains(&"tools".to_string()));
    }
}

// ============================================================================
// TYPE-STATE CAPABILITY BUILDERS - TURBOMCP LEAPFROG IMPLEMENTATION
// ============================================================================

/// Type-state capability builders for compile-time validation
///
/// This module provides const-generic builders that ensure capabilities
/// are configured correctly at compile time with zero-cost abstractions
/// and advanced compile-time safety features.
pub mod builders {
    use crate::types::{
        ClientCapabilities, CompletionCapabilities, ElicitationCapabilities, LoggingCapabilities,
        PromptsCapabilities, ResourcesCapabilities, RootsCapabilities, SamplingCapabilities,
        ServerCapabilities, ToolsCapabilities,
    };
    use serde_json;
    use std::collections::HashMap;
    use std::marker::PhantomData;

    // ========================================================================
    // SERVER CAPABILITIES BUILDER - TYPE-STATE SYSTEM
    // ========================================================================

    /// Type-state for ServerCapabilitiesBuilder
    ///
    /// Each const generic represents whether a capability is enabled:
    /// - EXPERIMENTAL: Experimental capabilities
    /// - LOGGING: Logging capabilities
    /// - COMPLETIONS: Completion capabilities
    /// - PROMPTS: Prompt capabilities
    /// - RESOURCES: Resource capabilities
    /// - TOOLS: Tool capabilities
    #[derive(Debug, Clone)]
    pub struct ServerCapabilitiesBuilderState<
        const EXPERIMENTAL: bool = false,
        const LOGGING: bool = false,
        const COMPLETIONS: bool = false,
        const PROMPTS: bool = false,
        const RESOURCES: bool = false,
        const TOOLS: bool = false,
    >;

    /// Const-generic ServerCapabilities builder with compile-time validation
    ///
    /// This builder ensures that capability-specific methods are only available
    /// when the corresponding capability is enabled, providing compile-time safety
    /// with compile-time validation.
    #[derive(Debug, Clone)]
    pub struct ServerCapabilitiesBuilder<S = ServerCapabilitiesBuilderState> {
        experimental: Option<HashMap<String, serde_json::Value>>,
        logging: Option<LoggingCapabilities>,
        completions: Option<CompletionCapabilities>,
        prompts: Option<PromptsCapabilities>,
        resources: Option<ResourcesCapabilities>,
        tools: Option<ToolsCapabilities>,

        // TurboMCP Extensions
        negotiator: Option<super::CapabilityNegotiator>,
        strict_validation: bool,

        _state: PhantomData<S>,
    }

    impl ServerCapabilities {
        /// Create a new ServerCapabilities builder with type-state validation
        ///
        /// Returns a builder that ensures capabilities are configured correctly
        /// at compile time, preventing runtime configuration errors.
        pub fn builder() -> ServerCapabilitiesBuilder {
            ServerCapabilitiesBuilder::new()
        }
    }

    impl Default for ServerCapabilitiesBuilder {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ServerCapabilitiesBuilder {
        /// Create a new ServerCapabilities builder
        pub fn new() -> Self {
            Self {
                experimental: None,
                logging: None,
                completions: None,
                prompts: None,
                resources: None,
                tools: None,
                negotiator: None,
                strict_validation: false,
                _state: PhantomData,
            }
        }
    }

    // Generic implementation for all states
    impl<S> ServerCapabilitiesBuilder<S> {
        /// Build the final ServerCapabilities
        ///
        /// Consumes the builder and returns the configured ServerCapabilities.
        /// All compile-time validations have been enforced during building.
        pub fn build(self) -> ServerCapabilities {
            ServerCapabilities {
                experimental: self.experimental,
                logging: self.logging,
                completions: self.completions,
                prompts: self.prompts,
                resources: self.resources,
                tools: self.tools,
            }
        }

        /// TurboMCP Extension: Enable strict validation mode
        ///
        /// When enabled, the builder will perform additional runtime validations
        /// on top of the compile-time guarantees.
        pub fn with_strict_validation(mut self) -> Self {
            self.strict_validation = true;
            self
        }

        /// TurboMCP Extension: Set capability negotiator
        ///
        /// Integrates with TurboMCP's sophisticated capability negotiation system
        /// for advanced client-server capability matching.
        pub fn with_negotiator(mut self, negotiator: super::CapabilityNegotiator) -> Self {
            self.negotiator = Some(negotiator);
            self
        }

        /// TurboMCP Extension: Validate capability configuration
        ///
        /// Performs additional runtime validation to ensure the capability
        /// configuration makes sense in the current context.
        pub fn validate(&self) -> Result<(), String> {
            if self.strict_validation {
                // Perform additional validation when strict mode is enabled
                if self.tools.is_none() && self.prompts.is_none() && self.resources.is_none() {
                    return Err("Server must provide at least one capability (tools, prompts, or resources)".to_string());
                }

                // Validate experimental capabilities if present
                if let Some(ref experimental) = self.experimental {
                    for (key, value) in experimental {
                        if key.starts_with("turbomcp_") {
                            // Validate TurboMCP-specific experimental capabilities
                            match key.as_str() {
                                "turbomcp_simd_level" => {
                                    if !value.is_string() {
                                        return Err(
                                            "turbomcp_simd_level must be a string".to_string()
                                        );
                                    }
                                    let level = value.as_str().unwrap_or("");
                                    if !["none", "sse2", "sse4", "avx2", "avx512"].contains(&level)
                                    {
                                        return Err(format!("Invalid SIMD level: {}", level));
                                    }
                                }
                                "turbomcp_enterprise_security" => {
                                    if !value.is_boolean() {
                                        return Err(
                                            "turbomcp_enterprise_security must be a boolean"
                                                .to_string(),
                                        );
                                    }
                                }
                                _ => {
                                    // Allow other TurboMCP experimental features
                                }
                            }
                        }
                    }
                }
            }
            Ok(())
        }

        /// Get a summary of enabled capabilities
        ///
        /// Returns a human-readable summary of which capabilities are enabled.
        pub fn summary(&self) -> String {
            let mut capabilities = Vec::new();
            if self.experimental.is_some() {
                capabilities.push("experimental");
            }
            if self.logging.is_some() {
                capabilities.push("logging");
            }
            if self.completions.is_some() {
                capabilities.push("completions");
            }
            if self.prompts.is_some() {
                capabilities.push("prompts");
            }
            if self.resources.is_some() {
                capabilities.push("resources");
            }
            if self.tools.is_some() {
                capabilities.push("tools");
            }

            if capabilities.is_empty() {
                "No capabilities enabled".to_string()
            } else {
                format!("Enabled capabilities: {}", capabilities.join(", "))
            }
        }
    }

    // ========================================================================
    // CAPABILITY ENABLEMENT METHODS
    // ========================================================================

    // Enable Experimental Capabilities
    impl<const L: bool, const C: bool, const P: bool, const R: bool, const T: bool>
        ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<false, L, C, P, R, T>>
    {
        /// Enable experimental capabilities
        ///
        /// Transitions the builder to a state where experimental capability methods
        /// become available at compile time.
        pub fn enable_experimental(
            self,
        ) -> ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<true, L, C, P, R, T>>
        {
            ServerCapabilitiesBuilder {
                experimental: Some(HashMap::new()),
                logging: self.logging,
                completions: self.completions,
                prompts: self.prompts,
                resources: self.resources,
                tools: self.tools,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }

        /// Enable experimental capabilities with specific values
        pub fn enable_experimental_with(
            self,
            experimental: HashMap<String, serde_json::Value>,
        ) -> ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<true, L, C, P, R, T>>
        {
            ServerCapabilitiesBuilder {
                experimental: Some(experimental),
                logging: self.logging,
                completions: self.completions,
                prompts: self.prompts,
                resources: self.resources,
                tools: self.tools,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Enable Logging Capabilities
    impl<const E: bool, const C: bool, const P: bool, const R: bool, const T: bool>
        ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, false, C, P, R, T>>
    {
        /// Enable logging capabilities
        pub fn enable_logging(
            self,
        ) -> ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, true, C, P, R, T>>
        {
            ServerCapabilitiesBuilder {
                experimental: self.experimental,
                logging: Some(LoggingCapabilities),
                completions: self.completions,
                prompts: self.prompts,
                resources: self.resources,
                tools: self.tools,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Enable Completion Capabilities
    impl<const E: bool, const L: bool, const P: bool, const R: bool, const T: bool>
        ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, L, false, P, R, T>>
    {
        /// Enable completion capabilities
        pub fn enable_completions(
            self,
        ) -> ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, L, true, P, R, T>>
        {
            ServerCapabilitiesBuilder {
                experimental: self.experimental,
                logging: self.logging,
                completions: Some(CompletionCapabilities),
                prompts: self.prompts,
                resources: self.resources,
                tools: self.tools,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Enable Prompts Capabilities
    impl<const E: bool, const L: bool, const C: bool, const R: bool, const T: bool>
        ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, L, C, false, R, T>>
    {
        /// Enable prompts capabilities
        pub fn enable_prompts(
            self,
        ) -> ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, L, C, true, R, T>>
        {
            ServerCapabilitiesBuilder {
                experimental: self.experimental,
                logging: self.logging,
                completions: self.completions,
                prompts: Some(PromptsCapabilities { list_changed: None }),
                resources: self.resources,
                tools: self.tools,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Enable Resources Capabilities
    impl<const E: bool, const L: bool, const C: bool, const P: bool, const T: bool>
        ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, L, C, P, false, T>>
    {
        /// Enable resources capabilities
        pub fn enable_resources(
            self,
        ) -> ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, L, C, P, true, T>>
        {
            ServerCapabilitiesBuilder {
                experimental: self.experimental,
                logging: self.logging,
                completions: self.completions,
                prompts: self.prompts,
                resources: Some(ResourcesCapabilities {
                    subscribe: None,
                    list_changed: None,
                }),
                tools: self.tools,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Enable Tools Capabilities
    impl<const E: bool, const L: bool, const C: bool, const P: bool, const R: bool>
        ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, L, C, P, R, false>>
    {
        /// Enable tools capabilities
        pub fn enable_tools(
            self,
        ) -> ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, L, C, P, R, true>>
        {
            ServerCapabilitiesBuilder {
                experimental: self.experimental,
                logging: self.logging,
                completions: self.completions,
                prompts: self.prompts,
                resources: self.resources,
                tools: Some(ToolsCapabilities { list_changed: None }),
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // ========================================================================
    // SUB-CAPABILITY METHODS (ONLY AVAILABLE WHEN PARENT CAPABILITY ENABLED)
    // ========================================================================

    // Tools sub-capabilities (only available when TOOLS = true)
    impl<const E: bool, const L: bool, const C: bool, const P: bool, const R: bool>
        ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, L, C, P, R, true>>
    {
        /// Enable tool list changed notifications
        ///
        /// This method is only available when tools capabilities are enabled,
        /// providing advanced compile-time validation.
        pub fn enable_tool_list_changed(mut self) -> Self {
            if let Some(ref mut tools) = self.tools {
                tools.list_changed = Some(true);
            }
            self
        }
    }

    // Prompts sub-capabilities (only available when PROMPTS = true)
    impl<const E: bool, const L: bool, const C: bool, const R: bool, const T: bool>
        ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, L, C, true, R, T>>
    {
        /// Enable prompts list changed notifications
        pub fn enable_prompts_list_changed(mut self) -> Self {
            if let Some(ref mut prompts) = self.prompts {
                prompts.list_changed = Some(true);
            }
            self
        }
    }

    // Resources sub-capabilities (only available when RESOURCES = true)
    impl<const E: bool, const L: bool, const C: bool, const P: bool, const T: bool>
        ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<E, L, C, P, true, T>>
    {
        /// Enable resources list changed notifications
        pub fn enable_resources_list_changed(mut self) -> Self {
            if let Some(ref mut resources) = self.resources {
                resources.list_changed = Some(true);
            }
            self
        }

        /// Enable resources subscribe capability
        pub fn enable_resources_subscribe(mut self) -> Self {
            if let Some(ref mut resources) = self.resources {
                resources.subscribe = Some(true);
            }
            self
        }
    }

    // Experimental sub-capabilities (only available when EXPERIMENTAL = true)
    impl<const L: bool, const C: bool, const P: bool, const R: bool, const T: bool>
        ServerCapabilitiesBuilder<ServerCapabilitiesBuilderState<true, L, C, P, R, T>>
    {
        /// Add experimental capability
        ///
        /// This method is only available when experimental capabilities are enabled.
        pub fn add_experimental_capability<K, V>(mut self, key: K, value: V) -> Self
        where
            K: Into<String>,
            V: Into<serde_json::Value>,
        {
            if let Some(ref mut experimental) = self.experimental {
                experimental.insert(key.into(), value.into());
            }
            self
        }

        /// TurboMCP Extension: Add SIMD optimization hint
        ///
        /// Unique to TurboMCP - hints about SIMD capabilities for performance optimization.
        pub fn with_simd_optimization(mut self, level: &str) -> Self {
            if let Some(ref mut experimental) = self.experimental {
                experimental.insert(
                    "turbomcp_simd_level".to_string(),
                    serde_json::Value::String(level.to_string()),
                );
            }
            self
        }

        /// TurboMCP Extension: Add enterprise security metadata
        ///
        /// Unique to TurboMCP - metadata about security capabilities.
        pub fn with_enterprise_security(mut self, enabled: bool) -> Self {
            if let Some(ref mut experimental) = self.experimental {
                experimental.insert(
                    "turbomcp_enterprise_security".to_string(),
                    serde_json::Value::Bool(enabled),
                );
            }
            self
        }
    }

    // ========================================================================
    // CLIENT CAPABILITIES BUILDER - TYPE-STATE SYSTEM
    // ========================================================================

    /// Type-state for ClientCapabilitiesBuilder
    ///
    /// Each const generic represents whether a capability is enabled:
    /// - EXPERIMENTAL: Experimental capabilities
    /// - ROOTS: Roots capabilities
    /// - SAMPLING: Sampling capabilities
    /// - ELICITATION: Elicitation capabilities
    #[derive(Debug, Clone)]
    pub struct ClientCapabilitiesBuilderState<
        const EXPERIMENTAL: bool = false,
        const ROOTS: bool = false,
        const SAMPLING: bool = false,
        const ELICITATION: bool = false,
    >;

    /// Const-generic ClientCapabilities builder with compile-time validation
    ///
    /// This builder ensures that capability-specific methods are only available
    /// when the corresponding capability is enabled, providing compile-time safety
    /// with comprehensive compile-time validation.
    #[derive(Debug, Clone)]
    pub struct ClientCapabilitiesBuilder<S = ClientCapabilitiesBuilderState> {
        experimental: Option<HashMap<String, serde_json::Value>>,
        roots: Option<RootsCapabilities>,
        sampling: Option<SamplingCapabilities>,
        elicitation: Option<ElicitationCapabilities>,

        // TurboMCP Extensions
        negotiator: Option<super::CapabilityNegotiator>,
        strict_validation: bool,

        _state: PhantomData<S>,
    }

    impl ClientCapabilities {
        /// Create a new ClientCapabilities builder with type-state validation
        ///
        /// Returns a builder that ensures capabilities are configured correctly
        /// at compile time, preventing runtime configuration errors.
        ///
        /// By default, all capabilities are enabled (opt-out model).
        pub fn builder()
        -> ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<true, true, true, true>>
        {
            ClientCapabilitiesBuilder::new()
        }
    }

    impl Default for ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<true, true, true, true>> {
        fn default() -> Self {
            Self::new()
        }
    }

    impl ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<true, true, true, true>> {
        /// Create a new ClientCapabilities builder with all capabilities enabled (opt-out model)
        ///
        /// By default, all capabilities are enabled. Use `.without_*()` methods to disable
        /// specific capabilities if needed.
        ///
        /// This design provides forward compatibility - when new MCP capabilities are added,
        /// existing clients automatically support them without code changes.
        ///
        /// For clients that need explicit opt-in behavior, use `ClientCapabilitiesBuilder::minimal()`.
        pub fn new() -> Self {
            Self {
                experimental: Some(HashMap::new()),
                roots: Some(RootsCapabilities::default()),
                sampling: Some(SamplingCapabilities),
                elicitation: Some(ElicitationCapabilities::default()),
                negotiator: None,
                strict_validation: false,
                _state: PhantomData,
            }
        }
    }

    impl ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<false, false, false, false>> {
        /// Create a minimal ClientCapabilities builder with no capabilities enabled (opt-in model)
        ///
        /// Starts with all capabilities disabled. Use `.enable_*()` methods to opt-in
        /// to specific capabilities.
        ///
        /// Most users should use `ClientCapabilitiesBuilder::new()` instead, which provides
        /// an opt-out model with all capabilities enabled by default.
        ///
        /// Use this constructor when:
        /// - You need explicit control over which capabilities are enabled
        /// - You're building a restricted client (embedded, CLI-only, etc.)
        /// - You want to minimize the attack surface
        pub fn minimal() -> Self {
            Self {
                experimental: None,
                roots: None,
                sampling: None,
                elicitation: None,
                negotiator: None,
                strict_validation: false,
                _state: PhantomData,
            }
        }
    }

    // Generic implementation for all states
    impl<S> ClientCapabilitiesBuilder<S> {
        /// Build the final ClientCapabilities
        ///
        /// Consumes the builder and returns the configured ClientCapabilities.
        /// All compile-time validations have been enforced during building.
        pub fn build(self) -> ClientCapabilities {
            ClientCapabilities {
                experimental: self.experimental,
                roots: self.roots,
                sampling: self.sampling,
                elicitation: self.elicitation,
            }
        }

        /// TurboMCP Extension: Enable strict validation mode
        ///
        /// When enabled, the builder will perform additional runtime validations
        /// on top of the compile-time guarantees.
        pub fn with_strict_validation(mut self) -> Self {
            self.strict_validation = true;
            self
        }

        /// TurboMCP Extension: Set capability negotiator
        ///
        /// Integrates with TurboMCP's sophisticated capability negotiation system
        /// for advanced client-server capability matching.
        pub fn with_negotiator(mut self, negotiator: super::CapabilityNegotiator) -> Self {
            self.negotiator = Some(negotiator);
            self
        }

        /// TurboMCP Extension: Validate capability configuration
        ///
        /// Performs additional runtime validation to ensure the capability
        /// configuration makes sense in the current context.
        pub fn validate(&self) -> Result<(), String> {
            if self.strict_validation {
                // Validate experimental capabilities if present
                if let Some(ref experimental) = self.experimental {
                    for key in experimental.keys() {
                        if key.starts_with("turbomcp_") {
                            // Allow all TurboMCP experimental features
                            // Removed validation for deprecated methods:
                            // - turbomcp_llm_provider (removed)
                            // - turbomcp_ui_capabilities (removed)
                        }
                    }
                }
            }
            Ok(())
        }

        /// Get a summary of enabled capabilities
        ///
        /// Returns a human-readable summary of which capabilities are enabled.
        pub fn summary(&self) -> String {
            let mut capabilities = Vec::new();
            if self.experimental.is_some() {
                capabilities.push("experimental");
            }
            if self.roots.is_some() {
                capabilities.push("roots");
            }
            if self.sampling.is_some() {
                capabilities.push("sampling");
            }
            if self.elicitation.is_some() {
                capabilities.push("elicitation");
            }

            if capabilities.is_empty() {
                "No capabilities enabled".to_string()
            } else {
                format!("Enabled capabilities: {}", capabilities.join(", "))
            }
        }
    }

    // ========================================================================
    // CLIENT CAPABILITY ENABLEMENT METHODS
    // ========================================================================

    // Enable Experimental Capabilities
    impl<const R: bool, const S: bool, const E: bool>
        ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<false, R, S, E>>
    {
        /// Enable experimental capabilities
        ///
        /// Transitions the builder to a state where experimental capability methods
        /// become available at compile time.
        pub fn enable_experimental(
            self,
        ) -> ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<true, R, S, E>> {
            ClientCapabilitiesBuilder {
                experimental: Some(HashMap::new()),
                roots: self.roots,
                sampling: self.sampling,
                elicitation: self.elicitation,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }

        /// Enable experimental capabilities with specific values
        pub fn enable_experimental_with(
            self,
            experimental: HashMap<String, serde_json::Value>,
        ) -> ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<true, R, S, E>> {
            ClientCapabilitiesBuilder {
                experimental: Some(experimental),
                roots: self.roots,
                sampling: self.sampling,
                elicitation: self.elicitation,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Enable Roots Capabilities
    impl<const X: bool, const S: bool, const E: bool>
        ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, false, S, E>>
    {
        /// Enable roots capabilities
        pub fn enable_roots(
            self,
        ) -> ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, true, S, E>> {
            ClientCapabilitiesBuilder {
                experimental: self.experimental,
                roots: Some(RootsCapabilities { list_changed: None }),
                sampling: self.sampling,
                elicitation: self.elicitation,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Enable Sampling Capabilities
    impl<const X: bool, const R: bool, const E: bool>
        ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, R, false, E>>
    {
        /// Enable sampling capabilities
        pub fn enable_sampling(
            self,
        ) -> ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, R, true, E>> {
            ClientCapabilitiesBuilder {
                experimental: self.experimental,
                roots: self.roots,
                sampling: Some(SamplingCapabilities),
                elicitation: self.elicitation,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Enable Elicitation Capabilities
    impl<const X: bool, const R: bool, const S: bool>
        ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, R, S, false>>
    {
        /// Enable elicitation capabilities
        pub fn enable_elicitation(
            self,
        ) -> ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, R, S, true>> {
            ClientCapabilitiesBuilder {
                experimental: self.experimental,
                roots: self.roots,
                sampling: self.sampling,
                elicitation: Some(ElicitationCapabilities::default()),
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }

        /// Enable elicitation with schema validation
        pub fn enable_elicitation_with_schema_validation(
            self,
        ) -> ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, R, S, true>> {
            ClientCapabilitiesBuilder {
                experimental: self.experimental,
                roots: self.roots,
                sampling: self.sampling,
                elicitation: Some(ElicitationCapabilities::default().with_schema_validation()),
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // ========================================================================
    // CLIENT SUB-CAPABILITY METHODS
    // ========================================================================

    // Roots sub-capabilities (only available when ROOTS = true)
    impl<const X: bool, const S: bool, const E: bool>
        ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, true, S, E>>
    {
        /// Enable roots list changed notifications
        ///
        /// This method is only available when roots capabilities are enabled,
        /// providing compile-time safety.
        pub fn enable_roots_list_changed(mut self) -> Self {
            if let Some(ref mut roots) = self.roots {
                roots.list_changed = Some(true);
            }
            self
        }
    }

    // ========================================================================
    // CLIENT CAPABILITY DISABLEMENT METHODS (OPT-OUT API)
    // ========================================================================

    // Disable Experimental Capabilities
    impl<const R: bool, const S: bool, const E: bool>
        ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<true, R, S, E>>
    {
        /// Disable experimental capabilities (opt-out)
        ///
        /// Transitions the builder to a state where experimental capability methods
        /// are no longer available at compile time.
        pub fn without_experimental(
            self,
        ) -> ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<false, R, S, E>> {
            ClientCapabilitiesBuilder {
                experimental: None,
                roots: self.roots,
                sampling: self.sampling,
                elicitation: self.elicitation,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Disable Roots Capabilities
    impl<const X: bool, const S: bool, const E: bool>
        ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, true, S, E>>
    {
        /// Disable roots capabilities (opt-out)
        ///
        /// Transitions the builder to a state where roots capability methods
        /// are no longer available at compile time.
        pub fn without_roots(
            self,
        ) -> ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, false, S, E>> {
            ClientCapabilitiesBuilder {
                experimental: self.experimental,
                roots: None,
                sampling: self.sampling,
                elicitation: self.elicitation,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Disable Sampling Capabilities
    impl<const X: bool, const R: bool, const E: bool>
        ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, R, true, E>>
    {
        /// Disable sampling capabilities (opt-out)
        ///
        /// Transitions the builder to a state where sampling capability methods
        /// are no longer available at compile time.
        pub fn without_sampling(
            self,
        ) -> ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, R, false, E>> {
            ClientCapabilitiesBuilder {
                experimental: self.experimental,
                roots: self.roots,
                sampling: None,
                elicitation: self.elicitation,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Disable Elicitation Capabilities
    impl<const X: bool, const R: bool, const S: bool>
        ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, R, S, true>>
    {
        /// Disable elicitation capabilities (opt-out)
        ///
        /// Transitions the builder to a state where elicitation capability methods
        /// are no longer available at compile time.
        pub fn without_elicitation(
            self,
        ) -> ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<X, R, S, false>> {
            ClientCapabilitiesBuilder {
                experimental: self.experimental,
                roots: self.roots,
                sampling: self.sampling,
                elicitation: None,
                negotiator: self.negotiator,
                strict_validation: self.strict_validation,
                _state: PhantomData,
            }
        }
    }

    // Experimental sub-capabilities (only available when EXPERIMENTAL = true)
    impl<const R: bool, const S: bool, const E: bool>
        ClientCapabilitiesBuilder<ClientCapabilitiesBuilderState<true, R, S, E>>
    {
        /// Add experimental capability
        ///
        /// This method is only available when experimental capabilities are enabled.
        pub fn add_experimental_capability<K, V>(mut self, key: K, value: V) -> Self
        where
            K: Into<String>,
            V: Into<serde_json::Value>,
        {
            if let Some(ref mut experimental) = self.experimental {
                experimental.insert(key.into(), value.into());
            }
            self
        }
    }

    #[cfg(test)]
    mod type_state_tests {
        use super::*;

        #[test]
        fn test_server_capabilities_builder_type_state() {
            // Test basic builder construction
            let builder = ServerCapabilities::builder();
            assert!(format!("{:?}", builder).contains("ServerCapabilitiesBuilder"));

            // Test enabling capabilities changes the type
            let builder_with_tools = builder.enable_tools();

            // This should compile - enable_tool_list_changed is available when tools are enabled
            let _final_builder = builder_with_tools.enable_tool_list_changed();

            // Test building a server with multiple capabilities
            let full_capabilities = ServerCapabilitiesBuilder::new()
                .enable_experimental()
                .enable_logging()
                .enable_completions()
                .enable_prompts()
                .enable_resources()
                .enable_tools()
                .enable_tool_list_changed()
                .enable_prompts_list_changed()
                .enable_resources_list_changed()
                .enable_resources_subscribe()
                .build();

            assert!(full_capabilities.experimental.is_some());
            assert!(full_capabilities.logging.is_some());
            assert!(full_capabilities.completions.is_some());
            assert!(full_capabilities.prompts.is_some());
            assert!(full_capabilities.resources.is_some());
            assert!(full_capabilities.tools.is_some());

            // Validate sub-capabilities are set correctly
            if let Some(ref tools) = full_capabilities.tools {
                assert_eq!(tools.list_changed, Some(true));
            }

            if let Some(ref resources) = full_capabilities.resources {
                assert_eq!(resources.list_changed, Some(true));
                assert_eq!(resources.subscribe, Some(true));
            }
        }

        #[test]
        fn test_client_capabilities_builder_type_state() {
            // Test basic builder construction - now returns all-enabled state
            let builder = ClientCapabilities::builder();
            assert!(format!("{:?}", builder).contains("ClientCapabilitiesBuilder"));

            // Test opt-out: disabling capabilities changes the type
            let builder_without_roots = builder.without_roots();

            // After disabling roots, we can re-enable it
            let builder_with_roots = builder_without_roots.enable_roots();

            // This should compile - enable_roots_list_changed is available when roots are enabled
            let _final_builder = builder_with_roots.enable_roots_list_changed();

            // Test the opt-out default (new() has everything enabled)
            let full_capabilities = ClientCapabilitiesBuilder::new()
                .enable_roots_list_changed()
                .build();

            assert!(full_capabilities.experimental.is_some());
            assert!(full_capabilities.roots.is_some());
            assert!(full_capabilities.sampling.is_some());
            assert!(full_capabilities.elicitation.is_some());

            // Validate sub-capabilities are set correctly
            if let Some(ref roots) = full_capabilities.roots {
                assert_eq!(roots.list_changed, Some(true));
            }
        }

        #[test]
        fn test_opt_out_model() {
            // Default builder has everything enabled
            let caps = ClientCapabilitiesBuilder::new().build();
            assert!(caps.experimental.is_some());
            assert!(caps.roots.is_some());
            assert!(caps.sampling.is_some());
            assert!(caps.elicitation.is_some());

            // Can selectively disable capabilities
            let caps_without_elicitation = ClientCapabilitiesBuilder::new()
                .without_elicitation()
                .build();

            assert!(caps_without_elicitation.experimental.is_some());
            assert!(caps_without_elicitation.roots.is_some());
            assert!(caps_without_elicitation.sampling.is_some());
            assert!(caps_without_elicitation.elicitation.is_none());

            // Can disable multiple capabilities
            let minimal_caps = ClientCapabilitiesBuilder::new()
                .without_experimental()
                .without_roots()
                .without_elicitation()
                .build();

            assert!(minimal_caps.experimental.is_none());
            assert!(minimal_caps.roots.is_none());
            assert!(minimal_caps.sampling.is_some());
            assert!(minimal_caps.elicitation.is_none());
        }

        #[test]
        fn test_opt_in_with_minimal() {
            // Minimal builder starts with nothing enabled
            let caps = ClientCapabilitiesBuilder::minimal().build();
            assert!(caps.experimental.is_none());
            assert!(caps.roots.is_none());
            assert!(caps.sampling.is_none());
            assert!(caps.elicitation.is_none());

            // Can selectively enable capabilities
            let caps_with_sampling = ClientCapabilitiesBuilder::minimal()
                .enable_sampling()
                .build();

            assert!(caps_with_sampling.experimental.is_none());
            assert!(caps_with_sampling.roots.is_none());
            assert!(caps_with_sampling.sampling.is_some());
            assert!(caps_with_sampling.elicitation.is_none());

            // Can enable multiple capabilities
            let full_caps = ClientCapabilitiesBuilder::minimal()
                .enable_experimental()
                .enable_roots()
                .enable_sampling()
                .enable_elicitation()
                .build();

            assert!(full_caps.experimental.is_some());
            assert!(full_caps.roots.is_some());
            assert!(full_caps.sampling.is_some());
            assert!(full_caps.elicitation.is_some());
        }

        #[test]
        fn test_turbomcp_extensions() {
            // Test TurboMCP-specific server extensions (still use enable for servers)
            let server_caps = ServerCapabilities::builder()
                .enable_experimental()
                .with_simd_optimization("avx2")
                .with_enterprise_security(true)
                .build();

            if let Some(ref experimental) = server_caps.experimental {
                assert!(experimental.contains_key("turbomcp_simd_level"));
                assert!(experimental.contains_key("turbomcp_enterprise_security"));
                assert_eq!(
                    experimental.get("turbomcp_simd_level").unwrap().as_str(),
                    Some("avx2")
                );
                assert_eq!(
                    experimental
                        .get("turbomcp_enterprise_security")
                        .unwrap()
                        .as_bool(),
                    Some(true)
                );
            } else {
                panic!("Expected experimental capabilities to be set");
            }

            // Test client experimental capabilities using add_experimental_capability
            let client_caps = ClientCapabilities::builder()
                .add_experimental_capability("custom_feature", true)
                .build();

            if let Some(ref experimental) = client_caps.experimental {
                assert!(experimental.contains_key("custom_feature"));
            } else {
                panic!("Expected experimental capabilities to be set");
            }
        }

        #[test]
        fn test_minimal_constructor() {
            // Test client minimal() constructor - starts with nothing enabled
            let minimal_client_caps = ClientCapabilitiesBuilder::minimal().build();
            assert!(minimal_client_caps.experimental.is_none());
            assert!(minimal_client_caps.roots.is_none());
            assert!(minimal_client_caps.sampling.is_none());
            assert!(minimal_client_caps.elicitation.is_none());

            // Test that we can selectively enable from minimal
            let sampling_only = ClientCapabilitiesBuilder::minimal()
                .enable_sampling()
                .build();
            assert!(sampling_only.experimental.is_none());
            assert!(sampling_only.roots.is_none());
            assert!(sampling_only.sampling.is_some());
            assert!(sampling_only.elicitation.is_none());

            // Test enabling multiple capabilities from minimal
            let sampling_focused_client = ClientCapabilitiesBuilder::minimal()
                .enable_experimental()
                .enable_sampling()
                .build();
            assert!(sampling_focused_client.experimental.is_some());
            assert!(sampling_focused_client.roots.is_none());
            assert!(sampling_focused_client.sampling.is_some());
            assert!(sampling_focused_client.elicitation.is_none());
        }

        #[test]
        fn test_builder_default_implementations() {
            // Test that default implementations work - now defaults to all-enabled
            let default_server_builder = ServerCapabilitiesBuilder::default();
            let server_caps = default_server_builder.build();
            assert!(server_caps.tools.is_none()); // Server builder still opts in

            let default_client_builder = ClientCapabilitiesBuilder::default();
            let client_caps = default_client_builder.build();
            // Client default now has everything enabled
            assert!(client_caps.experimental.is_some());
            assert!(client_caps.roots.is_some());
            assert!(client_caps.sampling.is_some());
            assert!(client_caps.elicitation.is_some());
        }

        #[test]
        fn test_builder_chaining() {
            // Test that builder method chaining works correctly
            let server_caps = ServerCapabilities::builder()
                .enable_experimental()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .enable_tool_list_changed()
                .enable_prompts_list_changed()
                .enable_resources_list_changed()
                .enable_resources_subscribe()
                .add_experimental_capability("custom_feature", true)
                .build();

            assert!(server_caps.experimental.is_some());
            assert!(server_caps.tools.is_some());
            assert!(server_caps.prompts.is_some());
            assert!(server_caps.resources.is_some());

            // Verify custom experimental capability
            if let Some(ref experimental) = server_caps.experimental {
                assert!(experimental.contains_key("custom_feature"));
            }
        }

        #[test]
        fn test_with_negotiator_integration() {
            // Test TurboMCP capability negotiator integration
            let negotiator = super::super::CapabilityNegotiator::default();

            let server_caps = ServerCapabilities::builder()
                .enable_tools()
                .with_negotiator(negotiator.clone())
                .with_strict_validation()
                .build();

            assert!(server_caps.tools.is_some());
            // Note: negotiator and strict_validation are internal to the builder
            // and don't appear in the final ServerCapabilities struct
        }

        #[test]
        fn test_builder_validation_methods() {
            // Test server builder validation
            let server_builder = ServerCapabilities::builder()
                .enable_experimental()
                .enable_tools()
                .with_simd_optimization("avx2")
                .with_enterprise_security(true)
                .with_strict_validation();

            // Validation should pass for well-configured builder
            assert!(server_builder.validate().is_ok());

            // Test summary method
            let summary = server_builder.summary();
            assert!(summary.contains("experimental"));
            assert!(summary.contains("tools"));

            // Test client builder validation - now uses new() which starts all-enabled
            let client_builder = ClientCapabilities::builder()
                .add_experimental_capability("custom_feature", true)
                .with_strict_validation();

            // Validation should pass for well-configured builder
            assert!(client_builder.validate().is_ok());

            // Test summary method
            let summary = client_builder.summary();
            assert!(summary.contains("experimental"));
            assert!(summary.contains("sampling"));
        }

        #[test]
        fn test_builder_validation_errors() {
            // Test server validation errors
            let server_builder = ServerCapabilities::builder()
                .enable_experimental()
                .with_strict_validation();

            // Should fail validation - no actual capabilities enabled
            assert!(server_builder.validate().is_err());
            let error = server_builder.validate().unwrap_err();
            assert!(error.contains("at least one capability"));

            // Test invalid SIMD level
            let invalid_server_builder = ServerCapabilities::builder()
                .enable_experimental()
                .enable_tools()
                .add_experimental_capability("turbomcp_simd_level", "invalid_level")
                .with_strict_validation();

            assert!(invalid_server_builder.validate().is_err());
            let error = invalid_server_builder.validate().unwrap_err();
            assert!(error.contains("Invalid SIMD level"));

            // Client validation should always pass now (removed strict validation for UI/LLM)
            let client_builder = ClientCapabilities::builder()
                .add_experimental_capability("custom_feature", true)
                .with_strict_validation();

            assert!(client_builder.validate().is_ok());
        }

        #[test]
        fn test_builder_clone_support() {
            // Test that builders can be cloned
            let original_server_builder = ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts();

            let cloned_server_builder = original_server_builder.clone();

            // Both should produce equivalent capabilities
            let original_caps = original_server_builder.build();
            let cloned_caps = cloned_server_builder.build();

            assert_eq!(original_caps.tools.is_some(), cloned_caps.tools.is_some());
            assert_eq!(
                original_caps.prompts.is_some(),
                cloned_caps.prompts.is_some()
            );

            // Test client builder clone - use without_*() since default is all-enabled
            let original_client_builder = ClientCapabilities::builder()
                .without_experimental()
                .without_roots();

            let cloned_client_builder = original_client_builder.clone();

            let original_caps = original_client_builder.build();
            let cloned_caps = cloned_client_builder.build();

            assert_eq!(
                original_caps.experimental.is_some(),
                cloned_caps.experimental.is_some()
            );
            assert_eq!(original_caps.roots.is_some(), cloned_caps.roots.is_some());
            assert_eq!(
                original_caps.sampling.is_some(),
                cloned_caps.sampling.is_some()
            );
            assert_eq!(
                original_caps.elicitation.is_some(),
                cloned_caps.elicitation.is_some()
            );
        }
    }
}
