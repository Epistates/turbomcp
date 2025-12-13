# Handlers

Learn how to define tools, resources, prompts, and handle requests in TurboMCP.

## Handler Types

TurboMCP supports four types of handlers:

### Tools

Tools are functions the model can call to perform actions:

```rust
#[tool(description = "Add two numbers")]
async fn add(
    #[description = "First number"]
    a: i32,
    #[description = "Second number"]
    b: i32,
) -> McpResult<i32> {
    Ok(a + b)
}
```

### Resources

Resources provide static or dynamic information:

```rust
#[resource(uri = "data://users", description = "List all users")]
async fn list_users() -> McpResult<String> {
    Ok("User list".to_string())
}
```

### Prompts

Prompts return instruction templates:

```rust
#[prompt(description = "Generate code")]
async fn code_generation_prompt() -> McpResult<String> {
    Ok("Write Rust code to...".to_string())
}
```

### Samplings

Samplings enable bidirectional model interaction:

```rust
#[sampling(description = "Ask the model for advice")]
async fn ask_model() -> McpResult<String> {
    Ok("Please suggest...".to_string())
}
```

## Handler Parameters

### Basic Types

Handlers support all standard Rust types:

```rust
#[tool]
async fn process(
    text: String,
    count: i32,
    ratio: f64,
    enabled: bool,
) -> McpResult<String> {
    Ok(format!("{} x {}", text, count))
}
```

### Structured Types

Use `serde` to support complex types:

```rust
#[derive(serde::Deserialize)]
struct UserInput {
    name: String,
    age: u32,
}

#[tool]
async fn process_user(user: UserInput) -> McpResult<String> {
    Ok(format!("Processed {}", user.name))
}
```

### Optional Parameters

```rust
#[tool]
async fn search(
    query: String,
    limit: Option<usize>,
) -> McpResult<String> {
    let limit = limit.unwrap_or(10);
    Ok(format!("Found {} results", limit))
}
```

### Injected Dependencies

```rust
#[tool]
async fn handler(
    param: String,
    logger: Logger,
    cache: Cache,
    config: Config,
) -> McpResult<String> {
    Ok("Done".into())
}
```

## Handler Return Types

### Simple Types

```rust
#[tool]
async fn simple() -> McpResult<String> {
    Ok("result".into())
}

#[tool]
async fn number() -> McpResult<i32> {
    Ok(42)
}
```

### Structured Responses

```rust
#[derive(serde::Serialize)]
struct Response {
    status: String,
    data: Vec<String>,
}

#[tool]
async fn complex() -> McpResult<Response> {
    Ok(Response {
        status: "ok".into(),
        data: vec!["a", "b"].iter().map(|s| s.to_string()).collect(),
    })
}
```

### Error Handling

```rust
#[tool]
async fn operation(value: i32) -> McpResult<String> {
    if value < 0 {
        return Err(McpError::InvalidInput("Must be positive".into()));
    }
    Ok("Success".into())
}
```

## Error Types

TurboMCP provides standard error types:

```rust
Err(McpError::InvalidInput("description"))
Err(McpError::InvalidRequest("description"))
Err(McpError::InternalError("description"))
Err(McpError::NotFound("description"))
Err(McpError::MethodNotAllowed("description"))
Err(McpError::Unauthorized("description"))
```

## Next Steps

- **[Context & DI](context-injection.md)** - Dependency injection
- **[Transports](transports.md)** - Transport configuration
- **[Examples](../examples/basic.md)** - Real-world handlers
