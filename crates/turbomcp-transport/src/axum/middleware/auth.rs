//! Authentication middleware for API keys and JWT validation

use axum::{
    extract::State,
    http::StatusCode,
    middleware::Next,
    response::Response,
};

use crate::axum::config::AuthConfig;

/// Authentication middleware - validates tokens and API keys
///
/// This is a basic implementation that provides hooks for API key and JWT
/// authentication. For production use, integrate with your authentication
/// system (JWT validation, OAuth2, database lookups, etc.)
pub async fn authentication_middleware(
    State(auth_config): State<AuthConfig>,
    mut request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Check for API key authentication
    if let Some(api_key_header) = &auth_config.api_key_header {
        if let Some(provided_key) = request.headers().get(api_key_header) {
            // In production, validate against your API key store
            if provided_key
                .to_str()
                .map_err(|_| StatusCode::BAD_REQUEST)?
                .is_empty()
            {
                return Err(StatusCode::UNAUTHORIZED);
            }
            // Add authenticated context to request
            request.extensions_mut().insert("api_key_user".to_string());
        } else if auth_config.enabled {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    // Check for JWT authentication
    if let Some(_jwt_secret) = &auth_config.jwt_secret {
        if let Some(auth_header) = request.headers().get("Authorization") {
            let auth_str = auth_header.to_str().map_err(|_| StatusCode::BAD_REQUEST)?;
            if let Some(token) = auth_str.strip_prefix("Bearer ") {
                // In production, validate JWT token here using the secret
                // Example: decode and validate token with your JWT library
                if token.is_empty() {
                    return Err(StatusCode::UNAUTHORIZED);
                }
                // Add authenticated user context to request
                request.extensions_mut().insert("jwt_user".to_string());
            } else {
                return Err(StatusCode::UNAUTHORIZED);
            }
        } else if auth_config.enabled {
            return Err(StatusCode::UNAUTHORIZED);
        }
    }

    // Continue processing
    Ok(next.run(request).await)
}