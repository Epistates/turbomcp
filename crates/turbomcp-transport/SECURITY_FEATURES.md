# TurboMCP Transport Security Features

## Overview

TurboMCP Transport provides comprehensive security features out of the box, with environment-aware configurations that make it easy to deploy securely in development, staging, and production environments.

## 🛡️ Security Architecture

### Defense in Depth

TurboMCP implements a comprehensive security model with multiple layers of protection:

1. **CORS Protection** - Prevents cross-origin attacks
2. **Security Headers** - Protects against common web vulnerabilities  
3. **Rate Limiting** - Prevents DoS and abuse attacks
4. **Authentication** - JWT and API key validation
5. **TLS Configuration** - Secure transport encryption
6. **Request Validation** - Input sanitization and size limits

### Environment-Aware Security

Security configurations automatically adjust based on your deployment environment:

- **Development**: Permissive settings for easy local development
- **Staging**: Moderate security for testing production-like conditions
- **Production**: Maximum security with strict defaults

## 🚀 Quick Start

### Basic Usage

```rust
use turbomcp_transport::{AxumMcpExt, McpServerConfig};

// Development (permissive)
let app = Router::new()
    .merge(Router::<()>::turbo_mcp_routes_for_merge(
        my_service,
        McpServerConfig::development()
    ));

// Production (strict security)
let app = Router::new()
    .merge(Router::<()>::turbo_mcp_routes_for_merge(
        my_service,
        McpServerConfig::production()
    ));
```

### Custom Configuration

```rust
let config = McpServerConfig::production()
    .with_cors_origins(vec!["https://app.example.com".to_string()])
    .with_custom_csp("default-src 'self'; connect-src 'self' wss:")
    .with_rate_limit(100, 20)
    .with_jwt_auth("your-secret-key".to_string());

let app = Router::new()
    .merge(Router::<()>::turbo_mcp_routes_for_merge(my_service, config));
```

## 🔧 Configuration

### Environment Variables

TurboMCP automatically loads security configuration from environment variables:

#### TLS Configuration
```bash
# TLS Certificate and Key Files
export TLS_CERT_FILE="/path/to/cert.pem"
export TLS_KEY_FILE="/path/to/key.pem"
export TLS_MIN_VERSION="1.3"          # 1.2 or 1.3 (default: 1.3)
export TLS_ENABLE_HTTP2="true"        # Enable HTTP/2 (default: true)
```

#### CORS Configuration
```bash
# Allowed Origins (comma-separated)
export CORS_ALLOWED_ORIGINS="https://app.example.com,https://admin.example.com"
```

#### Authentication Configuration
```bash
# JWT Authentication
export AUTH_JWT_SECRET="your-secret-key"

# API Key Authentication  
export AUTH_API_KEY_HEADER="X-API-Key"

# Enable/disable authentication
export AUTH_ENABLED="true"
```

### Programmatic Configuration

#### CORS Configuration

```rust
let cors_config = CorsConfig::strict()
    .with_origins(vec!["https://trusted.com".to_string()])
    .with_credentials(true);
```

**Available CORS Presets:**
- `CorsConfig::permissive()` - Allows all origins (development)
- `CorsConfig::restrictive()` - Specific origins with credentials
- `CorsConfig::strict()` - Minimal headers, specific origins only
- `CorsConfig::disabled()` - No CORS headers

#### Security Headers

```rust
let security_config = SecurityConfig::production();
```

**Security Header Presets:**
- `SecurityConfig::development()` - Minimal headers for easy development
- `SecurityConfig::staging()` - Moderate security headers
- `SecurityConfig::production()` - Full security headers suite

**Headers Applied in Production:**
- `Content-Security-Policy`: Prevents XSS and code injection
- `Strict-Transport-Security`: Enforces HTTPS (2 years)
- `X-Frame-Options`: Prevents clickjacking (DENY)
- `X-Content-Type-Options`: Prevents MIME sniffing (nosniff)
- `Referrer-Policy`: Controls referrer information (no-referrer)
- `Permissions-Policy`: Restricts browser features
- `X-XSS-Protection`: Legacy XSS protection
- `X-DNS-Prefetch-Control`: Controls DNS prefetching

#### Rate Limiting

```rust
let rate_config = RateLimitConfig::strict()
    .with_requests_per_minute(120)
    .with_burst_capacity(20)
    .with_key_strategy(RateLimitKey::IpAddress);
```

**Rate Limiting Strategies:**
- `RateLimitKey::IpAddress` - Rate limit by client IP
- `RateLimitKey::UserId` - Rate limit by authenticated user
- `RateLimitKey::Custom` - Custom key extraction logic

**Rate Limiting Presets:**
- `RateLimitConfig::disabled()` - No rate limiting
- `RateLimitConfig::moderate()` - 300 requests/minute, 50 burst
- `RateLimitConfig::strict()` - 120 requests/minute, 20 burst

## 🔒 Security Features Detail

### 1. CORS Protection

Prevents unauthorized cross-origin requests:

```rust
// Environment-aware CORS
let config = McpServerConfig::production(); // Loads from CORS_ALLOWED_ORIGINS

// Manual configuration
let config = McpServerConfig::production()
    .with_cors_origins(vec!["https://app.example.com".to_string()]);
```

**Security Benefits:**
- Prevents cross-site request forgery (CSRF)
- Blocks unauthorized API access from malicious sites
- Configurable per environment

### 2. Security Headers

Comprehensive HTTP security headers:

```rust
let config = McpServerConfig::production()
    .with_custom_csp("default-src 'self'; script-src 'self' 'unsafe-inline'");
```

**Protection Against:**
- Cross-Site Scripting (XSS)
- Clickjacking attacks
- MIME type sniffing
- Man-in-the-middle attacks
- Information leakage

### 3. Rate Limiting

Token bucket algorithm prevents abuse:

```rust
let config = McpServerConfig::production()
    .with_rate_limit(100, 20); // 100 requests/minute, 20 burst
```

**Features:**
- Per-IP rate limiting by default
- Configurable burst capacity
- Rate limit headers in responses
- Custom key extraction strategies

### 4. Authentication

JWT and API key authentication:

```rust
// JWT Authentication
let config = McpServerConfig::production()
    .with_jwt_auth("your-secret-key".to_string());

// API Key Authentication
let config = McpServerConfig::production()
    .with_api_key_auth("X-API-Key".to_string());
```

**Features:**
- JWT token validation with configurable secrets
- API key authentication with custom headers
- Request context injection for authenticated users
- Extensible for custom authentication providers

### 5. TLS Configuration

Secure transport configuration:

```bash
export TLS_CERT_FILE="/etc/ssl/certs/server.pem"
export TLS_KEY_FILE="/etc/ssl/private/server.key"
export TLS_MIN_VERSION="1.3"
```

**Features:**
- Automatic TLS configuration from environment
- Configurable minimum TLS version
- HTTP/2 support
- Production-ready defaults

## 🎯 Security Best Practices

### Development Environment

```rust
let config = McpServerConfig::development();
// - Allows all CORS origins for easy testing
// - Minimal security headers  
// - No rate limiting
// - No authentication required
```

### Staging Environment

```rust
let config = McpServerConfig::staging()
    .with_cors_origins(vec!["https://staging.example.com".to_string()]);
// - Moderate security headers
// - CORS origins from environment or explicit configuration
// - Basic rate limiting (300 req/min)
// - Optional authentication
```

### Production Environment

```bash
# Required environment variables
export CORS_ALLOWED_ORIGINS="https://app.example.com,https://admin.example.com"
export TLS_CERT_FILE="/etc/ssl/certs/server.pem"
export TLS_KEY_FILE="/etc/ssl/private/server.key"
export AUTH_JWT_SECRET="your-very-secure-secret-key"
```

```rust
let config = McpServerConfig::production();
// - Full security headers suite
// - Strict CORS (only configured origins)
// - Aggressive rate limiting (120 req/min)
// - Authentication from environment
// - TLS 1.3 minimum
```

### Docker Deployment

```dockerfile
FROM rust:1.89-alpine AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM alpine:latest
RUN apk add --no-cache ca-certificates

# Security configuration
ENV TLS_CERT_FILE="/etc/ssl/certs/server.pem"
ENV TLS_KEY_FILE="/etc/ssl/private/server.key"
ENV TLS_MIN_VERSION="1.3"
ENV CORS_ALLOWED_ORIGINS="https://app.example.com"
ENV AUTH_JWT_SECRET="your-secret-key"

COPY --from=builder /app/target/release/your-server /usr/local/bin/
CMD ["your-server"]
```

## 🧪 Testing Security

### Unit Tests

TurboMCP includes comprehensive security tests:

```bash
cargo test --features http -- security
cargo test --features http -- cors
cargo test --features http -- rate_limit
cargo test --features http -- auth
```

### Integration Testing

```rust
#[tokio::test]
async fn test_production_security() {
    let config = McpServerConfig::production()
        .with_cors_origins(vec!["https://trusted.com".to_string()]);
    
    let app = Router::<()>::turbo_mcp_routes_for_merge(service, config);
    
    // Test CORS enforcement
    let response = app.oneshot(
        Request::builder()
            .uri("/mcp/capabilities")
            .header("Origin", "https://malicious.com")
            .body(Body::empty())
            .unwrap()
    ).await.unwrap();
    
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
```

### Load Testing

Test rate limiting under load:

```bash
# Install wrk
brew install wrk  # macOS
apt-get install wrk  # Ubuntu

# Test rate limiting
wrk -t12 -c400 -d30s http://localhost:3000/mcp/capabilities
```

## 🚨 Security Considerations

### Production Checklist

- [ ] **TLS Configured**: Certificate and key files set via environment variables
- [ ] **CORS Restricted**: Only trusted origins in `CORS_ALLOWED_ORIGINS`
- [ ] **Authentication Enabled**: JWT secret or API key configured
- [ ] **Rate Limiting Active**: Appropriate limits for your use case
- [ ] **Security Headers**: Full suite enabled in production config
- [ ] **Monitoring**: Log security events and rate limit violations
- [ ] **Regular Updates**: Keep TurboMCP and dependencies updated

### Common Vulnerabilities Mitigated

1. **Cross-Site Scripting (XSS)** - Content Security Policy headers
2. **Cross-Site Request Forgery (CSRF)** - CORS restrictions  
3. **Clickjacking** - X-Frame-Options header
4. **MIME Sniffing** - X-Content-Type-Options header
5. **Man-in-the-Middle** - HSTS header and TLS enforcement
6. **Denial of Service** - Rate limiting and request size limits
7. **Information Disclosure** - Referrer-Policy and error handling

### Security Headers Reference

| Header | Purpose | Production Value |
|--------|---------|------------------|
| Content-Security-Policy | Prevent XSS and code injection | `default-src 'self'; script-src 'self'` |
| Strict-Transport-Security | Enforce HTTPS | `max-age=63072000` (2 years) |
| X-Frame-Options | Prevent clickjacking | `DENY` |
| X-Content-Type-Options | Prevent MIME sniffing | `nosniff` |
| Referrer-Policy | Control referrer information | `no-referrer` |
| Permissions-Policy | Restrict browser features | `geolocation=(), microphone=(), camera=()` |

## 📚 Additional Resources

- [OWASP Security Headers](https://owasp.org/www-project-secure-headers/)
- [Mozilla Web Security](https://infosec.mozilla.org/guidelines/web_security)
- [TLS Best Practices](https://wiki.mozilla.org/Security/Server_Side_TLS)
- [CORS Specification](https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS)

## 🤝 Contributing

Security is a team effort! If you find security issues or have suggestions:

1. **Security Issues**: Report privately via GitHub Security Advisories
2. **Feature Requests**: Open an issue with the `security` label
3. **Documentation**: Help improve this security guide

## 📄 License

This security documentation is part of TurboMCP and follows the same MIT license.