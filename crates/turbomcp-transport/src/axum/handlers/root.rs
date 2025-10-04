//! Root endpoint handler for TurboMCP server information

use axum::{Json, response::IntoResponse};

/// Root handler - provides basic server information
pub async fn root_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "name": "TurboMCP Server",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "High-performance Model Context Protocol server",
        "endpoints": {
            "mcp": "/mcp",
            "capabilities": "/mcp/capabilities",
            "sse": "/mcp/sse",
            "websocket": "/mcp/ws",
            "health": "/mcp/health",
            "metrics": "/mcp/metrics"
        }
    }))
}
