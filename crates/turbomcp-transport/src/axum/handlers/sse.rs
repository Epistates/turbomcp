//! Server-Sent Events handler for real-time MCP communication

use std::convert::Infallible;

use axum::{
    extract::{Extension, Query, State},
    response::sse::{Event, KeepAlive, Sse},
};
use futures::Stream;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};

use crate::axum::service::McpAppState;
use crate::axum::types::SseQuery;
use crate::tower::SessionInfo;

/// Server-Sent Events handler for real-time communication
pub async fn sse_handler(
    State(app_state): State<McpAppState>,
    Query(_query): Query<SseQuery>,
    Extension(session): Extension<SessionInfo>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    info!("SSE connection established for session: {}", session.id);

    let mut receiver = app_state.sse_sender.subscribe();

    // Create event stream
    let stream = async_stream::stream! {
        // Send initial connection event
        yield Ok(Event::default()
            .event("connected")
            .data(serde_json::json!({
                "session_id": session.id,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }).to_string()));

        // Stream events from broadcast channel
        loop {
            match receiver.recv().await {
                Ok(message) => {
                    yield Ok(Event::default()
                        .event("message")
                        .data(message));
                }
                Err(broadcast::error::RecvError::Closed) => {
                    debug!("SSE broadcast channel closed");
                    break;
                }
                Err(broadcast::error::RecvError::Lagged(skipped)) => {
                    warn!("SSE client lagged, skipped {} messages", skipped);
                    yield Ok(Event::default()
                        .event("error")
                        .data(serde_json::json!({
                            "code": "LAGGED",
                            "message": format!("Skipped {} messages due to slow client", skipped)
                        }).to_string()));
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(app_state.config.sse_keep_alive))
}
