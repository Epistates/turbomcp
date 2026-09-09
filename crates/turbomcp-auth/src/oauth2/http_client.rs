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

#[cfg(feature = "dpop")]
use std::sync::Arc;
#[cfg(feature = "dpop")]
use tokio::sync::Mutex;
#[cfg(feature = "dpop")]
use turbomcp_dpop::{DpopKeyPair, DpopProofGenerator};

/// Type alias for the HTTP request used by oauth2
pub type HttpRequest = http::Request<Vec<u8>>;
/// Type alias for the HTTP response used by oauth2
pub type HttpResponse = http::Response<Vec<u8>>;

/// DPoP binding for OAuth token endpoint requests (RFC 9449).
///
/// Holds the proof generator, an optional pinned key pair, and a cache for the
/// most recently observed `DPoP-Nonce` from the authorization server. When
/// attached to an [`OAuth2HttpClient`], every outgoing request gets a fresh
/// `DPoP` proof header bound to the actual method/URL, and `use_dpop_nonce`
/// challenges from the AS are followed once with the supplied nonce per
/// RFC 9449 §8.
#[cfg(feature = "dpop")]
#[derive(Clone)]
pub struct DpopBinding {
    generator: Arc<DpopProofGenerator>,
    key_pair: Option<Arc<DpopKeyPair>>,
    server_nonce: Arc<Mutex<Option<String>>>,
}

#[cfg(feature = "dpop")]
impl DpopBinding {
    /// Create a binding with the given generator. The generator's default key
    /// (or one created on demand) is used unless [`Self::with_key_pair`] is called.
    pub fn new(generator: Arc<DpopProofGenerator>) -> Self {
        Self {
            generator,
            key_pair: None,
            server_nonce: Arc::new(Mutex::new(None)),
        }
    }

    /// Pin a specific key pair for proofs. Use this when the caller needs the
    /// same key across the token endpoint and resource server (so `cnf.jkt`
    /// matches), rather than letting the proof generator pick one.
    #[must_use]
    pub fn with_key_pair(mut self, key: Arc<DpopKeyPair>) -> Self {
        self.key_pair = Some(key);
        self
    }
}

#[cfg(feature = "dpop")]
impl std::fmt::Debug for DpopBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DpopBinding")
            .field("key_pair", &self.key_pair.is_some())
            .finish()
    }
}

/// HTTP client adapter for oauth2 using reqwest 0.13+
///
/// This wrapper implements `AsyncHttpClient` to bridge the gap between
/// reqwest 0.13's API and oauth2 5.0's expected interface.
#[derive(Clone)]
pub struct OAuth2HttpClient {
    inner: reqwest::Client,
    /// Optional DPoP binding. When present, every request gets a `DPoP` header.
    #[cfg(feature = "dpop")]
    dpop: Option<DpopBinding>,
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

        Ok(Self {
            inner,
            #[cfg(feature = "dpop")]
            dpop: None,
        })
    }

    /// Create from an existing reqwest client
    ///
    /// # Warning
    /// Ensure the client is configured with `redirect::Policy::none()`
    /// to prevent SSRF attacks in OAuth flows.
    pub fn from_client(client: reqwest::Client) -> Self {
        Self {
            inner: client,
            #[cfg(feature = "dpop")]
            dpop: None,
        }
    }

    /// Attach a DPoP binding so every outgoing request carries a `DPoP` proof
    /// header per RFC 9449. Without this, `OAuth2Client::exchange_code_for_token`
    /// and friends issue plain bearer requests even when the configured
    /// authorization server requires DPoP.
    #[cfg(feature = "dpop")]
    #[must_use]
    pub fn with_dpop(mut self, binding: DpopBinding) -> Self {
        self.dpop = Some(binding);
        self
    }

    /// Generate a DPoP proof JWT for the given method/URL using the binding's
    /// generator and (optional) pinned key. Returns the proof or any error
    /// from the generator.
    #[cfg(feature = "dpop")]
    async fn build_dpop_proof(
        &self,
        method: &str,
        url: &str,
        access_token: Option<&str>,
        nonce: Option<&str>,
    ) -> Result<String, OAuth2HttpError> {
        let Some(binding) = &self.dpop else {
            return Err(OAuth2HttpError::Dpop(
                "DPoP binding missing when generating proof".to_string(),
            ));
        };
        let key_ref = binding.key_pair.as_deref();
        let proof = binding
            .generator
            .generate_proof_with_params(method, url, access_token, nonce, key_ref)
            .await
            .map_err(|e| OAuth2HttpError::Dpop(e.to_string()))?;
        Ok(proof.to_jwt_string())
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

        // Send once. If a DPoP binding is configured we attach a proof, capture
        // any returned `DPoP-Nonce`, and retry once on `use_dpop_nonce`. Errors
        // from proof generation surface as `OAuth2HttpError::Dpop`.
        #[cfg(feature = "dpop")]
        if self.dpop.is_some() {
            return self.send_with_dpop(&parts, &method, &url, body).await;
        }

        let mut req_builder = self.inner.request(method, &url);
        for (name, value) in parts.headers.iter() {
            req_builder = req_builder.header(name.as_str(), value.as_bytes());
        }
        req_builder = req_builder.body(body);

        let response = req_builder.send().await?;
        Self::convert_response(response).await
    }

    /// Convert a reqwest::Response into the oauth2::http::Response shape.
    async fn convert_response(
        response: reqwest::Response,
    ) -> Result<HttpResponse, OAuth2HttpError> {
        let status = StatusCode::from_u16(response.status().as_u16())
            .map_err(|_| OAuth2HttpError::InvalidHeader("Invalid status code".to_string()))?;

        let mut builder = http::Response::builder().status(status);

        for (name, value) in response.headers().iter() {
            let header_value = HeaderValue::from_bytes(value.as_bytes())
                .map_err(|e| OAuth2HttpError::InvalidHeader(e.to_string()))?;
            builder = builder.header(name.as_str(), header_value);
        }

        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| OAuth2HttpError::BodyRead(e.to_string()))?;

        builder
            .body(body_bytes.to_vec())
            .map_err(|e| OAuth2HttpError::InvalidHeader(e.to_string()))
    }

    /// Send a request with the DPoP binding attached.
    ///
    /// If the AS responds with `error="use_dpop_nonce"` and a `DPoP-Nonce`
    /// header (RFC 9449 §8), the request is retried once with the supplied
    /// nonce included in the proof.
    #[cfg(feature = "dpop")]
    async fn send_with_dpop(
        &self,
        parts: &http::request::Parts,
        method: &reqwest::Method,
        url: &str,
        body: Vec<u8>,
    ) -> Result<HttpResponse, OAuth2HttpError> {
        let cached_nonce = {
            let guard = self.dpop.as_ref().unwrap().server_nonce.lock().await;
            guard.clone()
        };

        let proof = self
            .build_dpop_proof(method.as_str(), url, None, cached_nonce.as_deref())
            .await?;

        let mut req = self.inner.request(method.clone(), url);
        for (name, value) in parts.headers.iter() {
            req = req.header(name.as_str(), value.as_bytes());
        }
        req = req.header("DPoP", proof).body(body.clone());

        let response = req.send().await?;

        // Capture any DPoP-Nonce the server hands back so the next request
        // (or a retry) carries it.
        if let Some(nonce_value) = response.headers().get("DPoP-Nonce")
            && let Ok(s) = nonce_value.to_str()
        {
            let mut guard = self.dpop.as_ref().unwrap().server_nonce.lock().await;
            *guard = Some(s.to_string());
        }

        // Detect `use_dpop_nonce` challenge: per RFC 9449 §8, AS replies 400
        // (or 401 at RS) with `error="use_dpop_nonce"` and a fresh nonce.
        if response.status().as_u16() == 400 || response.status().as_u16() == 401 {
            let new_nonce = response
                .headers()
                .get("DPoP-Nonce")
                .and_then(|v| v.to_str().ok())
                .map(str::to_string);

            // We need the body to inspect the error. Buffer the response, then
            // either retry or return it.
            let buffered = Self::convert_response(response).await?;

            if let Some(nonce) = new_nonce.as_deref() {
                // RFC 9449 §8: the response body is JSON `{ "error": "use_dpop_nonce", ... }`.
                // Substring matching can false-positive on `error_description` or
                // unrelated text, so parse the JSON and require an exact match
                // on the `error` field.
                let is_nonce_challenge =
                    serde_json::from_slice::<serde_json::Value>(buffered.body())
                        .ok()
                        .and_then(|v| {
                            v.get("error")
                                .and_then(|e| e.as_str())
                                .map(|s| s == "use_dpop_nonce")
                        })
                        .unwrap_or(false);
                if is_nonce_challenge {
                    let proof = self
                        .build_dpop_proof(method.as_str(), url, None, Some(nonce))
                        .await?;

                    let mut retry = self.inner.request(method.clone(), url);
                    for (name, value) in parts.headers.iter() {
                        retry = retry.header(name.as_str(), value.as_bytes());
                    }
                    retry = retry.header("DPoP", proof).body(body);

                    let retry_response = retry.send().await?;
                    if let Some(n) = retry_response
                        .headers()
                        .get("DPoP-Nonce")
                        .and_then(|v| v.to_str().ok())
                    {
                        let mut guard = self.dpop.as_ref().unwrap().server_nonce.lock().await;
                        *guard = Some(n.to_string());
                    }
                    return Self::convert_response(retry_response).await;
                }
            }

            return Ok(buffered);
        }

        Self::convert_response(response).await
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

    /// DPoP proof generation failed
    Dpop(String),
}

impl std::fmt::Display for OAuth2HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Request(e) => write!(f, "HTTP request failed: {e}"),
            Self::InvalidHeader(msg) => write!(f, "Invalid header value: {msg}"),
            Self::BodyRead(msg) => write!(f, "Failed to read response body: {msg}"),
            Self::Dpop(msg) => write!(f, "DPoP proof generation failed: {msg}"),
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
