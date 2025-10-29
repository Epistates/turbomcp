//! Authentication Providers
//!
//! This module contains various authentication provider implementations.

pub mod api_key;
pub mod oauth2;

pub use api_key::ApiKeyProvider;
pub use oauth2::OAuth2Provider;
