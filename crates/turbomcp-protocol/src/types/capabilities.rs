//! MCP capability negotiation types.
//!
//! Canonical definitions live in [`turbomcp_types`]; this module re-exports
//! them so existing `turbomcp_protocol::types::capabilities::*` imports keep
//! working.
//!
//! # Capability Types
//!
//! - [`ClientCapabilities`] — client-side capabilities.
//! - [`ServerCapabilities`] — server-side capabilities.
//! - Feature-specific capability structures for each MCP feature.

pub use turbomcp_types::{
    ClientCapabilities, ClientTasksCapabilities, ClientTasksRequestsCapabilities,
    CompletionCapabilities, ElicitationCapabilities, ElicitationFormCapabilities,
    ElicitationUrlCapabilities, LoggingCapabilities, PromptsCapabilities, ResourcesCapabilities,
    RootsCapabilities, SamplingCapabilities, ServerCapabilities, ServerTasksCapabilities,
    ServerTasksRequestsCapabilities, TasksCancelCapabilities, TasksElicitationCapabilities,
    TasksElicitationCreateCapabilities, TasksListCapabilities, TasksSamplingCapabilities,
    TasksSamplingCreateMessageCapabilities, TasksToolsCallCapabilities, TasksToolsCapabilities,
    ToolsCapabilities,
};
