# Handlers

Learn how to define tools, resources, prompts, and handle requests in TurboMCP.

## Handler Types

TurboMCP supports three primary types of handlers via procedural macros:

### Tools

Tools are functions the model can call to perform actions. The `#[tool]` macro automatically generates the JSON schema from your function signature.

```rust
#[tool("Add two numbers")]
async fn add(
    &self,
    #[description("First number")]
    a: i32,
    #[description("Second number")]
    b: i32,
) -> i32 {
    a + b
}
```

### Resources

Resources provide static or dynamic information. The `#[resource]` macro maps a URI template to your function.

```rust
// Matches exactly "data://users"
#[resource("data://users")]
async fn list_users(&self) -> String {
    "User list...".to_string()
}

// Matches templates like "file://path/to/file.txt"
#[resource("file://{path}")]
async fn read_file(&self, path: String) -> String {
    format!("Reading {}", path)
}
```

You can also specify the MIME type:

```rust
#[resource("data://image", mime_type = "image/png")]
async fn get_image(&self) -> Vec<u8> {
    // Return raw bytes
    vec![0x89, 0x50, 0x4E, 0x47, ...]
}
```

### Prompts

Prompts return instruction templates for the LLM.

```rust
#[prompt("code-review")]
async fn code_review(&self, code: String) -> String {
    format!("Please review this code:\n\n{}", code)
}
```

## Handler Parameters

### Basic Types

Handlers support all standard Rust types that implement `serde::Deserialize`:

```rust
#[tool]
async fn process(
    &self,
    text: String,
    count: i32,
    ratio: f64,
    enabled: bool,
) -> String {
    format!("{} x {}", text, count)
}
```

### Structured Types

Use structs to organize complex arguments:

```rust
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct UserInput {
    name: String,
    age: u32,
}

#[tool]
async fn process_user(&self, user: UserInput) -> String {
    format!("Processed {}", user.name)
}
```

### Optional Parameters

Use `Option<T>` for optional arguments. The generated schema will mark them as not required.

```rust
#[tool]
async fn search(
    &self,
    query: String,
    limit: Option<usize>,
) -> String {
    let limit = limit.unwrap_or(10);
    format!("Found {} results", limit)
}
```

### Request Context

If you need access to the request context (e.g., to check the request ID or user info), add a parameter named `ctx` of type `RequestContext`. This is a special parameter injected by the macro; it is **not** exposed in the tool's JSON schema.

```rust
use turbomcp::prelude::*;

#[tool]
async fn handler(&self, ctx: RequestContext, param: String) -> String {
    let request_id = ctx.request_id;
    format!("Request ID: {}", request_id)
}
```

### Server State (Dependency Injection)

For other dependencies like databases, caches, or configuration, store them in your server struct. Since your server struct is `Clone`, consider using `Arc` for shared state.

```rust
#[derive(Clone)]
struct MyServer {
    db: Arc<Database>,
    config: Arc<Config>,
}

#[server(name = "my-server")]
impl MyServer {
    #[tool]
    async fn query_db(&self, id: String) -> String {
        // Access state via self
        self.db.get(id).await
    }
}
```

## Handler Return Types

### Simple Types

Handlers can return any type that implements `IntoToolResult`, `IntoResourceResult`, or `IntoPromptResult`. This includes most primitives:

```rust
#[tool]
async fn simple(&self) -> String {
    "result".into()
}

#[tool]
async fn number(&self) -> i32 {
    42
}
```

### Result Type

Use `McpResult<T>` (alias for `Result<T, McpError>`) to handle errors gracefully:

```rust
#[tool]
async fn operation(&self, value: i32) -> McpResult<String> {
    if value < 0 {
        return Err(McpError::invalid_params("Must be positive"));
    }
    Ok("Success".into())
}
```

### Binary Data

For resources, you can return `Vec<u8>` or `bytes::Bytes` to send binary data (automatically base64 encoded for JSON transport):

```rust
#[resource("file://{path}")]
async fn read_binary(&self, path: String) -> McpResult<Vec<u8>> {
    let data = std::fs::read(path).map_err(|e| McpError::internal(e.to_string()))?;
    Ok(data)
}
```

## Error Handling

TurboMCP provides standard error constructors on `McpError`:

```rust
Err(McpError::invalid_params("Invalid input"))
Err(McpError::internal("Database failed"))
Err(McpError::tool_not_found("Tool missing"))
Err(McpError::resource_not_found("File not found"))
```

## Next Steps

- **[Examples](../examples/basic.md)** - Real-world handlers
- **[Transports](transports.md)** - Configuring HTTP/TCP transports