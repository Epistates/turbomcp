//! Handler traits for MCP operations
//!
//! This module contains all the handler traits that define the interface
//! for processing different types of MCP requests and operations.

pub mod completion;
pub mod elicitation;
pub mod logging;
pub mod ping;
pub mod prompt;
pub mod resource;
pub mod resource_template;
pub mod sampling;
pub mod tool;

// Re-export all traits for backwards compatibility
pub use completion::CompletionHandler;
pub use elicitation::ElicitationHandler;
pub use logging::LoggingHandler;
pub use ping::PingHandler;
pub use prompt::PromptHandler;
pub use resource::ResourceHandler;
pub use resource_template::ResourceTemplateHandler;
pub use sampling::SamplingHandler;
pub use tool::ToolHandler;
