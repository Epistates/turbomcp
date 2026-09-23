# Handlers

Learn how to define tools, resources, prompts, and handle requests in TurboMCP.

Handlers are async methods in an `impl` block marked `#[server]`. The macro
finds the marked methods, generates their schemas, and implements
`McpHandler` for your type. Each example on this page compiles on its own in a
crate that depends on `turbomcp` and `tokio` (plus `schemars` or `base64` where
noted); run the server with `.run_stdio().await` or another transport.

## Handler Types

TurboMCP supports three primary types of handlers via procedural macros, plus
markers for the optional MCP methods (see [Optional Handlers](#optional-handlers)).

### Tools

Tools are functions the model can call to perform actions. The `#[tool]` macro automatically generates the JSON schema from your function signature.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Calculator;

#[server(name = "calculator", version = "1.0.0")]
impl Calculator {
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
}
```

`#[tool]` also takes the `ToolAnnotations` hints (`read_only`, `destructive`,
`idempotent`, `open_world`), `title`, `tags`, `version`, `icons`,
`output_schema = Type`, and `task_support = "forbidden" | "optional" | "required"`.

### Resources

Resources provide static or dynamic information. The `#[resource]` macro maps a URI, or an RFC 6570 URI template, to your method. A resource handler always takes the requested URI and the request context, and returns `McpResult<T>`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Files;

#[server]
impl Files {
    /// Matches exactly "data://users"
    #[resource("data://users")]
    async fn list_users(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        Ok("User list...".to_string())
    }

    /// Matches URIs like "file://path/to/file.txt"
    #[resource("file://{path}")]
    async fn read_file(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        // Template variables are not bound to parameters: take them from the URI
        let path = uri.trim_start_matches("file://");
        Ok(format!("Reading {}", path))
    }
}
```

A concrete URI is matched before any template, whatever order they are
declared in.

You can also specify the MIME type and the `ResourceAnnotations`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Docs;

#[server]
impl Docs {
    #[resource(
        "docs://readme",
        mime_type = "text/markdown",
        audience = ["user"],
        priority = 0.9,
        last_modified = "2025-01-12T15:00:58Z",
        size = 13
    )]
    async fn readme(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        Ok("# Hello docs".to_string())
    }
}
```

The declared `mime_type` is advertised in `resources/list` and applied to a
read that returns a single entry. `size` applies only to a concrete URI.

### Prompts

Prompts return instruction templates for the LLM. The prompt's name is the
method name, and `#[prompt("...")]` sets its description. Arguments are
`String` (required) or `Option<String>` (optional), and the request context
comes last:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Reviewer;

#[server]
impl Reviewer {
    #[prompt("Ask for a code review")]
    async fn code_review(
        &self,
        #[title("Code")]
        #[description("The code to review")]
        code: String,
        focus: Option<String>,
        ctx: &RequestContext,
    ) -> String {
        let focus = focus.unwrap_or_else(|| "correctness".to_string());
        format!("Please review this code for {focus}:\n\n{code}")
    }
}
```

`#[title]` gives an argument a display label for clients that render a form.

## Handler Parameters

### Basic Types

Tool parameters can be any type that implements `serde::Deserialize` and
`schemars::JsonSchema`:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
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
}
```

An argument the tool does not declare is rejected: the generated schema sets
`additionalProperties: false`, and the dispatcher reports an unknown, missing,
or mistyped argument as a tool execution error.

### Structured Types

Use structs to organize complex arguments (add `schemars = "1"` to your
dependencies for the derive):

```rust
use turbomcp::prelude::*;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct UserInput {
    name: String,
    age: u32,
}

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn process_user(&self, user: UserInput) -> String {
        format!("Processed {}", user.name)
    }
}
```

### Optional Parameters

Use `Option<T>` for optional arguments. The generated schema will mark them as not required.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn search(
        &self,
        query: String,
        limit: Option<usize>,
    ) -> String {
        let limit = limit.unwrap_or(10);
        format!("Found {} results", limit)
    }
}
```

### Request Context

If you need access to the request context (e.g., to check the request ID or user info), add a `&RequestContext` parameter. The macro recognizes it by type, in any position, and does **not** expose it in the tool's JSON schema.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn handler(&self, ctx: &RequestContext, param: String) -> String {
        let request_id = ctx.request_id();
        format!("Request ID: {} (user: {:?})", request_id, ctx.subject())
    }
}
```

The context also carries the server-to-client operations: `report_progress`,
`sample`, `elicit_form` / `elicit_url`, `list_roots`, and
`notify_resource_updated`. Long-running tools should check
`ctx.is_cancelled()`, since cancellation is cooperative.

### Server State

For other dependencies like databases, caches, or configuration, store them in your server struct. Since your server struct is `Clone`, use `Arc` for shared state.

```rust
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

struct Config {
    greeting: String,
}

#[derive(Clone)]
struct MyServer {
    db: Arc<RwLock<HashMap<String, String>>>,
    config: Arc<Config>,
}

#[server(name = "my-server")]
impl MyServer {
    #[tool]
    async fn query_db(&self, id: String) -> McpResult<String> {
        // Access state via self
        let db = self.db.read().await;
        let value = db
            .get(&id)
            .ok_or_else(|| McpError::invalid_params(format!("no record {id}")))?;
        Ok(format!("{} {value}", self.config.greeting))
    }
}
```

## Handler Return Types

### Simple Types

Tools return any type that implements `IntoToolResult` — `String`, the
numeric types, `bool`, `()`, `serde_json::Value`, `Vec<T: Serialize>`,
`Json<T>`, and `ToolResult`. Prompts return `IntoPromptResult` types (`String`,
`PromptResult`, `Vec<Message>`). Resources return `McpResult` of an
`IntoResourceResult` type (`String`, `&str`, `ResourceResult`).

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn simple(&self) -> String {
        "result".into()
    }

    #[tool]
    async fn number(&self) -> i32 {
        42
    }
}
```

### Result Type

Use `McpResult<T>` (alias for `Result<T, McpError>`) to handle errors gracefully.
A tool's error reaches the client as a tool execution error (`isError: true`)
that the model can read and correct, with the error kind in `_meta`. A
resource's or prompt's error is a JSON-RPC error.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct MyServer;

#[server]
impl MyServer {
    #[tool]
    async fn operation(&self, value: i32) -> McpResult<String> {
        if value < 0 {
            return Err(McpError::invalid_params("Must be positive"));
        }
        Ok("Success".into())
    }
}
```

### Binary Data

A resource returns binary data as a `ResourceResult` with base64-encoded blob
contents (add `base64 = "0.22"` to your dependencies for the encoding):

```rust
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use turbomcp::prelude::*;

#[derive(Clone)]
struct Images;

#[server]
impl Images {
    #[resource("image://{name}", mime_type = "image/png")]
    async fn read_image(&self, uri: String, ctx: &RequestContext) -> McpResult<ResourceResult> {
        let name = uri.trim_start_matches("image://");
        let data = std::fs::read(format!("images/{name}.png"))
            .map_err(|e| McpError::resource_not_found(e.to_string()))?;
        Ok(ResourceResult::binary(uri, STANDARD.encode(data), "image/png"))
    }
}
```

## Optional Handlers

Five markers opt into MCP methods a server may leave out. Each may appear at
most once, and each advertises the capability it serves:

- `#[completion]` — `completion/complete`; advertises `completions`
- `#[subscribe]` and `#[unsubscribe]` — `resources/subscribe` and
  `resources/unsubscribe`; advertises `resources.subscribe`. Declaring one
  without the other is a compile error.
- `#[set_level]` — observes `logging/setLevel`. Every server advertises
  `logging` and records the level without it.
- `#[roots_changed]` — runs on `notifications/roots/list_changed`

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Docs;

#[server(name = "docs", version = "1.0.0")]
impl Docs {
    #[resource("docs://{page}")]
    async fn page(&self, uri: String, ctx: &RequestContext) -> McpResult<String> {
        Ok(format!("contents of {uri}"))
    }

    #[completion]
    async fn complete(&self, params: serde_json::Value) -> McpResult<serde_json::Value> {
        let prefix = params["argument"]["value"].as_str().unwrap_or("");
        let values: Vec<&str> = ["intro", "install"]
            .into_iter()
            .filter(|page| page.starts_with(prefix))
            .collect();
        Ok(serde_json::json!({ "completion": { "values": values } }))
    }

    #[subscribe]
    async fn watch(&self, uri: String, ctx: &RequestContext) -> McpResult<()> {
        Ok(())
    }

    #[unsubscribe]
    async fn unwatch(&self, uri: String, ctx: &RequestContext) -> McpResult<()> {
        Ok(())
    }
}
```

`#[server(page_size = N)]` paginates the list methods at `N` entries.

## Error Handling

TurboMCP provides standard error constructors on `McpError`:

```rust
use turbomcp::McpError;

let errors = [
    McpError::invalid_params("Invalid input"),
    McpError::internal("Database failed"),
    McpError::tool_not_found("Tool missing"),
    McpError::resource_not_found("file:///missing.txt"),
];
```

## Next Steps

- **[Examples](../examples/basic.md)** - Real-world handlers
- **[Transports](transports.md)** - Configuring HTTP/TCP transports
