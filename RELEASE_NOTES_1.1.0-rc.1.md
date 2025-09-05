# TurboMCP 1.1.0-rc.1 Release Notes

## 🚀 Major New Features

### 🔐 RFC 9449 DPoP Implementation (Demonstration of Proof-of-Possession)
**NEW: `turbomcp-dpop` crate** - Enterprise-grade OAuth 2.0 security enhancement

#### Key Features:
- **Complete RFC 9449 compliance** - Full Demonstration of Proof-of-Possession implementation
- **Progressive security levels**:
  - `Standard` - Traditional OAuth 2.0 with PKCE
  - `Enhanced` - DPoP token binding for replay attack prevention
  - `Maximum` - Full DPoP with additional security features
- **Multiple cryptographic algorithms**: ES256 (ECDSA), RS256 (RSA), PS256 (RSA-PSS)
- **Enterprise HSM support**: PKCS#11, YubiHSM integration for hardware-backed keys
- **Production storage options**: Redis, in-memory, custom storage backends
- **Automatic key rotation** with configurable policies
- **Comprehensive security validation** including replay attack prevention

#### Integration Points:
```rust
use turbomcp::auth::{OAuth2Config, SecurityLevel, DpopConfig};

let config = OAuth2Config::new()
    .client_id("your-client-id")
    .client_secret("your-client-secret")
    .security_level(SecurityLevel::Enhanced)  // Enable DPoP
    .dpop_config(DpopConfig::memory());

let provider = OAuth2Provider::new(config, provider_type, token_storage).await?;
```

### 🏗️ Enhanced Authentication Architecture

#### Security-First OAuth 2.0 Implementation:
- **Async OAuth2Provider** - Complete rewrite for DPoP compatibility
- **Token binding protection** - Cryptographic token-to-request binding
- **Configurable security levels** - Choose appropriate security for your use case
- **Multiple storage backends** - Memory, Redis, custom implementations

## 🔧 Breaking Changes

### OAuth2Provider Changes:
```rust
// Before (1.0.3)
let provider = OAuth2Provider::new(config, provider_type, token_storage)?;

// After (1.1.0-rc.1)
let provider = OAuth2Provider::new(config, provider_type, token_storage).await?;
//                                                                      ^^^^^^^^
//                                                                      Now async
```

### OAuth2Config Changes:
```rust
// Before (1.0.3)
OAuth2Config::new().client_id("id").client_secret("secret")

// After (1.1.0-rc.1)
OAuth2Config::new()
    .client_id("id")
    .client_secret("secret")
    .security_level(SecurityLevel::Standard)  // Required field
```

## 🧪 Testing & Quality Improvements

### Production-Grade Test Infrastructure:
- **Real Redis integration tests** using Docker containers (no mocks!)
- **Comprehensive DPoP security validation** - 400+ test cases
- **HSM integration testing** for enterprise environments
- **Concurrent operation testing** for production reliability
- **RFC compliance validation** ensuring spec adherence

### Zero-Tolerance Quality Standards:
- All tests use real implementations (Docker, Testcontainers)
- Comprehensive security scenario testing
- Performance benchmarks for cryptographic operations
- Memory safety validation with production workloads

## 📦 Workspace Updates

### New Crate: `turbomcp-dpop`
```toml
[dependencies]
turbomcp = { version = "1.1.0-rc.1", features = ["dpop"] }
turbomcp-dpop = "1.1.0-rc.1"  # For advanced DPoP usage
```

### Feature Flags:
- `dpop` - Enable DPoP support in main TurboMCP crate
- `hsm-support` - Hardware Security Module integration
- `redis-storage` - Redis-backed nonce storage

## 🔒 Security Enhancements

### DPoP Security Benefits:
1. **Replay Attack Prevention** - Cryptographic nonces prevent request replay
2. **Token Binding** - Tokens cryptographically bound to client keys
3. **Man-in-the-Middle Protection** - Request integrity validation
4. **Hardware-Backed Security** - Optional HSM integration for key storage

### Production Security Features:
- Automatic key expiration and rotation
- Configurable clock skew tolerance
- Comprehensive audit logging
- Error classification (Critical/High/Medium/Low)

## 🚧 Release Candidate Notes

This is a **release candidate** for 1.1.0. Key focus areas for feedback:

1. **DPoP Integration API** - Is the security level configuration intuitive?
2. **Breaking Changes** - Are migration paths clear for existing OAuth implementations?
3. **Performance Impact** - How does DPoP affect your application performance?
4. **Documentation** - Are the security concepts well explained?

### Known Limitations:
- Redis cluster support planned for 1.1.0 final
- Additional HSM vendors (AWS KMS, Azure Key Vault) planned for 1.2.0
- WebAuthn integration planned for 1.2.0

## 🛠️ Migration Guide

### For Existing OAuth Users:

1. **Update OAuth2Provider calls to async**:
   ```rust
   // Add .await to provider creation
   let provider = OAuth2Provider::new(config, provider_type, storage).await?;
   ```

2. **Add security_level to OAuth2Config**:
   ```rust
   let config = OAuth2Config::new()
       .client_id("id")
       .client_secret("secret")
       .security_level(SecurityLevel::Standard);  // Add this line
   ```

3. **Optional: Enable DPoP for enhanced security**:
   ```rust
   .security_level(SecurityLevel::Enhanced)
   .dpop_config(DpopConfig::memory())
   ```

## 📚 Documentation & Examples

### New Examples:
- `feature_oauth_authentication.rs` - Updated OAuth integration
- `basic_dpop.rs` - Simple DPoP implementation
- `dpop_security_tests.rs` - Security validation patterns

### Enhanced Documentation:
- Complete DPoP security model explanation
- HSM integration guides
- Production deployment recommendations
- Security best practices

## 🎯 Next Steps

### For 1.1.0 Final:
- [ ] Community feedback integration
- [ ] Redis cluster support
- [ ] Performance optimizations based on benchmarks
- [ ] Additional security hardening

### For 1.2.0:
- [ ] AWS KMS and Azure Key Vault integration
- [ ] WebAuthn support for passwordless authentication
- [ ] Advanced audit logging and monitoring
- [ ] Kubernetes operator for production deployment

## 🙏 Acknowledgments

This release represents a significant security advancement for the MCP ecosystem, implementing cutting-edge OAuth 2.0 security extensions while maintaining the ergonomic developer experience TurboMCP is known for.

**Special thanks to the security community for RFC 9449 and the ongoing work to improve OAuth 2.0 security.**

---

## 🔗 Links

- [RFC 9449 - OAuth 2.0 Demonstration of Proof-of-Possession at the Application Layer (DPoP)](https://datatracker.ietf.org/doc/html/rfc9449)
- [TurboMCP Documentation](https://docs.rs/turbomcp)
- [GitHub Repository](https://github.com/Epistates/turbomcp)
- [Security Reporting](https://github.com/Epistates/turbomcp/security/policy)

**Full Changelog**: https://github.com/Epistates/turbomcp/compare/v1.0.3...v1.1.0-rc.1