# TurboMCP DPoP

[![Crates.io](https://img.shields.io/crates/v/turbomcp-dpop.svg)](https://crates.io/crates/turbomcp-dpop)
[![Documentation](https://docs.rs/turbomcp-dpop/badge.svg)](https://docs.rs/turbomcp-dpop)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](../LICENSE)

**RFC 9449 compliant Demonstration of Proof-of-Possession (DPoP) implementation for TurboMCP**

## Overview

`turbomcp-dpop` provides an implementation of RFC 9449 Demonstration of Proof-of-Possession (DPoP) for enhanced OAuth 2.0 security. DPoP is a mechanism that enables the binding of access tokens to a specific client, preventing token theft and replay attacks.

## Key Features

### 🔐 **RFC 9449 Compliance**
- Complete DPoP specification implementation
- JWT-based proof-of-possession tokens
- Cryptographic binding to access tokens
- Nonce handling and replay protection

### 🛡️ **Security Features**
- **Token Binding** - Cryptographically bind tokens to client keys
- **Replay Protection** - Automatic nonce generation and validation
- **Key Management** - Secure key generation and storage options
- **Time-based Validation** - JWT expiration and timestamp checks

### 🔧 **Flexible Architecture**
- **Multiple Key Stores** - Memory, Redis, HSM support
- **Algorithm Support** - ES256, RS256 key algorithms
- **Integration Ready** - Works seamlessly with TurboMCP OAuth flows
- **Production Ready** - Comprehensive error handling and logging

## Quick Start

### Basic Usage

```rust
use turbomcp_dpop::{DpopKeyManager, DpopProofGenerator};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a key manager (in-memory for demo)
    let key_manager = DpopKeyManager::new_memory().await?;
    
    // Create proof generator
    let proof_generator = DpopProofGenerator::new(key_manager.into());
    
    // Generate DPoP proof for API request
    let proof = proof_generator.generate_proof(
        "POST",
        "https://api.example.com/resource", 
        Some("access_token_here")
    ).await?;
    
    println!("DPoP proof: {}", proof);
    Ok(())
}
```

### With TurboMCP Integration

```rust
use turbomcp::prelude::*;
use turbomcp_dpop::{DpopKeyManager, DpopProofGenerator};

#[derive(Clone)]
struct SecureServer {
    dpop_generator: DpopProofGenerator,
}

#[server]
impl SecureServer {
    #[tool("Make authenticated API call with DPoP")]
    async fn secure_api_call(&self, url: String) -> McpResult<String> {
        // Generate DPoP proof for the request
        let proof = self.dpop_generator.generate_proof(
            "GET", 
            &url, 
            None
        ).await?;
        
        // Use proof in HTTP headers
        // DPoP: <proof_jwt>
        // Authorization: Bearer <access_token>
        
        Ok(format!("Request to {} with DPoP proof", url))
    }
}
```

## Key Management

### In-Memory Key Store (Development)

```rust
use turbomcp_dpop::DpopKeyManager;

let key_manager = DpopKeyManager::new_memory().await?;
```

### Redis Key Store (Production)

```rust
use turbomcp_dpop::DpopKeyManager;

let key_manager = DpopKeyManager::new_redis("redis://localhost:6379").await?;
```

### HSM Integration (High Security)

```rust
use turbomcp_dpop::DpopKeyManager;

let key_manager = DpopKeyManager::new_hsm(hsm_config).await?;
```

## Architecture

DPoP enhances OAuth 2.0 security through:

1. **Key Generation** - Client generates a public/private key pair
2. **Proof Creation** - Client creates JWT proofs for each request  
3. **Token Binding** - Access tokens are bound to the client's public key
4. **Verification** - Server validates proofs and token binding

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│   Client    │────▶│   Server    │────▶│   Resource  │
│             │     │             │     │   Server    │
│ - Generates │     │ - Validates │     │ - Verifies  │
│   DPoP JWT  │     │   DPoP JWT  │     │   Token     │
│ - Binds to  │     │ - Issues    │     │   Binding   │
│   Token     │     │   Token     │     │             │
└─────────────┘     └─────────────┘     └─────────────┘
```

## Security Considerations

### Production Deployment

- **Use persistent key storage** (Redis/HSM) for production
- **Implement key rotation** policies
- **Monitor for replay attacks** through nonce tracking
- **Secure key material** with appropriate access controls

### Key Rotation

```rust
use turbomcp_dpop::{DpopKeyManager, KeyRotationPolicy};

let key_manager = DpopKeyManager::new_redis("redis://localhost:6379")
    .await?
    .with_rotation_policy(KeyRotationPolicy::Daily)
    .with_cleanup_expired_keys(true);
```

## Integration with OAuth 2.0

### Authorization Server Integration

```rust
use turbomcp_dpop::{DpopValidator, DpopConfig};

// Validate DPoP proof during token request
let validator = DpopValidator::new(DpopConfig::default());

match validator.validate_proof(&dpop_header, &request) {
    Ok(claims) => {
        // Bind access token to DPoP key
        let access_token = bind_token_to_dpop_key(claims.public_key);
        // Issue token...
    }
    Err(e) => {
        // Reject request
        return Err(e);
    }
}
```

### Resource Server Integration

```rust
use turbomcp_dpop::{DpopValidator, AccessTokenValidator};

// Validate both access token and DPoP proof
let dpop_validator = DpopValidator::new(DpopConfig::default());
let token_validator = AccessTokenValidator::new();

// Validate DPoP proof
let dpop_claims = dpop_validator.validate_proof(&dpop_header, &request)?;

// Validate access token binding
let token_claims = token_validator.validate_token(&access_token)?;

if !token_claims.is_bound_to_key(&dpop_claims.public_key) {
    return Err(DpopError::TokenBindingMismatch);
}
```

## Error Handling

```rust
use turbomcp_dpop::{DpopError, DpopResult};

match proof_generator.generate_proof("GET", &url, None).await {
    Ok(proof) => println!("Generated proof: {}", proof),
    Err(DpopError::KeyManagementError(e)) => {
        eprintln!("Key management error: {}", e);
    }
    Err(DpopError::JwtError(e)) => {
        eprintln!("JWT error: {}", e);
    }
    Err(DpopError::ValidationError(msg)) => {
        eprintln!("Validation error: {}", msg);
    }
}
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `redis` | Enable Redis key storage | ❌ |
| `hsm` | Enable HSM integration | ❌ |
| `serde` | Enable serde serialization | ✅ |

## Related Documentation

- **[RFC 9449](https://datatracker.ietf.org/doc/html/rfc9449)** - DPoP specification
- **[TurboMCP OAuth Guide](../turbomcp/docs/oauth.md)** - OAuth 2.0 integration
- **[Security Best Practices](../turbomcp-transport/SECURITY_FEATURES.md)** - Production security

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) high-performance Rust SDK for the Model Context Protocol.*