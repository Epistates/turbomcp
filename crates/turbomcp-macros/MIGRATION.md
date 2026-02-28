> **Note:** This is the v1.x to v2.0.4 migration guide. For v2.x to v3.x migration, see the [top-level MIGRATION.md](../../MIGRATION.md).

# TurboMCP Macros 2.0.4 Migration Guide

This guide helps you migrate from turbomcp-macros 1.x to 2.0.4.

## 📋 Table of Contents

- [Overview](#overview)
- [Breaking Changes](#breaking-changes)
- [New Features](#new-features)
- [Migration Steps](#migration-steps)
- [Troubleshooting](#troubleshooting)

## 🚀 Overview

**TurboMCP Macros 2.0.4** maintains 100% backward compatibility while adding powerful new features.

### Key Changes
- **Zero Breaking Changes**: All existing code works without modification
- **New Macros**: `#[elicitation]`, `#[completion]`, `#[template]`, `#[ping]`
- **Enhanced `elicit!` Macro**: Simplified server-initiated user input
- **Improved Context Injection**: More flexible parameter positioning
- **Better Error Messages**: Enhanced compile-time diagnostics
- **Stdio Safety**: Compile-time validation prevents unsafe stdout writes in stdio transport servers

### Migration Timeline
- **Zero Impact**: Existing code requires no changes
- **Opt-In Features**: New macros available when needed
- **Full Compatibility**: v1.x code works identically in v2.0.0

## 💥 Breaking Changes

### None! 🎉

**TurboMCP Macros 2.0.4 has ZERO breaking changes.**

All existing macro usage from v1.x continues to work without modification:
- `#[server]` - Works identically
- `#[tool]` - Works identically
- `#[resource]` - Works identically
- `#[prompt]` - Works identically
- Helper macros (`mcp_error!`, etc.) - Work identically

The 2.0 release is **purely additive** - we only added new features without changing existing behavior.

## ✨ New Features

### 1. New MCP Protocol Macros

Four new attribute macros for complete MCP 2025-06-18 protocol coverage:

#### `#[elicitation]` - Structured Input Collection

```rust
use turbomcp::prelude::*;

#[server]
impl MyServer {
    #[elicitation("Collect user preferences")]
    async fn get_preferences(
        &self,
        schema: serde_json::Value
    ) -> McpResult<serde_json::Value> {
        // Server requests structured input from client
        Ok(serde_json::json!({
            "theme": "dark",
            "language": "en"
        }))
    }
}
```

**Use Case:** Interactive configuration, form data collection, step-by-step wizards

#### `#[completion]` - Intelligent Autocompletion

```rust
#[server]
impl MyServer {
    #[completion("Complete file paths")]
    async fn complete_path(
        &self,
        partial: String
    ) -> McpResult<Vec<String>> {
        // Provide completion suggestions
        Ok(vec![
            "config.json".to_string(),
            "data.txt".to_string()
        ])
    }
}
```

**Use Case:** IDE-like autocompletion, parameter suggestions, command completion

#### `#[template]` - Resource Templates

```rust
#[server]
impl MyServer {
    #[template("users/{user_id}/profile")]
    async fn get_user_profile(
        &self,
        user_id: String
    ) -> McpResult<String> {
        // Dynamic resource with RFC 6570 URI template
        Ok(format!("Profile for user: {}", user_id))
    }
}
```

**Use Case:** Dynamic resources, parameterized URIs, RESTful patterns

#### `#[ping]` - Health Monitoring

```rust
#[server]
impl MyServer {
    #[ping("Health check")]
    async fn health_check(&self) -> McpResult<String> {
        // Bidirectional health monitoring
        Ok("Server is healthy".to_string())
    }
}
```

**Use Case:** Connection monitoring, server health checks, keepalive

### 2. Enhanced `elicit!` Macro

**NEW:** Simplified syntax for server-initiated user input:

```rust
use turbomcp::prelude::*;
use turbomcp_protocol::elicitation::ElicitationSchema;

#[tool("Configure preferences")]
async fn configure(&self, ctx: Context) -> McpResult<String> {
    let schema = ElicitationSchema::new()
        .add_string_property("theme", Some("Color theme"))
        .add_boolean_property("notifications", Some("Enable notifications"));

    // Clean, simple elicitation
    let result = elicit!(ctx, "Configure your preferences", schema).await?;

    if let Some(data) = result.content {
        let theme = data.get("theme")
            .and_then(|v| v.as_str())
            .unwrap_or("default");
        Ok(format!("Configured with {} theme", theme))
    } else {
        Err(McpError::Context("Configuration cancelled".to_string()))
    }
}
```

**Benefits:**
- Zero protocol complexity - handles all MCP details automatically
- Type safe - compile-time validation
- Ergonomic - simple 3-parameter syntax
- Error handling - automatic conversion

**Before (v1.x):**
```rust
// Complex manual protocol handling
let request = ElicitRequest {
    message: "Configure preferences".to_string(),
    schema: serde_json::to_value(schema)?,
};
let response = ctx.server_to_client()?.elicit(request, ctx.clone()).await?;
// Manual response parsing...
```

**After (v2.0):**
```rust
// Simple macro call
let result = elicit!(ctx, "Configure preferences", schema).await?;
```

### 3. Enhanced Context Injection

Context parameter can now appear **anywhere** in function signature:

```rust
// Context first (traditional)
#[tool("Process")]
async fn process(ctx: Context, data: String) -> McpResult<String> {
    ctx.info("Processing").await?;
    Ok(data)
}

// Context in middle (NEW in v2.0)
#[tool("Transform")]
async fn transform(input: String, ctx: Context, format: String) -> McpResult<String> {
    ctx.info(&format!("Transforming to {}", format)).await?;
    Ok(transformed)
}

// Context last (NEW in v2.0)
#[tool("Validate")]
async fn validate(data: String, strict: bool, ctx: Context) -> McpResult<bool> {
    ctx.info("Validating").await?;
    Ok(is_valid)
}

// No context (works in v1.x and v2.0)
#[tool("Add")]
async fn add(a: f64, b: f64) -> McpResult<f64> {
    Ok(a + b)
}
```

**Benefit:** More natural function signatures matching your domain logic

### 4. Improved Roots Configuration

```rust
#[server(
    name = "my-server",
    version = "2.0.0",
    root = "file:///workspace:Project Workspace",
    root = "file:///tmp:Temporary Files"
)]
impl MyServer {
    // ...
}
```

**Benefit:** Declarative filesystem boundaries in server macro

### 5. Enhanced Schema Generation

Better JSON Schema generation with:
- Improved enum handling
- Better optional parameter support
- Enhanced documentation extraction
- Nested type support

```rust
#[derive(Serialize, Deserialize)]
enum UserRole {
    Admin,
    User,
    Guest,
}

#[tool("Create user")]
async fn create_user(
    name: String,
    role: UserRole,  // Automatically generates enum constraint
    email: Option<String>  // Properly marked as optional
) -> McpResult<User> {
    // Schema automatically handles enums and optionals correctly
    Ok(User { name, role, email })
}
```

## 🔄 Migration Steps

### Step 1: Update Dependencies

```toml
# Before (v1.x)
[dependencies]
turbomcp-macros = "1.1.2"

# After (v2.0)
[dependencies]
turbomcp-macros = "2.0.4"
```

### Step 2: Build and Test

```bash
# Clean build
cargo clean
cargo build --all-features

# Run tests
cargo test

# Your existing code should work without any changes!
```

### Step 3: Optional - Adopt New Features

Now that you're on v2.0, you can opt-in to new features:

#### Add Elicitation Support

```rust
// NEW in v2.0.0
#[elicitation("Collect configuration")]
async fn get_config(&self, schema: serde_json::Value) -> McpResult<serde_json::Value> {
    Ok(serde_json::json!({"setting": "value"}))
}
```

#### Add Completion Support

```rust
// NEW in v2.0.0
#[completion("Complete commands")]
async fn complete_commands(&self, partial: String) -> McpResult<Vec<String>> {
    Ok(vec!["help".to_string(), "status".to_string()])
}
```

#### Use Enhanced elicit! Macro

```rust
// Upgrade from manual protocol handling to simple macro
#[tool("Interactive setup")]
async fn setup(&self, ctx: Context) -> McpResult<String> {
    let schema = ElicitationSchema::new()
        .add_string_property("name", Some("Your name"));

    let result = elicit!(ctx, "Enter your information", schema).await?;
    Ok(format!("Hello, {}!", result.content.unwrap()))
}
```

## 🐛 Troubleshooting

### Issue: Code works on v1.x but not v2.0

**This should not happen!** If you encounter this:

1. Check that you updated turbomcp-macros correctly:
   ```bash
   cargo tree | grep turbomcp-macros
   # Should show: turbomcp-macros v2.0.4
   ```

2. Ensure no stale artifacts:
   ```bash
   cargo clean
   cargo build
   ```

3. File an issue - this is a bug we need to fix!
   https://github.com/Epistates/turbomcp/issues

### Issue: New macros not recognized

**Solution:** Make sure you've updated to v2.0:

```toml
[dependencies]
turbomcp-macros = "2.0.4"  # Not "1.x"
```

Then import the prelude:
```rust
use turbomcp::prelude::*;
```

### Issue: `elicit!` macro not found

**Solution:** The `elicit!` macro is in the main turbomcp crate:

```rust
use turbomcp::prelude::*;  // Includes elicit! macro
use turbomcp_protocol::elicitation::ElicitationSchema;

// Now you can use it
let result = elicit!(ctx, "message", schema).await?;
```

### Issue: Context parameter positioning doesn't work

**Solution:** Ensure you're using the correct parameter name and type:

```rust
// Correct - parameter named 'ctx' of type Context
async fn my_tool(data: String, ctx: Context, flag: bool) -> McpResult<String>

// Incorrect - wrong parameter name
async fn my_tool(data: String, context: Context, flag: bool) -> McpResult<String>
//                                ^^^^^^^ Must be 'ctx'

// Incorrect - wrong type
async fn my_tool(data: String, ctx: &Context, flag: bool) -> McpResult<String>
//                                  ^^^^^^^^ Should be Context, not &Context
```

## 📊 Feature Comparison

| Feature | v1.x | v2.0 |
|---------|------|--------|
| `#[server]` | ✅ | ✅ |
| `#[tool]` | ✅ | ✅ |
| `#[resource]` | ✅ | ✅ |
| `#[prompt]` | ✅ | ✅ |
| `#[elicitation]` | ❌ | ✅ NEW |
| `#[completion]` | ❌ | ✅ NEW |
| `#[template]` | ❌ | ✅ NEW |
| `#[ping]` | ❌ | ✅ NEW |
| `elicit!` macro | Basic | ✅ Enhanced |
| Context anywhere | ❌ | ✅ NEW |
| Roots config | ❌ | ✅ NEW |
| Enhanced schemas | ❌ | ✅ Improved |
| Stdio safety | ❌ | ✅ NEW |

## 🎯 Common Migration Patterns

### Pattern 1: Existing Server (No Changes)

```rust
// v1.x code
#[derive(Clone)]
struct Calculator;

#[server]
impl Calculator {
    #[tool("Add numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }
}

// v2.0 - IDENTICAL! No changes needed
#[derive(Clone)]
struct Calculator;

#[server]
impl Calculator {
    #[tool("Add numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }
}
```

### Pattern 2: Add New Protocol Methods

```rust
// v1.x code (still works)
#[server]
impl MyServer {
    #[tool("Process")]
    async fn process(&self, input: String) -> McpResult<String> {
        Ok(input)
    }
}

// v2.0 - Add new features alongside existing code
#[server]
impl MyServer {
    // Existing tool - still works!
    #[tool("Process")]
    async fn process(&self, input: String) -> McpResult<String> {
        Ok(input)
    }

    // NEW: Add completion support
    #[completion("Complete inputs")]
    async fn complete_input(&self, partial: String) -> McpResult<Vec<String>> {
        Ok(vec!["option1".to_string(), "option2".to_string()])
    }

    // NEW: Add health check
    #[ping("Health")]
    async fn health(&self) -> McpResult<String> {
        Ok("OK".to_string())
    }
}
```

### Pattern 3: Upgrade to Enhanced elicit!

```rust
// v1.x - Manual protocol handling
async fn configure(&self, ctx: Context) -> McpResult<String> {
    // Complex protocol code...
    let request = ElicitRequest { /* ... */ };
    let response = ctx.server_to_client()?.elicit(request, ctx.clone()).await?;
    // Parse response...
}

// v2.0 - Simple macro
async fn configure(&self, ctx: Context) -> McpResult<String> {
    let schema = ElicitationSchema::new()
        .add_string_property("name", Some("Your name"));
    let result = elicit!(ctx, "Enter info", schema).await?;
    Ok(format!("Hello, {}!", result.content.unwrap()))
}
```

### Pattern 4: Flexible Context Positioning

```rust
// v1.x - Context must be first (after self)
#[tool("Old way")]
async fn old_way(&self, ctx: Context, data: String) -> McpResult<String> {
    Ok(data)
}

// v2.0 - Context can be anywhere!
#[tool("New flexibility")]
async fn new_way(&self, data: String, ctx: Context) -> McpResult<String> {
    Ok(data)
}

// Or in the middle
#[tool("Maximum flexibility")]
async fn flexible(&self, input: String, ctx: Context, format: String) -> McpResult<String> {
    Ok(format!("{}: {}", format, input))
}
```

## 📚 Additional Resources

- **Main Migration Guide**: See `../../MIGRATION.md` for workspace-level changes
- **API Documentation**: https://docs.rs/turbomcp-macros
- **Examples**: See `../../examples/` for updated 2.0 examples
- **Macro Guide**: See README.md for comprehensive macro documentation

## 🎉 Benefits of 2.0

After migration (which requires zero code changes!), you gain:

- ✅ **Backward Compatible** - All v1.x code works unchanged
- ✅ **New Protocol Features** - 4 new MCP macros for advanced features
- ✅ **Better Ergonomics** - Enhanced `elicit!` macro and flexible context
- ✅ **Improved Schemas** - Better JSON Schema generation
- ✅ **Future Proof** - Ready for MCP protocol evolution

## 🤝 Getting Help

- **Issues**: https://github.com/Epistates/turbomcp/issues
- **Discussions**: https://github.com/Epistates/turbomcp/discussions
- **Documentation**: https://docs.rs/turbomcp-macros

## 📝 Version Compatibility

| turbomcp-macros | Status | Migration |
|-----------------|--------|-----------|
| 2.0.4           | ✅ Current | Zero changes needed |
| 1.1.x           | 🟡 Maintenance | Update to 2.0 for new features |
| 1.0.x           | ⚠️ EOL | Upgrade recommended |
