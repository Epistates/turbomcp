//! Security utilities for TurboMCP
//!
//! This module provides focused security utilities integrated into turbomcp-core
//! as part of the distributed security model. These utilities follow the principle
//! of doing one thing well, providing essential security primitives without
//! over-engineering.
//!
//! ## Core Functions
//!
//! - [`validate_path`] - Basic path validation with traversal attack prevention
//! - [`validate_path_within`] - Path validation with directory boundary enforcement
//! - [`validate_file_extension`] - Simple file extension validation

pub mod validation;

// Re-export main functions for convenience
pub use validation::{validate_file_extension, validate_path, validate_path_within};
