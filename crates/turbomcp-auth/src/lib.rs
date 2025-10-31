//! # TurboMCP Auth - Unified Authentication Framework
//!
//! World-class authentication and authorization for TurboMCP with standards-compliant
//! implementations of OAuth 2.1, JWT, API keys, and DPoP token binding.
//!
//! ## Design Principles
//!
//! - **Single Source of Truth**: ONE canonical `AuthContext` type used everywhere
//! - **Feature-Gated Complexity**: Simple by default, powerful when needed
//! - **Zero-Cost Abstractions**: No overhead for unused features
//! - **Standards-Compliant**: OAuth 2.1, RFC 7519 (JWT), RFC 9449 (DPoP), RFC 9728
//!
//! ## Key Features
//!
//! - **Unified AuthContext** - Single type for all authentication scenarios
//! - **OAuth 2.1** - RFC 8707/9728/7591 compliant with PKCE support
//! - **Multi-Provider** - Google, GitHub, Microsoft, GitLab out of the box
//! - **API Key Auth** - Simple and secure API key authentication
//! - **RBAC Support** - Role-based access control with fine-grained permissions
//! - **Session Management** - Flexible token storage and lifecycle management
//! - **DPoP Support** - Optional RFC 9449 proof-of-possession tokens
//!
//! ## Architecture
//!
//! - [`context`] - Unified `AuthContext` type (THE canonical auth representation)
//! - [`types`] - Core types (UserInfo, TokenInfo, provider traits)
//! - [`config`] - Configuration types for authentication providers
//! - [`providers`] - Authentication provider implementations
//!   - `api_key` - API key authentication
//!   - `oauth2` - OAuth 2.1 provider
//! - [`manager`] - Authentication manager for provider orchestration
//! - [`oauth2`] - OAuth 2.1 client with authorization flows
//! - [`server`] - Server-side authentication helpers (RFC 9728 Protected Resource)
//!
//! ## Quick Start
//!
//! ```rust
//! use turbomcp_auth::{AuthContext, UserInfo};
//! use std::time::SystemTime;
//! use std::collections::HashMap;
//!
//! // Create an auth context using the builder
//! let user = UserInfo {
//!     id: "user123".to_string(),
//!     username: "alice".to_string(),
//!     email: Some("alice@example.com".to_string()),
//!     display_name: Some("Alice".to_string()),
//!     avatar_url: None,
//!     metadata: HashMap::new(),
//! };
//!
//! let auth = AuthContext::builder()
//!     .subject("user123")
//!     .user(user)
//!     .provider("api-key")
//!     .roles(vec!["admin".to_string(), "user".to_string()])
//!     .permissions(vec!["write:data".to_string()])
//!     .build()
//!     .unwrap();
//!
//! // Check authorization
//! if auth.has_role("admin") && auth.has_permission("write:data") {
//!     println!("User {} has write access", auth.sub);
//! }
//! ```
//!
//! ## Feature Flags
//!
//! ### Default Features
//! - `api-key` - API key authentication
//! - `oauth2` - OAuth 2.1 flows and providers
//!
//! ### Core Authentication Methods
//! - `jwt` - JWT token validation
//! - `custom` - Custom auth provider support (traits only)
//!
//! ### Advanced Features
//! - `dpop` - RFC 9449 DPoP token binding
//! - `rbac` - Role-based access control helpers
//!
//! ### Token Lifecycle
//! - `token-refresh` - Automatic token refresh
//! - `token-revocation` - Token revocation support
//!
//! ### Observability
//! - `metrics` - Metrics collection (future)
//! - `tracing-ext` - Extended tracing support
//!
//! ### Middleware
//! - `middleware` - Tower middleware support (future)
//!
//! ### Batteries-Included
//! - `full` - All features enabled
//!
//! ## Standards Compliance
//!
//! - **RFC 7519** - JSON Web Token (JWT)
//! - **RFC 6749** - OAuth 2.0 Authorization Framework
//! - **RFC 7636** - Proof Key for Code Exchange (PKCE)
//! - **RFC 8707** - OAuth 2.0 Resource Indicators
//! - **RFC 9449** - OAuth 2.0 Demonstrating Proof-of-Possession (DPoP)
//! - **RFC 9728** - OAuth 2.0 Protected Resource Metadata

// Submodules
pub mod config;
pub mod context;
pub mod introspection;
pub mod jwt;
pub mod manager;
pub mod oauth2;
pub mod providers;
pub mod server;
pub mod types;

// Re-export configuration types
#[doc(inline)]
pub use config::*;

// Re-export legacy types (excluding old AuthContext to avoid conflict with unified version)
#[doc(inline)]
pub use types::{
    AccessToken, AuthCredentials, AuthMiddleware, AuthProvider, DefaultAuthMiddleware, TokenInfo,
    TokenStorage, UserInfo,
};

// Re-export unified context types (this is the canonical AuthContext)
#[doc(inline)]
pub use context::{AuthContext, AuthContextBuilder, ValidationConfig};

// Re-export providers
#[doc(inline)]
pub use providers::*;

// Re-export manager
#[doc(inline)]
pub use manager::AuthManager;

// Re-export DPoP types when feature is enabled
#[cfg(feature = "dpop")]
pub use turbomcp_dpop as dpop;
