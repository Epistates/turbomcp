//! HTTP-frontend security primitives for the runtime proxy.
//!
//! The MCP spec recommends Origin enforcement and CORS as defense-in-depth
//! against DNS-rebinding / CSRF when a proxy binds to localhost. The runtime
//! proxy layers these checks around the supported `turbomcp-server` HTTP router
//! instead of relying on the legacy transport Axum adapter.
//!
//! Configuration is intentionally explicit:
//! - `allowed_origins` is empty by default → any browser-issued request that
//!   carries an `Origin` header is rejected with 403, regardless of bind
//!   address. Operators must opt in to browser access.
//! - Server-to-server clients (no `Origin` header) are unaffected.
//! - When the allowlist is non-empty we layer `CorsLayer` on top so preflight
//!   requests get the spec-compliant ACAO/ACAM/ACAH responses.

use std::sync::Arc;

use axum::{
    extract::Request,
    http::{HeaderName, HeaderValue, Method, StatusCode, header},
    middleware::Next,
    response::Response,
};
use tower_http::cors::{AllowOrigin, CorsLayer};

/// MCP Streamable HTTP session identifier header.
const MCP_SESSION_ID_HEADER: HeaderName = HeaderName::from_static("mcp-session-id");
/// Header used by browser SSE clients to resume an event stream.
const LAST_EVENT_ID_HEADER: HeaderName = HeaderName::from_static("last-event-id");
/// Negotiated protocol version, which Streamable HTTP clients send on every
/// request after `initialize`.
const MCP_PROTOCOL_VERSION_HEADER: HeaderName = HeaderName::from_static("mcp-protocol-version");

/// Shared state for the origin-validation middleware.
///
/// Cloned per request via axum's `Extension`/`State` plumbing, so we wrap the
/// allowlist in `Arc` to keep the per-request cost a single refcount bump.
#[derive(Debug, Clone)]
pub struct OriginAllowlist {
    inner: Arc<Vec<HeaderValue>>,
}

impl OriginAllowlist {
    /// Build an allowlist from a list of fully-qualified origins
    /// (e.g. `https://app.example.com`). Origins that fail to parse as
    /// `HeaderValue` are silently dropped — caller should validate before
    /// reaching this point.
    pub fn new<I, S>(origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let inner = origins
            .into_iter()
            .filter_map(|o| HeaderValue::from_str(o.as_ref()).ok())
            .collect::<Vec<_>>();
        Self {
            inner: Arc::new(inner),
        }
    }

    /// Are there any allowed origins configured?
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Iterate over allowed origins as `HeaderValue`.
    pub fn header_values(&self) -> impl Iterator<Item = &HeaderValue> {
        self.inner.iter()
    }

    fn matches(&self, candidate: &HeaderValue) -> bool {
        self.inner.iter().any(|allowed| allowed == candidate)
    }
}

/// Axum middleware that rejects browser-issued requests whose `Origin` header
/// is not on the configured allowlist.
///
/// Behavior:
/// - **No `Origin` header** → allow. Non-browser MCP clients (other servers,
///   curl, native apps) issue requests without `Origin` and are unaffected.
/// - **`Origin: null`** → reject with 403. The literal `null` is what browsers
///   send for sandboxed iframes / `data:` documents and must never be on an
///   allowlist by accident.
/// - **`Origin` in allowlist** → allow.
/// - **Otherwise** → reject with 403.
///
/// # Errors
///
/// Returns `StatusCode::FORBIDDEN` when the request's `Origin` header is the
/// literal `null` or is not present in the configured allowlist.
pub async fn origin_guard(
    axum::extract::State(allowlist): axum::extract::State<OriginAllowlist>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let Some(origin_value) = request.headers().get(header::ORIGIN) else {
        return Ok(next.run(request).await);
    };
    if origin_value.as_bytes() == b"null" || !allowlist.matches(origin_value) {
        tracing::warn!(
            origin = %String::from_utf8_lossy(origin_value.as_bytes()),
            "Rejecting request with disallowed Origin header"
        );
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(next.run(request).await)
}

/// Build a `CorsLayer` from the configured allowlist.
///
/// Returns `None` if the allowlist is empty (defense-in-depth: no CORS layer
/// means browsers also can't trick the server into echoing back permissive
/// headers; the origin guard above will reject the request anyway).
#[must_use]
pub fn build_cors_layer(allowlist: &OriginAllowlist) -> Option<CorsLayer> {
    if allowlist.is_empty() {
        return None;
    }
    let origins = allowlist.header_values().cloned().collect::<Vec<_>>();
    // Streamable HTTP needs all of it from a browser: `MCP-Protocol-Version`
    // rides on every post-initialize request, and `DELETE` is how a client
    // ends its session. Leaving either out of the preflight answer made the
    // browser refuse the request before it was sent.
    Some(
        CorsLayer::new()
            .allow_origin(AllowOrigin::list(origins))
            .allow_methods([Method::GET, Method::POST, Method::DELETE, Method::OPTIONS])
            .allow_headers([
                header::CONTENT_TYPE,
                header::AUTHORIZATION,
                header::ACCEPT,
                MCP_SESSION_ID_HEADER,
                MCP_PROTOCOL_VERSION_HEADER,
                LAST_EVENT_ID_HEADER,
            ])
            .expose_headers([MCP_SESSION_ID_HEADER, MCP_PROTOCOL_VERSION_HEADER]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_allowlist_is_empty() {
        let allowlist = OriginAllowlist::new(Vec::<String>::new());
        assert!(allowlist.is_empty());
        assert!(build_cors_layer(&allowlist).is_none());
    }

    #[test]
    fn allowlist_matches_exact_origin() {
        let allowlist = OriginAllowlist::new(["https://app.example.com", "http://localhost:8080"]);
        assert!(!allowlist.is_empty());
        let h = HeaderValue::from_static("https://app.example.com");
        assert!(allowlist.matches(&h));
        let other = HeaderValue::from_static("https://evil.example.com");
        assert!(!allowlist.matches(&other));
    }

    #[test]
    fn allowlist_does_not_match_null_origin() {
        let allowlist = OriginAllowlist::new(["https://app.example.com"]);
        let h = HeaderValue::from_static("null");
        assert!(!allowlist.matches(&h));
    }

    #[test]
    fn cors_layer_built_when_allowlist_non_empty() {
        let allowlist = OriginAllowlist::new(["https://app.example.com"]);
        assert!(build_cors_layer(&allowlist).is_some());
    }

    /// A browser preflights a post-initialize request because it carries
    /// `MCP-Protocol-Version`, and preflights `DELETE` because it isn't a
    /// simple method. Both have to be allowed or the browser never sends them.
    #[tokio::test]
    async fn preflight_allows_the_streamable_http_surface() {
        use tower::ServiceExt;

        let allowlist = OriginAllowlist::new(["https://app.example.com"]);
        let app = axum::Router::new()
            .route("/mcp", axum::routing::any(|| async { "ok" }))
            .layer(build_cors_layer(&allowlist).expect("cors layer"));

        let preflight = axum::http::Request::builder()
            .method(Method::OPTIONS)
            .uri("/mcp")
            .header(header::ORIGIN, "https://app.example.com")
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "DELETE")
            .header(
                header::ACCESS_CONTROL_REQUEST_HEADERS,
                "mcp-protocol-version,mcp-session-id",
            )
            .body(axum::body::Body::empty())
            .unwrap();
        let response = app.oneshot(preflight).await.unwrap();

        let allowed = |name| {
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_ascii_lowercase()
        };
        assert!(allowed(header::ACCESS_CONTROL_ALLOW_METHODS).contains("delete"));
        assert!(allowed(header::ACCESS_CONTROL_ALLOW_HEADERS).contains("mcp-protocol-version"));
    }
}
