//! HTTP Client Adapter for OAuth2
//!
//! This module provides a custom HTTP client adapter that bridges reqwest 0.13+
//! with the oauth2 crate's `AsyncHttpClient` trait. This allows TurboMCP to use
//! the latest reqwest version while maintaining compatibility with oauth2.
//!
//! ## Why This Adapter Exists
//!
//! The oauth2 crate 5.0 depends on reqwest 0.12.x and implements `AsyncHttpClient`
//! for `oauth2::reqwest::Client`. When the workspace uses reqwest 0.13+, the types
//! are incompatible. This adapter implements the trait manually.
//!
//! ## Security Configuration
//!
//! The adapter is configured to:
//! - NOT follow redirects (SSRF protection per OAuth2 security guidance)
//! - Use rustls for TLS (no OpenSSL dependency)

use oauth2::AsyncHttpClient;
use oauth2::http::{self, HeaderValue, StatusCode};
use std::error::Error as StdError;
use std::future::Future;
use std::pin::Pin;

/// Type alias for the HTTP request used by oauth2
pub type HttpRequest = http::Request<Vec<u8>>;
/// Type alias for the HTTP response used by oauth2
pub type HttpResponse = http::Response<Vec<u8>>;

/// HTTP client adapter for oauth2 using reqwest 0.13+
///
/// This wrapper implements `AsyncHttpClient` to bridge the gap between
/// reqwest 0.13's API and oauth2 5.0's expected interface.
#[derive(Clone)]
pub struct OAuth2HttpClient {
    inner: reqwest::Client,
}

impl OAuth2HttpClient {
    /// Create a new OAuth2 HTTP client with security-hardened defaults
    ///
    /// # Security Configuration
    /// - Redirects disabled (SSRF protection)
    /// - Connection pooling enabled (performance)
    /// - Timeout configured (DoS protection)
    pub fn new() -> Result<Self, reqwest::Error> {
        let inner = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self { inner })
    }

    /// Create from an existing reqwest client
    ///
    /// # Warning
    /// Ensure the client is configured with `redirect::Policy::none()`
    /// to prevent SSRF attacks in OAuth flows.
    pub fn from_client(client: reqwest::Client) -> Self {
        Self { inner: client }
    }

    /// Execute an HTTP request and convert to oauth2 response format
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, OAuth2HttpError> {
        // Convert oauth2::http::Request to reqwest::Request
        let (parts, body) = request.into_parts();

        let url = parts.uri.to_string();
        let method = match parts.method.as_str() {
            "GET" => reqwest::Method::GET,
            "POST" => reqwest::Method::POST,
            "PUT" => reqwest::Method::PUT,
            "DELETE" => reqwest::Method::DELETE,
            "PATCH" => reqwest::Method::PATCH,
            "HEAD" => reqwest::Method::HEAD,
            "OPTIONS" => reqwest::Method::OPTIONS,
            other => reqwest::Method::from_bytes(other.as_bytes())
                .map_err(|_| OAuth2HttpError::InvalidHeader(format!("Invalid method: {other}")))?,
        };

        let mut req_builder = self.inner.request(method, &url);

        // Copy headers
        for (name, value) in parts.headers.iter() {
            req_builder = req_builder.header(name.as_str(), value.as_bytes());
        }

        // Set body
        req_builder = req_builder.body(body);

        // Execute request
        let response = req_builder.send().await?;

        // Convert reqwest::Response to oauth2::http::Response
        let status = StatusCode::from_u16(response.status().as_u16())
            .map_err(|_| OAuth2HttpError::InvalidHeader("Invalid status code".to_string()))?;

        let mut builder = http::Response::builder().status(status);

        // Copy response headers
        for (name, value) in response.headers().iter() {
            let header_value = HeaderValue::from_bytes(value.as_bytes())
                .map_err(|e| OAuth2HttpError::InvalidHeader(e.to_string()))?;
            builder = builder.header(name.as_str(), header_value);
        }

        // Read body
        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| OAuth2HttpError::BodyRead(e.to_string()))?;

        builder
            .body(body_bytes.to_vec())
            .map_err(|e| OAuth2HttpError::InvalidHeader(e.to_string()))
    }
}

impl Default for OAuth2HttpClient {
    fn default() -> Self {
        Self::new().expect("Failed to create default HTTP client")
    }
}

impl std::fmt::Debug for OAuth2HttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuth2HttpClient")
            .field("inner", &"<reqwest::Client>")
            .finish()
    }
}

/// Error type for HTTP client operations
#[derive(Debug)]
pub enum OAuth2HttpError {
    /// Request execution failed
    Request(reqwest::Error),

    /// Invalid header value
    InvalidHeader(String),

    /// Response body read failed
    BodyRead(String),
}

impl std::fmt::Display for OAuth2HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(e) => write!(f, "HTTP request failed: {e}"),
            Self::InvalidHeader(msg) => write!(f, "Invalid header value: {msg}"),
            Self::BodyRead(msg) => write!(f, "Failed to read response body: {msg}"),
        }
    }
}

impl StdError for OAuth2HttpError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Request(e) => Some(e),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for OAuth2HttpError {
    fn from(e: reqwest::Error) -> Self {
        Self::Request(e)
    }
}

/// Future type for the OAuth2 HTTP client
pub type OAuth2HttpFuture<'c> =
    Pin<Box<dyn Future<Output = Result<HttpResponse, OAuth2HttpError>> + Send + 'c>>;

impl<'c> AsyncHttpClient<'c> for OAuth2HttpClient {
    type Error = OAuth2HttpError;
    type Future = OAuth2HttpFuture<'c>;

    fn call(&'c self, request: HttpRequest) -> Self::Future {
        Box::pin(async move { self.execute(request).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = OAuth2HttpClient::new();
        assert!(client.is_ok());
    }

    #[test]
    fn test_default() {
        let _client = OAuth2HttpClient::default();
    }

    #[test]
    fn test_error_display() {
        let err = OAuth2HttpError::InvalidHeader("test".to_string());
        assert!(err.to_string().contains("Invalid header value"));
    }
}
