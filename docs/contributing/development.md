# Development Guide

Complete guide to setting up your development environment and contributing to TurboMCP.

## Table of Contents

- [Getting Started](#getting-started)
- [Development Environment](#development-environment)
- [Project Structure](#project-structure)
- [Development Workflow](#development-workflow)
- [Testing](#testing)
- [Code Quality](#code-quality)
- [Documentation](#documentation)
- [Pull Request Process](#pull-request-process)
- [Release Process](#release-process)

## Getting Started

### Prerequisites

**Required:**
- Rust 1.89.0 or later
- Git 2.x or later
- Just command runner (install: `cargo install just`)

**Optional but Recommended:**
- cargo-expand (for macro debugging: `cargo install cargo-expand`)
- cargo-audit (for security checks: `cargo install cargo-audit`)
- cargo-outdated (for dependency updates: `cargo install cargo-outdated`)
- cargo-watch (for auto-rebuild: `cargo install cargo-watch`)

### Initial Setup

```bash
# Clone the repository
git clone https://github.com/turbomcp/turbomcp.git
cd turbomcp

# Build the project
cargo build --workspace

# Run tests to verify setup
just test

# Build documentation
cargo doc --workspace --no-deps --open
```

If everything builds successfully, you're ready to start developing!

## Development Environment

### Recommended IDE Setup

**VS Code:**

Install these extensions:
- rust-analyzer (essential)
- crates (dependency management)
- Even Better TOML (Cargo.toml editing)
- CodeLLDB (debugging)
- Error Lens (inline errors)

`.vscode/settings.json`:

```json
{
  "rust-analyzer.cargo.features": "all",
  "rust-analyzer.checkOnSave.command": "clippy",
  "rust-analyzer.checkOnSave.allTargets": false,
  "editor.formatOnSave": true,
  "editor.rulers": [100],
  "[rust]": {
    "editor.defaultFormatter": "rust-lang.rust-analyzer"
  }
}
```

**IntelliJ IDEA / CLion:**

1. Install IntelliJ Rust plugin
2. Enable Clippy in Settings > Languages & Frameworks > Rust > Cargo
3. Enable rustfmt on save

**Neovim / Vim:**

```lua
-- Using nvim-lspconfig
require('lspconfig').rust_analyzer.setup{
  settings = {
    ["rust-analyzer"] = {
      cargo = {
        allFeatures = true,
      },
      checkOnSave = {
        command = "clippy",
      },
    }
  }
}
```

### Environment Variables

Create `.env` (not committed to git):

```bash
# Development settings
RUST_LOG=debug
RUST_BACKTRACE=1

# Test database (if needed)
DATABASE_URL=postgres://localhost/turbomcp_test

# Feature flags for development
TURBOMCP_FEATURES=simd,auth,websocket
```

## Project Structure

```
turbomcp/
├── crates/
│   ├── turbomcp/              # Main SDK crate
│   │   ├── src/
│   │   │   ├── lib.rs         # Public API exports
│   │   │   ├── prelude.rs     # Convenient imports
│   │   │   └── config/        # Configuration types
│   │   ├── examples/          # Example servers (26 examples)
│   │   └── tests/             # Integration tests
│   │
│   ├── turbomcp-protocol/     # Foundation layer
│   │   ├── src/
│   │   │   ├── messages/      # JSON-RPC & MCP messages
│   │   │   ├── context/       # Request context
│   │   │   ├── session/       # Session management
│   │   │   └── registry/      # Component registry
│   │   └── tests/
│   │
│   ├── turbomcp-transport/    # Transport layer
│   │   ├── src/
│   │   │   ├── stdio/         # STDIO transport
│   │   │   ├── http/          # HTTP/SSE transport
│   │   │   ├── websocket/     # WebSocket transport
│   │   │   ├── tcp/           # TCP transport
│   │   │   └── unix/          # Unix socket transport
│   │   └── tests/
│   │
│   ├── turbomcp-server/       # Server framework
│   │   ├── src/
│   │   │   ├── routing/       # Request routing
│   │   │   ├── middleware/    # Middleware stack
│   │   │   └── runtime/       # Server runtime
│   │   └── tests/
│   │
│   ├── turbomcp-client/       # Client implementation
│   │   └── src/
│   │
│   ├── turbomcp-macros/       # Procedural macros
│   │   ├── src/
│   │   │   ├── server.rs      # #[server] macro
│   │   │   ├── tool.rs        # #[tool] macro
│   │   │   ├── resource.rs    # #[resource] macro
│   │   │   └── prompt.rs      # #[prompt] macro
│   │   └── tests/
│   │
│   ├── turbomcp-cli/          # CLI tools
│   ├── turbomcp-auth/         # OAuth 2.1 authentication
│   ├── turbomcp-dpop/         # DPoP support
│   └── turbomcp-proxy/        # Universal adapter
│
├── tests/                      # Workspace-level integration tests
├── benches/                    # Benchmarks
├── scripts/                    # Build and utility scripts
├── docs/                       # Documentation
├── Cargo.toml                  # Workspace configuration
├── justfile                    # Task runner commands
└── CLAUDE.md                   # AI assistant guidance
```

### Crate Responsibilities

**turbomcp-protocol** (Foundation):
- JSON-RPC 2.0 message types
- MCP protocol implementation
- SIMD JSON processing
- Session state management
- Error types

**turbomcp-transport** (Network):
- Transport protocol implementations
- Connection pooling
- TLS/security
- Circuit breakers

**turbomcp-server** (Infrastructure):
- Handler registry
- Middleware pipeline
- Request routing
- Health checks

**turbomcp-client** (Client):
- Connection management
- Request/response correlation
- Auto-retry logic

**turbomcp-macros** (Developer Experience):
- Procedural macros
- Schema generation
- Code generation

**turbomcp** (High-Level API):
- Public API surface
- Configuration presets
- Convenience wrappers

## Development Workflow

### Creating a Branch

```bash
# Feature branch
git checkout -b feature/add-new-capability

# Bug fix branch
git checkout -b fix/handle-empty-requests

# Documentation branch
git checkout -b docs/improve-architecture-guide
```

### Making Changes

#### 1. Write Code

```bash
# Auto-rebuild on changes
cargo watch -x "check --all-features"

# Or use specific features
cargo watch -x "check -p turbomcp-server --features http,websocket"
```

#### 2. Add Tests

Every change should include tests:

```rust
// Unit test
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_feature() {
        let result = my_function(42);
        assert_eq!(result, 42);
    }

    #[tokio::test]
    async fn test_async_feature() {
        let result = my_async_function().await.unwrap();
        assert!(result.is_some());
    }
}
```

#### 3. Run Tests

```bash
# Run all tests
just test

# Run only unit tests (skip clippy/fmt)
just test-only

# Run specific crate tests
cargo test -p turbomcp-protocol

# Run specific test
cargo test test_my_feature

# Run with output
cargo test test_my_feature -- --nocapture

# Run tests with specific features
cargo test -p turbomcp-transport --features stdio,tcp
```

#### 4. Check Code Quality

```bash
# Format code
cargo fmt --all

# Run Clippy
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Check without building
cargo check --workspace --all-targets
```

#### 5. Update Documentation

```bash
# Build docs
cargo doc --workspace --no-deps --open

# Check for broken links
cargo doc --workspace --no-deps 2>&1 | grep -i "warning"

# Spell check (if using cspell)
cspell "**/*.md" "**/*.rs"
```

### Commit Guidelines

**Commit Message Format:**

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `test`: Adding or updating tests
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `chore`: Maintenance tasks
- `ci`: CI/CD changes

**Examples:**

```bash
# Good commit messages
git commit -m "feat(server): add WebSocket transport support"
git commit -m "fix(protocol): handle malformed JSON-RPC requests"
git commit -m "docs(architecture): add dependency injection guide"
git commit -m "test(transport): add comprehensive TCP transport tests"

# With body
git commit -m "feat(macros): add #[prompt] macro

Implements prompt template macro with argument validation
and automatic schema generation.

Closes #123"
```

**Bad commit messages:**
- "fix stuff"
- "update code"
- "WIP"
- "asdf"

### Running Examples

```bash
# List all examples
cargo run --example

# Run specific example
cargo run --example hello_world
cargo run --example macro_server
cargo run --example http_app

# Run with features
cargo run --example http_app --features http,simd

# Run with debug output
RUST_LOG=debug cargo run --example macro_server
```

### Debugging

**Print Debugging:**

```rust
// Use debug!() macro instead of println!()
use log::debug;

debug!("Processing request: {:?}", request);
debug!("Handler found: {}", handler_name);
```

**Using Debugger (VS Code):**

`.vscode/launch.json`:

```json
{
  "version": "0.2.0",
  "configurations": [
    {
      "type": "lldb",
      "request": "launch",
      "name": "Debug example 'hello_world'",
      "cargo": {
        "args": ["build", "--example", "hello_world"],
        "filter": {
          "name": "hello_world",
          "kind": "example"
        }
      },
      "args": [],
      "cwd": "${workspaceFolder}"
    }
  ]
}
```

**Expanding Macros:**

```bash
# See what the #[tool] macro generates
cargo expand -p turbomcp-macros tool

# Expand specific module
cargo expand -p turbomcp server::core
```

## Testing

### Test Organization

**Unit Tests** (`#[cfg(test)]` modules):
- Test individual functions
- Mock dependencies
- Fast execution

**Integration Tests** (`tests/` directory):
- Test public API
- Test cross-crate interactions
- Real transports

**Example Tests** (in `examples/`):
- Ensure examples build and run
- Serve as documentation

### Writing Tests

**Unit Test:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_request() {
        let json = r#"{"jsonrpc":"2.0","method":"ping","id":1}"#;
        let request = parse_json_rpc_request(json).unwrap();

        assert_eq!(request.method, "ping");
        assert_eq!(request.id, Some(json!(1)));
    }
}
```

**Async Test:**

```rust
#[tokio::test]
async fn test_server_initialization() {
    let server = McpServer::new()
        .with_test_config()
        .stdio()
        .build();

    let response = server.initialize(InitializeRequest::default()).await;
    assert!(response.is_ok());
}
```

**Property-Based Test:**

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn test_valid_identifiers(s in "[a-zA-Z_][a-zA-Z0-9_]*") {
        assert!(is_valid_identifier(&s));
    }
}
```

**Benchmark:**

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn bench_json_parsing(c: &mut Criterion) {
    let json = r#"{"jsonrpc":"2.0","method":"test","id":1}"#;

    c.bench_function("parse_json_rpc", |b| {
        b.iter(|| parse_json_rpc_request(black_box(json)))
    });
}

criterion_group!(benches, bench_json_parsing);
criterion_main!(benches);
```

### Test Coverage

```bash
# Install tarpaulin
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --workspace --out Html --output-dir coverage/

# Open report
open coverage/index.html
```

### Testing Transport-Specific Code

**Important:** Transport tests require feature flags:

```bash
# ALWAYS test transport with features enabled
cargo test -p turbomcp-transport --lib --tests --features stdio,tcp

# NEVER run without features (will skip most tests)
cargo test -p turbomcp-transport  # ❌ WRONG
```

## Code Quality

### Formatting

```bash
# Format all code
cargo fmt --all

# Check formatting without modifying
cargo fmt --all -- --check
```

**Custom rustfmt config** (`rustfmt.toml`):

```toml
edition = "2021"
max_width = 100
use_small_heuristics = "Max"
imports_granularity = "Crate"
group_imports = "StdExternalCrate"
```

### Linting

```bash
# Run Clippy
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Fix auto-fixable issues
cargo clippy --workspace --all-targets --all-features --fix
```

**Custom Clippy config** (`.cargo/config.toml`):

```toml
[target.'cfg(all())']
rustflags = [
  "-W", "clippy::pedantic",
  "-W", "clippy::nursery",
  "-A", "clippy::module_name_repetitions",
]
```

### Security Audit

```bash
# Check for known vulnerabilities
cargo audit

# Fix vulnerable dependencies
cargo update
cargo audit
```

### Dependency Management

```bash
# Check for outdated dependencies
cargo outdated

# Update dependencies
cargo update

# Remove unused dependencies
cargo machete  # Install: cargo install cargo-machete
```

## Documentation

### Code Documentation

**Modules:**

```rust
//! # Module Name
//!
//! Brief description of what this module does.
//!
//! ## Examples
//!
//! ```
//! use turbomcp::prelude::*;
//!
//! let server = McpServer::new();
//! ```

mod my_module;
```

**Functions:**

```rust
/// Calculate the sum of two numbers.
///
/// # Arguments
///
/// * `a` - First number
/// * `b` - Second number
///
/// # Returns
///
/// Sum of `a` and `b`
///
/// # Examples
///
/// ```
/// use turbomcp::math::add;
///
/// assert_eq!(add(2, 3), 5);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

**Structs:**

```rust
/// MCP server instance.
///
/// The server manages handlers, middleware, and transport protocols.
///
/// # Examples
///
/// ```
/// use turbomcp::prelude::*;
///
/// #[tokio::main]
/// async fn main() -> Result<()> {
///     McpServer::new()
///         .stdio()
///         .run()
///         .await
/// }
/// ```
pub struct McpServer<S> {
    // fields...
}
```

### Markdown Documentation

**Structure:**

```markdown
# Page Title

Brief one-line description.

## Overview

Detailed introduction explaining what, why, and when to use this feature.

## Quick Start

Minimal example to get started.

## Detailed Usage

In-depth explanations with examples.

## Advanced Topics

Complex scenarios and edge cases.

## Related Documentation

- [Link to related docs](./other-page.md)
```

**Code Examples:**

```markdown
# Feature Name

## Basic Example

\`\`\`rust
use turbomcp::prelude::*;

#[tool]
pub async fn my_tool(input: String) -> McpResult<String> {
    Ok(input.to_uppercase())
}
\`\`\`

## With Dependencies

\`\`\`rust
#[tool]
pub async fn db_query(
    query: String,
    db: Database,
    logger: Logger,
) -> McpResult<Vec<Row>> {
    logger.info("Executing query").await?;
    db.query(&query).await
}
\`\`\`
```

## Pull Request Process

### Before Submitting

**Checklist:**

- [ ] Code builds without warnings
- [ ] All tests pass (`just test`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] No Clippy warnings (`cargo clippy`)
- [ ] Documentation updated
- [ ] Examples added (if applicable)
- [ ] CHANGELOG.md updated
- [ ] Commit messages follow guidelines

### Creating a Pull Request

1. **Push your branch:**

```bash
git push origin feature/my-feature
```

2. **Create PR on GitHub:**
   - Click "New Pull Request"
   - Select your branch
   - Fill in the template

3. **PR Template:**

```markdown
## Description

Brief description of changes.

## Type of Change

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to not work as expected)
- [ ] Documentation update

## Changes Made

- Added WebSocket transport support
- Updated documentation with examples
- Added comprehensive tests

## Testing

- [ ] Unit tests added
- [ ] Integration tests added
- [ ] Examples updated
- [ ] Manual testing completed

## Checklist

- [ ] Code follows style guidelines
- [ ] Self-review completed
- [ ] Comments added for complex logic
- [ ] Documentation updated
- [ ] No new warnings
- [ ] Tests pass locally

## Related Issues

Closes #123
```

### Review Process

1. **Automated Checks:**
   - GitHub Actions runs tests
   - Clippy checks for warnings
   - Format check
   - Documentation build

2. **Code Review:**
   - Maintainer reviews code
   - Requests changes if needed
   - Approves when ready

3. **Addressing Feedback:**

```bash
# Make requested changes
git add .
git commit -m "fix: address review feedback"
git push origin feature/my-feature
```

4. **Merging:**
   - Maintainer squashes and merges
   - PR closes automatically
   - Branch deleted

## Release Process

### Versioning

TurboMCP follows [Semantic Versioning](https://semver.org/):

- **Major**: Breaking changes (2.0.0 -> 3.0.0)
- **Minor**: New features, backward compatible (2.1.0 -> 2.2.0)
- **Patch**: Bug fixes (2.1.1 -> 2.1.2)

### Preparing a Release

**1. Update Version Numbers:**

```bash
# Update Cargo.toml in all crates
sed -i 's/version = "2.1.1"/version = "2.2.0"/' crates/*/Cargo.toml
```

**2. Update CHANGELOG.md:**

```markdown
## [2.2.0] - 2025-12-10

### Added
- WebSocket transport support
- DPoP authentication

### Fixed
- Memory leak in connection pool

### Changed
- Improved error messages

### Breaking Changes
- None
```

**3. Run Full Test Suite:**

```bash
just test
cargo test --workspace --all-features
cargo clippy --workspace --all-features -- -D warnings
cargo doc --workspace --no-deps
```

**4. Create Release Commit:**

```bash
git add .
git commit -m "chore: release v2.2.0"
git tag -a v2.2.0 -m "Release v2.2.0"
```

**5. Push to GitHub:**

```bash
git push origin main
git push origin v2.2.0
```

**6. Publish to crates.io:**

```bash
# Publish in dependency order
cargo publish -p turbomcp-protocol
cargo publish -p turbomcp-transport
cargo publish -p turbomcp-macros
cargo publish -p turbomcp-server
cargo publish -p turbomcp-client
cargo publish -p turbomcp
```

## Troubleshooting

### Common Issues

**Issue: Tests fail with feature errors**

```bash
# Solution: Enable required features
cargo test -p turbomcp-transport --features stdio,tcp
```

**Issue: Clippy warnings**

```bash
# Solution: Fix warnings or allow specific lints
#![allow(clippy::module_name_repetitions)]
```

**Issue: Documentation doesn't build**

```bash
# Solution: Check for broken links and invalid examples
cargo doc --workspace --no-deps 2>&1 | grep -i "warning"
```

**Issue: Macro expansion errors**

```bash
# Solution: Use cargo expand to debug
cargo expand -p turbomcp-macros tool
```

## Getting Help

**Resources:**
- [Documentation](https://docs.turbomcp.dev)
- [GitHub Discussions](https://github.com/turbomcp/turbomcp/discussions)
- [GitHub Issues](https://github.com/turbomcp/turbomcp/issues)
- [Discord](https://discord.gg/turbomcp) (if available)

**Before Asking:**
1. Search existing issues and discussions
2. Check documentation
3. Review examples
4. Try debugging yourself

**When Asking:**
- Provide minimal reproducible example
- Include environment details
- Share error messages
- Explain what you've tried

## Related Documentation

- [Code of Conduct](./code-of-conduct.md)
- [Documentation Guide](./documentation.md)
- [Architecture Overview](../architecture/system-design.md)
- [Testing Guide](../guide/testing.md)
