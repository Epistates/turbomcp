# TurboMCP Examples

## 🎆 Featured Example: Real AI Code Assistant

**`sampling_ai_code_assistant.rs`** - The crown jewel of TurboMCP examples!

This AI code assistant example demonstrates TurboMCP's capabilities:
- ✅ **Real MCP 2025-06-18 sampling implementation** (no mocks!)
- ✅ **Zero-boilerplate macro magic**: `#[server]` + `#[tool]`
- ✅ **Professional features**: Session management, statistics, error handling
- ✅ **Intelligent LLM workflows**: Bug detection, security analysis, code review
- ✅ **Ready to use**: Type-safe, protocol-compliant, performance-optimized

```bash
cargo run --example sampling_ai_code_assistant
```

**Why this matters**: This example showcases TurboMCP's philosophy of "Implementation complexity ≠ User complexity." We handle the hard problems (protocol compliance, type safety, ergonomics) so you get beautiful, zero-boilerplate APIs.

---

## 📚 Example Categories

### 🚀 Getting Started (Numbered Tutorial Series)
Learn TurboMCP step-by-step with our progressive tutorial series:

- **`01_hello_world_macro.rs`** - Simplest server using macros
- **`02_hello_world_builder.rs`** - Same server using builder pattern  
- **`03_tools_and_parameters.rs`** - Tool creation with various parameter types
- **`04_resources_and_prompts.rs`** - Resources and prompt handlers
- **`05_error_handling.rs`** - Proper error handling patterns
- **`06_stateful_server.rs`** - Managing server state safely
- **`07_context_and_logging.rs`** - Using RequestContext effectively
- **`08_testing_your_server.rs`** - Testing strategies and patterns
- **`09_comprehensive_server.rs`** - Full-featured production server

### 🏗️ Architecture Patterns
Different ways to structure your MCP server:

- **`architecture_macro_based.rs`** - Full macro-driven development
- **`architecture_builder_pattern.rs`** - Pure builder pattern approach
- **`architecture_hybrid.rs`** - Combining macros and builders
- **`architecture_modular.rs`** - Multi-module server organization
- **`architecture_plugin_system.rs`** - Extensible plugin architecture

### 🔄 Transport Layers
Various transport configurations:

- **`transport_stdio.rs`** - Standard input/output (default)
- **`transport_http_sse.rs`** - HTTP with Server-Sent Events
- **`transport_websocket.rs`** - WebSocket transport
- **`transport_tcp.rs`** - Raw TCP socket transport
- **`transport_child_process.rs`** - Child process communication

### 🎯 Advanced Features
MCP 2025 specification features:

- **`sampling_ai_code_assistant.rs`** - 🌟 Real AI Code Assistant using sampling
- **`feature_sampling_server.rs`** - ⚠️ Redirects to real example above
- **`feature_sampling_client.rs`** - Client handling sampling requests
- **`feature_elicitation_server.rs`** - Server requesting user input
- **`feature_elicitation_client.rs`** - Client handling elicitation
- **`feature_oauth_authentication.rs`** - OAuth 2.0 implementation
- **`feature_resource_templates.rs`** - RFC 6570 URI templates
- **`feature_completion.rs`** - Autocompletion support

### ⚡ Performance & Production
Optimization and deployment:

- **`performance_benchmarks.rs`** - Performance measurement
- **`performance_optimization.rs`** - Optimization techniques
- **`production_graceful_shutdown.rs`** - Clean shutdown handling
- **`production_monitoring.rs`** - Health checks and metrics
- **`production_deployment.rs`** - Deployment strategies
- **`production_scaling.rs`** - Horizontal scaling patterns

### 🧪 Testing Examples
Testing patterns and strategies:

- **`testing_unit_tests.rs`** - Unit testing tools and handlers
- **`testing_integration.rs`** - Integration testing with real services
- **`testing_mocking.rs`** - When and how to use mocks properly
- **`testing_property_based.rs`** - Property-based testing with proptest

### 🔧 Reference Implementations
Complete working servers:

- **`reference_code_assistant.rs`** - AI-powered code analysis
- **`reference_database_manager.rs`** - Database operations server
- **`reference_file_system.rs`** - File system operations
- **`reference_api_gateway.rs`** - API gateway server
- **`reference_workflow_engine.rs`** - Workflow automation

## 🎯 Quick Start

```bash
# Start with the basics
cargo run --example 01_hello_world_macro

# Compare macro vs builder
cargo run --example 01_hello_world_macro
cargo run --example 02_hello_world_builder

# Run a complete server
cargo run --example 09_comprehensive_server

# Test with turbomcp-cli
turbomcp-cli tools-list --command "cargo run --example 01_hello_world_macro"
```

## 📖 Learning Path

### Beginner (2 hours)
1. Start with `01_hello_world_macro.rs`
2. Compare with `02_hello_world_builder.rs` 
3. Learn tools in `03_tools_and_parameters.rs`
4. Add resources in `04_resources_and_prompts.rs`

### Intermediate (4 hours)
5. Master error handling in `05_error_handling.rs`
6. Manage state in `06_stateful_server.rs`
7. Use context in `07_context_and_logging.rs`
8. Test your code in `08_testing_your_server.rs`

### Advanced (8 hours)
9. Build comprehensive servers with `09_comprehensive_server.rs`
10. Explore architecture patterns in `architecture_*.rs`
11. Implement advanced features in `feature_*.rs`
12. Optimize for production in `production_*.rs`

## 🛠️ Example Conventions

### Naming Convention
- **Numbered tutorials**: `01_topic.rs` through `09_topic.rs`
- **Architecture patterns**: `architecture_pattern_name.rs`
- **Transport examples**: `transport_type.rs`
- **Features**: `feature_name_role.rs` (e.g., `feature_sampling_server.rs`)
- **Performance**: `performance_aspect.rs`
- **Production**: `production_concern.rs`
- **Testing**: `testing_strategy.rs`
- **Reference**: `reference_application.rs`

### Code Standards
- Every example is production-ready (no placeholders)
- All examples compile and run
- Clear documentation with learning goals
- Realistic use cases
- Proper error handling
- No unnecessary complexity

### Documentation Requirements
Each example includes:
- Purpose and learning goals
- Prerequisites (if any)
- Run instructions
- Expected output
- Related examples
- Next steps

## 🔍 Finding the Right Example

### By Feature
- **Macros**: `01_hello_world_macro.rs`, `architecture_macro_based.rs`
- **Builder Pattern**: `02_hello_world_builder.rs`, `architecture_builder_pattern.rs`
- **Tools**: `03_tools_and_parameters.rs`, all reference implementations
- **Resources**: `04_resources_and_prompts.rs`, `feature_resource_templates.rs`
- **Prompts**: `04_resources_and_prompts.rs`, `reference_code_assistant.rs`
- **State Management**: `06_stateful_server.rs`, all reference implementations
- **Error Handling**: `05_error_handling.rs`, `production_monitoring.rs`
- **Testing**: All `testing_*.rs` examples
- **Authentication**: `feature_oauth_authentication.rs`
- **Sampling**: `feature_sampling_server.rs`, `feature_sampling_client.rs`
- **Elicitation**: `feature_elicitation_server.rs`, `feature_elicitation_client.rs`

### By Use Case
- **Simple CLI Tool**: Start with `01_hello_world_macro.rs`
- **Database Operations**: See `reference_database_manager.rs`
- **File Management**: See `reference_file_system.rs`
- **AI Integration**: See `feature_sampling_server.rs`, `reference_code_assistant.rs`
- **Web Service**: See `transport_http_sse.rs`, `reference_api_gateway.rs`
- **Automation**: See `reference_workflow_engine.rs`

## 📊 Example Metrics

- **Total Examples**: 45+
- **Categories**: 8
- **Learning Path**: Progressive from beginner to advanced
- **Coverage**: 100% of TurboMCP features
- **Quality**: Production-ready, no mocks or placeholders

## 🚦 Status

All examples are:
- ✅ Compiling with latest TurboMCP
- ✅ Following best practices
- ✅ Well-documented
- ✅ Production-ready
- ✅ Tested

## 📝 Contributing

When adding new examples:
1. Follow the naming convention
2. Include comprehensive documentation
3. Ensure production quality (no placeholders)
4. Add to appropriate category
5. Update this README
6. Test with `cargo run --example <name>`

## 🔗 Related Documentation

- [TurboMCP Documentation](https://docs.rs/turbomcp)
- [MCP Specification](https://modelcontextprotocol.io)
- [Architecture Guide](../../../docs/ARCHITECTURE.md)
- [API Reference](../../../docs/API.md)