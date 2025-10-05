//! Metrics handler for monitoring and observability

use axum::{Json, extract::State};

use crate::axum::service::McpAppState;

/// Metrics handler - returns detailed service metrics
pub async fn metrics_handler(State(app_state): State<McpAppState>) -> Json<serde_json::Value> {
    let sessions = app_state.session_manager.list_sessions().await;
    let total_sessions = sessions.len();
    let avg_duration = if total_sessions > 0 {
        sessions.iter().map(|s| s.duration().as_secs()).sum::<u64>() / total_sessions as u64
    } else {
        0
    };

    Json(serde_json::json!({
        "sessions": {
            "active": total_sessions,
            "max": app_state.config.max_connections,
            "average_duration_seconds": avg_duration
        },
        "server": {
            "uptime_seconds": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            "version": env!("CARGO_PKG_VERSION")
        }
    }))
}
