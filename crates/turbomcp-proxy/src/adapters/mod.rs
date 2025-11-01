//! Protocol adapter layer
//!
//! This module provides protocol adapters for exposing MCP servers
//! via different protocols (REST, GraphQL).
//!
//! # Phase 6 - Protocol Adapters
//!
//! This phase implements adapters to translate MCP protocol capabilities into standard web APIs.

#[cfg(feature = "graphql")]
pub mod graphql;
#[cfg(feature = "rest")]
pub mod rest;

#[cfg(feature = "graphql")]
pub use graphql::{GraphQLAdapter, GraphQLAdapterConfig};
#[cfg(feature = "rest")]
pub use rest::{RestAdapter, RestAdapterConfig};
