# Macros API Reference

Complete reference for TurboMCP procedural macros that enable zero-boilerplate server development.

## Overview

TurboMCP provides procedural macros that generate an `McpHandler`
implementation — handler listing, schema generation, argument parsing, and
dispatch — from an ordinary `impl` block. They run at compile time. Import them
with `use turbomcp::prelude::*;`.

Every example on this page compiles on its own (some need the `schemars` or
`chrono` crate as a dependency, as noted).

## Core Macros

### #[server]

The `#[server]` macro transforms an inherent `impl` block into a fully
functional MCP server by implementing `McpHandler` for the type. The type must
be `Clone`.

#### Basic Usage

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[turbomcp::server(
    name = "my-server",
    version = "1.0.0"
)]
impl MyServer {
    /// Handler methods go here
    #[tool]
    async fn ping(&self) -> String {
        "pong".to_string()
    }
}
```

#### Attributes

| Attribute | Type | Required | Default | Description |
|-----------|------|----------|---------|-------------|
| `name` | expression | No | type name | Server identifier |
| `version` | expression | No | `"1.0.0"` | Version string |
| `description` | expression | No | — | What this implementation is |
| `title` | expression | No | — | Human-readable display name |
| `instructions` | expression | No | — | How to use the server; returned by `initialize` |
| `website_url` | expression | No | — | Homepage |
| `icons` | `[expression, …]` | No | — | Icon source URIs |
| `page_size` | expression | No | — | Paginate the list methods at this many entries |

An unknown attribute is a compile error. `logging` is accepted as a no-op for
compatibility. `transports = [...]` was removed in v3 and is a compile error:
enable transports with Cargo features and choose one at runtime.

#### Generated Code

The macro generates one `impl McpHandler for YourType`:

1. **Metadata and listing:** `server_info`, `instructions`,
   `server_capabilities`, `list_tools`, `list_resources`,
   `list_resource_templates`, `list_prompts`, and `page_size` when set.
2. **Dispatch:** `call_tool`, `read_resource`, and `get_prompt`, which parse
   arguments, call your methods, and convert their return values; plus the
   optional methods for any marker you declare.
3. **Schema generation:** a JSON Schema for each tool's parameters, and an
   output schema for tools that return `Json<T>`.

The capabilities it advertises follow the block: `tools`, `resources`, and
`prompts` for the handlers present, `completions` for `#[completion]`,
`resources.subscribe` for `#[subscribe]`, and `logging` always.

Run methods (`run_stdio`, `run_http`, …) are not generated: they come from
`McpHandlerExt`, which every `McpHandler` gets. See the
[Server API](server.md).

#### Example

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[turbomcp::server(name = "calculator", version = "1.0.0")]
impl Calculator {
    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    #[tool("Multiply two numbers")]
    async fn multiply(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a * b)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    Calculator.run_stdio().await?;
    Ok(())
}
```

### #[tool]

The `#[tool]` macro marks a method as a tool handler and generates its input
schema. The tool's name is the method name (a raw identifier loses its `r#`).

#### Basic Usage

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Process a value (the doc comment is the description)
    #[tool]
    async fn my_tool(&self, param: String) -> McpResult<String> {
        Ok(format!("Processed: {}", param))
    }
}
```

#### Attributes

| Attribute | Description |
|-----------|-------------|
| `"..."` (shorthand) or `description = "..."` | Description; overrides the doc comment |
| `title = "..."` | Display name |
| `read_only`, `destructive`, `idempotent`, `open_world` = `bool` | `ToolAnnotations` hints |
| `output_schema = Type` | Output schema (inferred for a `Json<T>` return) |
| `task_support = "forbidden" \| "optional" \| "required"` | Advertised as `execution.taskSupport` |
| `tags = [...]`, `version = "..."`, `icons = [...]` | Metadata (`tags` and `version` go in `_meta`) |

#### With Description

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool(description = "Searches files matching a pattern", read_only = true)]
    async fn search_files(&self, pattern: String) -> McpResult<Vec<String>> {
        // Implementation
        Ok(vec![])
    }
}
```

#### Parameter Documentation

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool(description = "Create a user account")]
    async fn create_user(
        &self,
        #[description("User's full name")]
        name: String,
        #[description("Email address")]
        email: String,
        #[description("Optional phone number")]
        phone: Option<String>
    ) -> McpResult<String> {
        Ok(format!("Created user: {}", name))
    }
}
```

#### Supported Parameter Types

Any type that implements `serde::Deserialize` and `schemars::JsonSchema`.
A `&RequestContext` parameter, in any position, receives the request context
and is left out of the schema.

**Primitives and collections:**
```rust
use std::collections::HashMap;
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn primitives(
        &self,
        bool_param: bool,
        int_param: i32,
        float_param: f64,
        string_param: String
    ) -> McpResult<String> { Ok("Done".into()) }

    #[tool]
    async fn collections(
        &self,
        vec_param: Vec<String>,
        optional_param: Option<i32>,
        map_param: HashMap<String, i32>
    ) -> McpResult<String> { Ok("Done".into()) }
}
```

**Custom Types** (needs `schemars` as a dependency):
```rust
use serde::Deserialize;
use turbomcp::prelude::*;

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct User {
    name: String,
    email: String,
}

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn process_user(&self, user: User) -> McpResult<String> {
        Ok(format!("Processing {}", user.name))
    }
}
```

#### Return Types

A tool returns any `IntoToolResult` type: `String`, `&str`, the numeric
types, `bool`, `()`, `serde_json::Value`, `Vec<T: Serialize>`, `Json<T>`,
`ToolResult`, `Option<T>`, or a `Result` of one. An `Err` becomes a tool
execution error (`isError: true`); for `McpError` its kind is kept in `_meta`.

#### Generated Schema

For `create_user` above, the input schema is:

```json
{
  "type": "object",
  "properties": {
    "name": {
      "type": "string",
      "description": "User's full name"
    },
    "email": {
      "type": "string",
      "description": "Email address"
    },
    "phone": {
      "type": ["string", "null"],
      "description": "Optional phone number"
    }
  },
  "required": ["name", "email"],
  "additionalProperties": false,
  "$schema": "https://json-schema.org/draft/2020-12/schema"
}
```

The dispatcher enforces `additionalProperties: false`: an argument the tool
does not declare is reported as a tool execution error.

### #[resource]

The `#[resource]` macro marks a method as a resource handler.

**Syntax:**
- `#[resource("uri://template")]` - URI or RFC 6570 URI template (required, first)
- `#[resource("uri://template", mime_type = "application/json")]` - With MIME type
- `audience = ["user", "assistant"]`, `priority = 0.0..=1.0`,
  `last_modified = "2025-01-12T15:00:58Z"` - `ResourceAnnotations`
- `size = N` - size in bytes (concrete URIs only)
- `description`, `title`, `tags`, `version`, `icons` - as on `#[tool]`

**Note:** Unlike `#[tool]` and `#[prompt]`, the resource macro takes a URI template as
its first argument (not a description). Use doc comments or `description = "..."`
for the description.

The handler signature is always `(&self, uri: String, ctx: &RequestContext)`
returning `McpResult<T>` for an `IntoResourceResult` type (`String`, `&str`,
`ResourceResult`).

#### Basic Usage

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Application configuration
    #[resource("config://app")]
    async fn get_config(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        Ok(ResourceResult::text(&uri, r#"{"setting": "value"}"#))
    }
}
```

#### With MIME Type

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Application configuration file
    #[resource("config://app", mime_type = "application/json", audience = ["user"], priority = 0.8)]
    async fn get_config(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        Ok(ResourceResult::text(&uri, r#"{"setting": "value"}"#))
    }
}
```

#### URI Templates

Resources use URI templates for dynamic content. Template variables are not
bound to parameters; the handler receives the full URI:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Read a file by path
    #[resource("file://{path}")]
    async fn read_file(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        // Extract path from uri: "file:///home/user/file.txt" -> "/home/user/file.txt"
        let path = uri.strip_prefix("file://").unwrap_or(&uri);
        let content = tokio::fs::read_to_string(path).await?;
        Ok(ResourceResult::text(&uri, content))
    }
}
```

A concrete URI is matched before any template, whatever the declaration order.

#### Resource Result Types

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// Text content
    #[resource("text://{path}")]
    async fn text_file(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        let path = uri.strip_prefix("text://").unwrap_or(&uri);
        let content = std::fs::read_to_string(path)?;
        Ok(ResourceResult::text(&uri, content))
    }

    /// Binary content: base64-encode the bytes (e.g. with the `base64` crate)
    #[resource("blob://logo")]
    async fn binary_file(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        let base64_data = "iVBORw0KGgo=";
        Ok(ResourceResult::binary(&uri, base64_data, "image/png"))
    }

    /// Current server time (needs `chrono` as a dependency)
    #[resource("time://now")]
    async fn current_time(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        let now = chrono::Utc::now().to_rfc3339();
        Ok(ResourceResult::text(&uri, now))
    }
}
```

### #[prompt]

The `#[prompt]` macro marks a method as a prompt handler. The prompt's name is
the method name. Arguments are `String` (required) or `Option<String>`
(optional), and `ctx: &RequestContext` comes last.

#### Basic Usage

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[prompt]
    async fn greeting(&self, name: String, ctx: &RequestContext) -> McpResult<PromptResult> {
        Ok(PromptResult::user(format!("Hello, {}!", name)))
    }
}
```

#### With Description

`#[prompt("...")]` and `description = "..."` set the description. Parameters
take `#[description("...")]` and `#[title("...")]`, a display label:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[prompt(description = "Generate a code review prompt")]
    async fn code_review(
        &self,
        #[title("Language")]
        #[description("Programming language")]
        language: String,
        #[description("Code to review")]
        code: String,
        ctx: &RequestContext,
    ) -> McpResult<PromptResult> {
        Ok(PromptResult::user(format!(
            "Please review this {} code:\n\n{}",
            language, code
        )))
    }
}
```

A prompt's `Err` is returned as a JSON-RPC error, not rendered as a message.

#### Multi-Message Prompts

Use the builder pattern for multi-turn conversations, or construct messages
directly:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[prompt]
    async fn conversation(&self, ctx: &RequestContext) -> McpResult<PromptResult> {
        Ok(PromptResult::user("How do I use TurboMCP?")
            .add_assistant("TurboMCP is a Rust SDK for MCP. Here's how to get started...")
            .with_description("A helpful conversation about TurboMCP"))
    }

    #[prompt]
    async fn custom_messages(&self, ctx: &RequestContext) -> McpResult<PromptResult> {
        let messages = vec![
            Message::user("What is the weather?"),
            Message::assistant("I'll check the weather for you."),
        ];
        Ok(PromptResult::new(messages))
    }
}
```

### Optional Handler Markers

| Marker | Signature | Serves | Advertises |
|--------|-----------|--------|------------|
| `#[completion]` | `(&self, params: serde_json::Value[, ctx]) -> McpResult<serde_json::Value>` | `completion/complete` | `completions` |
| `#[subscribe]` | `(&self, uri: String[, ctx]) -> McpResult<()>` | `resources/subscribe` | `resources.subscribe` |
| `#[unsubscribe]` | `(&self, uri: String[, ctx]) -> McpResult<()>` | `resources/unsubscribe` | — |
| `#[set_level]` | `(&self, level: String[, ctx]) -> McpResult<()>` | `logging/setLevel` | — (`logging` is always advertised) |
| `#[roots_changed]` | `(&self[, ctx]) -> McpResult<()>` | `notifications/roots/list_changed` | — |

Each may appear once. `#[subscribe]` requires `#[unsubscribe]`. Without a
marker, the method answers `capability_not_supported` — except
`logging/setLevel`, which every server accepts and records per session.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    /// `params` is `{ ref, argument, context? }`; return `{ completion: { values, … } }`
    #[completion]
    async fn complete(&self, params: serde_json::Value) -> McpResult<serde_json::Value> {
        let prefix = params["argument"]["value"].as_str().unwrap_or("");
        let values: Vec<&str> = ["rust", "ruby"]
            .into_iter()
            .filter(|lang| lang.starts_with(prefix))
            .collect();
        Ok(serde_json::json!({ "completion": { "values": values } }))
    }

    #[set_level]
    async fn level_changed(&self, level: String, ctx: &RequestContext) -> McpResult<()> {
        tracing::info!(%level, "client set the log level");
        Ok(())
    }
}
```

## Advanced Features

### Context Injection

A `&RequestContext` parameter gives a handler the request's metadata and the
server-to-client operations. There is no other injection: application state
lives on the server type.

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

#[derive(Clone, Default)]
struct MyServer {
    cache: Arc<RwLock<HashMap<String, String>>>,
}

#[server]
impl MyServer {
    #[tool]
    async fn with_context(&self, ctx: &RequestContext, data: String) -> McpResult<String> {
        tracing::info!(request_id = ctx.request_id(), "processing request");
        self.cache.write().await.insert("key".into(), data.clone());
        ctx.report_progress(1.0, Some(1.0), None).await?;
        Ok(format!("Processed: {}", data))
    }
}
```

**What the context provides:**
- `request_id()`, `transport()`, `session_id()`, `headers()` / `header(name)`
- `principal()`, `subject()`, `is_authenticated()`, `roles()`, `has_any_role(...)`
- `is_cancelled()` for cooperative cancellation
- `report_progress(...)`, `sample(...)`, `elicit_form(...)`, `elicit_url(...)`,
  `list_roots()`, `notify_resource_updated(uri)`, and the list-changed notifications
- client logging through `turbomcp_protocol::RichContextExt` (`ctx.info(...)`)

### Validation

There are no validation attributes on parameters. Validate in the handler and
return `McpError::invalid_params`, which the client sees as a tool execution
error it can correct:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn validated_tool(&self, name: String, age: u8) -> McpResult<String> {
        if name.is_empty() || name.len() > 100 {
            return Err(McpError::invalid_params("name must be 1-100 characters"));
        }
        if age > 150 {
            return Err(McpError::invalid_params("age must be at most 150"));
        }
        Ok("Valid input".to_string())
    }
}
```

Constraints declared with `schemars` attributes on a parameter type do appear
in the generated schema (see [Custom Schema Annotations](#custom-schema-annotations)).

### Default Values

Use `Option<T>` and supply the default in the handler:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn with_defaults(
        &self,
        required: String,
        optional: Option<String>
    ) -> McpResult<String> {
        let value = optional.unwrap_or_else(|| "default_value".to_string());
        Ok(format!("{}: {}", required, value))
    }
}
```

### Async Methods

Handlers are ordinary `async fn`s, so they can await anything:

```rust
use std::time::Duration;
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[turbomcp::server(name = "async-server", version = "1.0.0")]
impl MyServer {
    #[tool]
    async fn async_operation(&self) -> McpResult<String> {
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok("Done".to_string())
    }
}
```

## Schema Generation

### Automatic Schema

The macros generate JSON Schema from Rust types through `schemars`:

```rust
use serde::Deserialize;
use turbomcp::prelude::*;

#[derive(Deserialize, schemars::JsonSchema)]
struct Address {
    street: String,
    city: String,
    zip: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct Person {
    name: String,
    age: u8,
    email: Option<String>,
    address: Address,
}

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn create_person(&self, person: Person) -> McpResult<String> {
        Ok(format!("Created {}", person.name))
    }
}
```

**Generated schema.** A type used inside another is referenced through the
schema's own `$defs`, which the macro hoists to the top level so every `$ref`
resolves:
```json
{
  "type": "object",
  "properties": {
    "person": {
      "type": "object",
      "properties": {
        "name": {"type": "string"},
        "age": {"type": "integer", "format": "uint8", "minimum": 0, "maximum": 255},
        "email": {"type": ["string", "null"]},
        "address": {"$ref": "#/$defs/Address"}
      },
      "required": ["name", "age", "address"]
    }
  },
  "required": ["person"],
  "additionalProperties": false,
  "$defs": {
    "Address": {
      "type": "object",
      "properties": {
        "street": {"type": "string"},
        "city": {"type": "string"},
        "zip": {"type": "string"}
      },
      "required": ["street", "city", "zip"]
    }
  },
  "$schema": "https://json-schema.org/draft/2020-12/schema"
}
```

### Custom Schema Annotations

Use schemars attributes for custom schema:

```rust
use serde::Deserialize;
use schemars::JsonSchema;

#[derive(Deserialize, JsonSchema)]
struct CustomType {
    #[schemars(range(min = 0, max = 100))]
    percentage: f64,

    #[schemars(regex(pattern = "^[A-Z]{2}$"))]
    country_code: String,

    #[schemars(url)]
    website: String,
}
```

## Compilation

### Macro Expansion

View generated code with `cargo expand`:

```bash
cargo install cargo-expand
cargo expand --lib
```

### Build-Time Validation

Macros reject mistakes at compile time. Each item below is a compile error
(so this block is not compiled):

```rust,ignore
// ❌ Unknown attribute key
#[turbomcp::server(name = "test", versoin = "1.0.0")]
impl MyServer { }

// ❌ `transports` was removed; use Cargo features
#[turbomcp::server(name = "test", transports = ["stdio"])]
impl MyServer { }

// ❌ #[server] on a trait impl
#[turbomcp::server(name = "test")]
impl SomeTrait for MyServer { }

// ❌ #[subscribe] without #[unsubscribe]
#[turbomcp::server(name = "test")]
impl MyServer {
    #[subscribe]
    async fn watch(&self, uri: String) -> McpResult<()> { Ok(()) }
}

// ❌ #[tool] outside a #[server] block
#[tool]
async fn orphan() -> String { String::new() }
```

## Best Practices

### 1. Document All Handlers

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    // Good
    #[tool(description = "Comprehensive description of what this tool does")]
    async fn well_documented(
        &self,
        #[description("Clear parameter description")]
        param: String
    ) -> McpResult<String> { Ok(param) }

    // Avoid
    #[tool]
    async fn undocumented(&self, p: String) -> McpResult<String> { Ok(p) }
}
```

### 2. Use Strong Types

```rust
use serde::Deserialize;
use turbomcp::prelude::*;

// Good
#[derive(Deserialize, schemars::JsonSchema)]
struct SearchOptions {
    case_sensitive: bool,
    max_results: usize,
}

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn search(&self, query: String, options: SearchOptions) -> McpResult<Vec<String>> {
        Ok(vec![])
    }

    // Avoid: loosely related flags as separate parameters
    #[tool]
    async fn search_flat(&self, query: String, case_sensitive: bool, max_results: usize) -> McpResult<Vec<String>> {
        Ok(vec![])
    }
}
```

### 3. Handle Errors Properly

```rust
use turbomcp::prelude::*;

fn validate_input(input: &str) -> McpResult<()> {
    if input.is_empty() {
        return Err(McpError::invalid_params("input must not be empty"));
    }
    Ok(())
}

async fn perform_operation(input: &str) -> Result<String, std::io::Error> {
    Ok(input.to_string())
}

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    // Good
    #[tool]
    async fn safe_operation(&self, input: String) -> McpResult<String> {
        validate_input(&input)?;
        perform_operation(&input)
            .await
            .map_err(|e| McpError::internal(e.to_string()))
    }

    // Avoid
    #[tool]
    async fn unsafe_operation(&self, input: String) -> McpResult<String> {
        Ok(perform_operation(&input).await.unwrap())
    }
}
```

### 4. Keep Handlers Focused

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Mailer;

#[server]
impl Mailer {
    // Good - Single responsibility
    #[tool("Validate email format")]
    async fn validate_email(&self, email: String) -> McpResult<bool> {
        Ok(email.contains('@'))
    }

    #[tool("Send email")]
    async fn send_email(&self, to: String, subject: String, body: String) -> McpResult<String> {
        Ok(format!("sent to {to}"))
    }

    // Avoid - Too many responsibilities
    #[tool("Validate and send email")]
    async fn validate_and_send(&self, email: String, subject: String, body: String) -> McpResult<String> {
        Ok(String::new())
    }
}
```

### 5. Use Appropriate Handler Types

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Config;

#[server]
impl Config {
    // Good - data the client reads is a resource
    /// Get application configuration
    #[resource("config://app")]
    async fn get_config(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        Ok(ResourceResult::text(&uri, r#"{"setting": "value"}"#))
    }

    // An action the model takes is a tool
    #[tool(description = "Update configuration")]
    async fn update_config(&self, config: String) -> McpResult<String> {
        Ok("updated".to_string())
    }

    // Avoid - reading data through a tool
    #[tool(description = "Get configuration")]  // Should be #[resource]
    async fn read_config(&self) -> McpResult<String> {
        Ok(String::new())
    }
}
```

## Troubleshooting

### "Cannot find attribute macro"

Import the prelude:

```rust
use turbomcp::prelude::*;
```

### "the trait bound `...: IntoToolResult` is not satisfied"

A tool's return type must implement `IntoToolResult`. A plain `String` works,
as does `McpResult<String>`; a custom struct needs wrapping in `Json<T>` (and
`Serialize` + `JsonSchema`):

```rust
use serde::Serialize;
use turbomcp::prelude::*;

#[derive(Serialize, schemars::JsonSchema)]
struct Report {
    ok: bool,
}

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn plain(&self) -> String {
        "fine".to_string()
    }

    #[tool]
    async fn structured(&self) -> McpResult<Json<Report>> {
        Ok(Json(Report { ok: true }))
    }
}
```

### "the trait bound `...: JsonSchema` is not satisfied"

Custom parameter types need `Deserialize` and `JsonSchema`:

```rust
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
struct CustomType {
    field: String,
}
```

### "Server does not implement Clone"

The server struct must be `Clone`:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;

struct State;

#[derive(Clone)]
struct MyServer {
    // Use Arc for shared state
    state: Arc<RwLock<State>>,
}
```

## Examples

### Complete Server with All Handler Types

```rust
use turbomcp::prelude::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
struct FullServer {
    cache: Arc<RwLock<HashMap<String, String>>>,
}

#[turbomcp::server(name = "full-server", version = "1.0.0")]
impl FullServer {
    fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[tool(description = "Store a value in cache")]
    async fn set(
        &self,
        #[description("Cache key")]
        key: String,
        #[description("Value to store")]
        value: String
    ) -> McpResult<String> {
        let mut cache = self.cache.write().await;
        cache.insert(key.clone(), value);
        Ok(format!("Stored key: {}", key))
    }

    #[tool(description = "Get a value from cache", read_only = true)]
    async fn get(
        &self,
        #[description("Cache key")]
        key: String
    ) -> McpResult<Option<String>> {
        let cache = self.cache.read().await;
        Ok(cache.get(&key).cloned())
    }

    /// List all cached keys
    #[resource("cache://keys", mime_type = "application/json")]
    async fn list_keys(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        let cache = self.cache.read().await;
        let keys: Vec<String> = cache.keys().cloned().collect();
        Ok(ResourceResult::json(&uri, &keys)?)
    }

    #[prompt(description = "Generate cache query prompt")]
    async fn query_prompt(
        &self,
        #[description("Key to query")]
        key: String,
        ctx: &RequestContext,
    ) -> McpResult<PromptResult> {
        Ok(PromptResult::user(format!("What is the value of cache key '{}'?", key)))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    FullServer::new().run_stdio().await?;
    Ok(())
}
```

## Next Steps

- **[Server API](server.md)** - Complete server reference
- **[Context Injection](../guide/context-injection.md)** - Request context guide
- **[Examples](../examples/basic.md)** - Real-world examples

## See Also

- [Procedural Macros - The Rust Book](https://doc.rust-lang.org/book/ch19-06-macros.html)
- [API Documentation (docs.rs)](https://docs.rs/turbomcp)
- [Source Code](https://github.com/Epistates/turbomcp)
