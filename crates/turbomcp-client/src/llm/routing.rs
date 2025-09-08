//! Request routing and provider selection

use crate::llm::core::{LLMError, LLMRequest, LLMResult};
use serde::{Deserialize, Serialize};

/// Strategies for routing requests to providers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutingStrategy {
    /// Use specific provider
    Specific { provider: String },
    /// Round-robin between providers
    RoundRobin { providers: Vec<String> },
    /// Route based on request properties
    RuleBased { rules: Vec<RouteRule> },
    /// Route to least loaded provider
    LoadBalanced { providers: Vec<String> },
}

/// Rule for routing requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRule {
    /// Condition to match
    pub condition: RouteCondition,
    /// Provider to route to
    pub provider: String,
    /// Rule priority (higher = more priority)
    pub priority: i32,
}

/// Condition for route matching
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RouteCondition {
    /// Match specific model
    ModelEquals { model: String },
    /// Match model pattern
    ModelContains { pattern: String },
    /// Match request metadata
    MetadataEquals { key: String, value: String },
    /// Match streaming requests
    IsStreaming,
    /// Match requests with images
    HasImages,
    /// Always match
    Always,
}

/// Request router for intelligent provider selection
#[derive(Debug)]
pub struct RequestRouter {
    strategy: RoutingStrategy,
    round_robin_index: std::sync::Mutex<usize>,
}

impl RequestRouter {
    /// Create a new request router
    pub fn new(strategy: RoutingStrategy) -> Self {
        Self {
            strategy,
            round_robin_index: std::sync::Mutex::new(0),
        }
    }

    /// Route a request to determine which provider to use
    pub fn route_request(&self, request: &LLMRequest) -> LLMResult<String> {
        match &self.strategy {
            RoutingStrategy::Specific { provider } => Ok(provider.clone()),

            RoutingStrategy::RoundRobin { providers } => {
                if providers.is_empty() {
                    return Err(LLMError::configuration(
                        "No providers configured for round-robin",
                    ));
                }

                let mut index = self.round_robin_index.lock().unwrap();
                let provider = providers[*index % providers.len()].clone();
                *index = (*index + 1) % providers.len();
                Ok(provider)
            }

            RoutingStrategy::RuleBased { rules } => {
                let mut matching_rules: Vec<_> = rules
                    .iter()
                    .filter(|rule| self.matches_condition(&rule.condition, request))
                    .collect();

                // Sort by priority (descending)
                matching_rules.sort_by(|a, b| b.priority.cmp(&a.priority));

                matching_rules
                    .first()
                    .map(|rule| rule.provider.clone())
                    .ok_or_else(|| LLMError::configuration("No routing rules matched the request"))
            }

            RoutingStrategy::LoadBalanced { providers } => {
                if providers.is_empty() {
                    return Err(LLMError::configuration(
                        "No providers configured for load balancing",
                    ));
                }

                // TODO: Implement actual load balancing
                // For now, just use round-robin
                let mut index = self.round_robin_index.lock().unwrap();
                let provider = providers[*index % providers.len()].clone();
                *index = (*index + 1) % providers.len();
                Ok(provider)
            }
        }
    }

    fn matches_condition(&self, condition: &RouteCondition, request: &LLMRequest) -> bool {
        match condition {
            RouteCondition::ModelEquals { model } => request.model == *model,

            RouteCondition::ModelContains { pattern } => request.model.contains(pattern),

            RouteCondition::MetadataEquals { key, value } => request
                .metadata
                .get(key)
                .and_then(|v| v.as_str())
                .map(|v| v == value)
                .unwrap_or(false),

            RouteCondition::IsStreaming => request.stream,

            RouteCondition::HasImages => request.messages.iter().any(|msg| msg.content.is_image()),

            RouteCondition::Always => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::core::LLMMessage;

    #[test]
    fn test_specific_routing() {
        let strategy = RoutingStrategy::Specific {
            provider: "openai".to_string(),
        };

        let router = RequestRouter::new(strategy);
        let request = LLMRequest::new("gpt-4", vec![LLMMessage::user("Hello")]);

        let result = router.route_request(&request).unwrap();
        assert_eq!(result, "openai");
    }

    #[test]
    fn test_round_robin_routing() {
        let strategy = RoutingStrategy::RoundRobin {
            providers: vec!["openai".to_string(), "anthropic".to_string()],
        };

        let router = RequestRouter::new(strategy);
        let request = LLMRequest::new("gpt-4", vec![LLMMessage::user("Hello")]);

        let result1 = router.route_request(&request).unwrap();
        let result2 = router.route_request(&request).unwrap();
        let result3 = router.route_request(&request).unwrap();

        assert_eq!(result1, "openai");
        assert_eq!(result2, "anthropic");
        assert_eq!(result3, "openai"); // Back to first
    }

    #[test]
    fn test_rule_based_routing() {
        let rules = vec![
            RouteRule {
                condition: RouteCondition::ModelContains {
                    pattern: "gpt".to_string(),
                },
                provider: "openai".to_string(),
                priority: 10,
            },
            RouteRule {
                condition: RouteCondition::ModelContains {
                    pattern: "claude".to_string(),
                },
                provider: "anthropic".to_string(),
                priority: 10,
            },
            RouteRule {
                condition: RouteCondition::Always,
                provider: "ollama".to_string(),
                priority: 1,
            },
        ];

        let strategy = RoutingStrategy::RuleBased { rules };
        let router = RequestRouter::new(strategy);

        // Test GPT model routing
        let gpt_request = LLMRequest::new("gpt-4", vec![LLMMessage::user("Hello")]);
        let result = router.route_request(&gpt_request).unwrap();
        assert_eq!(result, "openai");

        // Test Claude model routing
        let claude_request = LLMRequest::new("claude-3-sonnet", vec![LLMMessage::user("Hello")]);
        let result = router.route_request(&claude_request).unwrap();
        assert_eq!(result, "anthropic");

        // Test fallback routing
        let other_request = LLMRequest::new("llama2", vec![LLMMessage::user("Hello")]);
        let result = router.route_request(&other_request).unwrap();
        assert_eq!(result, "ollama");
    }

    #[test]
    fn test_streaming_condition() {
        let rules = vec![RouteRule {
            condition: RouteCondition::IsStreaming,
            provider: "streaming_provider".to_string(),
            priority: 10,
        }];

        let strategy = RoutingStrategy::RuleBased { rules };
        let router = RequestRouter::new(strategy);

        let streaming_request =
            LLMRequest::new("gpt-4", vec![LLMMessage::user("Hello")]).with_streaming(true);

        let result = router.route_request(&streaming_request).unwrap();
        assert_eq!(result, "streaming_provider");
    }
}
