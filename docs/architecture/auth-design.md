# TurboMCP v3 Authentication Architecture

## Design Principles

1. **Portable by Default** - Same auth code works on native and WASM
2. **Platform-Optimized** - Use native crypto on each platform
3. **Incremental Adoption** - Start simple, add complexity as needed
4. **Zero Boilerplate** - Macro integration for common patterns

## Current State

```
turbomcp-auth (native only)
├── OAuth 2.1 flows (tokio + reqwest)
├── JWT validation (jsonwebtoken)
├── API key validation
└── Tower middleware
```

**Problem**: Tightly coupled to tokio/reqwest, won't compile for WASM.

## Proposed Architecture

### Layer 1: Core Traits (turbomcp-core)

Platform-adaptive traits using existing `MaybeSend`/`MaybeSync` pattern:

```rust
// turbomcp-core/src/auth.rs

use crate::{MaybeSend, MaybeSync, RequestContext};

/// Credential types
#[derive(Debug, Clone)]
pub enum Credential {
    Bearer(String),
    ApiKey(String),
    Basic { username: String, password: String },
    Custom(String, String), // (scheme, value)
}

/// Validated identity after authentication
#[derive(Debug, Clone)]
pub struct Principal {
    /// Unique subject identifier (sub claim in JWT)
    pub subject: String,
    /// Token issuer (iss claim)
    pub issuer: Option<String>,
    /// Token audience (aud claim)
    pub audience: Option<String>,
    /// Expiration time
    pub expires_at: Option<u64>,
    /// Custom claims
    pub claims: HashMap<String, Value>,
}

/// Core authentication trait - validates credentials and returns principal
pub trait Authenticator: MaybeSend + MaybeSync + Clone {
    type Error: std::error::Error + MaybeSend;

    /// Validate credentials and return authenticated principal
    fn authenticate(
        &self,
        credential: &Credential,
    ) -> impl Future<Output = Result<Principal, Self::Error>> + MaybeSend;
}

/// Extracts credentials from request context
pub trait CredentialExtractor: MaybeSend + MaybeSync {
    fn extract(&self, ctx: &RequestContext) -> Option<Credential>;
}

/// Default extractor: Authorization header -> Bearer/Basic
pub struct HeaderExtractor;

impl CredentialExtractor for HeaderExtractor {
    fn extract(&self, ctx: &RequestContext) -> Option<Credential> {
        let auth = ctx.header("authorization")?;
        if let Some(token) = auth.strip_prefix("Bearer ") {
            Some(Credential::Bearer(token.to_string()))
        } else if let Some(basic) = auth.strip_prefix("Basic ") {
            // Decode base64 and split
            // ...
            None
        } else {
            None
        }
    }
}
```

### Layer 2: JWT Validation (shared logic)

```rust
// turbomcp-core/src/auth/jwt.rs

/// JWT validation configuration (platform-agnostic)
#[derive(Debug, Clone)]
pub struct JwtConfig {
    /// Expected issuer (iss claim)
    pub issuer: Option<String>,
    /// Expected audience (aud claim)
    pub audience: Option<String>,
    /// Allowed algorithms
    pub algorithms: Vec<Algorithm>,
    /// Clock skew tolerance in seconds
    pub leeway: u64,
}

/// Decoded but unverified JWT (for inspection)
#[derive(Debug)]
pub struct UnverifiedJwt<'a> {
    pub header: JwtHeader,
    pub claims: &'a str, // Raw claims JSON
    pub signature: &'a [u8],
}

/// JWT header
#[derive(Debug, Clone, Deserialize)]
pub struct JwtHeader {
    pub alg: String,
    pub typ: Option<String>,
    pub kid: Option<String>,
}

/// Standard JWT claims
#[derive(Debug, Clone, Deserialize)]
pub struct StandardClaims {
    pub sub: Option<String>,
    pub iss: Option<String>,
    pub aud: Option<StringOrArray>,
    pub exp: Option<u64>,
    pub nbf: Option<u64>,
    pub iat: Option<u64>,
}

/// Algorithm enum (subset supported on all platforms)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Algorithm {
    HS256,
    HS384,
    HS512,
    RS256,
    RS384,
    RS512,
    ES256,
    ES384,
}
```

### Layer 3: Platform Implementations

#### Native (turbomcp-auth)

```rust
// turbomcp-auth/src/jwt.rs

use turbomcp_core::auth::{Authenticator, Credential, Principal, JwtConfig};
use jsonwebtoken::{decode, DecodingKey, Validation};

pub struct JwtAuthenticator {
    config: JwtConfig,
    keys: Arc<RwLock<JwksCache>>,
}

impl JwtAuthenticator {
    pub async fn from_jwks_url(url: &str, config: JwtConfig) -> Result<Self, Error> {
        let keys = fetch_jwks(url).await?;
        Ok(Self {
            config,
            keys: Arc::new(RwLock::new(JwksCache::new(keys))),
        })
    }
}

impl Authenticator for JwtAuthenticator {
    type Error = AuthError;

    async fn authenticate(&self, credential: &Credential) -> Result<Principal, Self::Error> {
        let Credential::Bearer(token) = credential else {
            return Err(AuthError::InvalidCredentialType);
        };

        // Use jsonwebtoken crate
        let key = self.keys.read().await.get_key(&header.kid)?;
        let data = decode::<Claims>(token, &key, &validation)?;

        Ok(Principal::from_claims(data.claims))
    }
}
```

#### WASM (turbomcp-wasm)

```rust
// turbomcp-wasm/src/auth/jwt.rs

use turbomcp_core::auth::{Authenticator, Credential, Principal, JwtConfig};
use web_sys::SubtleCrypto;

pub struct WasmJwtAuthenticator {
    config: JwtConfig,
    keys: Rc<RefCell<JwksCache>>,
    crypto: SubtleCrypto,
}

impl WasmJwtAuthenticator {
    pub async fn from_jwks_url(url: &str, config: JwtConfig) -> Result<Self, Error> {
        // Fetch JWKS using Fetch API
        let response = fetch(url).await?;
        let jwks: Jwks = response.json().await?;

        let crypto = web_sys::window()
            .unwrap()
            .crypto()
            .unwrap()
            .subtle();

        Ok(Self {
            config,
            keys: Rc::new(RefCell::new(JwksCache::new(jwks))),
            crypto,
        })
    }
}

impl Authenticator for WasmJwtAuthenticator {
    type Error = AuthError;

    async fn authenticate(&self, credential: &Credential) -> Result<Principal, Self::Error> {
        let Credential::Bearer(token) = credential else {
            return Err(AuthError::InvalidCredentialType);
        };

        // Parse JWT parts
        let jwt = parse_jwt(token)?;

        // Get key from JWKS
        let jwk = self.keys.borrow().get_key(&jwt.header.kid)?;

        // Import key using Web Crypto API
        let key = import_jwk(&self.crypto, jwk).await?;

        // Verify signature using Web Crypto API
        verify_signature(&self.crypto, &key, &jwt).await?;

        // Validate claims
        let claims: StandardClaims = serde_json::from_str(jwt.claims)?;
        validate_claims(&claims, &self.config)?;

        Ok(Principal::from_claims(claims))
    }
}
```

### Layer 4: Server Integration

```rust
// turbomcp-core/src/auth/middleware.rs

/// Auth configuration for MCP servers
pub struct AuthConfig<A: Authenticator, E: CredentialExtractor = HeaderExtractor> {
    pub authenticator: A,
    pub extractor: E,
    pub required: bool, // Reject unauthenticated requests?
}

/// Extension trait for McpHandler to add auth
pub trait AuthenticatedHandler: McpHandler {
    fn with_auth<A: Authenticator>(self, auth: AuthConfig<A>) -> AuthenticatedServer<Self, A>
    where
        Self: Sized;
}
```

### Layer 5: Macro Integration

```rust
// User code - works on both native and WASM!

#[derive(Clone)]
struct MyServer;

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// This tool requires authentication
    #[tool("Get user profile")]
    async fn profile(&self, ctx: &RequestContext) -> Result<String, ToolError> {
        // Principal is available if auth middleware validated the request
        let principal = ctx.principal()
            .ok_or_else(|| ToolError::new("Authentication required"))?;

        Ok(format!("Hello, {}!", principal.subject))
    }

    /// Public tool - no auth required
    #[tool("Health check")]
    async fn health(&self) -> &'static str {
        "OK"
    }
}

// Native main.rs
#[tokio::main]
async fn main() {
    let auth = JwtAuthenticator::from_jwks_url(
        "https://example.auth0.com/.well-known/jwks.json",
        JwtConfig::default()
            .issuer("https://example.auth0.com/")
            .audience("my-api"),
    ).await.unwrap();

    MyServer
        .with_auth(AuthConfig::new(auth))
        .run_stdio()
        .await
        .unwrap();
}

// WASM worker.rs
#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {
    let auth = WasmJwtAuthenticator::from_jwks_url(
        "https://example.auth0.com/.well-known/jwks.json",
        JwtConfig::default()
            .issuer("https://example.auth0.com/")
            .audience("my-api"),
    ).await?;

    MyServer
        .into_mcp_server()
        .with_auth(AuthConfig::new(auth))
        .handle(req)
        .await
}
```

## Cloudflare Access Integration

Special-case for CF Access which validates at the edge:

```rust
// turbomcp-wasm/src/auth/cloudflare.rs

/// Cloudflare Access authenticator
/// Validates CF-Access-* headers set by Cloudflare Access
pub struct CloudflareAccess {
    team_domain: String,
    audience: String,
}

impl CloudflareAccess {
    pub fn new(team_domain: &str, audience: &str) -> Self {
        Self {
            team_domain: team_domain.to_string(),
            audience: audience.to_string(),
        }
    }

    /// Create from Workers environment bindings
    pub fn from_env(env: &Env) -> Result<Self, Error> {
        Ok(Self {
            team_domain: env.var("CF_ACCESS_TEAM_DOMAIN")?.to_string(),
            audience: env.var("CF_ACCESS_AUDIENCE")?.to_string(),
        })
    }
}

impl Authenticator for CloudflareAccess {
    type Error = AuthError;

    async fn authenticate(&self, credential: &Credential) -> Result<Principal, Self::Error> {
        // CF Access sets CF-Access-JWT-Assertion header
        // We validate it against CF's public keys
        let Credential::Custom(scheme, token) = credential else {
            return Err(AuthError::InvalidCredentialType);
        };

        // Fetch CF Access public keys
        let keys_url = format!(
            "https://{}.cloudflareaccess.com/cdn-cgi/access/certs",
            self.team_domain
        );

        // Validate JWT against CF's keys
        // ...

        Ok(principal)
    }
}

/// Extractor for CF Access headers
pub struct CloudflareAccessExtractor;

impl CredentialExtractor for CloudflareAccessExtractor {
    fn extract(&self, ctx: &RequestContext) -> Option<Credential> {
        let jwt = ctx.header("cf-access-jwt-assertion")?;
        Some(Credential::Custom("CF-Access".to_string(), jwt.to_string()))
    }
}
```

## Implementation Phases

### Phase 1: Core Traits (1-2 days)
- Add `auth` module to `turbomcp-core`
- Define `Authenticator`, `CredentialExtractor`, `Principal`
- Add `principal()` method to `RequestContext`

### Phase 2: WASM JWT Validator (2-3 days)
- Implement `WasmJwtAuthenticator` using Web Crypto API
- JWKS fetching with Fetch API
- RS256, ES256 support (most common)
- Unit tests with wasm-bindgen-test

### Phase 3: Server Integration (1-2 days)
- Add `with_auth()` to `McpServer` builder
- Auth middleware that validates and attaches principal
- Reject unauthorized requests (configurable)

### Phase 4: Cloudflare Access (1 day)
- `CloudflareAccess` authenticator
- `CloudflareAccessExtractor`
- Documentation and examples

### Phase 5: Native Refactor (2-3 days)
- Refactor `turbomcp-auth` to use shared traits
- Maintain backward compatibility
- Add portable auth config types

## API Comparison

| Feature | Native | WASM |
|---------|--------|------|
| JWT Validation | `JwtAuthenticator` | `WasmJwtAuthenticator` |
| JWKS Fetching | reqwest + cache | fetch + cache |
| Crypto | jsonwebtoken (ring) | Web Crypto API |
| OAuth Flows | Full support | Phase 2 |
| API Keys | ✅ | ✅ |
| CF Access | N/A | `CloudflareAccess` |

## Security Considerations

1. **Key Storage**: Never store private keys in WASM
2. **Token Leakage**: Use secure contexts, avoid logging tokens
3. **Timing Attacks**: Use constant-time comparison for signatures
4. **JWKS Caching**: Cache with TTL, handle rotation
5. **Clock Skew**: Configure leeway for exp/nbf validation

## Open Questions

1. Should auth traits live in `turbomcp-core` or new `turbomcp-auth-traits` crate?
2. Should we support custom claim validation via closure/trait?
3. How to handle token refresh in WASM (no background tasks)?
4. Should `principal()` return `Option<&Principal>` or `Result<&Principal, AuthError>`?

---

## Research: Best Practices Q1 2026

### Sources

- [Secure OAuth Implementation Without Local API Keys Using Rust and Cloudflare Workers](https://compiledthoughts.pages.dev/blog/oauth-without-local-api-keys-rust-cloudflare-workers/)
- [wasm-service-oauth - GitHub](https://github.com/stevelr/wasm-service-oauth)
- [Cloudflare Workers Rust Language Support](https://developers.cloudflare.com/workers/languages/rust/)
- [jwt-rustcrypto - WASM-compatible JWT](https://docs.rs/jwt-rustcrypto)
- [jwt-compact - WASM-tested JWT](https://slowli.github.io/jwt-compact/jwt_compact/)
- [JWT-Simple - WASM notes](https://lib.rs/crates/jwt-simple)

### Key Findings

#### 1. WASM JWT Libraries

| Library | WASM Support | Algorithms | Notes |
|---------|-------------|------------|-------|
| `jwt-rustcrypto` | ✅ Explicit | HMAC, RSA, ECDSA | "Can be compiled as Rust library or WebAssembly" |
| `jwt-compact` | ✅ Tested | RS256, ES256 | Has WASM E2E tests, needs `getrandom` features |
| `jwt-simple` | ⚠️ Limited | Multiple | Recommends native JS for WASM instead |
| `jsonwebtoken` | ❌ No | Multiple | Uses `ring` which requires native code |

**Recommendation**: Use `jwt-rustcrypto` for pure Rust WASM, or Web Crypto API via `web-sys` for best performance.

#### 2. Performance Consideration

From jwt-simple docs:
> "There are high-quality JWT implementations in JavaScript, leveraging the WebCrypto API, that provide better performance and security guarantees than a WebAssembly module."

**Implication**: For browser targets, consider thin WASM wrapper around Web Crypto API rather than pure Rust crypto. For Cloudflare Workers, Rust WASM is fine since there's no JS alternative with same type safety.

#### 3. Cloudflare Workers Constraints

From [Cloudflare docs](https://developers.cloudflare.com/workers/languages/rust/):
- ❌ No tokio or async_std (threaded runtimes)
- ❌ Must target `wasm32-unknown-unknown`
- ✅ Zero cold starts (efficient WASM)
- ✅ Secrets stored encrypted in Worker environment
- ✅ 300+ edge locations globally

**Architecture Impact**: Must use `wasm-bindgen-futures` for async, not tokio.

#### 4. OAuth 2.1 Best Practices

From [PKCE Authentication Guide](https://codewithmukesh.com/blog/pkce-authentication-blazor-wasm-amazon-cognito/):
- **PKCE required** for public clients (SPAs, WASM)
- **No client secrets** in browser/WASM code
- **Short-lived access tokens** (5-15 min) + refresh tokens
- **Hosted UI preferred** - removes XSS/CSRF attack surface

#### 5. Token Storage

From [Blazor WASM JWT Guide](https://www.djamware.com/post/6918248bc2494048b32e079f/build-a-secure-blazor-webassembly-app-with-aspnet-core-10-and-jwt-authentication):
- Session storage for tokens (cleared on tab close)
- Never local storage for sensitive tokens
- Authorization: Bearer header for API calls

#### 6. Secrets Management

From [OAuth Without Local API Keys](https://compiledthoughts.pages.dev/blog/oauth-without-local-api-keys-rust-cloudflare-workers/):
> "Configuration parameters are passed in an OAuthConfig struct... secret parameters [are set] in the environment, so they aren't part of the compiled wasm binary."

**Implementation**: Use Cloudflare environment bindings, not compiled-in secrets.

### Updated Recommendations

1. **JWT Validation in WASM**:
   - Primary: Web Crypto API via `web-sys` (fastest, most secure)
   - Fallback: `jwt-rustcrypto` for environments without Web Crypto

2. **OAuth Flows**:
   - PKCE only (no client secrets)
   - Use provider's hosted UI when possible
   - Store tokens in session storage, not local storage

3. **Cloudflare Workers Specific**:
   - Secrets via `env.var()` and `env.secret()`
   - Consider CF Access for enterprise auth
   - Use Workers KV for token/JWKS caching

4. **Library Selection**:
   ```toml
   # WASM-compatible dependencies
   jwt-rustcrypto = "0.x"  # If pure Rust needed
   web-sys = { features = ["SubtleCrypto", "CryptoKey"] }  # Preferred
   getrandom = { features = ["js"] }  # Required for any crypto
   ```
