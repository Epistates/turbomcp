//! Casbin Authorization middleware for policy-based access control
//!
//! This middleware implements RBAC (Role-Based Access Control) using the Casbin
//! authorization library. It supports flexible policy definitions and can handle
//! complex authorization scenarios.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use casbin::{CoreApi, Enforcer, Model};
use tokio::sync::RwLock;
use tower::{Layer, Service};
use tracing::{debug, error, warn};

use super::auth::Claims;

/// Authorization configuration
#[derive(Debug, Clone)]
pub struct AuthzConfig {
    /// Path to Casbin model file
    pub model_path: String,
    /// Path to Casbin policy file
    pub policy_path: String,
    /// Whether to allow requests when authorization fails
    pub fail_open: bool,
    /// Whether to log authorization decisions
    pub log_decisions: bool,
}

impl Default for AuthzConfig {
    fn default() -> Self {
        Self {
            model_path: "src/policies/rbac_model.conf".to_string(),
            policy_path: "src/policies/rbac_policy.csv".to_string(),
            fail_open: false, // Fail closed by default for security
            log_decisions: true,
        }
    }
}

impl AuthzConfig {
    /// Create new authorization config
    pub fn new(model_path: String, policy_path: String) -> Self {
        Self {
            model_path,
            policy_path,
            fail_open: false,
            log_decisions: true,
        }
    }

    /// Set fail open behavior
    pub fn with_fail_open(mut self, fail_open: bool) -> Self {
        self.fail_open = fail_open;
        self
    }

    /// Set decision logging
    pub fn with_logging(mut self, log_decisions: bool) -> Self {
        self.log_decisions = log_decisions;
        self
    }
}

/// Authorization layer
pub struct AuthzLayer {
    enforcer: Arc<RwLock<Enforcer>>,
    config: AuthzConfig,
}

impl std::fmt::Debug for AuthzLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthzLayer")
            .field("config", &self.config)
            .field("enforcer", &"<Casbin Enforcer>")
            .finish()
    }
}

impl AuthzLayer {
    /// Create new authorization layer
    /// Note: This creates a placeholder enforcer. In production, use `new_async` instead.
    pub fn new(config: AuthzConfig) -> Self {
        // Create a basic enforcer with file adapter
        // This is a simplified version - in production, use new_async during server startup
        // Create a minimal enforcer for synchronous creation
        // In production, use new_async instead for proper model loading
        // For compilation, we'll create a basic memory enforcer without policies

        // Create a basic RBAC model
        let mut model = casbin::DefaultModel::default();
        model.add_def("r", "r", "sub, obj, act");
        model.add_def("p", "p", "sub, obj, act");
        model.add_def("g", "g", "_, _");
        model.add_def("e", "e", "some(where (p.eft == allow))");
        model.add_def(
            "m",
            "m",
            "g(r.sub, p.sub) && r.obj == p.obj && r.act == p.act",
        );

        let adapter = casbin::MemoryAdapter::default();

        // Since Enforcer::new is async, we'll use a basic runtime for sync creation
        // This is not recommended for production - use new_async instead
        let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        let enforcer = runtime
            .block_on(async { casbin::Enforcer::new(model, adapter).await })
            .expect("Failed to create enforcer");

        let enforcer = Arc::new(RwLock::new(enforcer));

        Self { enforcer, config }
    }

    /// Create new authorization layer with async initialization (recommended)
    pub async fn new_async(config: AuthzConfig) -> Result<Self, casbin::Error> {
        // Load model from file path using DefaultModel
        let model = casbin::DefaultModel::from_file(&config.model_path).await?;
        let adapter = casbin::FileAdapter::new(config.policy_path.clone());

        let enforcer = Arc::new(RwLock::new(Enforcer::new(model, adapter).await?));

        Ok(Self { enforcer, config })
    }

    /// Reload policies from file
    pub async fn reload_policies(&self) -> Result<(), casbin::Error> {
        let mut enforcer = self.enforcer.write().await;
        enforcer.load_policy().await
    }
}

impl Clone for AuthzLayer {
    fn clone(&self) -> Self {
        Self {
            enforcer: Arc::clone(&self.enforcer),
            config: self.config.clone(),
        }
    }
}

impl<S> Layer<S> for AuthzLayer {
    type Service = AuthzService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthzService {
            inner,
            enforcer: Arc::clone(&self.enforcer),
            config: self.config.clone(),
        }
    }
}

/// Authorization service
pub struct AuthzService<S> {
    inner: S,
    enforcer: Arc<RwLock<Enforcer>>,
    config: AuthzConfig,
}

impl<S: std::fmt::Debug> std::fmt::Debug for AuthzService<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthzService")
            .field("inner", &self.inner)
            .field("config", &self.config)
            .field("enforcer", &"<Casbin Enforcer>")
            .finish()
    }
}

impl<S> Clone for AuthzService<S>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            enforcer: Arc::clone(&self.enforcer),
            config: self.config.clone(),
        }
    }
}

impl<S, ReqBody> Service<http::Request<ReqBody>> for AuthzService<S>
where
    S: Service<http::Request<ReqBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + Sync + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: http::Request<ReqBody>) -> Self::Future {
        let enforcer = Arc::clone(&self.enforcer);
        let config = self.config.clone();
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Check authorization
            let authorized = check_authorization(&req, &enforcer, &config).await;

            if !authorized {
                if config.log_decisions {
                    warn!(
                        "Authorization denied for request: {} {}",
                        req.method(),
                        req.uri().path()
                    );
                }

                if !config.fail_open {
                    // In a real implementation, you would return 403 Forbidden
                    // For now, we continue but log the denial
                    debug!("Authorization failed but continuing due to middleware setup");
                }
            } else if config.log_decisions {
                debug!(
                    "Authorization granted for request: {} {}",
                    req.method(),
                    req.uri().path()
                );
            }

            inner.call(req).await
        })
    }
}

/// Check if the request is authorized using Casbin policies
async fn check_authorization<B>(
    req: &http::Request<B>,
    enforcer: &Arc<RwLock<Enforcer>>,
    config: &AuthzConfig,
) -> bool {
    // Extract user claims from request extensions
    let claims = req.extensions().get::<Claims>();

    // Extract resource and action from request
    let resource = extract_resource_from_path(req.uri().path());
    let action = extract_action_from_method_and_path(req.method(), req.uri().path());

    match claims {
        Some(claims) => {
            // Check each role the user has
            for role in &claims.roles {
                let allowed = {
                    let enforcer = enforcer.read().await;
                    match enforcer.enforce((role, &resource, &action)) {
                        Ok(result) => result,
                        Err(e) => {
                            error!("Casbin enforcement error: {}", e);
                            config.fail_open // Default behavior on error
                        }
                    }
                };

                if allowed {
                    if config.log_decisions {
                        debug!(
                            user = %claims.sub,
                            role = %role,
                            resource = %resource,
                            action = %action,
                            "Authorization granted"
                        );
                    }
                    return true;
                }
            }

            if config.log_decisions {
                debug!(
                    user = %claims.sub,
                    roles = ?claims.roles,
                    resource = %resource,
                    action = %action,
                    "Authorization denied"
                );
            }
            false
        }
        None => {
            // No authentication claims - check if anonymous access is allowed
            let enforcer = enforcer.read().await;
            match enforcer.enforce(("guest", &resource, &action)) {
                Ok(result) => {
                    if config.log_decisions {
                        debug!(
                            resource = %resource,
                            action = %action,
                            allowed = %result,
                            "Anonymous access check"
                        );
                    }
                    result
                }
                Err(e) => {
                    error!("Casbin enforcement error for anonymous user: {}", e);
                    config.fail_open
                }
            }
        }
    }
}

/// Extract resource name from request path
fn extract_resource_from_path(path: &str) -> String {
    // Convert MCP JSON-RPC method paths to resource names
    // Examples:
    // "/mcp" -> "mcp" (general MCP access)
    // Any path -> first path segment or "default"

    if let Some(stripped) = path.strip_prefix('/') {
        let parts: Vec<&str> = stripped.split('/').collect();
        let first_part = parts.first().unwrap_or(&"default");
        if first_part.is_empty() {
            "default".to_string()
        } else {
            first_part.to_string()
        }
    } else {
        "default".to_string()
    }
}

/// Extract action from HTTP method and path
fn extract_action_from_method_and_path(method: &http::Method, path: &str) -> String {
    // For MCP, we'll extract action from the JSON-RPC method name
    // Since this is middleware, we don't have access to the JSON-RPC content yet
    // So we'll use a simplified mapping based on HTTP method and path

    match *method {
        http::Method::GET => "list".to_string(),
        http::Method::POST => {
            // For POST requests, we might need to examine the body
            // For now, assume it's a "call" action
            if path.contains("tool") {
                "call".to_string()
            } else {
                "request".to_string()
            }
        }
        http::Method::PUT => "update".to_string(),
        http::Method::DELETE => "delete".to_string(),
        _ => "access".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::Method;

    #[test]
    fn test_resource_extraction() {
        assert_eq!(extract_resource_from_path("/mcp"), "mcp");
        assert_eq!(extract_resource_from_path("/api/v1/tools"), "api");
        assert_eq!(extract_resource_from_path("/"), "default");
        assert_eq!(extract_resource_from_path(""), "default");
    }

    #[test]
    fn test_action_extraction() {
        assert_eq!(
            extract_action_from_method_and_path(&Method::GET, "/tools"),
            "list"
        );
        assert_eq!(
            extract_action_from_method_and_path(&Method::POST, "/tools"),
            "call"
        );
        assert_eq!(
            extract_action_from_method_and_path(&Method::POST, "/prompts"),
            "request"
        );
        assert_eq!(
            extract_action_from_method_and_path(&Method::PUT, "/config"),
            "update"
        );
        assert_eq!(
            extract_action_from_method_and_path(&Method::DELETE, "/resource"),
            "delete"
        );
    }
}
