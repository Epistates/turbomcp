# TurboMCP DPoP

RFC 9449 compliant DPoP (Demonstrating Proof-of-Possession) implementation for OAuth 2.0.

## Features

- **RFC 9449 Compliance** - Full specification implementation
- **Cryptographic Security** - ES256 (ECDSA P-256) only for maximum security
- **Token Binding** - Prevents stolen token usage
- **Replay Protection** - Nonce tracking and timestamp validation
- **HSM Support** - PKCS#11 and YubiHSM integration
- **Redis Storage** - Distributed nonce tracking

## Algorithm Choice: ES256 Only

**TurboMCP DPoP exclusively supports ES256 (ECDSA P-256)** as of v2.2.0+. This is an intentional security decision, not a limitation.

### Why ES256 Only?

| Criterion | ES256 (ECDSA P-256) | RSA (RS256/PS256) |
|:----------|:--------------------|:------------------|
| **Security** | Timing-attack resistant | Vulnerable (RUSTSEC-2023-0071) |
| **Key Size** | 256 bits | 2048-4096 bits |
| **Signature Size** | 64 bytes | 256-512 bytes |
| **Performance** | Faster signing/verification | Slower operations |
| **2026 Compliance** | Recommended by NIST | Being phased out |

### Security Advisory

RSA algorithm support was removed due to **RUSTSEC-2023-0071**, which affects the `rsa` crate's PKCS#1 v1.5 padding implementation. The vulnerability allows timing side-channel attacks that can leak private key information.

**Q1 2026 Best Practices** recommend:
- ES256 (P-256) for new implementations
- ES384 (P-384) for higher security requirements
- Avoiding RSA for new DPoP/JWT signing implementations

### Migration from RSA

If you're migrating from an RSA-based DPoP implementation:

1. **Generate new ES256 keys**: Existing RSA keys cannot be converted
2. **Update client configurations**: Point to new JWKS endpoint
3. **Rotate during maintenance window**: Old tokens remain valid until expiry
4. **Update JWKS endpoints**: Serve only ES256 public keys

```rust
use turbomcp_dpop::DpopKeyPair;

// Generate new ES256 key pair
let key_pair = DpopKeyPair::generate()?;

// Export public key for JWKS
let jwk = key_pair.public_jwk();
```

### References

- [RFC 9449: DPoP](https://www.rfc-editor.org/rfc/rfc9449.html)
- [RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html)
- [NIST SP 800-186: Recommendations for Discrete Logarithm-based Cryptography](https://csrc.nist.gov/publications/detail/sp/800-186/final)

## Usage

```toml
[dependencies]
turbomcp-dpop = "3.0.0-beta.4"

# With Redis storage
turbomcp-dpop = { version = "3.0.0-beta.4", features = ["redis-storage"] }

# With HSM support
turbomcp-dpop = { version = "3.0.0-beta.4", features = ["hsm"] }
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
