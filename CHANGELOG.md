# Changelog

All notable changes to TurboMCP will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.0.5] - 2025-09-09

### 🎯 Major Examples Overhaul
- **Reduced from 41 to 12 focused examples** (70% reduction)
- Created clear learning progression from basics to production
- Added comprehensive EXAMPLES_GUIDE.md with learning path
- New `06_architecture_patterns.rs` showing builder vs macro equivalence
- New `06b_architecture_client.rs` separate client for testing both patterns
- Consolidated all transport demos into `07_transport_showcase.rs`
- Merged all elicitation patterns into `08_elicitation_complete.rs`
- Fixed all compilation errors across examples
- Every example now works end-to-end without placeholders
- **New two-terminal HTTP examples**: `08_elicitation_server.rs` and `08_elicitation_client.rs` for real-world testing

### 🚀 Developer Experience Improvements
- **Enhanced Error Handling**: New `McpErrorExt` trait with ergonomic error conversion methods
  - `.tool_error("context")?` instead of verbose `.map_err()` calls
  - `.network_error()`, `.protocol_error()`, `.resource_error()`, `.transport_error()` methods
  - Automatic `From` trait implementations for common error types (`std::io::Error`, `reqwest::Error`, `chrono::ParseError`, etc.)
- **Improved Prelude**: Enhanced documentation showing that `use turbomcp::prelude::*;` eliminates complex import chains
- **Better Feature Discovery**: Comprehensive 🎯 Feature Selection Guide in documentation and Cargo.toml
  - Clear recommendations for `minimal` vs `full` feature sets
  - Beginner-friendly guidance with specific use cases
  - Prominent placement of minimal features for basic tool servers
- **Comprehensive Method Documentation**: New 📚 Generated Methods Reference documenting all `#[server]` macro-generated methods
  - Transport methods (`run_stdio()`, `run_http()`, `run_tcp()`, etc.)
  - Metadata and testing methods (`server_info()`, tool metadata functions)
  - Context injection behavior and flexible parameter positioning

### ✨ New Features
- **Client Plugin System**: Extensible plugin architecture for client customization
- **LLM Integration**: Comprehensive LLM provider system with sampling protocol
- **Bidirectional Handlers**: Full support for all 4 MCP handler types:
  - ElicitationHandler for server-initiated prompts
  - ProgressHandler for operation tracking
  - LogHandler for structured logging
  - ResourceUpdateHandler for file change notifications
- **HTTP Transport**: MCP 2025-06-18 compliant streamable HTTP/SSE transport
- **Enhanced Builder API**: Improved ServerBuilder and ClientBuilder patterns

### 🛠 Improvements
- **Simplified API surface** while maintaining full functionality
- **Enhanced Cargo.toml**: Reorganized feature flags with clear descriptions and recommendations
- **Better error messages** and compile-time validation
- **Improved test coverage** with real integration tests (800+ tests passing)
- **Updated all dependencies** to latest versions
- **Enhanced documentation** with clear examples and comprehensive method reference
- **Ergonomic imports**: Single prelude import provides everything needed for most use cases
- **Production-ready error handling**: Comprehensive error conversion utilities eliminate boilerplate

### 🐛 Bug Fixes
- Fixed schema generation in macro system
- Resolved handler registration issues
- Fixed transport lifecycle management
- Corrected async trait implementations

### 📚 Documentation
- Complete examples guide with difficulty ratings
- Learning path from "Hello World" to production
- Feature matrix showing which examples demonstrate what
- Clear explanation of builder vs macro trade-offs

### 🏗 Internal Changes
- Cleaned up legacy code and unused files
- Improved module organization
- Better separation of concerns
- Consistent error handling patterns

## [1.0.4] - 2025-01-07

### Added
- Initial production release
- Core MCP protocol implementation
- Macro-based server definition
- Multi-transport support (STDIO, HTTP, WebSocket, TCP)
- Comprehensive tool and resource management
- Elicitation support for server-initiated prompts

## [1.0.3] - 2025-01-06

### Added
- Sampling protocol support
- Roots configuration
- Enhanced security features

## [1.0.2] - 2025-01-05

### Added
- OAuth 2.0 authentication
- Rate limiting
- CORS support

## [1.0.1] - 2025-01-04

### Added
- Basic MCP server functionality
- Tool registration system
- Resource management

## [1.0.0] - 2025-01-03

### Added
- Initial release
- Basic MCP protocol support
- STDIO transport