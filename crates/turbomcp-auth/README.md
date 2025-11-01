# TurboMCP Auth

OAuth 2.1 and authentication for TurboMCP with MCP protocol compliance.

## Features

- **OAuth 2.1 Flows** - RFC 8707/9728/7591 compliant with PKCE support
  - Authorization Code flow (with PKCE for public/confidential clients)
  - Client Credentials flow (server-to-server)
  - Token refresh and validation
- **Multi-Provider Support** - Google, GitHub, Microsoft, GitLab, Apple, Okta, Auth0, Keycloak (with provider-specific OAuth 2.1 configurations)
- **OAuth2Provider** - Full AuthProvider implementation for OAuth 2.1
- **API Key Authentication** - Simple API key-based authentication
- **Server-Side Helpers** - RFC 9728 Protected Resource Metadata and WWW-Authenticate headers
- **Session Management** - Secure token management with configurable storage
- **DPoP Support** - Optional RFC 9449 proof-of-possession tokens
- **Comprehensive Validation** - RFC 8707 canonical URI validation, token format validation

## Quick Start

### Client: OAuth 2.1 Authorization Code Flow

```rust
use turbomcp_auth::{
    oauth2::OAuth2Client,
    config::{OAuth2Config, OAuth2FlowType, ProviderType},
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create OAuth2 configuration
    let config = OAuth2Config {
        client_id: "my-client-id".to_string(),
        client_secret: "my-client-secret".to_string(),
        auth_url: "https://provider.example.com/oauth/authorize".to_string(),
        token_url: "https://provider.example.com/oauth/token".to_string(),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["openid".to_string(), "profile".to_string()],
        flow_type: OAuth2FlowType::AuthorizationCode,
        additional_params: Default::default(),
        security_level: Default::default(),
        #[cfg(feature = "dpop")]
        dpop_config: None,
        mcp_resource_uri: None,
        auto_resource_indicators: true,
    };

    // Create OAuth2 client
    let client = OAuth2Client::new(&config, ProviderType::Generic)?;

    // Step 1: Generate authorization URL with PKCE
    let state = uuid::Uuid::new_v4().to_string();
    let (auth_url, code_verifier) = client.authorization_code_flow(config.scopes, state);

    println!("1. Open authorization URL in browser:\n{}\n", auth_url);

    // Step 2: User authorizes, redirect comes with code
    // After user authorizes and redirects with code...

    // Step 3: Exchange code for token
    let token = client.exchange_code_for_token("auth_code".to_string(), code_verifier).await?;
    println!("Access token: {}", token.access_token);

    // Step 4: Use token to access protected resources
    // Authorization: Bearer {token.access_token}

    // Step 5: Refresh token when expired
    if let Some(refresh_token) = &token.refresh_token {
        let new_token = client.refresh_access_token(refresh_token).await?;
        println!("Refreshed token: {}", new_token.access_token);
    }

    Ok(())
}
```

### Server: Protected Resource with RFC 9728 Metadata

```rust
use turbomcp_auth::server::{
    ProtectedResourceMetadataBuilder, WwwAuthenticateBuilder, BearerTokenValidator,
};

// Serve Protected Resource Metadata at /.well-known/protected-resource
fn get_metadata() -> Result<String, Box<dyn std::error::Error>> {
    let metadata = ProtectedResourceMetadataBuilder::new(
        "https://mcp.example.com".to_string(),
        "https://auth.example.com/.well-known/oauth-authorization-server".to_string(),
    )
    .with_scopes(vec!["mcp:read".to_string(), "mcp:write".to_string()])
    .with_documentation("https://mcp.example.com/docs".to_string())
    .build();

    Ok(serde_json::to_string_pretty(&metadata)?)
}

// Handle 401 Unauthorized responses
fn handle_unauthorized() -> (String, String) {
    let www_auth = WwwAuthenticateBuilder::new(
        "https://mcp.example.com/.well-known/protected-resource".to_string(),
    )
    .with_scope("mcp:read".to_string())
    .build();

    (www_auth, "Unauthorized".to_string())
}

// Validate incoming bearer tokens
fn extract_token(auth_header: &str) -> Result<String, Box<dyn std::error::Error>> {
    let token = BearerTokenValidator::extract_from_header(auth_header)?;
    BearerTokenValidator::validate_format(&token)?;
    Ok(token)
}
```

## Usage

```toml
[dependencies]
turbomcp-auth = "2.1.1"

# With DPoP support for enhanced security
turbomcp-auth = { version = "2.1.1", features = ["dpop"] }

# With tokio runtime
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4"] }
```

## Feature Flags

- `default` - Core authentication (no optional features)
- `dpop` - Enable DPoP (RFC 9449) token binding support via `turbomcp-dpop`

## Supported Providers

TurboMCP Auth supports all major OAuth 2.1 providers with pre-configured endpoints and scopes:

| Provider | Type | Scopes | Support | Notes |
|----------|------|--------|---------|-------|
| **Google** | Social | `openid`, `email`, `profile` | ✅ Full OAuth 2.1 | PKCE required |
| **GitHub** | Social | `user:email`, `read:user` | ✅ Full OAuth 2.1 | Token refresh via offline_access |
| **Microsoft** | Enterprise | `openid`, `profile`, `email`, `User.Read` | ✅ Full OAuth 2.1 | Azure AD integrated |
| **GitLab** | Self-Hosted | `read_user`, `openid` | ✅ Full OAuth 2.1 | Self-hosted compatible |
| **Apple** | Identity | `openid`, `email`, `name` | ✅ Full OAuth 2.1 | Requires response_mode=form_post |
| **Okta** | Enterprise | `openid`, `email`, `profile` | ✅ Full OAuth 2.1 | Enterprise SSO ready |
| **Auth0** | Identity Platform | `openid`, `email`, `profile` | ✅ Full OAuth 2.1 | Unified identity management |
| **Keycloak** | Open Source OIDC | `openid`, `email`, `profile` | ✅ Full OAuth 2.1 | Self-hosted OIDC provider |
| **Generic** | Custom | Configurable | ✅ Full OAuth 2.1 | Any OIDC-compliant provider |

All providers support:
- ✅ PKCE (RFC 7636) - Automatic proof key generation
- ✅ Token refresh - Automatic and manual refresh
- ✅ Resource Indicators (RFC 8707) - MCP server binding
- ✅ Protected Resource Metadata (RFC 9728) - Server-side discovery
- ✅ DPoP optional (RFC 9449) - Token binding for enhanced security

### Provider Examples

#### Google Sign-In
```rust
let client = OAuth2Client::new(&config, ProviderType::Google)?;
```

#### Microsoft Azure AD
```rust
let client = OAuth2Client::new(&config, ProviderType::Microsoft)?;
```

#### Apple Sign In
```rust
let client = OAuth2Client::new(&config, ProviderType::Apple)?;
// Note: Apple requires PKCE and response_mode=form_post
```

#### Okta Enterprise
```rust
let client = OAuth2Client::new(&config, ProviderType::Okta)?;
// Replace {domain} in auth/token URLs with your Okta domain
```

#### Auth0
```rust
let client = OAuth2Client::new(&config, ProviderType::Auth0)?;
// Configure with your Auth0 tenant domain
```

#### Keycloak Self-Hosted
```rust
let client = OAuth2Client::new(&config, ProviderType::Keycloak)?;
// Configure with your Keycloak realm and server URL
```

#### Custom Provider
```rust
let client = OAuth2Client::new(&config, ProviderType::Generic)?;
// Or use ProviderType::Custom("my-provider".to_string())
```

## Architecture

### Core Components

- **OAuth2Client** (`oauth2::OAuth2Client`)
  - Authorization Code flow with PKCE (RFC 7636)
  - Client Credentials flow (server-to-server)
  - Token refresh and validation
  - Provider-specific configurations for:
    - **Social Login**: Google, GitHub
    - **Enterprise**: Microsoft, Okta, Keycloak
    - **Identity Platforms**: Apple, Auth0
    - **Custom**: Generic provider with configurable endpoints

- **OAuth2Provider** (`providers::OAuth2Provider`)
  - Implements AuthProvider trait
  - Token validation via userinfo endpoint
  - Token caching and refresh management
  - Integration with authentication manager

- **AuthManager** (`manager::AuthManager`)
  - Coordinates multiple authentication providers
  - Stateless authentication (MCP compliant)
  - Token validation on every request

- **Server Helpers** (`server::*`)
  - `ProtectedResourceMetadataBuilder` - RFC 9728 metadata generation
  - `WwwAuthenticateBuilder` - RFC 9728 401 response headers
  - `BearerTokenValidator` - Token extraction and validation

### RFC Compliance

- **RFC 7636** - PKCE (Proof Key for Public OAuth Clients)
- **RFC 7591** - Dynamic Client Registration Protocol
- **RFC 8707** - Resource Indicators for OAuth 2.0
- **RFC 9728** - OAuth 2.0 Protected Resource Metadata
- **RFC 9449** - DPoP (optional, via `turbomcp-dpop`)

## Examples

Run the examples to see the implementations in action:

```bash
# OAuth 2.1 Authorization Code Flow
cargo run --example oauth2_auth_code_flow

# Protected Resource Server with RFC 9728
cargo run --example protected_resource_server
```

## Security Best Practices

1. **Use HTTPS** - Always use HTTPS for redirect URIs and token endpoints
2. **PKCE** - Automatically enabled for Authorization Code flow (RFC 7636)
3. **Token Storage** - Tokens are never logged or serialized unnecessarily
4. **Constant-Time Comparison** - Token validation uses constant-time comparison
5. **DPoP** - Enable DPoP feature for enhanced security (RFC 9449)
6. **Scope Validation** - Always validate token scopes server-side
7. **Short Expiration** - Use short-lived access tokens with refresh tokens

## Testing

```bash
cargo test --lib --package turbomcp-auth
cargo test --lib --package turbomcp-dpop  # If dpop feature enabled
```

## License

MIT
