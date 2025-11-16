//! Basic MCP middleware for session management

use axum::{http::StatusCode, middleware::Next, response::Response};
use tracing::trace;

use crate::axum::handlers::SessionInfo;

/// Basic MCP middleware for session management
///
/// This middleware ensures every request has an associated session, creating
/// one if it doesn't exist. Sessions are used for tracking client state
/// and enabling bidirectional communication.
pub async fn mcp_middleware(
    mut request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Create or retrieve session
    let session = match request.extensions().get::<SessionInfo>() {
        Some(session) => session.clone(),
        None => {
            // Create new session - in production, you might want to extract this
            // from headers or query parameters
            let session = SessionInfo::new();
            request.extensions_mut().insert(session.clone());
            session
        }
    };

    trace!("Processing request for session: {}", session.id);

    // Continue processing
    let response = next.run(request).await;

    trace!("Request completed for session: {}", session.id);
    Ok(response)
}
