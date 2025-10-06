# TurboMCP Auth

OAuth 2.1 and authentication for TurboMCP with MCP protocol compliance.

## Features

- **OAuth 2.1** - RFC 8707/9728/7591 compliant with MCP resource binding
- **Multi-Provider** - Google, GitHub, Microsoft with PKCE
- **API Key Auth** - Simple API key authentication
- **Session Management** - Secure token management
- **DPoP Support** - Optional RFC 9449 proof-of-possession

## Usage

```toml
[dependencies]
turbomcp-auth = "2.0.0"

# With DPoP support
turbomcp-auth = { version = "2.0.0", features = ["dpop"] }
```

## Feature Flags

- `default` - Core authentication (no optional features)
- `dpop` - Enable DPoP (RFC 9449) support

## License

MIT
