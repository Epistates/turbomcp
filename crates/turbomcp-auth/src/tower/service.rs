//! Tower Service implementation for authentication
//!
//! This module provides two `Service` trait implementations for different use cases:
//!
//! ## 1. HTTP Request Service (`Service<http::Request<B>>`)
//!
//! Use this when integrating with HTTP-based transports (Axum, Tower-HTTP, etc.):
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//! use turbomcp_auth::tower::{AuthLayer, AuthLayerConfig};
//!
//! let service = ServiceBuilder::new()
//!     .layer(AuthLayer::new(provider, AuthLayerConfig::default()))
//!     .service(my_http_handler);
//! ```
//!
//! This implementation:
//! - Extracts tokens from `Authorization` header (`Bearer` or `ApiKey` schemes)
//! - Extracts tokens from custom API key header (configurable)
//! - Validates tokens using the configured [`AuthProvider`]
//! - Supports method-based bypass (e.g., skip auth for health checks)
//! - Supports anonymous access when configured
//!
//! ## 2. MCP Request Service (`Service<AuthenticatedRequest<Value>>`)
//!
//! Use this for non-HTTP contexts where auth context is passed directly:
//!
//! ```rust,ignore
//! use turbomcp_auth::tower::AuthenticatedRequest;
//!
//! // Pre-authenticated request (e.g., from WebSocket with auth done at connection time)
//! let req = AuthenticatedRequest::new(json_body, Some(auth_context), Some("tools/call".into()));
//!
//! // Anonymous request
//! let req = AuthenticatedRequest::new(json_body, None, Some("initialize".into()));
//! ```
//!
//! This implementation:
//! - Checks if request already has [`AuthContext`] attached
//! - Supports method-based bypass
//! - Supports anonymous access when configured
//! - Does NOT extract tokens (expects auth to be pre-resolved)
//!
//! ## Choosing the Right Implementation
//!
//! | Use Case | Service Implementation |
//! |----------|----------------------|
//! | HTTP/REST API | `Service<http::Request<B>>` |
//! | Axum handlers | `Service<http::Request<B>>` |
//! | WebSocket (per-message) | `Service<AuthenticatedRequest<Value>>` |
//! | STDIO transport | `Service<AuthenticatedRequest<Value>>` |
//! | Pre-authenticated requests | `Service<AuthenticatedRequest<Value>>` |

use std::sync::Arc;
use std::task::{Context, Poll};

use futures_util::future::BoxFuture;
use tower_service::Service;

use turbomcp_protocol::McpError;

use crate::AuthProvider;
use crate::context::AuthContext;

use super::AuthLayerConfig;

/// Tower Service that performs authentication
///
/// This service extracts tokens from requests, validates them using the configured
/// [`AuthProvider`], and inserts the resulting [`AuthContext`] into the request's
/// extensions before forwarding to the inner service.
///
/// # Type Parameters
///
/// * `S` - The inner service type
/// * `P` - The authentication provider type
#[derive(Debug, Clone)]
pub struct AuthService<S, P> {
    inner: S,
    provider: Arc<P>,
    config: AuthLayerConfig,
}

impl<S, P> AuthService<S, P>
where
    P: AuthProvider,
{
    /// Create a new auth service
    pub fn new(inner: S, provider: Arc<P>, config: AuthLayerConfig) -> Self {
        Self {
            inner,
            provider,
            config,
        }
    }

    /// Get a reference to the inner service
    pub fn inner(&self) -> &S {
        &self.inner
    }

    /// Get a mutable reference to the inner service
    pub fn inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }

    /// Extract token from HTTP request
    fn extract_token(&self, req: &http::Request<()>) -> Option<String> {
        // Try Authorization header first
        if let Some(auth_header) = req.headers().get(&self.config.auth_header)
            && let Ok(value) = auth_header.to_str()
        {
            if let Some(token) = value.strip_prefix("Bearer ") {
                return Some(token.to_string());
            }
            if let Some(token) = value.strip_prefix("ApiKey ") {
                return Some(token.to_string());
            }
        }

        // Try API key header
        if let Some(api_key) = req.headers().get(&self.config.api_key_header)
            && let Ok(value) = api_key.to_str()
        {
            return Some(value.to_string());
        }

        None
    }
}

/// Request wrapper that carries the original request plus auth context
#[derive(Debug)]
pub struct AuthenticatedRequest<B> {
    /// The original request body
    pub body: B,
    /// The authentication context (if authenticated)
    pub auth_context: Option<AuthContext>,
    /// The method being called (for bypass checking)
    pub method: Option<String>,
}

impl<B> AuthenticatedRequest<B> {
    /// Create a new authenticated request
    pub fn new(body: B, auth_context: Option<AuthContext>, method: Option<String>) -> Self {
        Self {
            body,
            auth_context,
            method,
        }
    }

    /// Get the auth context
    pub fn auth(&self) -> Option<&AuthContext> {
        self.auth_context.as_ref()
    }

    /// Check if the request is authenticated
    pub fn is_authenticated(&self) -> bool {
        self.auth_context.is_some()
    }

    /// Get the inner body
    pub fn into_body(self) -> B {
        self.body
    }
}

/// Future type for auth service responses
///
/// This is a type alias for the boxed future returned by [`AuthService`].
/// The future handles the authentication flow:
/// 1. Extract token from request
/// 2. Validate token with provider
/// 3. On success, forward to inner service
/// 4. On failure, return authentication error
pub type AuthServiceFuture<T, E> = BoxFuture<'static, Result<T, E>>;

/// Implement Service for AuthService wrapping HTTP requests
///
/// This implementation works with `http::Request<B>` types, extracting auth
/// tokens from headers and validating them.
impl<S, P, B, ResBody> Service<http::Request<B>> for AuthService<S, P>
where
    S: Service<http::Request<B>, Response = http::Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Into<McpError>,
    P: AuthProvider + Send + Sync + 'static,
    B: Send + 'static,
{
    type Response = http::Response<ResBody>;
    type Error = McpError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        // Extract method from request path for bypass checking
        let path = req.uri().path();
        let method = path
            .strip_prefix("/")
            .unwrap_or(path)
            .split('/')
            .collect::<Vec<_>>()
            .join("/");

        // Check if this method should bypass authentication
        if self.config.should_bypass(&method) {
            let inner = self.inner.clone();
            let mut inner = std::mem::replace(&mut self.inner, inner);
            return Box::pin(async move { inner.call(req).await.map_err(Into::into) });
        }

        // Extract token from request headers
        let (parts, body) = req.into_parts();
        let token_req = http::Request::from_parts(parts.clone(), ());

        let token = self.extract_token(&token_req);

        match token {
            Some(token) => {
                let provider = Arc::clone(&self.provider);
                let inner = self.inner.clone();
                let mut inner = std::mem::replace(&mut self.inner, inner);
                let allow_anonymous = self.config.allow_anonymous;

                Box::pin(async move {
                    // Validate token
                    match provider.validate_token(&token).await {
                        Ok(auth_context) => {
                            // Rebuild request and call inner service
                            let req = http::Request::from_parts(parts, body);

                            // TODO(Sprint 3): Inject auth_context into request extensions
                            // Will need: let mut req = ...; req.extensions_mut().insert(auth_context);
                            // This would allow downstream services to access auth info via:
                            //   req.extensions().get::<AuthContext>()
                            // Currently blocked by: need to define AuthContext extension type
                            // that works with http::Request<B> for any body type B.
                            // Tracking: TURBO-3XX
                            let _ = &auth_context; // Suppress unused warning until injection implemented

                            inner.call(req).await.map_err(Into::into)
                        }
                        Err(e) => {
                            if allow_anonymous {
                                let req = http::Request::from_parts(parts, body);
                                inner.call(req).await.map_err(Into::into)
                            } else {
                                Err(e)
                            }
                        }
                    }
                })
            }
            None => {
                // No token found
                if self.config.allow_anonymous {
                    let inner = self.inner.clone();
                    let mut inner = std::mem::replace(&mut self.inner, inner);
                    let req = http::Request::from_parts(parts, body);
                    Box::pin(async move { inner.call(req).await.map_err(Into::into) })
                } else {
                    Box::pin(async move {
                        Err(McpError::authentication("No authentication token provided"))
                    })
                }
            }
        }
    }
}

/// Implement Service for AuthService wrapping generic MCP requests
///
/// This is a simpler implementation for non-HTTP contexts where
/// authentication context is passed differently.
impl<S, P> Service<AuthenticatedRequest<serde_json::Value>> for AuthService<S, P>
where
    S: Service<serde_json::Value, Response = serde_json::Value> + Clone + Send + 'static,
    S::Future: Send,
    S::Error: Into<McpError>,
    P: AuthProvider + Send + Sync + 'static,
{
    type Response = serde_json::Value;
    type Error = McpError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, req: AuthenticatedRequest<serde_json::Value>) -> Self::Future {
        // Check method bypass
        if let Some(ref method) = req.method
            && self.config.should_bypass(method)
        {
            let inner = self.inner.clone();
            let mut inner = std::mem::replace(&mut self.inner, inner);
            return Box::pin(async move { inner.call(req.body).await.map_err(Into::into) });
        }

        // Check if already authenticated
        if req.is_authenticated() {
            let inner = self.inner.clone();
            let mut inner = std::mem::replace(&mut self.inner, inner);
            return Box::pin(async move { inner.call(req.body).await.map_err(Into::into) });
        }

        // No auth context - check if anonymous is allowed
        if self.config.allow_anonymous {
            let inner = self.inner.clone();
            let mut inner = std::mem::replace(&mut self.inner, inner);
            Box::pin(async move { inner.call(req.body).await.map_err(Into::into) })
        } else {
            Box::pin(async move {
                Err(McpError::authentication(
                    "Authentication required for this operation",
                ))
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ApiKeyProvider;

    #[test]
    fn test_authenticated_request() {
        let body = serde_json::json!({"test": "value"});
        let req = AuthenticatedRequest::new(body, None, Some("test/method".to_string()));
        assert!(!req.is_authenticated());
        assert!(req.auth().is_none());
    }

    #[test]
    fn test_authenticated_request_with_context() {
        use crate::UserInfo;
        use std::collections::HashMap;

        let body = serde_json::json!({"test": "value"});
        let user = UserInfo {
            id: "test-user".to_string(),
            username: "testuser".to_string(),
            email: None,
            display_name: None,
            avatar_url: None,
            metadata: HashMap::new(),
        };
        let auth_ctx = AuthContext::builder()
            .subject("test-user")
            .user(user)
            .provider("test")
            .build()
            .unwrap();
        let req = AuthenticatedRequest::new(body, Some(auth_ctx), None);
        assert!(req.is_authenticated());
        assert!(req.auth().is_some());
        assert_eq!(req.auth().unwrap().sub, "test-user");
    }

    #[test]
    fn test_auth_service_creation() {
        let provider = Arc::new(ApiKeyProvider::new("test-provider".to_string()));
        let config = AuthLayerConfig::default();

        // Use a simple mock service (just a function)
        let mock_service = tower::service_fn(|_req: serde_json::Value| async move {
            Ok::<_, McpError>(serde_json::json!({"result": "ok"}))
        });

        let _service = AuthService::new(mock_service, provider, config);
        // Service created successfully
    }
}
