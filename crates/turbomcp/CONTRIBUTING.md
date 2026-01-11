# Contributing to TurboMCP

Thank you for your interest in contributing to TurboMCP! We welcome contributions from the community and are excited to see what you'll build.

## Table of Contents

1. [Code of Conduct](#code-of-conduct)
2. [Getting Started](#getting-started)
3. [Development Workflow](#development-workflow)
4. [Coding Standards](#coding-standards)
5. [Testing Guidelines](#testing-guidelines)
6. [Documentation](#documentation)
7. [Pull Request Process](#pull-request-process)
8. [Issue Reporting](#issue-reporting)
9. [Community](#community)

## Code of Conduct

This project adheres to the Contributor Covenant Code of Conduct. By participating, you are expected to uphold this code. Please report unacceptable behavior to [turbomcp@anthropic.com](mailto:turbomcp@anthropic.com).

### Our Pledge

We pledge to make participation in our project a harassment-free experience for everyone, regardless of age, body size, disability, ethnicity, gender identity and expression, level of experience, nationality, personal appearance, race, religion, or sexual identity and orientation.

## Getting Started

### Prerequisites

- **Rust**: 1.89+ with 2021 edition
- **Git**: For version control
- **MCP Inspector**: For testing (`npx @modelcontextprotocol/inspector`)

### Setting Up Development Environment

1. **Fork the repository** on GitHub
2. **Clone your fork**:
   ```bash
   git clone https://github.com/your-username/turbomcp.git
   cd turbomcp
   ```

3. **Add upstream remote**:
   ```bash
   git remote add upstream https://github.com/anthropics/nextgen-mcp.git
   ```

4. **Install development dependencies**:
   ```bash
   cargo build
   cargo test
   ```

5. **Verify your setup**:
   ```bash
   cargo run --example calculator
   ```

### Project Structure

```
turbomcp/
├── src/           # Core library code
├── examples/      # Example implementations
├── tests/         # Integration tests
├── benches/       # Performance benchmarks
├── docs/          # Documentation
└── scripts/       # Build and utility scripts
```

## Development Workflow

### Branching Strategy

- **main**: Production-ready code
- **develop**: Integration branch for features
- **feature/**: Feature development (`feature/my-new-feature`)
- **bugfix/**: Bug fixes (`bugfix/issue-123`)
- **hotfix/**: Critical fixes (`hotfix/security-patch`)

### Making Changes

1. **Create a feature branch**:
   ```bash
   git checkout -b feature/my-awesome-feature
   ```

2. **Make your changes** following our coding standards

3. **Write tests** for your changes

4. **Run the test suite**:
   ```bash
   cargo test
   cargo clippy
   cargo fmt --check
   ```

5. **Update documentation** if needed

6. **Commit your changes**:
   ```bash
   git add .
   git commit -m "feat: add awesome new feature
   
   - Implements XYZ functionality
   - Adds comprehensive tests
   - Updates documentation
   
   Closes #123"
   ```

### Commit Message Format

We use [Conventional Commits](https://www.conventionalcommits.org/) for consistent commit messages:

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types**:
- `feat`: New features
- `fix`: Bug fixes
- `docs`: Documentation changes
- `style`: Code style changes (formatting, etc.)
- `refactor`: Code refactoring
- `test`: Adding or modifying tests
- `chore`: Build process or auxiliary tool changes

**Examples**:
```bash
feat(server): add OAuth 2.0 authentication support

Implements complete OAuth 2.0 flow with PKCE support:
- Authorization code flow with state parameter
- Token refresh and validation
- Comprehensive error handling

Closes #456

fix(uri): prevent directory traversal in template matching

Security fix for URI template parameter extraction that could
allow directory traversal attacks.

BREAKING CHANGE: URI parameters now validate against allowed patterns
```

## Coding Standards

### Rust Style Guide

We follow the official [Rust Style Guide](https://rust-lang.github.io/api-guidelines/) with these additions:

#### Code Formatting
- Use `cargo fmt` for automatic formatting
- Line length: 100 characters maximum
- Use 4 spaces for indentation (no tabs)

#### Naming Conventions
```rust
// Types: PascalCase
struct ServerConfiguration;
enum ErrorType;

// Functions and variables: snake_case  
fn handle_request() -> Result<(), Error>;
let user_input = "example";

// Constants: SCREAMING_SNAKE_CASE
const MAX_CONNECTIONS: usize = 100;

// Modules: snake_case
mod request_handler;
```

#### Error Handling
```rust
// Prefer ? operator for error propagation
fn read_file(path: &str) -> McpResult<String> {
    let contents = fs::read_to_string(path)
        .map_err(|e| McpError::resource(format!("Failed to read {}: {}", path, e)))?;
    Ok(contents)
}

// Use descriptive error messages
return Err(McpError::tool("Invalid input: expected positive number".to_string()));
```

#### Documentation
```rust
/// Brief description of the function
///
/// Longer description explaining the purpose, behavior, and any important
/// details about the function.
///
/// # Arguments
///
/// * `param1` - Description of parameter
/// * `param2` - Description of parameter
///
/// # Returns
///
/// Description of return value
///
/// # Errors
///
/// Description of when this function returns an error
///
/// # Examples
///
/// ```rust
/// let result = my_function("example", 42)?;
/// assert_eq!(result, expected_value);
/// ```
pub async fn my_function(param1: &str, param2: i32) -> McpResult<String> {
    // Implementation
}
```

### Performance Guidelines

- **Prefer zero-copy operations** where possible
- **Use `Arc` and `RwLock`** for shared state
- **Minimize allocations** in hot paths
- **Use streaming** for large data operations
- **Profile before optimizing** - use `cargo bench`

## Testing Guidelines

### Test Organization

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    // Unit tests for individual functions
    #[test]
    fn test_basic_functionality() {
        // Arrange
        let input = create_test_input();
        
        // Act
        let result = function_under_test(input);
        
        // Assert
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), expected_output);
    }
    
    // Async tests
    #[tokio::test]
    async fn test_async_operation() {
        let server = TestServer::new();
        let result = server.async_operation().await;
        assert!(result.is_ok());
    }
}
```

### Test Categories

1. **Unit Tests**: Test individual functions and methods
2. **Integration Tests**: Test component interactions
3. **Property Tests**: Test with generated inputs (using `proptest`)
4. **Performance Tests**: Benchmark critical paths

### Test Coverage Requirements

- **New features**: Must have >90% test coverage
- **Bug fixes**: Must include regression tests
- **Performance improvements**: Must include benchmarks
- **Breaking changes**: Must update all affected tests

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test module
cargo test server_tests

# Run tests with coverage
cargo tarpaulin --out html

# Run property tests
cargo test proptest

# Run benchmarks
cargo bench
```

## Documentation

### Types of Documentation

1. **API Documentation**: Inline `///` comments
2. **User Guides**: Markdown files in `/docs`
3. **Examples**: Working code in `/examples`
4. **Tutorials**: Step-by-step guides

### Documentation Standards

- **Every public API** must have documentation
- **Include examples** for complex functionality
- **Keep examples up-to-date** with code changes
- **Use clear, concise language**
- **Explain the "why", not just the "what"**

### Building Documentation

```bash
# Generate API docs
cargo doc --no-deps --open

# Check documentation links
cargo doc --no-deps --document-private-items

# Build all documentation
scripts/build-docs.sh
```

## Pull Request Process

### Before Submitting

- [ ] All tests pass (`cargo test`)
- [ ] Code follows style guidelines (`cargo clippy`)
- [ ] Code is formatted (`cargo fmt`)
- [ ] Documentation is updated
- [ ] Examples work with your changes
- [ ] No new compiler warnings

### PR Description Template

```markdown
## Summary
Brief description of what this PR does and why.

## Changes
- List of specific changes made
- Include any breaking changes
- Note any new dependencies

## Testing
- [ ] Unit tests added/updated
- [ ] Integration tests pass
- [ ] Manual testing performed
- [ ] Performance impact assessed

## Documentation
- [ ] API documentation updated
- [ ] User guides updated
- [ ] Examples updated
- [ ] Changelog entry added

## Checklist
- [ ] PR title follows conventional commits format
- [ ] Code follows project style guidelines
- [ ] Tests pass locally
- [ ] Documentation builds successfully

## Related Issues
Closes #123
Relates to #456
```

### Review Process

1. **Automated checks** must pass (CI/CD)
2. **At least one approval** from a maintainer
3. **All feedback addressed** or discussed
4. **Squash and merge** for clean history

## Issue Reporting

### Bug Reports

Use the bug report template and include:

- **TurboMCP version**
- **Rust version**
- **Operating system**
- **Minimal reproduction case**
- **Expected vs actual behavior**
- **Error messages/stack traces**

### Feature Requests

Use the feature request template and include:

- **Problem description**: What problem does this solve?
- **Proposed solution**: How should it work?
- **Alternatives considered**: What other approaches were considered?
- **Additional context**: Any other relevant information

### Security Issues

**Do not open public issues for security vulnerabilities.**

Email security issues to: [security@anthropic.com](mailto:security@anthropic.com)

Include:
- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

## Community

### Communication Channels

- **GitHub Issues**: Bug reports and feature requests
- **GitHub Discussions**: Questions and community discussion
- **Discord**: Real-time chat and support
- **Email**: [turbomcp@anthropic.com](mailto:turbomcp@anthropic.com)

### Getting Help

1. **Check existing documentation** and examples
2. **Search GitHub issues** for similar problems
3. **Ask in GitHub Discussions** for usage questions
4. **Join Discord** for real-time help

### Recognition

Contributors are recognized in:
- **CONTRIBUTORS.md**: List of all contributors
- **Release notes**: Major contributions highlighted
- **Documentation**: Example authors credited

## Release Process

### Versioning

We follow [Semantic Versioning (SemVer)](https://semver.org/):

- **MAJOR**: Breaking changes
- **MINOR**: New features (backward compatible)
- **PATCH**: Bug fixes (backward compatible)

### Release Checklist

- [ ] All tests pass on CI
- [ ] Documentation updated
- [ ] Changelog updated
- [ ] Version numbers bumped
- [ ] Release notes prepared
- [ ] Performance benchmarks run
- [ ] Security review completed (for major releases)

## Development Tools

### Recommended Editor Setup

**VS Code Extensions**:
- `rust-analyzer`: Rust language support
- `CodeLLDB`: Debugging
- `Better TOML`: TOML syntax
- `markdownlint`: Markdown linting

**Configuration** (`.vscode/settings.json`):
```json
{
    "rust-analyzer.check.command": "clippy",
    "rust-analyzer.rustfmt.extraArgs": ["+nightly"],
    "editor.formatOnSave": true,
    "editor.rulers": [100]
}
```

### Useful Commands

```bash
# Development workflow
cargo watch -x test                    # Auto-run tests on change
cargo expand                           # Expand macros
cargo tree                             # Dependency tree
cargo audit                            # Security audit
cargo outdated                         # Check for outdated deps

# Performance
cargo flamegraph                       # CPU profiling
cargo instruments                      # macOS profiling
cargo asm                              # Assembly output

# Documentation
cargo doc --open                       # Generate and open docs
cargo deadlinks                        # Check for dead links
mdbook serve docs                      # Serve documentation locally
```

## FAQ

### Q: How do I add a new feature to the macro system?
A: See the [Macro Development Guide](docs/macro-development.md) for detailed instructions on extending the `#[tool]`, `#[resource]`, and `#[prompt]` macros.

### Q: What's the difference between a tool and a resource?
A: Tools perform actions and return results, while resources provide access to external data. See the [Architecture Guide](docs/architecture.md) for more details.

### Q: How do I handle large file operations efficiently?
A: Use streaming patterns and progress reporting. See the file manager example for best practices.

### Q: Can I contribute examples for other languages/frameworks?
A: Yes! We welcome examples showing TurboMCP integration with other Rust frameworks like Axum, Warp, or Rocket.

---

Thank you for contributing to TurboMCP! Your efforts help make MCP development more ergonomic and powerful for everyone. 🚀