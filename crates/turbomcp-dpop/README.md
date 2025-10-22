# TurboMCP DPoP

RFC 9449 compliant DPoP (Demonstrating Proof-of-Possession) implementation for OAuth 2.0.

## Features

- **RFC 9449 Compliance** - Full specification implementation
- **Cryptographic Security** - RSA, ECDSA P-256, and PSS support
- **Token Binding** - Prevents stolen token usage
- **Replay Protection** - Nonce tracking and timestamp validation
- **HSM Support** - PKCS#11 and YubiHSM integration
- **Redis Storage** - Distributed nonce tracking

## Usage

```toml
[dependencies]
turbomcp-dpop = "2.0.4"

# With Redis storage
turbomcp-dpop = { version = "2.0.4", features = ["redis-storage"] }

# With HSM support
turbomcp-dpop = { version = "2.0.4", features = ["hsm"] }
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
