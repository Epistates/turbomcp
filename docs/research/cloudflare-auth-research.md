# Cloudflare Workers Authentication Research

## Research Date: January 2026

This document captures research findings for implementing authentication in Cloudflare Workers for turbomcp-wasm.

## Key Resources

### Official Documentation
- [Cloudflare MCP Authorization](https://developers.cloudflare.com/agents/model-context-protocol/authorization/)
- [Validate JWTs - Cloudflare One](https://developers.cloudflare.com/cloudflare-one/access-controls/applications/http-apps/authorization-cookie/validating-json/)
- [Web Crypto API - Cloudflare Workers](https://developers.cloudflare.com/workers/runtime-apis/web-crypto/)
- [Configure JWT Worker - API Shield](https://developers.cloudflare.com/api-shield/security/jwt-validation/jwt-worker/)
- [Cloudflare Workers Rust Support](https://developers.cloudflare.com/workers/languages/rust/)

### Libraries
- [cloudflare/workers-oauth-provider](https://github.com/cloudflare/workers-oauth-provider) - Official OAuth 2.1 provider library
- [@tsndr/cloudflare-worker-jwt](https://github.com/tsndr/cloudflare-worker-jwt) - Lightweight JWT library (JS)
- [@sagi.io/workers-jwt](https://github.com/sagi/workers-jwt) - JWT generation using WebCrypto (JS)

## OAuth 2.1 in Cloudflare Workers

### Official OAuth Provider Library

Cloudflare provides a TypeScript library implementing OAuth 2.1 with PKCE:
- Acts as a wrapper around Worker code
- Automatic token management
- API handler receives authenticated user details as parameter
- Agnostic to user management and UI frameworks

### Three Integration Patterns

1. **Worker handles authorization itself** - Complete OAuth flow in your MCP server
2. **Third-party OAuth provider** - GitHub, Google, etc.
3. **Authorization-as-a-service** - Stytch, Auth0, WorkOS

### State Management

Workers don't maintain state between executions. Use Cloudflare Storage with namespaces:
- `USERS` - User sessions
- `CODES` - Authorization codes
- `TOKENS` - Access/refresh tokens

## Cloudflare Access JWT Validation

### Token Location

When Cloudflare Access is in front of your Worker:
- Header: `Cf-Access-Jwt-Assertion` (recommended)
- Cookie: `CF_Authorization` (not guaranteed to be passed)

### Public Key Endpoint

```
https://<team-name>.cloudflareaccess.com/cdn-cgi/access/certs
```

**Important**: Cloudflare rotates signing keys every 6 weeks. Must programmatically update keys.

### Required Validations

1. **Signature verification** - Using public key from JWKS endpoint
2. **Issuer (`iss`) claim** - Must match `https://<team-name>.cloudflareaccess.com`
3. **Audience (`aud`) claim** - Unique per application (Application Audience tag)
4. **Expiration** - Check `exp` claim
5. **Not Before** - Check `nbf` claim if present

### Security Warning

> "Validation of the header alone is not sufficient — the JWT and signature must be confirmed to avoid identity spoofing."

## Web Crypto API in Workers

### Supported Algorithms

| Algorithm | Type | Support |
|-----------|------|---------|
| RS256 | RSA-PKCS1-v1_5 with SHA-256 | Supported |
| RS384 | RSA-PKCS1-v1_5 with SHA-384 | Supported |
| RS512 | RSA-PKCS1-v1_5 with SHA-512 | Supported |
| ES256 | ECDSA with P-256 | Supported |
| ES384 | ECDSA with P-384 | Supported |
| HS256 | HMAC with SHA-256 | Supported |
| HS384 | HMAC with SHA-384 | Supported |
| HS512 | HMAC with SHA-512 | Supported |

### WebCrypto Operations

Workers support standard Web APIs including `crypto.subtle`:
- `importKey()` - Import JWK keys
- `verify()` - Verify signatures
- `sign()` - Sign data (for token generation)

### Performance Note

> "For lightweight tasks like checking an authorization token, sticking to pure JavaScript is probably both faster and easier than WASM. WASM programs operate in their own separate memory space, which means data must be copied in and out."

**Recommendation**: Use JS interop or Web Crypto directly in WASM rather than pure-Rust crypto.

## Rust/WASM JWT Considerations

### Current State

No mature Rust JWT crate compiles cleanly to `wasm32-unknown-unknown` with Web Crypto API support. Options:

1. **Use Web Crypto via web-sys** (Our approach)
   - Direct access to SubtleCrypto
   - RS256, ES256 verification
   - JWKS fetching via Fetch API

2. **JavaScript interop**
   - Use `jose` or `cloudflare-worker-jwt` via wasm-bindgen
   - More overhead but battle-tested

3. **Pure Rust crypto** (Not recommended)
   - Large WASM binary size
   - Memory copying overhead
   - Potential security issues

### Web-sys Crypto Features Required

```toml
[dependencies.web-sys]
features = [
    "Crypto",
    "CryptoKey",
    "SubtleCrypto",
    "RsaHashedImportParams",
    "EcKeyImportParams",
    "HmacImportParams",
    "JsonWebKey",
]
```

## Implementation Strategy for turbomcp-wasm

### Phase 1: JWT Validation (Current)

1. Parse JWT header/payload (base64url decode)
2. Extract `kid` from header
3. Fetch JWKS from issuer (with caching)
4. Import public key using Web Crypto
5. Verify signature
6. Validate claims (iss, aud, exp, nbf)
7. Return Principal

### Phase 2: Cloudflare Access Integration

1. Extract `Cf-Access-Jwt-Assertion` header
2. Auto-configure JWKS URL from team name
3. Validate audience tag
4. Extract user identity claims

### Phase 3: OAuth 2.1 PKCE Flow (Future)

1. Authorization endpoint
2. Token endpoint
3. Token refresh
4. State/PKCE management via Workers KV

## Security Best Practices

From Cloudflare documentation:

1. **Map OAuth scopes to minimal permissions**
2. **Store secrets in Cloudflare encrypted environment variables**
3. **Log rejected tokens to detect replay attempts**
4. **Rotate keys regularly**
5. **Test failure flows** for clean re-authentication
6. **Treat every redirect as an attack surface**
7. **CSRF protection** - Validate `state` parameter

## References

- [OAuth Auth Server through Workers](https://blog.cloudflare.com/oauth-2-0-authentication-server/)
- [Protecting APIs with JWT Validation](https://blog.cloudflare.com/protecting-apis-with-jwt-validation/)
- [How to Authenticate Google APIs on Cloudflare Workers in 2025](https://medium.com/@tamnvhustcc/how-to-authenticate-google-apis-on-cloudflare-workers-in-2025-a-complete-guide-with-custom-jwt-80614398425a)
- [Kinde: Verifying JWTs in Cloudflare Workers](https://kinde.com/blog/engineering/verifying-jwts-in-cloudflare-workers/)
- [OAuth2 and Cloudflare Workers](https://ryan-schachte.com/blog/oauth_cloudflare_workers/)

## Diligence Review (January 2026)

### Library Choice Validation

**Decision**: Use Web Crypto API via `web-sys` instead of pure-Rust JWT crates.

**Evidence Supporting This Decision**:

1. **jwt-simple crate documentation** explicitly recommends:
   > "using a native JavaScript implementation is highly recommended instead. There are high-quality JWT implementations in JavaScript, leveraging the WebCrypto API, that provide better performance and security guarantees than a WebAssembly module."

2. **Cloudflare documentation** states:
   > "For lightweight tasks like checking an authorization token, sticking to pure JavaScript is probably both faster and easier than WASM. WASM programs operate in their own separate memory space, which means data must be copied in and out."

3. **jwt-compact** has WASM support but uses pure-Rust crypto (ring/ed25519-dalek) which increases binary size and has memory copying overhead.

**Conclusion**: ✅ Web Crypto API is the correct choice for WASM JWT validation.

### Implementation Review Checklist

| Component | Status | Notes |
|-----------|--------|-------|
| Base64URL Decoding | ✅ Good | Uses browser's native `atob()` |
| JWT Parsing | ✅ Good | Correctly splits header.payload.signature |
| Signature Verification | ✅ Good | Uses SubtleCrypto (constant-time, audited) |
| Claim Validation | ✅ Good | Validates exp, nbf, iss, aud with leeway |
| JWKS Caching | ✅ Good | TTL-based cache with refresh |
| Error Handling | ✅ Good | Specific error variants |
| Algorithm Restriction | ✅ Good | Config allows restricting algorithms |
| CF Access Integration | ✅ Good | Uses recommended header |

### Security Considerations

**Algorithm Confusion Attack**: ✅ Prevented
- Config allows restricting to specific algorithms
- CloudflareAccessAuthenticator defaults to RS256 only

**Timing Attacks**: ✅ Mitigated
- Signature verification delegated to SubtleCrypto (constant-time)

**Key Confusion**: ✅ Mitigated
- Keys looked up by `kid` when present
- Falls back to algorithm matching

### Improvements Made

1. **JWKS Retry Logic**: Added exponential backoff for transient failures
2. **Key Rotation Handling**: Auto-refresh JWKS on signature verification failure

### Future Work (Phase 3)

1. **Replay Protection**: Optional JTI tracking with Workers KV
2. **OAuth 2.1 PKCE Flow**: Full authorization server implementation
3. **Rate Limiting**: Prevent brute force attacks
