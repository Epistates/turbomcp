//! Capabilities endpoint handler for MCP server capabilities

use axum::{extract::State, Json};

use crate::axum::service::McpAppState;

/// Capabilities handler - returns MCP server capabilities
pub async fn capabilities_handler(State(app_state): State<McpAppState>) -> Json<serde_json::Value> {
    Json(app_state.get_capabilities())
}