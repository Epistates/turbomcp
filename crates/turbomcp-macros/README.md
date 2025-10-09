# TurboMCP Macros

[![Crates.io](https://img.shields.io/crates/v/turbomcp-macros.svg)](https://crates.io/crates/turbomcp-macros)
[![Documentation](https://docs.rs/turbomcp-macros/badge.svg)](https://docs.rs/turbomcp-macros)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Zero-boilerplate procedural macros for ergonomic MCP server development with automatic schema generation and compile-time validation.**

## Overview

`turbomcp-macros` provides the procedural macros that make TurboMCP development effortless. These macros eliminate boilerplate code while providing compile-time validation, automatic schema generation, and type-safe parameter handling.

## Key Features

### 🎯 **Zero Boilerplate Design**
- **Automatic registration** - Tools and resources registered automatically
- **Schema generation** - JSON schemas generated from Rust types  
- **Parameter extraction** - Type-safe parameter conversion and validation
- **Error handling** - Automatic error type conversion and propagation
- **Ergonomic Macros** - `elicit!` for user input, seamless Context API delegation

### ✅ **Compile-Time Validation**
- **Type checking** - Parameter types validated at compile time
- **Schema validation** - Generated schemas validated for correctness
- **IDE support** - Full IntelliSense and error reporting
- **Macro hygiene** - Proper variable scoping and name collision prevention

### 📋 **Automatic Schema Generation**
- **JSON Schema** - Complete JSON Schema generation from Rust types
- **Parameter documentation** - Extract documentation from function signatures
- **Type introspection** - Deep analysis of parameter and return types
- **Schema caching** - Efficient schema generation and reuse

### 🔍 **Context Injection**
- **Flexible positioning** - Context parameter can appear anywhere in function signature
- **Send-safe** - Proper Send/Sync bounds for async context
- **Type safety** - Compile-time validation of context usage
- **Optional context** - Functions can opt-in or out of context injection

## Architecture

```
┌─────────────────────────────────────────────┐
│              TurboMCP Macros                │
├─────────────────────────────────────────────┤
│ Procedural Macro Processing                │
│ ├── #[server] trait implementation         │
│ ├── #[tool] function registration          │
│ ├── #[resource] handler registration       │
│ └── #[prompt] template registration        │
├─────────────────────────────────────────────┤
│ Schema Generation Engine                   │
│ ├── Type introspection                     │
│ ├── JSON Schema creation                   │
│ ├── Parameter validation                   │
│ └── Documentation extraction               │
├─────────────────────────────────────────────┤
│ Code Generation                            │
│ ├── Handler registration code              │
│ ├── Parameter extraction logic             │
│ ├── Error conversion helpers               │
│ └── Schema metadata functions              │
├─────────────────────────────────────────────┤
│ Compile-Time Validation                    │
│ ├── Type compatibility checking            │
│ ├── Parameter validation                   │
│ ├── Context injection validation           │
│ └── Schema correctness verification        │
└─────────────────────────────────────────────┘
```

## Core Macros

### MCP 2025-06-18 Enhanced Macros

New macros for the latest MCP protocol features:

#### `elicit!` - Elegant User Input (NEW)
```rust
use turbomcp::prelude::*;
use turbomcp_protocol::elicitation::ElicitationSchema;

#[tool("Configure user preferences")]
async fn configure_preferences(&self, ctx: Context) -> McpResult<String> {
    let schema = ElicitationSchema::new()
        .add_string_property("theme", Some("Color theme preference"))
        .add_boolean_property("notifications", Some("Enable notifications"));
    
    // Simple macro handles all protocol complexity
    let result = elicit!(ctx, "Please configure your preferences", schema).await?;
    
    if let Some(data) = result.content {
        let theme = data.get("theme").and_then(|v| v.as_str()).unwrap_or("default");
        Ok(format!("Configured with {} theme", theme))
    } else {
        Err(McpError::Context("Configuration cancelled".to_string()))
    }
}
```

#### `#[elicitation]` - Attribute-based Elicitation
```rust
#[elicitation("Collect user preferences")]
async fn get_preferences(&self, schema: serde_json::Value) -> McpResult<serde_json::Value> {
    // Server requests structured input from client
    Ok(serde_json::json!({"theme": "dark", "language": "en"}))
}
```

#### `#[completion]` - Intelligent Autocompletion
```rust
#[completion("Complete file paths")]
async fn complete_path(&self, partial: String) -> McpResult<Vec<String>> {
    // Provide completion suggestions
    Ok(vec!["config.json".to_string(), "data.txt".to_string()])
}
```

#### `#[template]` - Resource Templates
```rust
#[template("users/{user_id}/profile")]
async fn get_user_profile(&self, user_id: String) -> McpResult<String> {
    // Dynamic resource with RFC 6570 URI template
    Ok(format!("Profile for user: {}", user_id))
}
```

#### `#[ping]` - Health Monitoring
```rust
#[ping("Health check")]
async fn health_check(&self) -> McpResult<String> {
    // Bidirectional health monitoring
    Ok("Server is healthy".to_string())
}
```

### `#[server]` - Server Implementation

Automatically implements the MCP server trait for a struct:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server]
impl Calculator {
    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }
    
    #[tool("Get server status")]
    async fn status(&self, ctx: Context) -> McpResult<String> {
        ctx.info("Status requested").await?;
        Ok("Server running".to_string())
    }
}

// Generated code includes:
// - Automatic trait implementation
// - Handler registration
// - Schema generation
// - Transport integration
```

**Generated Capabilities:**
- Automatic `MCP` trait implementation
- Handler registry setup with all annotated functions
- Schema generation for all tools/resources/prompts
- Transport method implementations (`run_stdio`, `run_http`, etc.)

### `#[tool]` - Tool Registration

Transforms functions into MCP tools with automatic parameter handling:

```rust
#[tool("Calculate mathematical expressions")]
async fn calculate(
    #[description("Mathematical expression to evaluate")]
    expression: String,
    #[description("Precision for floating point results")]
    precision: Option<u32>,
    ctx: Context
) -> McpResult<serde_json::Value> {
    ctx.info(&format!("Calculating: {}", expression)).await?;
    
    let precision = precision.unwrap_or(2);
    // ... calculation logic
    
    Ok(serde_json::json!({
        "result": result,
        "expression": expression,
        "precision": precision
    }))
}
```

**Generated Features:**
- JSON Schema with parameter descriptions
- Type-safe parameter extraction from JSON
- Optional parameter handling
- Context injection (can appear anywhere in signature)
- Automatic error conversion
- Tool metadata functions

### `#[resource]` - Resource Registration

Creates URI template-based resource handlers:

```rust
#[resource("file://{path}")]
async fn read_file(
    #[description("File path to read")]
    path: String,
    #[description("Maximum file size in bytes")]
    max_size: Option<usize>,
    ctx: Context
) -> McpResult<String> {
    let max_size = max_size.unwrap_or(1024 * 1024); // 1MB default
    
    if std::fs::metadata(&path)?.len() > max_size as u64 {
        return Err(McpError::InvalidInput("File too large".to_string()));
    }
    
    ctx.info(&format!("Reading file: {}", path)).await?;
    
    tokio::fs::read_to_string(&path).await
        .map_err(|e| McpError::Resource(e.to_string()))
}
```

**URI Template Features:**
- Automatic URI pattern matching
- Path parameter extraction
- Query parameter support
- URI validation
- Resource metadata generation

### `#[prompt]` - Prompt Template Registration

Creates prompt templates with parameter substitution:

```rust
#[prompt("code_review")]
async fn code_review_prompt(
    #[description("Programming language")]
    language: String,
    #[description("Code to review")]
    code: String,
    #[description("Focus areas for review")]
    focus: Option<Vec<String>>,
    ctx: Context
) -> McpResult<String> {
    let focus_areas = focus.unwrap_or_else(|| vec![
        "security".to_string(),
        "performance".to_string(),
        "maintainability".to_string()
    ]);
    
    ctx.info(&format!("Generating {} code review prompt", language)).await?;
    
    Ok(format!(
        "Please review the following {} code focusing on {}:\n\n```{}\n{}\n```",
        language,
        focus_areas.join(", "),
        language,
        code
    ))
}
```

## Advanced Features

### Context Injection

The `Context` parameter can appear anywhere in the function signature:

```rust
// Context first
#[tool("Process data")]
async fn process(ctx: Context, data: String) -> McpResult<String> {
    ctx.info("Processing started").await?;
    Ok(format!("Processed: {}", data))
}

// Context in middle
#[tool("Transform data")]
async fn transform(input: String, ctx: Context, format: String) -> McpResult<String> {
    ctx.info(&format!("Transforming to {}", format)).await?;
    // transformation logic
    Ok(transformed)
}

// Context last
#[tool("Validate input")]
async fn validate(data: String, strict: bool, ctx: Context) -> McpResult<bool> {
    ctx.info("Validating input").await?;
    // validation logic
    Ok(is_valid)
}

// No context
#[tool("Simple calculation")]
async fn add(a: f64, b: f64) -> McpResult<f64> {
    Ok(a + b)
}
```

### Parameter Descriptions

Use the `#[description]` attribute for rich parameter documentation:

```rust
#[tool("Search documents")]
async fn search(
    #[description("Search query string")]
    query: String,
    
    #[description("Maximum number of results to return")]
    #[default(10)]
    limit: Option<u32>,
    
    #[description("Include archived documents in search")]
    #[default(false)]
    include_archived: Option<bool>,
    
    #[description("Sort results by relevance or date")]
    #[allowed("relevance", "date")]
    sort_by: Option<String>,
) -> McpResult<SearchResults> {
    // Implementation
}
```

**Generated Schema:**
```json
{
  "type": "object",
  "properties": {
    "query": {
      "type": "string",
      "description": "Search query string"
    },
    "limit": {
      "type": "integer",
      "description": "Maximum number of results to return",
      "default": 10
    },
    "include_archived": {
      "type": "boolean", 
      "description": "Include archived documents in search",
      "default": false
    },
    "sort_by": {
      "type": "string",
      "description": "Sort results by relevance or date",
      "enum": ["relevance", "date"]
    }
  },
  "required": ["query"]
}
```

### Custom Types and Schema Generation

The macros automatically generate schemas for custom types:

```rust
#[derive(Serialize, Deserialize)]
struct User {
    id: u64,
    name: String,
    email: Option<String>,
    active: bool,
}

#[derive(Serialize, Deserialize)]
struct CreateUserRequest {
    name: String,
    email: String,
    role: UserRole,
}

#[derive(Serialize, Deserialize)]
enum UserRole {
    Admin,
    User,
    Guest,
}

#[tool("Create a new user")]
async fn create_user(request: CreateUserRequest) -> McpResult<User> {
    // Schema automatically generated for both CreateUserRequest and User
    // Enums become string unions in JSON Schema
    // Optional fields marked appropriately
    Ok(User {
        id: generate_id(),
        name: request.name,
        email: Some(request.email),
        active: true,
    })
}
```

### Helper Macros

#### `elicit!` - Ergonomic Elicitation Macro (NEW)

The `elicit!` macro provides elegant server-initiated user input:

```rust
use turbomcp::prelude::*;
use turbomcp_protocol::elicitation::ElicitationSchema;

// Build schema with type safety
let schema = ElicitationSchema::new()
    .add_string_property("name", Some("Your name"))
    .add_boolean_property("subscribe", Some("Subscribe to newsletter"));

// Clean, simple macro call
let result = elicit!(ctx, "Please provide your information", schema).await?;
```

**Key Benefits:**
- **Zero Protocol Complexity** - Handles all MCP elicitation protocol details
- **Type Safe** - Compile-time validation of context and schema
- **Ergonomic** - Simple 3-parameter syntax: `(context, message, schema)`
- **Error Handling** - Automatic error conversion and propagation

#### Error Handling Macros

Ergonomic error creation macros:

```rust
use turbomcp::prelude::*;

#[tool("Divide numbers")]
async fn divide(a: f64, b: f64) -> McpResult<f64> {
    if b == 0.0 {
        return Err(mcp_error!("Division by zero: {} / {}", a, b));
    }
    
    Ok(a / b)
}

#[tool("Process file")]
async fn process_file(path: String) -> McpResult<String> {
    let content = tokio::fs::read_to_string(&path).await
        .map_err(|e| mcp_error!("Failed to read file {}: {}", path, e))?;
    
    // Processing logic
    Ok(processed_content)
}
```

## Metadata Access

The macros generate metadata access functions:

```rust
#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool("Example tool")]
    async fn example(&self, input: String) -> McpResult<String> {
        Ok(input)
    }
}

// Generated metadata functions
let (name, description, schema) = MyServer::example_tool_metadata();
assert_eq!(name, "example");
assert_eq!(description, "Example tool");
// schema contains the complete JSON Schema

// Test the generated function directly
let result = MyServer.test_tool_call("example", serde_json::json!({
    "input": "test"
})).await?;
```

## Macro Attributes

### Tool Attributes

| Attribute | Description | Example |
|-----------|-------------|---------|
| `#[description]` | Parameter description | `#[description("User ID")]` |
| `#[default]` | Default value for optional parameters | `#[default(10)]` |
| `#[allowed]` | Allowed string values (enum) | `#[allowed("read", "write")]` |
| `#[range]` | Numeric range validation | `#[range(0, 100)]` |
| `#[pattern]` | Regex pattern validation | `#[pattern(r"^\d{3}-\d{2}-\d{4}$")]` |

### Resource Attributes

| Attribute | Description | Example |
|-----------|-------------|---------|
| URI template | Resource URI pattern | `#[resource("file://{path}")]` |
| `#[mime_type]` | Content MIME type | `#[mime_type("text/plain")]` |
| `#[binary]` | Binary resource flag | `#[binary(true)]` |

## Generated Code Examples

### Tool Registration

Input:
```rust
#[tool("Add numbers")]
async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
    Ok(a + b)
}
```

Generated (simplified):
```rust
// Metadata function
pub fn add_tool_metadata() -> (&'static str, &'static str, serde_json::Value) {
    ("add", "Add numbers", serde_json::json!({
        "type": "object",
        "properties": {
            "a": {"type": "number"},
            "b": {"type": "number"}
        },
        "required": ["a", "b"]
    }))
}

// Handler registration
async fn register_handlers(&self, registry: &mut HandlerRegistry) -> McpResult<()> {
    registry.register_tool("add", |params| {
        let a: f64 = extract_param(&params, "a")?;
        let b: f64 = extract_param(&params, "b")?;
        self.add(a, b).await
    }).await?;
    
    Ok(())
}

// Direct test function
pub async fn test_tool_call(&self, name: &str, params: serde_json::Value) -> McpResult<serde_json::Value> {
    match name {
        "add" => {
            let a: f64 = extract_param(&params, "a")?;
            let b: f64 = extract_param(&params, "b")?;
            let result = self.add(a, b).await?;
            Ok(serde_json::to_value(result)?)
        },
        _ => Err(McpError::InvalidInput(format!("Unknown tool: {}", name)))
    }
}
```

## IDE Integration

The macros provide excellent IDE support:

- **IntelliSense** - Full auto-completion for generated functions
- **Error highlighting** - Compile-time error detection
- **Type information** - Hover information for generated code
- **Go to definition** - Navigate to macro-generated implementations
- **Refactoring support** - Safe renaming and extraction

## Testing Support

The macros generate testing utilities:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_calculator_tools() {
        let calc = Calculator;
        
        // Test add tool directly
        let result = calc.test_tool_call("add", serde_json::json!({
            "a": 5.0,
            "b": 3.0
        })).await.unwrap();
        
        assert_eq!(result, serde_json::json!(8.0));
        
        // Test schema generation
        let (name, desc, schema) = Calculator::add_tool_metadata();
        assert_eq!(name, "add");
        assert_eq!(desc, "Add two numbers");
        assert!(schema["properties"]["a"]["type"] == "number");
    }
}
```

## Performance

The macros generate efficient code:

- **Zero runtime overhead** - All processing happens at compile time
- **Optimized registration** - Efficient handler lookup and dispatch
- **Schema caching** - Schemas generated once and reused
- **Minimal allocations** - Smart parameter extraction with minimal copying

## Development

### Building

```bash
# Build macros crate
cargo build

# Test macro expansion
cargo expand --package turbomcp-macros

# Run macro tests
cargo test
```

### Debugging Macros

```bash
# See expanded macro code
cargo expand --bin my_server

# Debug specific macro
RUST_LOG=debug cargo build
```

## Related Crates

- **[turbomcp](../turbomcp/)** - Main framework (uses these macros)
- **[turbomcp-protocol](../turbomcp-protocol/)** - Protocol types and core utilities
- **[turbomcp-server](../turbomcp-server/)** - Server framework

## External Resources

- **[The Rust Reference - Procedural Macros](https://doc.rust-lang.org/reference/procedural-macros.html)** - Rust macro documentation
- **[JSON Schema Specification](https://json-schema.org/)** - Schema format specification
- **[serde Documentation](https://serde.rs/)** - Serialization framework used by macros

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) high-performance Rust SDK for the Model Context Protocol.*