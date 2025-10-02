//! Composite handler pattern for handling multiple types of requests

use crate::handlers::traits::*;

/// Composite handler that can handle multiple types of requests
pub trait CompositeHandler: Send + Sync {
    /// Get tool handler if this composite handles tools
    fn as_tool_handler(&self) -> Option<&dyn ToolHandler> {
        None
    }

    /// Get prompt handler if this composite handles prompts
    fn as_prompt_handler(&self) -> Option<&dyn PromptHandler> {
        None
    }

    /// Get resource handler if this composite handles resources
    fn as_resource_handler(&self) -> Option<&dyn ResourceHandler> {
        None
    }

    /// Get sampling handler if this composite handles sampling
    fn as_sampling_handler(&self) -> Option<&dyn SamplingHandler> {
        None
    }

    /// Get logging handler if this composite handles logging
    fn as_logging_handler(&self) -> Option<&dyn LoggingHandler> {
        None
    }

    /// Get elicitation handler if this composite handles elicitation
    fn as_elicitation_handler(&self) -> Option<&dyn ElicitationHandler> {
        None
    }

    /// Get completion handler if this composite handles completion
    fn as_completion_handler(&self) -> Option<&dyn CompletionHandler> {
        None
    }

    /// Get resource template handler if this composite handles resource templates
    fn as_resource_template_handler(&self) -> Option<&dyn ResourceTemplateHandler> {
        None
    }

    /// Get ping handler if this composite handles ping
    fn as_ping_handler(&self) -> Option<&dyn PingHandler> {
        None
    }
}
