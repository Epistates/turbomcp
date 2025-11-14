# TurboMCP DPoP

RFC 9449 compliant DPoP (Demonstrating Proof-of-Possession) implementation for OAuth 2.0.

## Features

- **RFC 9449 Compliance** - Full specification implementation
- **Cryptographic Security** - ES256 (ECDSA P-256) only for maximum security
- **Token Binding** - Prevents stolen token usage
- **Replay Protection** - Nonce tracking and timestamp validation
- **HSM Support** - PKCS#11 and YubiHSM integration
- **Redis Storage** - Distributed nonce tracking

## Security Notice

**v2.2.0+** removes RSA algorithm support (RS256, PS256) to eliminate timing attack vulnerabilities (RUSTSEC-2023-0071). Only ES256 (ECDSA P-256) is supported for superior security, faster performance, and smaller key sizes.

## Usage

```toml
[dependencies]
turbomcp-dpop = "2.2.0"

# With Redis storage
turbomcp-dpop = { version = "2.2.0", features = ["redis-storage"] }

# With HSM support
turbomcp-dpop = { version = "2.2.0", features = ["hsm"] }
```

## Feature Flags

- `default` - Core DPoP functionality
- `redis-storage` - Redis backend for nonce tracking
- `hsm-pkcs11` - PKCS#11 HSM support
- `hsm-yubico` - YubiHSM support
- `hsm` - All HSM backends
- `test-utils` - Test utilities

## License

MIT
