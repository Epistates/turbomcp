//! Security headers middleware for comprehensive HTTP security

use axum::{
    extract::State,
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};

use crate::axum::config::{SecurityConfig, FrameOptions};

/// Security headers middleware - applies comprehensive security headers
///
/// This middleware applies various security headers based on the SecurityConfig
/// to protect against common web vulnerabilities like XSS, clickjacking, and
/// content type sniffing attacks.
pub async fn security_headers_middleware(
    State(security_config): State<SecurityConfig>,
    request: axum::http::Request<axum::body::Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let mut response = next.run(request).await;

    // Apply security headers based on configuration
    let headers = response.headers_mut();

    // Content Security Policy
    if let Some(csp) = &security_config.content_security_policy
        && let Ok(header_value) = HeaderValue::from_str(csp)
    {
        headers.insert("Content-Security-Policy", header_value);
    }

    // HTTP Strict Transport Security
    if let Some(hsts_max_age) = security_config.hsts_max_age {
        let hsts_value = format!("max-age={}", hsts_max_age.as_secs());
        if let Ok(header_value) = HeaderValue::from_str(&hsts_value) {
            headers.insert("Strict-Transport-Security", header_value);
        }
    }

    // X-Frame-Options
    match security_config.frame_options {
        FrameOptions::Deny => {
            headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
        }
        FrameOptions::SameOrigin => {
            headers.insert("X-Frame-Options", HeaderValue::from_static("SAMEORIGIN"));
        }
        FrameOptions::AllowFrom(ref origin) => {
            let frame_value = format!("ALLOW-FROM {}", origin);
            if let Ok(header_value) = HeaderValue::from_str(&frame_value) {
                headers.insert("X-Frame-Options", header_value);
            }
        }
        FrameOptions::Disabled => {}
    }

    // X-Content-Type-Options
    if security_config.content_type_options {
        headers.insert(
            "X-Content-Type-Options",
            HeaderValue::from_static("nosniff"),
        );
    }

    // Referrer-Policy
    if let Some(referrer_policy) = &security_config.referrer_policy
        && let Ok(header_value) = HeaderValue::from_str(referrer_policy)
    {
        headers.insert("Referrer-Policy", header_value);
    }

    // Permissions-Policy
    if let Some(permissions_policy) = &security_config.permissions_policy
        && let Ok(header_value) = HeaderValue::from_str(permissions_policy)
    {
        headers.insert("Permissions-Policy", header_value);
    }

    // Additional security headers
    headers.insert(
        "X-XSS-Protection",
        HeaderValue::from_static("1; mode=block"),
    );
    headers.insert("X-DNS-Prefetch-Control", HeaderValue::from_static("off"));

    Ok(response)
}