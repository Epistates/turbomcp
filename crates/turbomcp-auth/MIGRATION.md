# turbomcp-auth Migration Guide

For workspace-wide breaking changes (unified errors, transport crate splits, MCP spec updates),
see the [top-level MIGRATION.md](../../MIGRATION.md).

---

## v2.x to v3.0

### Dependency update

```toml
[dependencies]
turbomcp-auth = "3.0"

# Or via the umbrella crate:
turbomcp = { version = "3.0", features = ["auth"] }
```

### Unified error types

In v2.x the crate mixed local error types with `turbomcp_server::ServerError`. In v3.0 the
single canonical error type `McpError` from `turbomcp-protocol` is used throughout.

```rust
// Before (v2.x)
use turbomcp_server::{ServerError, ServerResult};

// After (v3.0)
use turbomcp_protocol::{Error as McpError, Result as McpResult};
```

### RSA algorithm removal

v3.0 removes RS256 and PS256 from DPoP to eliminate timing-attack vulnerabilities
(RUSTSEC-2023-0071). Only `DpopAlgorithm::ES256` (ECDSA P-256) is accepted. Update any
code that references the removed variants.

### New MCP 2025-11-25 draft authorization features

Three new feature-gated modules were added. All are disabled by default.

| Feature                   | Module                                 | Specification          |
|---------------------------|----------------------------------------|------------------------|
| `mcp-cimd`                | `turbomcp_auth::cimd`                  | SEP-991                |
| `mcp-oidc-discovery`      | `turbomcp_auth::discovery`             | RFC 8414 / OIDC 1.0    |
| `mcp-incremental-consent` | `turbomcp_auth::incremental_consent`   | SEP-835                |

`mcp-ssrf` (SSRF protection) is an implicit dependency of `mcp-cimd` and
`mcp-oidc-discovery` and does not need to be declared separately.

### Tower middleware feature name

`tower` is kept as an alias but `middleware` is now the canonical feature name.

```toml
# Both compile in v3.0; prefer middleware
turbomcp-auth = { version = "3.0", features = ["middleware"] }
```

### Observability

`init_auth_metrics` is re-exported at the crate root. No import-path change is required if
you were already using the re-export.

---

## v1.x to v2.0

### New standalone crate

Authentication types lived inside the main `turbomcp` crate in v1.x. In v2.0 they were
extracted into the dedicated `turbomcp-auth` crate.

```toml
# Before (v1.x)
[dependencies]
turbomcp = { version = "1.x", features = ["auth"] }

# After (v2.0) - dedicated crate
[dependencies]
turbomcp-auth = "2.0"

# Or continue using the umbrella crate (re-exports are preserved)
turbomcp = { version = "2.0", features = ["auth"] }
```

### Import paths

```rust
// Before (v1.x)
use turbomcp::auth::OAuth2Config;
use turbomcp::auth::AuthContext;

// After (v2.0+) - direct crate
use turbomcp_auth::OAuth2Config;
use turbomcp_auth::AuthContext;
```

The re-export `turbomcp::auth` continues to work when the `auth` feature is enabled on the
umbrella crate.

### Correct type names

Several types were renamed or clarified during extraction. The table below lists the names
that actually exist in the codebase.

| Location | Correct type name | Notes |
|----------|-------------------|-------|
| `turbomcp_auth::oauth2::client` | `OAuth2Client` | Low-level OAuth 2.1 client |
| `turbomcp_auth::providers` (re-exported at crate root) | `OAuth2Provider` | Higher-level provider wrapping `OAuth2Client`; integrates with `AuthManager` |
| `turbomcp_auth::providers` (re-exported at crate root) | `ApiKeyProvider` | API key authentication |
| `turbomcp_auth::config` (re-exported at crate root) | `OAuth2Config` | OAuth 2.1 configuration |
| `turbomcp_auth::config` (re-exported at crate root) | `AuthConfig` | Top-level authentication configuration |
| `turbomcp_auth::context` (re-exported at crate root) | `AuthContext` / `AuthContextBuilder` / `ValidationConfig` | Unified auth context |
| `turbomcp_auth::manager` (re-exported at crate root) | `AuthManager` | Provider orchestration |
| `turbomcp_auth::types` (re-exported at crate root) | `AuthProvider` / `TokenInfo` / `UserInfo` / `AccessToken` / `TokenStorage` | Core traits and types |
| `turbomcp_auth::audit` (re-exported at crate root) | `AuditLogger` / `AuditRecord` / `AuthEvent` / `EventOutcome` | Structured audit logging |
| `turbomcp_auth::rate_limit` (re-exported at crate root) | `RateLimiter` / `RateLimitConfig` / `RateLimitKey` / `RateLimitInfo` / `EndpointLimit` | Rate limiting |

### DPoP is now an explicit opt-in feature

In v1.x DPoP was bundled with the `auth` feature. In v2.0+ it is a separate dependency that
must be enabled explicitly.

```toml
# Before (v1.x)
turbomcp = { version = "1.x", features = ["auth"] }

# After (v2.0+)
turbomcp-auth = { version = "2.0", features = ["dpop"] }
# Or via umbrella crate:
turbomcp = { version = "2.0", features = ["auth", "dpop"] }
```

When `dpop` is enabled, `turbomcp_dpop` is re-exported as `turbomcp_auth::dpop`.

### AuthContext is the canonical auth representation

In v1.x multiple auth context types existed across modules. In v2.0 `AuthContext` is the
single type used everywhere. Construct it with `AuthContext::builder()`:

```rust
use turbomcp_auth::{AuthContext, UserInfo};
use std::collections::HashMap;

let user = UserInfo {
    id: "user123".to_string(),
    username: "alice".to_string(),
    email: Some("alice@example.com".to_string()),
    display_name: Some("Alice".to_string()),
    avatar_url: None,
    metadata: HashMap::new(),
};

let auth = AuthContext::builder()
    .subject("user123")
    .user(user)
    .provider("api-key")
    .roles(vec!["admin".to_string()])
    .permissions(vec!["write:data".to_string()])
    .build()
    .unwrap();

assert!(auth.has_role("admin"));
assert!(auth.has_permission("write:data"));
```

### OAuth2Config field types

`OAuth2Config.client_secret` changed from `String` to `secrecy::SecretString` for automatic
memory zeroization on drop. Access the value with `.expose_secret()` when required.

```rust
use secrecy::SecretString;

let config = OAuth2Config {
    client_id: "id".to_string(),
    client_secret: SecretString::new("secret".to_string().into()),
    // ... other fields
};
```

---

## Additional resources

- Top-level migration guide: `../../MIGRATION.md`
- API documentation: https://docs.rs/turbomcp-auth
- RFC 8707 Resource Indicators: https://datatracker.ietf.org/doc/html/rfc8707
- RFC 9449 DPoP: https://datatracker.ietf.org/doc/html/rfc9449
- RFC 9728 Protected Resource Metadata: https://datatracker.ietf.org/doc/html/rfc9728
