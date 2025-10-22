# TurboMCP Auth

OAuth 2.1 and authentication for TurboMCP with MCP protocol compliance.

## Features

- **OAuth 2.1** - RFC 8707/9728/7591 compliant with MCP resource binding
- **Multi-Provider** - Google, GitHub, Microsoft, GitLab with PKCE
- **API Key Auth** - Simple API key authentication
- **Session Management** - Secure token management with configurable storage
- **DPoP Support** - Optional RFC 9449 proof-of-possession tokens

## Usage

```toml
[dependencies]
turbomcp-auth = "2.0.4"

# With DPoP support
turbomcp-auth = { version = "2.0.4", features = ["dpop"] }
```

## Feature Flags

- `default` - Core authentication (no optional features)
- `dpop` - Enable DPoP (RFC 9449) token binding support via `turbomcp-dpop`

## Architecture

This crate provides:

- **Authentication Manager** - Coordinates multiple authentication providers
- **OAuth 2.1 Client** - Supports Authorization Code, Client Credentials, and Device flows
- **API Key Provider** - Simple API key-based authentication
- **Session Management** - Token storage and lifecycle management
- **RFC Compliance** - Resource Indicators (RFC 8707), Protected Resource Metadata (RFC 9728), Dynamic Client Registration (RFC 7591)

See the [module documentation](https://docs.rs/turbomcp-auth) for detailed usage examples.

## License

MIT
