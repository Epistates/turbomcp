//! Completion handler trait for argument autocompletion

use async_trait::async_trait;
use serde_json::Value;
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{CompleteRequestParams, CompletionResponse};

use crate::ServerResult;

/// Completion handler trait for argument autocompletion
#[async_trait]
pub trait CompletionHandler: Send + Sync {
    /// Handle a completion request
    async fn handle(
        &self,
        request: CompleteRequestParams,
        ctx: RequestContext,
    ) -> ServerResult<CompletionResponse>;

    /// Get maximum number of completions to return
    fn max_completions(&self) -> usize {
        50
    }

    /// Check if completion is supported for the given reference
    fn supports_completion(&self, _reference: &str) -> bool {
        true
    }

    /// Get completion suggestions based on context
    async fn get_completions(
        &self,
        reference: &str,
        argument: Option<&str>,
        partial_value: Option<&str>,
        ctx: RequestContext,
    ) -> ServerResult<Vec<Value>>;

    /// Filter and rank completion options
    fn filter_completions(
        &self,
        completions: Vec<Value>,
        partial_value: Option<&str>,
    ) -> Vec<Value> {
        // Default implementation: simple prefix matching
        if let Some(partial) = partial_value {
            let partial_lower = partial.to_lowercase();
            completions
                .into_iter()
                .filter(|comp| {
                    if let Some(value) = comp.get("value").and_then(|v| v.as_str()) {
                        value.to_lowercase().starts_with(&partial_lower)
                    } else {
                        false
                    }
                })
                .take(self.max_completions())
                .collect()
        } else {
            completions
                .into_iter()
                .take(self.max_completions())
                .collect()
        }
    }
}
