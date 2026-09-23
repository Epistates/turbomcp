# WASM OAuth Support Roadmap

## Status: Partially shipped

Phase 1 (JWT validation) and parts of Phase 3 (Cloudflare Access) ship in
`turbomcp-wasm` with its `auth` feature. An OAuth 2.1 *authorization server*
for Workers (`turbomcp_wasm::auth::provider`) also ships, in demo mode: it
auto-approves authorization requests and must not be used in production without
real user authentication. Client-side OAuth flows (Phase 2) remain planned.

## Summary

Add OAuth/JWT authentication support for WASM MCP servers running on Cloudflare Workers and other edge platforms.

## Motivation

The `turbomcp-auth` crate depends on `tokio` and `reqwest`, which don't compile for `wasm32-unknown-unknown`. Users want to protect their MCP servers on Cloudflare Workers with OAuth.

## Phases

### Phase 1: JWT Validation (Quick Win) — shipped

**Goal**: Validate incoming JWTs without full OAuth flows

- [x] `auth` feature on `turbomcp-wasm` (rather than on `turbomcp-auth`)
- [x] Web Crypto API for JWT signature verification (`WasmJwtAuthenticator`)
- [x] Bearer token extraction from headers (`HeaderExtractor`)
- [x] Works with Cloudflare Access, Auth0, Okta, etc.
- [x] JWKS fetching via the Fetch API, with caching (`JwksCache`)

### Phase 2: OAuth Client Flows — planned

**Goal**: Full OAuth 2.1 PKCE flow in WASM

- [ ] Authorization code flow with PKCE using Fetch API
- [ ] Token refresh using Fetch API
- [ ] Secure token storage patterns for Workers

### Phase 3: Cloudflare-Specific Integrations — in progress

**Goal**: First-class Cloudflare Workers support

- [x] Cloudflare Access integration (`CloudflareAccessAuthenticator`, `CloudflareAccessExtractor`)
- [ ] Workers KV for token caching
- [x] Durable Objects for OAuth token storage (`DurableObjectTokenStore`)

## Using It Today

Wrap the server in `WithAuth` (or call `AuthExt::with_auth`) with an
authenticator. Requests without valid credentials are refused with `401` and a
`WWW-Authenticate` challenge; handlers read the validated identity from the
request context.

### JWT from Any Issuer

```rust
use std::sync::Arc;
use turbomcp_wasm::auth::{JwtConfig, WasmJwtAuthenticator};
use turbomcp_wasm::wasm_server::*;
use worker::*;

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let server = McpServer::builder("my-server", "1.0.0")
        .tool_with_ctx_no_args("whoami", "Who am I", |ctx: Arc<RequestContext>| async move {
            ctx.subject().unwrap_or("anonymous").to_string()
        })
        .build();

    let auth = WasmJwtAuthenticator::with_jwks(
        "https://example.auth0.com/.well-known/jwks.json",
        JwtConfig::new()
            .issuer("https://example.auth0.com/")
            .audience("https://my-server.example.workers.dev"),
    );

    WithAuth::new(server, auth)
        .with_resource_metadata("https://my-server.example.workers.dev/.well-known/oauth-protected-resource")
        .handle(req)
        .await
}
```

### Cloudflare Access

Put Cloudflare Access in front of your Worker and validate the JWT it adds:

```rust
use turbomcp_wasm::auth::CloudflareAccessAuthenticator;
use turbomcp_wasm::wasm_server::*;
use worker::*;

#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let server = McpServer::builder("my-server", "1.0.0")
        .tool_no_args("health", "Health check", || async { "OK".to_string() })
        .build();

    // Your Access team name and the application's audience tag
    server
        .with_auth(CloudflareAccessAuthenticator::new("my-team", "my-aud"))
        .handle(req)
        .await
}
```

## Related

- GitHub Issue #11 - Original Cloudflare Worker support request
- `turbomcp-auth` crate - Native OAuth implementation
- `turbomcp-wasm` crate - WASM server support, `auth` module
