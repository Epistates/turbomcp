//! Multi-tenancy middleware for tenant identification and isolation
//!
//! This module provides pluggable tenant extraction middleware for multi-tenant SaaS deployments.
//! Single-tenant applications can ignore this module entirely - tenant extraction is opt-in.
//!
//! ## Features
//!
//! - **Opt-in design**: Zero overhead when not configured
//! - **Multiple extraction strategies**: Headers, JWT claims, subdomains, API keys
//! - **Custom extractors**: Implement `TenantExtractor` trait for custom logic
//! - **Tower middleware**: Integrates seamlessly with existing middleware stack
//!
//! ## Quick Start
//!
//! ```rust
//! use turbomcp_server::middleware::tenancy::{TenantExtractionLayer, HeaderTenantExtractor};
//! use tower::ServiceBuilder;
//!
//! // Create extractor (looks for X-Tenant-ID header)
//! let extractor = HeaderTenantExtractor::new("X-Tenant-ID");
//!
//! // Add to middleware stack
//! let middleware = ServiceBuilder::new()
//!     .layer(TenantExtractionLayer::new(extractor))
//!     .service(my_service);
//! ```
//!
//! ## Built-in Extractors
//!
//! - **HeaderTenantExtractor**: Extract from HTTP header (e.g., `X-Tenant-ID`)
//! - **JwtTenantExtractor**: Extract from JWT claims (requires `auth` feature)
//! - **SubdomainTenantExtractor**: Extract from subdomain (e.g., `acme.api.example.com` → `acme`)
//! - **CompositeTenantExtractor**: Try multiple extractors in order

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use http::{HeaderMap, Request, Response};
use tower::{Layer, Service};
use tracing::debug;

/// Trait for extracting tenant identifiers from HTTP requests
///
/// Implement this trait to create custom tenant identification strategies.
/// All extractors must be `Send + Sync` for use in async Tower middleware.
///
/// ## Example: Custom API Key Extractor
///
/// ```rust
/// use turbomcp_server::middleware::tenancy::TenantExtractor;
/// use http::HeaderMap;
///
/// struct ApiKeyTenantExtractor;
///
/// impl TenantExtractor for ApiKeyTenantExtractor {
///     fn extract(&self, headers: &HeaderMap) -> Option<String> {
///         let api_key = headers.get("authorization")?.to_str().ok()?;
///
///         // Extract tenant from API key prefix (e.g., "sk_acme_..." → "acme")
///         if let Some(key) = api_key.strip_prefix("sk_") {
///             let parts: Vec<&str> = key.splitn(3, '_').collect();
///             if parts.len() >= 2 {
///                 return Some(parts[0].to_string());
///             }
///         }
///
///         None
///     }
/// }
/// ```
pub trait TenantExtractor: Send + Sync {
    /// Extract tenant ID from HTTP headers
    ///
    /// Returns `Some(tenant_id)` if a tenant can be identified, or `None` otherwise.
    /// The tenant ID should be a stable identifier (string) for the tenant.
    fn extract(&self, headers: &HeaderMap) -> Option<String>;

    /// Optional: Validate tenant ID format
    ///
    /// Override this to enforce tenant ID constraints (e.g., alphanumeric, max length).
    /// Default implementation accepts any non-empty string.
    fn validate(&self, tenant_id: &str) -> bool {
        !tenant_id.is_empty()
    }
}

/// Extract tenant ID from HTTP header
///
/// ## Example
///
/// ```rust
/// use turbomcp_server::middleware::tenancy::HeaderTenantExtractor;
///
/// // Look for X-Tenant-ID header
/// let extractor = HeaderTenantExtractor::new("X-Tenant-ID");
///
/// // Custom header name
/// let extractor = HeaderTenantExtractor::new("X-Organization-ID");
/// ```
#[derive(Debug, Clone)]
pub struct HeaderTenantExtractor {
    header_name: String,
}

impl HeaderTenantExtractor {
    /// Create a new header-based tenant extractor
    ///
    /// Header lookup is case-insensitive per HTTP specification.
    pub fn new(header_name: impl Into<String>) -> Self {
        Self {
            header_name: header_name.into(),
        }
    }
}

impl TenantExtractor for HeaderTenantExtractor {
    fn extract(&self, headers: &HeaderMap) -> Option<String> {
        headers
            .get(&self.header_name)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
    }
}

/// Extract tenant ID from API key prefix
///
/// Many SaaS applications encode tenant information in API key prefixes.
/// This extractor supports common patterns like `sk_tenant_secret` or `tenant_live_secret`.
///
/// ## Examples
///
/// - `sk_acme_abc123def456` → `acme` (with delimiter="_", position=1, prefix="sk_")
/// - `acme_live_abc123` → `acme` (with delimiter="_", position=0)
/// - `acme-corp:secret123` → `acme-corp` (with delimiter=":", position=0)
///
/// ## Usage
///
/// ```rust
/// use turbomcp_server::middleware::tenancy::ApiKeyTenantExtractor;
///
/// // Extract from API key: sk_acme_... → acme
/// let extractor = ApiKeyTenantExtractor::new("_", 1).with_prefix("sk_");
///
/// // Extract from API key: acme_live_... → acme
/// let extractor = ApiKeyTenantExtractor::new("_", 0);
///
/// // Extract from colon-separated: acme:secret → acme
/// let extractor = ApiKeyTenantExtractor::new(":", 0);
/// ```
#[derive(Debug, Clone)]
pub struct ApiKeyTenantExtractor {
    delimiter: char,
    position: usize,
    header_name: String,
    prefix: Option<String>,
}

impl ApiKeyTenantExtractor {
    /// Create a new API key tenant extractor
    ///
    /// # Arguments
    ///
    /// * `delimiter` - Character to split the API key on (e.g., '_', '-', ':')
    /// * `position` - Which component to extract as tenant ID (0-indexed)
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_server::middleware::tenancy::ApiKeyTenantExtractor;
    ///
    /// // For keys like "sk_acme_secret": split by '_', take position 1
    /// let extractor = ApiKeyTenantExtractor::new("_", 1);
    /// ```
    pub fn new(delimiter: impl Into<char>, position: usize) -> Self {
        Self {
            delimiter: delimiter.into(),
            position,
            header_name: "authorization".to_string(),
            prefix: None,
        }
    }

    /// Set the prefix to strip before extracting tenant
    ///
    /// Common prefixes: "sk_", "Bearer ", "Token ", etc.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_server::middleware::tenancy::ApiKeyTenantExtractor;
    ///
    /// // Strip "sk_" prefix before extraction
    /// let extractor = ApiKeyTenantExtractor::new("_", 0)
    ///     .with_prefix("sk_");
    /// ```
    pub fn with_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Set a custom header to extract from (default: "authorization")
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp_server::middleware::tenancy::ApiKeyTenantExtractor;
    ///
    /// // Extract from X-API-Key header instead
    /// let extractor = ApiKeyTenantExtractor::new("_", 1)
    ///     .with_header("x-api-key");
    /// ```
    pub fn with_header(mut self, header_name: impl Into<String>) -> Self {
        self.header_name = header_name.into();
        self
    }
}

impl TenantExtractor for ApiKeyTenantExtractor {
    fn extract(&self, headers: &HeaderMap) -> Option<String> {
        let auth_value = headers
            .get(&self.header_name)
            .and_then(|v| v.to_str().ok())?;

        // Strip "Bearer " if present (common OAuth pattern)
        let mut key = auth_value
            .strip_prefix("Bearer ")
            .unwrap_or(auth_value)
            .trim();

        // Strip "Token " if present (alternative auth pattern)
        key = key.strip_prefix("Token ").unwrap_or(key).trim();

        // Strip custom prefix if configured (e.g., "sk_")
        if let Some(prefix) = &self.prefix {
            key = key.strip_prefix(prefix.as_str()).unwrap_or(key);
        }

        // Split by delimiter and extract tenant at specified position
        let parts: Vec<&str> = key.split(self.delimiter).collect();
        parts.get(self.position).map(|s| s.to_string())
    }
}

/// Extract tenant ID from URL subdomain
///
/// Extracts the leftmost subdomain component as the tenant ID.
///
/// ## Examples
///
/// - `acme.api.example.com` → `acme`
/// - `widgets-inc.api.example.com` → `widgets-inc`
/// - `api.example.com` → `None` (no subdomain)
/// - `localhost` → `None`
///
/// ## Usage
///
/// ```rust
/// use turbomcp_server::middleware::tenancy::SubdomainTenantExtractor;
///
/// // Extract from Host header
/// let extractor = SubdomainTenantExtractor::new("api.example.com");
/// ```
#[derive(Debug, Clone)]
pub struct SubdomainTenantExtractor {
    base_domain: String,
}

impl SubdomainTenantExtractor {
    /// Create subdomain extractor with base domain
    ///
    /// `base_domain` should be your API's base domain (e.g., "api.example.com").
    pub fn new(base_domain: impl Into<String>) -> Self {
        Self {
            base_domain: base_domain.into(),
        }
    }
}

impl TenantExtractor for SubdomainTenantExtractor {
    fn extract(&self, headers: &HeaderMap) -> Option<String> {
        let host = headers.get("host").and_then(|v| v.to_str().ok())?;

        // Remove port if present
        let host = host.split(':').next()?;

        // Check if host ends with base domain
        if !host.ends_with(&self.base_domain) {
            return None;
        }

        // Extract subdomain (everything before base domain)
        let subdomain = host.strip_suffix(&self.base_domain)?.trim_end_matches('.');

        if subdomain.is_empty() {
            None
        } else {
            Some(subdomain.to_string())
        }
    }
}

/// Extract tenant ID from JWT claims (requires implementing custom extractor)
///
/// **Note**: JWT tenant extraction requires parsing JWT tokens, which needs the `base64`
/// and `serde_json` crates. To implement JWT-based tenant extraction:
///
/// 1. Add `base64` to your `Cargo.toml`
/// 2. Implement a custom `TenantExtractor` that decodes the JWT payload
/// 3. Extract the tenant claim from the decoded JSON
///
/// ## Example Implementation
///
/// ```rust,ignore
/// use turbomcp_server::middleware::tenancy::TenantExtractor;
/// use http::HeaderMap;
///
/// pub struct JwtTenantExtractor {
///     claim_name: String,
/// }
///
/// impl TenantExtractor for JwtTenantExtractor {
///     fn extract(&self, headers: &HeaderMap) -> Option<String> {
///         // 1. Extract Bearer token from Authorization header
///         let token = headers.get("authorization")?
///             .to_str().ok()?
///             .strip_prefix("Bearer ")?.trim();
///
///         // 2. Split JWT (header.payload.signature)
///         let parts: Vec<&str> = token.split('.').collect();
///         if parts.len() != 3 { return None; }
///
///         // 3. Base64 decode payload (middle part)
///         let payload = base64::decode(parts[1]).ok()?;
///
///         // 4. Parse JSON and extract claim
///         let claims: serde_json::Map<String, serde_json::Value> =
///             serde_json::from_slice(&payload).ok()?;
///
///         claims.get(&self.claim_name)
///             .and_then(|v| v.as_str())
///             .map(String::from)
///     }
/// }
/// ```
///
/// **Security Note**: This example doesn't validate JWT signatures. In production,
/// you should validate JWTs in your authentication middleware before tenant extraction.
/// Composite tenant extractor - tries multiple strategies in order
///
/// Useful when you need to support multiple tenant identification methods.
/// Tries each extractor until one succeeds.
///
/// ## Example
///
/// ```rust
/// use turbomcp_server::middleware::tenancy::{CompositeTenantExtractor, HeaderTenantExtractor, SubdomainTenantExtractor};
///
/// let extractor = CompositeTenantExtractor::new(vec![
///     Box::new(HeaderTenantExtractor::new("X-Tenant-ID")),
///     Box::new(SubdomainTenantExtractor::new("api.example.com")),
/// ]);
/// ```
pub struct CompositeTenantExtractor {
    extractors: Vec<Box<dyn TenantExtractor>>,
}

impl std::fmt::Debug for CompositeTenantExtractor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeTenantExtractor")
            .field("extractors_count", &self.extractors.len())
            .finish()
    }
}

impl CompositeTenantExtractor {
    /// Create composite extractor with multiple strategies
    pub fn new(extractors: Vec<Box<dyn TenantExtractor>>) -> Self {
        Self { extractors }
    }
}

impl TenantExtractor for CompositeTenantExtractor {
    fn extract(&self, headers: &HeaderMap) -> Option<String> {
        for extractor in &self.extractors {
            if let Some(tenant_id) = extractor.extract(headers)
                && extractor.validate(&tenant_id)
            {
                return Some(tenant_id);
            }
        }
        None
    }
}

/// Tower Layer for tenant extraction
///
/// Integrates tenant extraction into the Tower middleware stack.
/// Extracts tenant ID from requests and stores it in the request extensions.
///
/// ## Example
///
/// ```rust
/// use turbomcp_server::middleware::tenancy::{TenantExtractionLayer, HeaderTenantExtractor};
/// use tower::ServiceBuilder;
///
/// let extractor = HeaderTenantExtractor::new("X-Tenant-ID");
/// let layer = TenantExtractionLayer::new(extractor);
///
/// let service = ServiceBuilder::new()
///     .layer(layer)
///     .service(my_service);
/// ```
#[derive(Clone)]
pub struct TenantExtractionLayer<E> {
    extractor: Arc<E>,
}

impl<E> std::fmt::Debug for TenantExtractionLayer<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TenantExtractionLayer")
            .field("extractor", &"<TenantExtractor>")
            .finish()
    }
}

impl<E> TenantExtractionLayer<E>
where
    E: TenantExtractor + 'static,
{
    /// Create a new tenant extraction layer
    pub fn new(extractor: E) -> Self {
        Self {
            extractor: Arc::new(extractor),
        }
    }
}

impl<S, E> Layer<S> for TenantExtractionLayer<E>
where
    E: TenantExtractor + 'static,
{
    type Service = TenantExtractionService<S, E>;

    fn layer(&self, inner: S) -> Self::Service {
        TenantExtractionService {
            inner,
            extractor: Arc::clone(&self.extractor),
        }
    }
}

/// Tower Service that performs tenant extraction
#[derive(Clone)]
pub struct TenantExtractionService<S, E> {
    inner: S,
    extractor: Arc<E>,
}

impl<S, E> std::fmt::Debug for TenantExtractionService<S, E>
where
    S: std::fmt::Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TenantExtractionService")
            .field("inner", &self.inner)
            .field("extractor", &"<TenantExtractor>")
            .finish()
    }
}

impl<S, E> Service<Request<Bytes>> for TenantExtractionService<S, E>
where
    S: Service<Request<Bytes>, Response = Response<Bytes>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send,
    E: TenantExtractor + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<Bytes>) -> Self::Future {
        let extractor = Arc::clone(&self.extractor);
        let mut inner = self.inner.clone();

        Box::pin(async move {
            // Extract tenant ID from headers
            let tenant_id = extractor.extract(req.headers());

            if let Some(ref tenant_id) = tenant_id {
                debug!(tenant_id = %tenant_id, "Extracted tenant ID from request");

                // Store tenant ID in request extensions for downstream middleware
                req.extensions_mut().insert(TenantId(tenant_id.clone()));
            } else {
                debug!("No tenant ID found in request");
            }

            // Call inner service
            inner.call(req).await
        })
    }
}

/// Tenant ID extracted from request
///
/// Stored in request extensions by `TenantExtractionService`.
/// Downstream middleware and handlers can retrieve this to access the tenant ID.
///
/// ## Example
///
/// ```rust
/// use turbomcp_server::middleware::tenancy::TenantId;
/// use http::Request;
/// use bytes::Bytes;
///
/// fn handler(req: Request<Bytes>) -> Result<(), String> {
///     if let Some(tenant_id) = req.extensions().get::<TenantId>() {
///         println!("Request from tenant: {}", tenant_id.0);
///     }
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone)]
pub struct TenantId(pub String);

impl TenantId {
    /// Get the tenant ID as a string slice
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for TenantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http::HeaderValue;

    #[test]
    fn test_header_extractor() {
        let extractor = HeaderTenantExtractor::new("X-Tenant-ID");
        let mut headers = HeaderMap::new();

        // No header - returns None
        assert_eq!(extractor.extract(&headers), None);

        // With header - extracts value
        headers.insert("X-Tenant-ID", HeaderValue::from_static("acme-corp"));
        assert_eq!(extractor.extract(&headers), Some("acme-corp".to_string()));
    }

    #[test]
    fn test_api_key_extractor_with_prefix() {
        // After stripping "sk_", the key becomes "acme_secret123"
        // Split by '_' gives ["acme", "secret123"], so position 0 = "acme"
        let extractor = ApiKeyTenantExtractor::new('_', 0).with_prefix("sk_");
        let mut headers = HeaderMap::new();

        // No header - returns None
        assert_eq!(extractor.extract(&headers), None);

        // With API key: sk_acme_secret123 → (strip sk_) → acme_secret123 → (split _) → ["acme", "secret123"] → acme
        headers.insert(
            "authorization",
            HeaderValue::from_static("sk_acme_secret123"),
        );
        assert_eq!(extractor.extract(&headers), Some("acme".to_string()));

        // With Bearer prefix: Bearer sk_widgets_abc → (strip Bearer + sk_) → widgets_abc → ["widgets", "abc"] → widgets
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer sk_widgets_abc"),
        );
        assert_eq!(extractor.extract(&headers), Some("widgets".to_string()));
    }

    #[test]
    fn test_api_key_extractor_without_prefix() {
        let extractor = ApiKeyTenantExtractor::new('_', 0);
        let mut headers = HeaderMap::new();

        // API key: acme_live_secret123 → ["acme", "live", "secret123"] → acme (position 0)
        headers.insert(
            "authorization",
            HeaderValue::from_static("acme_live_secret123"),
        );
        assert_eq!(extractor.extract(&headers), Some("acme".to_string()));

        // API key: widgets-inc_prod_abc → ["widgets-inc", "prod", "abc"] → widgets-inc (position 0)
        headers.insert(
            "authorization",
            HeaderValue::from_static("widgets-inc_prod_abc"),
        );
        assert_eq!(extractor.extract(&headers), Some("widgets-inc".to_string()));
    }

    #[test]
    fn test_api_key_extractor_custom_delimiter() {
        let extractor = ApiKeyTenantExtractor::new(':', 0);
        let mut headers = HeaderMap::new();

        // Colon-separated: acme-corp:secret123 → ["acme-corp", "secret123"] → acme-corp (position 0)
        headers.insert(
            "authorization",
            HeaderValue::from_static("acme-corp:secret123"),
        );
        assert_eq!(extractor.extract(&headers), Some("acme-corp".to_string()));
    }

    #[test]
    fn test_api_key_extractor_custom_header() {
        // After stripping "sk_", key becomes "acme_secret", split gives ["acme", "secret"]
        let extractor = ApiKeyTenantExtractor::new('_', 0)
            .with_prefix("sk_")
            .with_header("x-api-key");
        let mut headers = HeaderMap::new();

        // Extract from X-API-Key header: sk_acme_secret → (strip sk_) → acme_secret → ["acme", "secret"] → acme
        headers.insert("x-api-key", HeaderValue::from_static("sk_acme_secret"));
        assert_eq!(extractor.extract(&headers), Some("acme".to_string()));
    }

    #[test]
    fn test_subdomain_extractor() {
        let extractor = SubdomainTenantExtractor::new("api.example.com");
        let mut headers = HeaderMap::new();

        // No subdomain
        headers.insert("host", HeaderValue::from_static("api.example.com"));
        assert_eq!(extractor.extract(&headers), None);

        // With subdomain
        headers.insert("host", HeaderValue::from_static("acme.api.example.com"));
        assert_eq!(extractor.extract(&headers), Some("acme".to_string()));

        // With port
        headers.insert(
            "host",
            HeaderValue::from_static("acme.api.example.com:8080"),
        );
        assert_eq!(extractor.extract(&headers), Some("acme".to_string()));

        // Multi-level subdomain (only first level)
        headers.insert("host", HeaderValue::from_static("foo.bar.api.example.com"));
        assert_eq!(extractor.extract(&headers), Some("foo.bar".to_string()));
    }

    #[test]
    fn test_composite_extractor() {
        let extractor = CompositeTenantExtractor::new(vec![
            Box::new(HeaderTenantExtractor::new("X-Tenant-ID")),
            Box::new(SubdomainTenantExtractor::new("api.example.com")),
        ]);

        let mut headers = HeaderMap::new();

        // Try header first
        headers.insert("X-Tenant-ID", HeaderValue::from_static("from-header"));
        assert_eq!(extractor.extract(&headers), Some("from-header".to_string()));

        // Fall back to subdomain if header missing
        headers.clear();
        headers.insert(
            "host",
            HeaderValue::from_static("from-subdomain.api.example.com"),
        );
        assert_eq!(
            extractor.extract(&headers),
            Some("from-subdomain".to_string())
        );

        // Return None if all fail
        headers.clear();
        assert_eq!(extractor.extract(&headers), None);
    }
}
