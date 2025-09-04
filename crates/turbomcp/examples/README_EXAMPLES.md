# TurboMCP Examples

## Overview

This directory contains examples demonstrating TurboMCP features and best practices.

## Core Examples (Working)

1. **01_hello_world.rs** - Minimal MCP server with a single tool
2. **02_tools_basics.rs** - Tool handlers with various parameter types  
3. **03_macros_vs_builders.rs** - Comparison of macro vs builder patterns
4. **04_comprehensive_server.rs** - Full-featured server with tools, prompts, resources
5. **06_macro_showcase.rs** - Complete macro reference guide
6. **07_performance.rs** - Performance optimization techniques
7. **09_macros.rs** - Macro usage patterns
8. **09_oauth_authentication.rs** - OAuth 2.0 authentication setup
9. **10_http_server.rs** - HTTP/SSE transport configuration
10. **11_child_process.rs** - Child process transport
11. **comprehensive_demo.rs** - Complete server implementation
12. **deployment_patterns.rs** - Production deployment strategies
13. **elicitation_demo.rs** - Elicitation feature demonstration
14. **graceful_shutdown.rs** - Graceful shutdown handling

## New MCP 2025-06-18 Features (v1.0.2)

### Available Protocol Types

The following new protocol types are available in v1.0.2:

```rust
use turbomcp_protocol::types::{
    // Elicitation
    ElicitRequest, ElicitResult, ElicitationAction,
    
    // Completion  
    CompleteRequest, CompleteRequestParams, CompletionResponse,
    CompletionReference, CompletionValue,
    
    // Resource Templates
    ListResourceTemplatesRequest, ListResourceTemplatesResult,
    ResourceTemplate, ResourceTemplateParameter,
    
    // Ping
    PingRequest, PingParams, PingResult,
};
```

### Enhanced Context Types

New context types for advanced features:

```rust
use turbomcp_core::context::{
    ElicitationContext,  // Server-initiated user input
    CompletionContext,   // Intelligent autocompletion
    PingContext,        // Bidirectional health monitoring
};
```

### New Macros (Defined but Infrastructure In Progress)

The following macros are defined in v1.0.2 but the handler infrastructure is still being integrated:

- `#[elicitation]` - Server-initiated user input requests
- `#[completion]` - Intelligent autocompletion
- `#[template]` - Resource templates with RFC 6570 URI templates
- `#[ping]` - Bidirectional health monitoring

These macros are available in `turbomcp_macros` but require additional server infrastructure that will be completed in a future release.

## Running Examples

```bash
# Run a specific example
cargo run --example 01_hello_world

# List all examples
cargo build --examples

# Test with turbomcp-cli
turbomcp-cli tools-list --command "cargo run --example 01_hello_world"
```

## Example Categories

### Getting Started
- 01_hello_world.rs - Start here
- 02_tools_basics.rs - Learn tool creation
- 09_macros.rs - Understand macros

### Advanced Features  
- 04_comprehensive_server.rs - Full server features
- 09_oauth_authentication.rs - Authentication
- 10_http_server.rs - HTTP transport

### Performance & Production
- 07_performance.rs - Optimization techniques
- deployment_patterns.rs - Deployment strategies
- graceful_shutdown.rs - Shutdown handling

### New v1.0.2 Features
- elicitation_demo.rs - Elicitation context usage (demonstrates concept)
- Protocol types are available for use in custom implementations

## Notes

- Examples use the `#[server]`, `#[tool]`, `#[prompt]`, and `#[resource]` macros which are fully functional
- The new v1.0.2 macros are defined but require additional infrastructure
- All examples compile and run with TurboMCP 1.0.2