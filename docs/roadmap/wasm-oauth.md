# WASM OAuth Support Roadmap

## Status: Planned

## Summary

Add OAuth/JWT authentication support for WASM MCP servers running on Cloudflare Workers and other edge platforms.

## Motivation

The current `turbomcp-auth` crate depends on `tokio` and `reqwest` which don't compile for `wasm32-unknown-unknown`. Users want to protect their MCP servers on Cloudflare Workers with OAuth.

## Phases

### Phase 1: JWT Validation (Quick Win)

**Goal**: Validate incoming JWTs without full OAuth flows

- [ ] Add `wasm` feature to `turbomcp-auth`
- [ ] Use Web Crypto API for JWT signature verification
- [ ] Support Bearer token extraction from headers
- [ ] Works with Cloudflare Access, Auth0, Okta, etc.

**Implementation Notes**:
- Use `web-sys` for Web Crypto API access
- Support RS256, ES256 algorithms
- JWKS fetching via Fetch API

### Phase 2: OAuth Client Flows

**Goal**: Full OAuth 2.1 PKCE flow in WASM

- [ ] Authorization code flow with PKCE using Fetch API
- [ ] Token refresh using Fetch API
- [ ] Secure token storage patterns for Workers

### Phase 3: Cloudflare-Specific Integrations

**Goal**: First-class Cloudflare Workers support

- [ ] Cloudflare Access integration helpers
- [ ] Workers KV for token caching
- [ ] Durable Objects for session management

## Current Workarounds

Until native WASM OAuth is implemented, users can:

### 1. Cloudflare Access (Recommended)

Put Cloudflare Access in front of your Worker. Access handles OAuth and passes validated identity via headers.

```rust
#[tool("Get user info")]
async fn whoami(&self, req: &Request) -> Result<String, ToolError> {
    // Cloudflare Access adds these headers after authentication
    let email = req.headers().get("CF-Access-Authenticated-User-Email")?;
    Ok(format!("Authenticated as: {}", email.unwrap_or("unknown")))
}
```

### 2. Manual JWT Validation

Validate JWTs manually using the `jsonwebtoken` crate:

```rust
use jsonwebtoken::{decode, DecodingKey, Validation, Algorithm};

fn validate_token(token: &str, secret: &[u8]) -> Result<Claims, ToolError> {
    let validation = Validation::new(Algorithm::HS256);
    let key = DecodingKey::from_secret(secret);
    let data = decode::<Claims>(token, &key, &validation)
        .map_err(|e| ToolError::new(format!("Invalid token: {}", e)))?;
    Ok(data.claims)
}
```

### 3. API Key Authentication

Simple API key validation works today:

```rust
#[tool("Protected operation")]
async fn protected(&self, req: &Request) -> Result<String, ToolError> {
    let api_key = req.headers()
        .get("X-API-Key")?
        .ok_or_else(|| ToolError::new("Missing API key"))?;

    if api_key != std::env::var("API_KEY").unwrap_or_default() {
        return Err(ToolError::new("Invalid API key"));
    }

    Ok("Access granted".to_string())
}
```

## Related

- GitHub Issue #11 - Original Cloudflare Worker support request
- `turbomcp-auth` crate - Native OAuth implementation
- `turbomcp-wasm` crate - WASM server support
