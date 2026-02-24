# Authentication & Authorization

Secure your MCP server with OAuth 2.1, API keys, and role-based access control.

## Overview

TurboMCP provides enterprise-grade authentication and authorization:

- **OAuth 2.1** - Industry-standard auth with PKCE
- **JWT Tokens** - Stateless authentication
- **API Keys** - Simple key-based access
- **RBAC** - Role-Based Access Control
- **DPoP** - Demonstration of Proof-of-Possession (RFC 9449)

## Quick Start: OAuth 2.1

### 1. Enable Auth Feature

Add to `Cargo.toml`:

```toml
turbomcp = { version = "3.0.0-beta.5", features = ["auth"] }
```

### 2. Configure OAuth

```rust
use turbomcp::auth::OAuthConfig;

let oauth = OAuthConfig::new()
    .client_id("your-client-id")
    .client_secret("your-client-secret")
    .redirect_uri("http://localhost:8080/callback")
    .scopes(vec!["openid", "profile", "email"]);

let server = McpServer::new()
    .with_oauth(oauth)
    .http(8080)
    .run()
    .await?;
```

### 3. Protect Your Handlers

```rust
#[tool]
#[requires_auth]  // Require authentication
async fn protected_tool(
    auth: AuthContext,  // Injected auth context
) -> McpResult<String> {
    // Access authenticated user
    let user_id = &auth.user_id;
    let email = &auth.email;

    println!("User {} ({}) called tool", user_id, email);
    Ok("Success".to_string())
}
```

## Authentication Methods

### Method 1: OAuth 2.1 (Recommended)

Complete OAuth 2.1 implementation with PKCE:

```rust
use turbomcp::auth::{OAuthConfig, OAuthProvider};

let oauth = OAuthConfig::new()
    .client_id("my-app-id")
    .client_secret("secret")
    .redirect_uri("https://myapp.com/callback")
    .provider(OAuthProvider::Google)
    .scopes(vec!["openid", "profile", "email"]);

let server = McpServer::new()
    .with_oauth(oauth)
    .http(8080)
    .run()
    .await?;
```

**Supported Providers:**
- Google
- GitHub
- Microsoft
- Auth0
- Custom OpenID Connect

### Method 2: JWT Tokens

Use JWT for stateless authentication:

```rust
use turbomcp::auth::JwtConfig;

let jwt = JwtConfig::new()
    .secret("your-secret-key")
    .issuer("your-domain.com")
    .expiration(Duration::from_hours(24));

let server = McpServer::new()
    .with_jwt(jwt)
    .http(8080)
    .run()
    .await?;
```

**Validating in handlers:**

```rust
#[tool]
async fn protected_tool(auth: AuthContext) -> McpResult<String> {
    // Verify JWT token
    if !auth.verify_token()? {
        return Err(McpError::Unauthorized("Invalid token".into()));
    }

    Ok("Success".to_string())
}
```

### Method 3: API Keys

Simple key-based authentication:

```rust
use turbomcp::auth::ApiKeyConfig;

let api_key = ApiKeyConfig::new()
    .header_name("X-API-Key")  // Header to check
    .keys(vec![
        ("prod-key-1", "Production API"),
        ("test-key-1", "Test API"),
    ]);

let server = McpServer::new()
    .with_api_key(api_key)
    .http(8080)
    .run()
    .await?;
```

**Using in client:**

```bash
curl -H "X-API-Key: prod-key-1" \
  http://localhost:8080/tools/call
```

## Authorization (RBAC)

Implement role-based access control:

```rust
#[tool]
#[requires_role("admin")]  // Require admin role
async fn admin_tool(auth: AuthContext) -> McpResult<String> {
    // Only admins can call this
    Ok("Admin action".to_string())
}

#[tool]
#[requires_role("user|admin")]  // User or admin
async fn user_tool(auth: AuthContext) -> McpResult<String> {
    Ok("User action".to_string())
}
```

### Custom Authorization

```rust
#[tool]
async fn custom_auth(auth: AuthContext) -> McpResult<String> {
    // Manual authorization check
    if !auth.has_permission("tool:execute")? {
        return Err(McpError::Unauthorized(
            "Missing permission: tool:execute".into()
        ));
    }

    // Check custom claims
    let organization: Option<String> = auth.claim("org")?;

    Ok("Success".to_string())
}
```

## DPoP (Demonstration of Proof-of-Possession)

Add RFC 9449 DPoP for token binding:

```toml
turbomcp = { version = "3.0.0-beta.5", features = ["auth", "dpop"] }
```

```rust
use turbomcp::auth::DPopConfig;

let dpop = DPopConfig::new()
    .enabled(true)
    .key_rotation_interval(Duration::from_hours(1));

let server = McpServer::new()
    .with_oauth(oauth)
    .with_dpop(dpop)
    .http(8080)
    .run()
    .await?;
```

## Request Authentication

Access authentication context in handlers:

```rust
use turbomcp::auth::AuthContext;

#[tool]
async fn handler(auth: AuthContext) -> McpResult<String> {
    // User information
    let user_id = &auth.user_id;
    let email = &auth.email;
    let name = &auth.name;

    // Token information
    let token = &auth.access_token;
    let expires_at = auth.expires_at;

    // Authorization
    let roles = &auth.roles;
    let permissions = &auth.permissions;

    // Custom claims
    let org: Option<String> = auth.claim("organization")?;
    let tier: Option<String> = auth.claim("tier")?;

    Ok(format!("User: {}", name))
}
```

## Middleware & Hooks

### Global Auth Middleware

Run on every request:

```rust
let server = McpServer::new()
    .with_auth_middleware(|request, auth| async {
        // Log authentication
        println!("User {} made request", auth.user_id);

        // Can modify request
        Ok(request)
    })
    .http(8080)
    .run()
    .await?;
```

### Per-Handler Guards

```rust
#[tool]
#[requires_auth]
#[requires_role("premium")]
#[rate_limit(100)]  // 100 requests per minute
async fn premium_feature(auth: AuthContext) -> McpResult<String> {
    Ok("Premium feature".to_string())
}
```

## Security Best Practices

### 1. Use HTTPS in Production

```rust
let server = McpServer::new()
    .http(8080)
    .with_tls(TlsConfig {
        cert_path: "/path/to/cert.pem",
        key_path: "/path/to/key.pem",
    })
    .run()
    .await?;
```

### 2. Secure Token Storage

```rust
#[tool]
async fn handler(auth: AuthContext) -> McpResult<String> {
    // Never log tokens
    println!("User {} authenticated", auth.user_id);
    // ❌ Don't do this:
    // println!("Token: {}", auth.access_token);

    Ok("Safe".to_string())
}
```

### 3. Validate Scopes

```rust
#[tool]
async fn sensitive_operation(auth: AuthContext) -> McpResult<String> {
    // Verify user has required scopes
    if !auth.has_scope("read:sensitive")? {
        return Err(McpError::Unauthorized(
            "Missing scope: read:sensitive".into()
        ));
    }

    Ok("Done".to_string())
}
```

### 4. Rate Limiting

```rust
let server = McpServer::new()
    .with_rate_limiter(RateLimitConfig {
        requests_per_minute: 60,
        burst_size: 10,
        enforce_per_user: true,  // Per-user limits
    })
    .http(8080)
    .run()
    .await?;
```

### 5. Token Rotation

```rust
#[tool]
async fn handler(auth: AuthContext) -> McpResult<String> {
    // Check if token is close to expiration
    if let Some(expires) = auth.expires_at {
        let remaining = expires.duration_since(Instant::now());
        if remaining < Duration::from_mins(5) {
            // Request token refresh
            auth.refresh_token().await?;
        }
    }

    Ok("Done".to_string())
}
```

## Testing with Auth

### Mock Authentication

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp::auth::MockAuthContext;

    #[tokio::test]
    async fn test_with_auth() {
        let auth = MockAuthContext::new()
            .user_id("test-user")
            .email("test@example.com")
            .roles(vec!["user"]);

        // Test handler with auth
        // (requires handlers to accept trait objects for testing)
    }
}
```

## Troubleshooting

### "Unauthorized" Errors

Check that:
1. Token is valid and not expired
2. User has required role/scope
3. OAuth configuration is correct

```rust
#[tool]
async fn debug_handler(auth: AuthContext) -> McpResult<String> {
    println!("User ID: {}", auth.user_id);
    println!("Roles: {:?}", auth.roles);
    println!("Expires: {:?}", auth.expires_at);
    Ok("Debug info logged".to_string())
}
```

### CORS Issues with Auth

Configure CORS to include auth headers:

```rust
let server = McpServer::new()
    .http(8080)
    .with_cors(CorsConfig {
        allowed_origins: vec!["https://myapp.com"],
        allowed_headers: vec![
            "Content-Type",
            "Authorization",
            "X-API-Key",
        ],
        ..Default::default()
    })
    .run()
    .await?;
```

## Advanced Topics

### Custom Auth Provider

Implement your own authentication:

```rust
use turbomcp::auth::AuthProvider;

struct MyAuthProvider {
    // Your implementation
}

#[async_trait::async_trait]
impl AuthProvider for MyAuthProvider {
    async fn authenticate(&self, request: &Request) -> Result<AuthContext> {
        // Your auth logic
        Ok(AuthContext { /* ... */ })
    }
}

let server = McpServer::new()
    .with_auth_provider(MyAuthProvider { /* ... */ })
    .http(8080)
    .run()
    .await?;
```

### Integration with Databases

Store users and permissions in database:

```rust
#[tool]
async fn handler(
    auth: AuthContext,
    db: Database,
) -> McpResult<String> {
    // Check if user is in database
    let user = db.query(
        "SELECT * FROM users WHERE id = ?",
        &[auth.user_id],
    ).await?;

    if user.is_empty() {
        return Err(McpError::Unauthorized("User not found".into()));
    }

    Ok("User verified".to_string())
}
```

## Next Steps

- **[Observability](observability.md)** - Monitor authentication events
- **[Examples](../examples/basic.md)** - Real-world auth patterns
- **[Deployment](../deployment/production.md)** - Production security

