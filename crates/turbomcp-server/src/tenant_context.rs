//! Extension trait for multi-tenant RequestContext support.
//!
//! This module provides tenant tracking capabilities using the existing
//! `metadata` field in `RequestContext`, avoiding protocol pollution.
//!
//! Only compiled when the `multi-tenancy` feature is enabled.

use turbomcp_protocol::context::RequestContext;
use turbomcp_protocol::{Error, Result};

/// Metadata key for tenant ID storage
const TENANT_ID_KEY: &str = "turbomcp.tenant_id";

/// Extension trait providing multi-tenant context capabilities.
///
/// This trait extends `RequestContext` with tenant tracking methods
/// without modifying the core protocol types.
///
/// # Design
///
/// Uses the existing `metadata: Arc<HashMap<String, serde_json::Value>>` field
/// to store tenant information. This ensures:
/// - Zero impact on non-multi-tenant users
/// - No protocol changes required
/// - Opt-in via feature flag and trait import
///
/// # Example
///
/// ```ignore
/// use turbomcp_server::TenantContextExt;
///
/// let ctx = RequestContext::new();
/// let ctx = ctx.with_tenant("acme-corp");
///
/// if let Some(tenant) = ctx.tenant() {
///     println!("Request from tenant: {}", tenant);
/// }
/// ```
pub trait TenantContextExt {
    /// Sets the tenant ID for this context, returning a new context.
    ///
    /// This is typically used by middleware to identify the tenant/organization
    /// in multi-tenant SaaS deployments.
    ///
    /// # Example
    /// ```ignore
    /// use turbomcp_server::TenantContextExt;
    ///
    /// let ctx = RequestContext::new().with_tenant("acme-corp");
    /// assert_eq!(ctx.tenant(), Some("acme-corp"));
    /// ```
    fn with_tenant(self, tenant_id: impl Into<String>) -> Self;

    /// Retrieves the tenant ID from this context, if set.
    ///
    /// Returns `None` for single-tenant deployments or if not configured.
    ///
    /// # Example
    /// ```ignore
    /// use turbomcp_server::TenantContextExt;
    ///
    /// let ctx = RequestContext::new().with_tenant("widgets-inc");
    /// assert_eq!(ctx.tenant(), Some("widgets-inc"));
    /// ```
    fn tenant(&self) -> Option<&str>;

    /// Requires a tenant ID to be present, returning an error if missing.
    ///
    /// This is useful for operations that must be tenant-scoped.
    ///
    /// # Errors
    ///
    /// Returns `ResourceAccessDenied` if no tenant ID is present in the context.
    ///
    /// # Example
    /// ```ignore
    /// use turbomcp_server::TenantContextExt;
    ///
    /// let ctx = RequestContext::new();
    /// assert!(ctx.require_tenant().is_err());
    ///
    /// let ctx = ctx.with_tenant("acme");
    /// assert_eq!(ctx.require_tenant().unwrap(), "acme");
    /// ```
    fn require_tenant(&self) -> Result<&str>;

    /// Validates that the current tenant owns the specified resource.
    ///
    /// This is a critical security method that prevents cross-tenant resource access.
    /// Always use this before performing operations on tenant-scoped resources.
    ///
    /// # Arguments
    ///
    /// * `resource_tenant_id` - The tenant ID that owns the resource being accessed
    ///
    /// # Errors
    ///
    /// Returns `ResourceAccessDenied` error if:
    /// - No tenant ID is present in the request context
    /// - The request tenant ID doesn't match the resource owner's tenant ID
    ///
    /// # Example
    /// ```ignore
    /// use turbomcp_server::TenantContextExt;
    ///
    /// let ctx = RequestContext::new().with_tenant("acme-corp");
    ///
    /// // Valid access - tenant matches
    /// assert!(ctx.validate_tenant_ownership("acme-corp").is_ok());
    ///
    /// // Invalid access - different tenant
    /// assert!(ctx.validate_tenant_ownership("widgets-inc").is_err());
    /// ```
    fn validate_tenant_ownership(&self, resource_tenant_id: &str) -> Result<()>;
}

impl TenantContextExt for RequestContext {
    fn with_tenant(mut self, tenant_id: impl Into<String>) -> Self {
        // Create new metadata HashMap with tenant ID
        let mut metadata = (*self.metadata).clone();
        metadata.insert(
            TENANT_ID_KEY.to_string(),
            serde_json::Value::String(tenant_id.into()),
        );
        self.metadata = std::sync::Arc::new(metadata);
        self
    }

    fn tenant(&self) -> Option<&str> {
        self.metadata.get(TENANT_ID_KEY).and_then(|v| v.as_str())
    }

    fn require_tenant(&self) -> Result<&str> {
        self.tenant().ok_or_else(|| {
            Error::new(
                turbomcp_protocol::ErrorKind::ResourceAccessDenied,
                "This operation requires a tenant ID. Multi-tenant authentication is not configured.",
            )
        })
    }

    fn validate_tenant_ownership(&self, resource_tenant_id: &str) -> Result<()> {
        match self.tenant() {
            None => Err(Error::new(
                turbomcp_protocol::ErrorKind::ResourceAccessDenied,
                "Tenant ID not found in request context. Multi-tenant authentication required.",
            )),
            Some(tenant_id) if tenant_id == resource_tenant_id => Ok(()),
            Some(tenant_id) => Err(Error::new(
                turbomcp_protocol::ErrorKind::ResourceAccessDenied,
                format!(
                    "Tenant '{}' is not authorized to access resource owned by tenant '{}' (request_tenant: {}, resource_tenant: {})",
                    tenant_id, resource_tenant_id, tenant_id, resource_tenant_id
                ),
            )
            .with_component("tenant_context")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenant_context_basic() {
        let ctx = RequestContext::new().with_tenant("test-tenant");
        assert_eq!(ctx.tenant(), Some("test-tenant"));
    }

    #[test]
    fn test_tenant_context_none() {
        let ctx = RequestContext::new();
        assert_eq!(ctx.tenant(), None);
    }

    #[test]
    fn test_require_tenant_present() {
        let ctx = RequestContext::new().with_tenant("acme");
        assert_eq!(ctx.require_tenant().unwrap(), "acme");
    }

    #[test]
    fn test_require_tenant_missing() {
        let ctx = RequestContext::new();
        assert!(ctx.require_tenant().is_err());
    }

    #[test]
    fn test_validate_ownership_match() {
        let ctx = RequestContext::new().with_tenant("acme-corp");
        assert!(ctx.validate_tenant_ownership("acme-corp").is_ok());
    }

    #[test]
    fn test_validate_ownership_mismatch() {
        let ctx = RequestContext::new().with_tenant("acme-corp");
        assert!(ctx.validate_tenant_ownership("widgets-inc").is_err());
    }

    #[test]
    fn test_validate_ownership_no_tenant() {
        let ctx = RequestContext::new();
        assert!(ctx.validate_tenant_ownership("acme-corp").is_err());
    }
}
