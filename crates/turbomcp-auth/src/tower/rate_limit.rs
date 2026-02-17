//! Tower middleware for rate limiting authentication endpoints
//!
//! This module provides composable rate limiting middleware that integrates
//! with the existing [`RateLimiter`] from the `rate_limit` module.
//!
//! # Usage
//!
//! ```rust,ignore
//! use tower::ServiceBuilder;
//! use turbomcp_auth::tower::{AuthLayer, RateLimitLayer};
//! use turbomcp_auth::rate_limit::RateLimiter;
//!
//! let service = ServiceBuilder::new()
//!     .layer(RateLimitLayer::new(RateLimiter::for_auth()))
//!     .layer(AuthLayer::new(provider))
//!     .service(my_inner_service);
//! ```
//!
//! # Key Extraction
//!
//! The middleware supports flexible key extraction strategies via the [`KeyExtractor`] trait:
//!
//! ```rust,ignore
//! use turbomcp_auth::tower::rate_limit::{RateLimitLayer, IpKeyExtractor};
//! use turbomcp_auth::rate_limit::RateLimiter;
//!
//! // Default: IP-based extraction
//! let layer = RateLimitLayer::new(RateLimiter::for_auth());
//!
//! // Custom extractor
//! struct CustomExtractor;
//! impl KeyExtractor for CustomExtractor {
//!     fn extract_key<B>(&self, req: &http::Request<B>) -> RateLimitKey {
//!         // Custom logic here
//!     }
//!
//!     fn extract_endpoint<B>(&self, req: &http::Request<B>) -> String {
//!         // Custom logic here
//!     }
//! }
//!
//! let layer = RateLimitLayer::with_key_extractor(
//!     RateLimiter::for_auth(),
//!     CustomExtractor,
//! );
//! ```

use std::sync::Arc;
use std::task::{Context, Poll};

use futures_util::future::BoxFuture;
use tower::Layer;
use tower_service::Service;

use crate::rate_limit::{RateLimitInfo, RateLimitKey, RateLimiter};

/// Strategy for extracting rate limit keys from requests
///
/// This trait allows custom key extraction strategies to be plugged into
/// the rate limiting middleware. Common strategies include:
///
/// - IP-based limiting (see [`IpKeyExtractor`])
/// - User-based limiting (extract from auth context)
/// - API key-based limiting (extract from headers)
/// - Composite keys (combine multiple factors)
pub trait KeyExtractor: Send + Sync + 'static {
    /// Extract a rate limit key from an HTTP request
    ///
    /// This is called before each request to determine which rate limit
    /// bucket to use.
    fn extract_key<B>(&self, req: &http::Request<B>) -> RateLimitKey;

    /// Extract the endpoint name from a request
    ///
    /// This is used to determine which endpoint-specific limits to apply.
    /// Common strategies include:
    ///
    /// - Extract from URI path (e.g., `/oauth/token` → `token`)
    /// - Extract from JSON-RPC method field
    /// - Use a fixed endpoint name
    fn extract_endpoint<B>(&self, req: &http::Request<B>) -> String;
}

/// Default key extractor that uses client IP from headers
///
/// This extractor follows the standard proxy header chain:
/// 1. `X-Forwarded-For` (first IP in comma-separated list)
/// 2. `X-Real-IP`
/// 3. Fallback to `"unknown"` if no IP headers present
///
/// # Security Considerations
///
/// - Only use this behind a trusted reverse proxy (nginx, Cloudflare, etc.)
/// - Validate that untrusted clients cannot set `X-Forwarded-For` headers
/// - Consider using authenticated user IDs instead of IPs when possible
#[derive(Debug, Clone, Default)]
pub struct IpKeyExtractor;

impl KeyExtractor for IpKeyExtractor {
    fn extract_key<B>(&self, req: &http::Request<B>) -> RateLimitKey {
        // Try X-Forwarded-For first (standard proxy header)
        // Take only the first IP in the chain (client IP)
        let ip = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(',').next())
            .map(str::trim)
            .or_else(|| {
                // Fall back to X-Real-IP (alternative proxy header)
                req.headers().get("x-real-ip").and_then(|v| v.to_str().ok())
            })
            .unwrap_or("unknown");

        RateLimitKey::ip(ip)
    }

    fn extract_endpoint<B>(&self, req: &http::Request<B>) -> String {
        let path = req.uri().path();
        // Extract last path segment as endpoint name
        // E.g., "/oauth/token" → "token", "/api/login" → "login"
        path.rsplit('/')
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown")
            .to_string()
    }
}

/// Tower Layer that adds rate limiting
///
/// This layer wraps services to enforce per-client rate limits on
/// authentication endpoints. It composes naturally with other Tower
/// middleware like [`AuthLayer`](super::AuthLayer).
///
/// # Type Parameters
///
/// * `K` - The key extraction strategy (defaults to [`IpKeyExtractor`])
///
/// # Example
///
/// ```rust,ignore
/// use tower::ServiceBuilder;
/// use turbomcp_auth::tower::RateLimitLayer;
/// use turbomcp_auth::rate_limit::RateLimiter;
///
/// let service = ServiceBuilder::new()
///     .layer(RateLimitLayer::new(RateLimiter::for_auth()))
///     .service(my_inner_service);
/// ```
#[derive(Debug, Clone)]
pub struct RateLimitLayer<K = IpKeyExtractor> {
    limiter: RateLimiter,
    key_extractor: Arc<K>,
}

impl RateLimitLayer<IpKeyExtractor> {
    /// Create a new rate limit layer with IP-based key extraction
    ///
    /// This is the recommended default for most use cases.
    /// Uses [`IpKeyExtractor`] to extract client IPs from proxy headers.
    pub fn new(limiter: RateLimiter) -> Self {
        Self {
            limiter,
            key_extractor: Arc::new(IpKeyExtractor),
        }
    }
}

impl<K: KeyExtractor> RateLimitLayer<K> {
    /// Create a new rate limit layer with a custom key extractor
    ///
    /// Use this when you need custom rate limiting logic, such as:
    /// - User-based limiting (requires auth context)
    /// - API key-based limiting
    /// - Composite keys (IP + user + endpoint)
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use turbomcp_auth::tower::rate_limit::{RateLimitLayer, KeyExtractor};
    /// use turbomcp_auth::rate_limit::{RateLimiter, RateLimitKey};
    ///
    /// struct UserKeyExtractor;
    /// impl KeyExtractor for UserKeyExtractor {
    ///     fn extract_key<B>(&self, req: &http::Request<B>) -> RateLimitKey {
    ///         // Extract user ID from auth context in request extensions
    ///         if let Some(auth) = req.extensions().get::<AuthContext>() {
    ///             RateLimitKey::user(&auth.sub)
    ///         } else {
    ///             RateLimitKey::ip("unknown")
    ///         }
    ///     }
    ///
    ///     fn extract_endpoint<B>(&self, req: &http::Request<B>) -> String {
    ///         req.uri().path().rsplit('/').next().unwrap_or("unknown").to_string()
    ///     }
    /// }
    ///
    /// let layer = RateLimitLayer::with_key_extractor(
    ///     RateLimiter::for_auth(),
    ///     UserKeyExtractor,
    /// );
    /// ```
    pub fn with_key_extractor(limiter: RateLimiter, key_extractor: K) -> Self {
        Self {
            limiter,
            key_extractor: Arc::new(key_extractor),
        }
    }
}

/// Tower Service that performs rate limiting
///
/// This service wraps an inner service and enforces rate limits before
/// forwarding requests. Rate-limited requests are rejected with a
/// [`RateLimitRejection`] error.
///
/// # Type Parameters
///
/// * `S` - The inner service type
/// * `K` - The key extraction strategy
#[derive(Debug, Clone)]
pub struct RateLimitService<S, K = IpKeyExtractor> {
    inner: S,
    limiter: RateLimiter,
    key_extractor: Arc<K>,
}

impl<S, K> RateLimitService<S, K> {
    /// Create a new rate limit service
    pub fn new(inner: S, limiter: RateLimiter, key_extractor: Arc<K>) -> Self {
        Self {
            inner,
            limiter,
            key_extractor,
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
}

// Implement Layer
impl<S, K: KeyExtractor + Clone> Layer<S> for RateLimitLayer<K> {
    type Service = RateLimitService<S, K>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService::new(inner, self.limiter.clone(), Arc::clone(&self.key_extractor))
    }
}

// Implement Service for HTTP requests
impl<S, K, B> Service<http::Request<B>> for RateLimitService<S, K>
where
    S: Service<http::Request<B>> + Clone + Send + 'static,
    S::Response: Send,
    S::Error: Send,
    S::Future: Send,
    K: KeyExtractor,
    B: Send + 'static,
{
    type Response = Result<S::Response, RateLimitRejection<S::Error>>;
    type Error = std::convert::Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // Always ready - we handle inner service errors in call()
        match self.inner.poll_ready(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(_)) => Poll::Ready(Ok(())), // Will fail on call()
            Poll::Pending => Poll::Pending,
        }
    }

    fn call(&mut self, req: http::Request<B>) -> Self::Future {
        // Extract key and endpoint BEFORE moving req
        let key = self.key_extractor.extract_key(&req);
        let endpoint = self.key_extractor.extract_endpoint(&req);

        let limiter = self.limiter.clone();
        let inner = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, inner);

        Box::pin(async move {
            // Check rate limit
            if let Err(info) = limiter.check(&key, &endpoint).await {
                return Ok(Err(RateLimitRejection::RateLimited(info)));
            }

            // Forward to inner service
            match inner.call(req).await {
                Ok(resp) => Ok(Ok(resp)),
                Err(e) => Ok(Err(RateLimitRejection::Inner(e))),
            }
        })
    }
}

/// Rejection type for rate-limited requests
///
/// This type captures both rate limit violations and inner service errors,
/// allowing Tower middleware to distinguish between different failure modes.
#[derive(Debug)]
pub enum RateLimitRejection<E> {
    /// Request was rate limited
    ///
    /// The request exceeded the configured rate limit for this client/endpoint.
    /// The [`RateLimitInfo`] contains details about the limit and when to retry.
    RateLimited(RateLimitInfo),

    /// Inner service error
    ///
    /// The request was allowed by the rate limiter, but the inner service
    /// returned an error.
    Inner(E),
}

impl<E: std::fmt::Display> std::fmt::Display for RateLimitRejection<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RateLimited(info) => write!(f, "Rate limited: {info}"),
            Self::Inner(e) => write!(f, "{e}"),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for RateLimitRejection<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::RateLimited(info) => Some(info),
            Self::Inner(e) => Some(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rate_limit::{EndpointLimit, RateLimitConfig};
    use std::time::Duration;

    #[test]
    fn test_ip_key_extractor_x_forwarded_for() {
        let extractor = IpKeyExtractor;

        let req = http::Request::builder()
            .header("x-forwarded-for", "192.168.1.100, 10.0.0.1")
            .body(())
            .unwrap();

        let key = extractor.extract_key(&req);
        assert_eq!(key.key_type, "ip");
        assert_eq!(key.value, "192.168.1.100");
    }

    #[test]
    fn test_ip_key_extractor_x_real_ip() {
        let extractor = IpKeyExtractor;

        let req = http::Request::builder()
            .header("x-real-ip", "10.20.30.40")
            .body(())
            .unwrap();

        let key = extractor.extract_key(&req);
        assert_eq!(key.key_type, "ip");
        assert_eq!(key.value, "10.20.30.40");
    }

    #[test]
    fn test_ip_key_extractor_no_headers() {
        let extractor = IpKeyExtractor;

        let req = http::Request::builder().body(()).unwrap();

        let key = extractor.extract_key(&req);
        assert_eq!(key.key_type, "ip");
        assert_eq!(key.value, "unknown");
    }

    #[test]
    fn test_ip_key_extractor_endpoint_from_path() {
        let extractor = IpKeyExtractor;

        let req = http::Request::builder()
            .uri("/oauth/token")
            .body(())
            .unwrap();

        let endpoint = extractor.extract_endpoint(&req);
        assert_eq!(endpoint, "token");
    }

    #[test]
    fn test_ip_key_extractor_endpoint_root_path() {
        let extractor = IpKeyExtractor;

        let req = http::Request::builder().uri("/").body(()).unwrap();

        let endpoint = extractor.extract_endpoint(&req);
        assert_eq!(endpoint, "unknown");
    }

    #[test]
    fn test_layer_creation() {
        let limiter = RateLimiter::for_auth();
        let _layer = RateLimitLayer::new(limiter);
    }

    #[test]
    fn test_layer_with_custom_extractor() {
        struct TestExtractor;
        impl KeyExtractor for TestExtractor {
            fn extract_key<B>(&self, _req: &http::Request<B>) -> RateLimitKey {
                RateLimitKey::ip("test")
            }

            fn extract_endpoint<B>(&self, _req: &http::Request<B>) -> String {
                "test".to_string()
            }
        }

        let limiter = RateLimiter::for_auth();
        let _layer = RateLimitLayer::with_key_extractor(limiter, TestExtractor);
    }

    #[tokio::test]
    async fn test_service_allows_under_limit() {
        // Create a very permissive limiter
        let limiter = RateLimiter::new(
            RateLimitConfig::builder()
                .default_limit(100, Duration::from_secs(60))
                .build(),
        );

        // Create a mock inner service that always succeeds
        let inner_service = tower::service_fn(|_req: http::Request<()>| async move {
            Ok::<_, std::convert::Infallible>(http::Response::new(()))
        });

        let mut service = RateLimitService::new(inner_service, limiter, Arc::new(IpKeyExtractor));

        let req = http::Request::builder()
            .header("x-forwarded-for", "192.168.1.1")
            .body(())
            .unwrap();

        let result = service.call(req).await;
        assert!(result.is_ok());
        let inner_result = result.unwrap();
        assert!(inner_result.is_ok());
    }

    #[tokio::test]
    async fn test_service_blocks_over_limit() {
        // Create a strict limiter (1 request per minute, no burst)
        let limiter = RateLimiter::new(
            RateLimitConfig::builder()
                .endpoint_limit(
                    "test",
                    EndpointLimit {
                        requests: 1,
                        window: Duration::from_secs(60),
                        burst: 0,
                    },
                )
                .build(),
        );

        // Create a mock inner service
        let inner_service = tower::service_fn(|_req: http::Request<()>| async move {
            Ok::<_, std::convert::Infallible>(http::Response::new(()))
        });

        let mut service = RateLimitService::new(inner_service, limiter, Arc::new(IpKeyExtractor));

        // First request should succeed
        let req1 = http::Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "192.168.1.1")
            .body(())
            .unwrap();

        let result1 = service.call(req1).await.unwrap();
        assert!(result1.is_ok());

        // Second request should be rate limited
        let req2 = http::Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "192.168.1.1")
            .body(())
            .unwrap();

        let result2 = service.call(req2).await.unwrap();
        assert!(result2.is_err());

        match result2 {
            Err(RateLimitRejection::RateLimited(info)) => {
                assert_eq!(info.limit, 1);
                assert!(info.retry_after.as_secs() > 0);
            }
            _ => panic!("Expected RateLimited error"),
        }
    }

    #[tokio::test]
    async fn test_service_different_ips_different_limits() {
        // Create a strict limiter
        let limiter = RateLimiter::new(
            RateLimitConfig::builder()
                .endpoint_limit(
                    "test",
                    EndpointLimit {
                        requests: 1,
                        window: Duration::from_secs(60),
                        burst: 0,
                    },
                )
                .build(),
        );

        let inner_service = tower::service_fn(|_req: http::Request<()>| async move {
            Ok::<_, std::convert::Infallible>(http::Response::new(()))
        });

        let mut service = RateLimitService::new(inner_service, limiter, Arc::new(IpKeyExtractor));

        // Request from IP 1
        let req1 = http::Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "192.168.1.1")
            .body(())
            .unwrap();

        let result1 = service.call(req1).await.unwrap();
        assert!(result1.is_ok());

        // Request from IP 2 should still work (separate limit)
        let req2 = http::Request::builder()
            .uri("/test")
            .header("x-forwarded-for", "192.168.1.2")
            .body(())
            .unwrap();

        let result2 = service.call(req2).await.unwrap();
        assert!(result2.is_ok());
    }

    #[test]
    fn test_rate_limit_rejection_display() {
        use crate::rate_limit::RateLimitInfo;

        let info = RateLimitInfo {
            retry_after: Duration::from_secs(30),
            current_count: 5,
            limit: 3,
            window: Duration::from_secs(60),
        };

        let rejection = RateLimitRejection::<std::io::Error>::RateLimited(info);
        let display = format!("{rejection}");
        assert!(display.contains("Rate limited"));
    }

    #[test]
    fn test_rate_limit_rejection_inner_display() {
        use std::io;

        let err = io::Error::other("test error");
        let rejection = RateLimitRejection::Inner(err);
        let display = format!("{rejection}");
        assert!(display.contains("test error"));
    }
}
