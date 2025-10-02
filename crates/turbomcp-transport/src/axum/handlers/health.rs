//! Health check handler for service monitoring

use axum::{extract::State, Json};

use crate::axum::service::McpAppState;

/// Health check handler - returns service health status
pub async fn health_handler(State(app_state): State<McpAppState>) -> Json<serde_json::Value> {
    let session_count = app_state.session_manager.active_session_count();

    Json(serde_json::json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "sessions": {
            "active": session_count,
            "max": app_state.config.max_connections
        },
        "version": env!("CARGO_PKG_VERSION")
    }))
}