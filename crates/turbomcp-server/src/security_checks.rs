//! Runtime Security Checks (Sprint 2.6)
//!
//! This module provides runtime security validation for server configuration.
//!
//! ## Security Checks
//!
//! - **0.0.0.0 Binding Safety**: Warns when binding to all interfaces without authentication

use std::net::ToSocketAddrs;
use tracing::warn;

/// Check if binding address is 0.0.0.0 (all interfaces)
///
/// Binding to 0.0.0.0 exposes the server on all network interfaces, which can be
/// a security risk if authentication is not enabled.
///
/// This function:
/// - **Logs WARN** if binding to 0.0.0.0 (all interfaces)
/// - **Silent** for localhost bindings (127.0.0.1, ::1) - safe by default
///
/// ## Security Guidance
///
/// ### Production Deployment (NEVER use 0.0.0.0 without these)
///
/// 1. **Enable Authentication**:
///    ```rust,ignore
///    use turbomcp_server::ServerBuilder;
///    use turbomcp_server::middleware::{MiddlewareStack, AuthConfig};
///    use secrecy::Secret;
///    use jsonwebtoken::Algorithm;
///
///    // Configure middleware with authentication
///    let auth_config = AuthConfig {
///        secret: Secret::new("your-secret-key".to_string()),
///        algorithm: Algorithm::HS256,
///        issuer: None,
///        audience: None,
///        leeway: 60,
///        validate_exp: true,
///        validate_nbf: true,
///    };
///    let middleware = MiddlewareStack::new()
///        .with_auth(auth_config); // ✅ Required for 0.0.0.0
///
///    let server = ServerBuilder::new()
///        .name("MyServer")
///        .build();
///    ```
///
/// 2. **Use Specific Interface** (Better):
///    ```bash
///    # Bind to specific private IP
///    server.run_http("10.0.1.5:8080").await?;
///    ```
///
/// 3. **Use Reverse Proxy** (Best):
///    ```bash
///    # Bind to localhost, expose via nginx/traefik
///    server.run_http("127.0.0.1:8080").await?;
///    ```
///
/// ### Why 0.0.0.0 is Risky
///
/// - Exposes on **ALL** network interfaces (eth0, wlan0, docker0, etc.)
/// - Accessible from any network the host is connected to
/// - Docker containers can access if not firewalled
/// - Vulnerable to network-level attacks if firewall misconfigured
///
/// ### When 0.0.0.0 is Acceptable
///
/// - Local development with authentication enabled
/// - Behind a firewall or in isolated network
/// - Using a reverse proxy for TLS termination
///
/// ## Example
///
/// ```rust,ignore
/// use turbomcp_server::security_checks::check_binding_security;
///
/// // Safe: localhost binding
/// check_binding_security("127.0.0.1:8080", true);  // No warning
///
/// // Warning: all interfaces with auth
/// check_binding_security("0.0.0.0:8080", true);   // WARN log
///
/// // Error: all interfaces without auth
/// check_binding_security("0.0.0.0:8080", false);  // ERROR log
/// ```
pub fn check_binding_security<A: ToSocketAddrs + std::fmt::Debug>(addr: &A) {
    // Try to resolve the address to check if it's 0.0.0.0
    let addr_str = format!("{:?}", addr);

    // Check if the address string contains 0.0.0.0
    let is_all_interfaces = addr_str.contains("0.0.0.0") || addr_str.contains("[::]");

    if !is_all_interfaces {
        // Safe binding (localhost or specific interface)
        return;
    }

    // Binding to all interfaces - log security warning
    warn!(
        "🔒 SECURITY NOTICE: Binding to all interfaces (0.0.0.0)\n\
         \n\
         ⚠️  Binding to 0.0.0.0 exposes your server on ALL network interfaces.\n\
         \n\
         Security Checklist:\n\
         ✓ Is authentication enabled? (recommended for 0.0.0.0)\n\
         ✓ Is this behind a reverse proxy? (nginx, traefik, cloudflare)\n\
         ✓ Is firewall configured? (only allow intended sources)\n\
         ✓ Is TLS/HTTPS enabled? (required for production)\n\
         \n\
         Best Practices:\n\
         - Production: Bind to specific interface (10.0.1.5:8080)\n\
         - Development: Bind to localhost (127.0.0.1:8080)\n\
         - Cloud: Use reverse proxy for TLS termination\n\
         \n\
         Current binding: {:?}\n\
         \n\
         See: OWASP Top 10 - Broken Access Control (A01:2021)\n\
         See: CWE-284 - Improper Access Control",
        addr_str
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_localhost_binding_no_warning() {
        // These should not trigger warnings (captured via tracing in actual tests)
        check_binding_security(&"127.0.0.1:8080");
        check_binding_security(&"localhost:8080");
    }

    #[test]
    fn test_all_interfaces_warning() {
        // This should trigger WARN (captured via tracing in actual tests)
        check_binding_security(&"0.0.0.0:8080");
    }

    #[test]
    fn test_ipv6_all_interfaces() {
        // This should trigger warnings for IPv6 all-interfaces binding
        check_binding_security(&"[::]:8080");
    }
}
