# Authentication & Authorization

Secure your MCP server with OAuth 2.1 bearer tokens, API keys, and role checks.

## Overview

Authentication in TurboMCP is a property of the **Streamable HTTP transport**,
as it is in the MCP specification:

- **MCP authorization** - `turbomcp-server`'s HTTP transport acts as an OAuth 2.1
  protected resource: it publishes RFC 9728 Protected Resource Metadata, answers
  unauthenticated requests `401` with a `WWW-Authenticate` challenge, and hands
  every bearer token to a `BearerTokenValidator`.
- **JWT validation** - `turbomcp-auth`'s `JwtBearerValidator` checks signature,
  issuer, expiry, audience, and required scopes.
- **Custom validators** - API keys, opaque tokens, or anything else, by
  implementing `BearerTokenValidator`.
- **Roles and claims** - the validated `Principal` reaches every handler through
  `RequestContext`.
- **OAuth 2.1 clients** - `turbomcp-auth` also has the client side (PKCE
  authorization code flow, provider presets), and `turbomcp-client` answers a
  server's challenge through an `AuthProvider`.

STDIO has no authentication (the client launched the server), and TCP, Unix, and
WebSocket servers have none built in: protect them at the network or file-system
level.

## Quick Start: JWT Bearer Tokens

### 1. Enable Features

```toml
turbomcp = { version = "3.5.0", features = ["http", "auth"] }
# HttpAuthorization is exported by turbomcp-server
turbomcp-server = { version = "3.5.0", features = ["http"] }
```

### 2. Configure Authorization

```rust
use turbomcp::auth::jwt::JwtValidator;
use turbomcp::auth::server::JwtBearerValidator;
use turbomcp::prelude::*;
use turbomcp_server::HttpAuthorization;

#[derive(Clone)]
struct MyServer;

#[server(name = "secure-server", version = "1.0.0")]
impl MyServer {
    /// Who is calling?
    #[tool]
    async fn whoami(&self, ctx: &RequestContext) -> String {
        ctx.subject().unwrap_or("anonymous").to_string()
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    // The server's canonical URL. Tokens must name it as their audience.
    let resource = "https://mcp.example.com/mcp";
    let jwt = JwtValidator::with_jwks_uri(
        "https://auth.example.com".to_string(),                  // issuer
        resource.to_string(),                                    // audience
        "https://auth.example.com/.well-known/jwks.json".to_string(),
    );

    let config = ServerConfig::builder()
        .authorization(
            HttpAuthorization::new(
                resource,
                "https://auth.example.com", // authorization server clients should use
                JwtBearerValidator::new(jwt).with_required_scopes(["mcp:tools"]),
            )
            .with_scopes_supported(["mcp:tools", "mcp:admin"]),
        )
        // CLIs and other servers send no Origin header
        .allow_missing_origin(true)
        .build();

    // `ServerBuilder::with_config` does not carry `authorization`,
    // so run the HTTP transport with the config directly.
    turbomcp_server::transport::http::run_with_config(&MyServer, "0.0.0.0:8080", &config).await
}
```

### 3. What the Server Does

- `GET /.well-known/oauth-protected-resource/mcp` returns the Protected Resource
  Metadata: the resource URL, its authorization servers, and supported scopes.
- A request without `Authorization: Bearer …`, or with a token the validator
  rejects, gets `401` and a `WWW-Authenticate: Bearer resource_metadata="…"`
  challenge. A valid token without a required scope gets `403`
  `insufficient_scope`.
- A valid token's `Principal` is set on the request context, and a session is
  bound to the principal that created it.

## Reading the Principal in Handlers

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Profile;

#[server]
impl Profile {
    /// Describe the caller.
    #[tool]
    async fn me(&self, ctx: &RequestContext) -> McpResult<String> {
        let Some(principal) = ctx.principal() else {
            return Err(McpError::authentication("Not signed in"));
        };

        // `JwtBearerValidator` records the token's scopes as the `scope` claim
        let scopes = principal
            .claims
            .get("scope")
            .and_then(|scope| scope.as_str())
            .unwrap_or("");

        Ok(format!(
            "{} (issuer {:?}, expires {:?}, scopes [{scopes}])",
            principal.subject, principal.issuer, principal.expires_at
        ))
    }
}
```

## Authorization (Roles and Scopes)

There are no authorization attributes: check the principal at the top of a
handler. `ctx.has_any_role(&[...])` reads `principal.roles`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Admin;

#[server]
impl Admin {
    /// Administrators only.
    #[tool]
    async fn admin_tool(&self, ctx: &RequestContext) -> McpResult<String> {
        if !ctx.has_any_role(&["admin"]) {
            return Err(McpError::permission_denied("Requires the admin role"));
        }
        Ok("Admin action".to_string())
    }

    /// Users or administrators.
    #[tool]
    async fn user_tool(&self, ctx: &RequestContext) -> McpResult<String> {
        if !ctx.has_any_role(&["user", "admin"]) {
            return Err(McpError::permission_denied("Requires the user role"));
        }
        Ok("User action".to_string())
    }
}
```

`JwtBearerValidator` does not copy the token's roles onto the principal. To
authorize by role, write a validator that does, reusing the same JWT checks:

```rust
use turbomcp::auth::jwt::JwtValidator;
use turbomcp::auth::server::{TokenValidationError, validate_bearer_token};
use turbomcp_core::auth::Principal;
use turbomcp_server::{BearerRejection, BearerTokenValidator, ValidationFuture};

struct JwtWithRoles {
    jwt: JwtValidator,
}

impl BearerTokenValidator for JwtWithRoles {
    fn validate<'a>(&'a self, token: &'a str) -> ValidationFuture<'a> {
        Box::pin(async move {
            match validate_bearer_token(&self.jwt, token, &["mcp:tools"]).await {
                Ok(context) => Ok(Principal::new(context.sub).with_roles(context.roles)),
                Err(TokenValidationError::InvalidToken(error)) => {
                    Err(BearerRejection::InvalidToken(error.to_string()))
                }
                Err(TokenValidationError::InsufficientScope { required, .. }) => {
                    Err(BearerRejection::InsufficientScope { required })
                }
            }
        })
    }
}
```

## API Keys

An API key is a bearer token your own validator recognizes. Compare keys in
constant time with `turbomcp-auth`'s helper:

```rust
use turbomcp::auth::api_key_validation::validate_api_key_multiple;
use turbomcp::prelude::*;
use turbomcp_core::auth::Principal;
use turbomcp_server::{
    BearerRejection, BearerTokenValidator, HttpAuthorization, ValidationFuture,
};

struct ApiKeys {
    keys: Vec<String>, // at least 32 characters each
}

impl BearerTokenValidator for ApiKeys {
    fn validate<'a>(&'a self, token: &'a str) -> ValidationFuture<'a> {
        Box::pin(async move {
            let keys: Vec<&str> = self.keys.iter().map(String::as_str).collect();
            if validate_api_key_multiple(token, &keys) {
                Ok(Principal::new("api-client").with_role("user"))
            } else {
                Err(BearerRejection::InvalidToken("unknown API key".into()))
            }
        })
    }
}

fn config(keys: Vec<String>) -> ServerConfig {
    ServerConfig::builder()
        .authorization(HttpAuthorization::new(
            "https://mcp.example.com/mcp",
            "https://auth.example.com",
            ApiKeys { keys },
        ))
        .allow_missing_origin(true)
        .build()
}
```

**Using in client:**

```bash
curl -X POST https://mcp.example.com/mcp \
  -H "Authorization: Bearer $API_KEY" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"curl","version":"1.0"}}}'
```

## Client Side

### Sending a Token

```rust
use turbomcp_client::Client;

#[tokio::main]
async fn main() -> turbomcp_client::Result<()> {
    let token = std::env::var("MCP_TOKEN").unwrap_or_default();
    let client = Client::connect_http_with("https://mcp.example.com", |config| {
        config.auth_token = Some(token);
    })
    .await?;
    Ok(())
}
```

To obtain a token when the server challenges, set `config.auth_provider` to an
`AuthProvider` (from `turbomcp-http`); see the
[turbomcp-client README](https://github.com/Epistates/turbomcp/tree/main/crates/turbomcp-client#http-transport).

### OAuth 2.1 Authorization Code Flow

`turbomcp-auth` implements the client side of OAuth 2.1 with PKCE, with presets
for Google, GitHub, Microsoft, GitLab, Apple, Okta, Auth0, and Keycloak:

```rust
use secrecy::{ExposeSecret, SecretString};
use turbomcp::auth::config::{OAuth2Config, OAuth2FlowType, ProviderType};
use turbomcp::auth::oauth2::OAuth2Client;

async fn sign_in(code_from_redirect: String) -> Result<String, Box<dyn std::error::Error>> {
    let config = OAuth2Config {
        client_id: "my-app-id".to_string(),
        client_secret: SecretString::from("secret".to_string()),
        auth_url: "https://accounts.google.com/o/oauth2/v2/auth".to_string(),
        token_url: "https://oauth2.googleapis.com/token".to_string(),
        revocation_url: None,
        redirect_uri: "https://myapp.com/callback".to_string(),
        scopes: vec!["openid".to_string(), "email".to_string()],
        flow_type: OAuth2FlowType::AuthorizationCode,
        additional_params: Default::default(),
        security_level: Default::default(),
        // With turbomcp-auth's `dpop` feature, also set `dpop_config: None`
        mcp_resource_uri: Some("https://mcp.example.com/mcp".to_string()),
        auto_resource_indicators: true,
        allow_custom_scheme_redirect: false,
    };
    let client = OAuth2Client::new(&config, ProviderType::Google)?;

    // 1. Send the user to `auth_url`; keep `verifier` for the exchange
    let (auth_url, verifier) = client.authorization_code_flow(config.scopes.clone(), "state".into());
    println!("Open {auth_url}");

    // 2. Exchange the code the redirect brought back
    let token = client
        .exchange_code_for_token(code_from_redirect, verifier.expose_secret().to_string())
        .await?;
    Ok(token.access_token)
}
```

`mcp_resource_uri` adds the RFC 8707 `resource` parameter, so the token is
issued for (and only for) that MCP server.

## DPoP (Demonstration of Proof-of-Possession)

`turbomcp-dpop` (feature `dpop`) implements RFC 9449 proof generation and
validation, with in-memory and Redis nonce tracking. The server's HTTP
authorization accepts `Bearer` tokens only; binding tokens with DPoP means
validating the `DPoP` proof in your own `BearerTokenValidator` or in front of
the server.

```toml
turbomcp = { version = "3.5.0", features = ["auth", "dpop"] }
```

## Security Best Practices

### 1. Use HTTPS in Production

The server transports do not terminate TLS. Run the HTTP transport behind a
reverse proxy or load balancer that does, and use the public `https://` URL as
the `HttpAuthorization` resource.

### 2. Validate the Audience

A validator must reject tokens issued for other services; this is what makes a
token stolen from another resource useless here. `JwtValidator` checks `aud`
against the audience you construct it with; a custom validator must do the same.

### 3. Never Log Tokens

Log `ctx.subject()`, never the `Authorization` header.

### 4. Validate Scopes

Require the scopes every request needs with
`JwtBearerValidator::with_required_scopes`, and check finer-grained ones in the
handler against the principal's `scope` claim.

### 5. Rate Limiting

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[tokio::main]
async fn main() -> McpResult<()> {
    MyServer
        .builder()
        .transport(Transport::http("0.0.0.0:8080"))
        .with_rate_limit(60, Duration::from_secs(60)) // per client
        .serve()
        .await
}
```

To combine a rate limit with authorization, set both on one `ServerConfig`
(`.rate_limit(RateLimitConfig::new(60, Duration::from_secs(60)))`) and run it
with `transport::http::run_with_config`.

## Testing with Auth

Build a context with a principal and call the handler directly:

```rust
use turbomcp::prelude::*;
use turbomcp_core::auth::Principal;

#[derive(Clone)]
struct Guarded;

#[server]
impl Guarded {
    /// Administrators only.
    #[tool]
    async fn admin_tool(&self, ctx: &RequestContext) -> McpResult<String> {
        if !ctx.has_any_role(&["admin"]) {
            return Err(McpError::permission_denied("Requires the admin role"));
        }
        Ok("ok".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn admins_only() {
        let admin = RequestContext::new().with_principal(Principal::new("alice").with_role("admin"));
        assert!(Guarded.admin_tool(&admin).await.is_ok());

        let guest = RequestContext::new().with_principal(Principal::new("bob"));
        assert!(Guarded.admin_tool(&guest).await.is_err());
    }
}
```

## Troubleshooting

### 401 on Every Request

Check that:
1. The token is valid and not expired
2. Its `aud` is exactly the `HttpAuthorization` resource URL
3. Its issuer and signing keys match the `JwtValidator` configuration

### 403 on Every Request

Either the token lacks a required scope (the `WWW-Authenticate` header says
`insufficient_scope`), or the request came from a non-loopback address without
an `Origin` header and `allow_missing_origin(true)` is not set.

### CORS Issues with Auth

Browser clients need the origin allowed and CORS answered:

```rust
use turbomcp::prelude::*;

let config = ServerConfig::builder()
    .allow_origin("https://myapp.com")
    .cors(true)
    .build();
```

## Next Steps

- **[Observability](observability.md)** - Monitor authentication events
- **[Examples](../examples/basic.md)** - Real-world auth patterns
- **[Deployment](../deployment/production.md)** - Production security
