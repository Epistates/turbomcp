//! RFC 9728 Protected Resource Metadata Router
//!
//! MCP servers MUST implement the OAuth 2.0 Protected Resource Metadata endpoint
//! per MCP specification (2025-06-18) and RFC 9728.
//!
//! This module provides a router that serves `/.well-known/oauth-protected-resource`
//! to enable client discovery of authorization servers and resource requirements.
//!
//! # Example
//!
//! ```rust,no_run
//! use turbomcp_transport::axum::auth_router;
//! use turbomcp_auth::server::ProtectedResourceMetadataBuilder;
//! use axum::Router;
//!
//! # async fn example() {
//! // Build metadata for your MCP server
//! let metadata = ProtectedResourceMetadataBuilder::new(
//!     "https://api.example.com".to_string(),
//!     "https://auth.example.com".to_string()
//! )
//! .with_scopes(vec![
//!     "mcp:tools".to_string(),
//!     "mcp:resources".to_string(),
//! ])
//! .build_struct();
//!
//! // Create the metadata router
//! let auth_router = auth_router::protected_resource_metadata_router(metadata);
//!
//! // Merge with your main router
//! let app = Router::new()
//!     .merge(auth_router)
//!     /* your other routes */;
//! # }
//! ```

use axum::{
    Router,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::get,
};
use serde_json::json;
use std::sync::Arc;
use turbomcp_auth::{BearerTokenMethod, ProtectedResourceMetadata};

/// State for protected resource metadata endpoint
#[derive(Clone)]
struct ProtectedResourceState {
    metadata: Arc<ProtectedResourceMetadata>,
}

/// Create a router for RFC 9728 Protected Resource Metadata
///
/// This router serves the `/.well-known/oauth-protected-resource` endpoint
/// required by MCP specification.
///
/// # MCP Specification Compliance
///
/// Per MCP spec (2025-06-18):
/// > MCP servers MUST implement the OAuth 2.0 Protected Resource Metadata (RFC9728)
/// > specification to indicate the locations of authorization servers.
///
/// # Arguments
///
/// * `metadata` - The protected resource metadata configuration
///
/// # Returns
///
/// A router that can be merged into your main Axum router
///
/// # Example
///
/// ```rust,no_run
/// use turbomcp_transport::axum::auth_router;
/// use turbomcp_auth::server::ProtectedResourceMetadataBuilder;
///
/// # async fn example() {
/// let metadata = ProtectedResourceMetadataBuilder::new(
///     "https://mcp.example.com".to_string(),
///     "https://auth.example.com".to_string()
/// )
/// .with_scopes(vec!["mcp:tools".to_string()])
/// .build_struct();
///
/// let router = auth_router::protected_resource_metadata_router(metadata);
/// # }
/// ```
pub fn protected_resource_metadata_router(metadata: ProtectedResourceMetadata) -> Router {
    let state = ProtectedResourceState {
        metadata: Arc::new(metadata),
    };

    Router::new()
        .route(
            "/.well-known/oauth-protected-resource",
            get(serve_protected_resource_metadata),
        )
        .with_state(state)
}

/// Handler for `/.well-known/oauth-protected-resource` endpoint
///
/// Returns RFC 9728 compliant JSON metadata describing the protected resource.
async fn serve_protected_resource_metadata(
    State(state): State<ProtectedResourceState>,
) -> impl IntoResponse {
    // Serialize to JSON per RFC 9728 Section 3
    let metadata_json = json!({
        "resource": state.metadata.resource,
        "authorization_servers": vec![state.metadata.authorization_server.clone()],
        "scopes_supported": state.metadata.scopes_supported,
        "bearer_methods_supported": state.metadata.bearer_methods_supported
            .as_ref()
            .map(|methods| methods.iter().map(|m| match m {
                BearerTokenMethod::Header => "header",
                BearerTokenMethod::Query => "query",
                BearerTokenMethod::Body => "body",
            }).collect::<Vec<_>>()),
        "resource_documentation": state.metadata.resource_documentation,
    });

    (StatusCode::OK, Json(metadata_json))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt; // for `oneshot`
    use turbomcp_auth::server::ProtectedResourceMetadataBuilder;

    #[tokio::test]
    async fn test_metadata_endpoint_basic() {
        let metadata = ProtectedResourceMetadataBuilder::new(
            "https://api.example.com".to_string(),
            "https://auth.example.com".to_string(),
        )
        .with_scopes(vec!["mcp:tools".to_string()])
        .build_struct();

        let app = protected_resource_metadata_router(metadata);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/.well-known/oauth-protected-resource")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["resource"], "https://api.example.com");
        assert_eq!(json["authorization_servers"][0], "https://auth.example.com");
        assert_eq!(json["scopes_supported"][0], "mcp:tools");
    }

    #[tokio::test]
    async fn test_metadata_endpoint_with_documentation() {
        let metadata = ProtectedResourceMetadataBuilder::new(
            "https://mcp.example.com".to_string(),
            "https://auth.example.com".to_string(),
        )
        .with_scopes(vec!["mcp:tools".to_string(), "mcp:resources".to_string()])
        .with_documentation("https://docs.example.com".to_string())
        .build_struct();

        let app = protected_resource_metadata_router(metadata);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/.well-known/oauth-protected-resource")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        assert_eq!(json["resource_documentation"], "https://docs.example.com");
        assert_eq!(json["scopes_supported"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn test_metadata_endpoint_bearer_methods() {
        let metadata = ProtectedResourceMetadataBuilder::new(
            "https://api.example.com".to_string(),
            "https://auth.example.com".to_string(),
        )
        .with_bearer_methods(vec![BearerTokenMethod::Header])
        .build_struct();

        let app = protected_resource_metadata_router(metadata);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/.well-known/oauth-protected-resource")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let methods = json["bearer_methods_supported"].as_array().unwrap();
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0], "header");
    }
}
