# turbomcp-wasm-macros

Zero-boilerplate procedural macros for building MCP servers in WASM environments like Cloudflare Workers, Deno Deploy, and other edge platforms.

## Features

- **`#[server]`** - Transform impl blocks into MCP servers
- **`#[tool]`** - Mark methods as MCP tool handlers
- **`#[resource]`** - Mark methods as MCP resource handlers
- **`#[prompt]`** - Mark methods as MCP prompt handlers

## Usage

Add `turbomcp-wasm` with the `macros` feature to your `Cargo.toml`:

```toml
[dependencies]
turbomcp-wasm = { version = "3.0", default-features = false, features = ["macros"] }
worker = "0.7"
serde = { version = "1.0", features = ["derive"] }
schemars = "1.0"
```

Then define your server:

```rust
use turbomcp_wasm::prelude::*;
use serde::Deserialize;

#[derive(Clone)]
struct MyServer {
    greeting: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct GreetArgs {
    /// The name of the person to greet
    name: String,
}

#[server(name = "my-server", version = "1.0.0", description = "My MCP server")]
impl MyServer {
    #[tool("Greet someone by name")]
    async fn greet(&self, args: GreetArgs) -> String {
        format!("{}, {}!", self.greeting, args.name)
    }

    #[tool("Get server status")]
    async fn status(&self) -> String {
        "Server is running".to_string()
    }

    #[resource("config://app")]
    async fn config(&self, uri: String) -> ResourceResult {
        ResourceResult::text(&uri, r#"{"theme": "dark"}"#)
    }

    #[prompt("Default greeting")]
    async fn greeting_prompt(&self) -> PromptResult {
        PromptResult::user("Hello! How can I help?")
    }
}

// In your Cloudflare Worker handler:
#[event(fetch)]
async fn fetch(req: Request, _env: Env, _ctx: Context) -> Result<Response> {
    let server = MyServer { greeting: "Hello".into() };
    server.into_mcp_server().handle(req).await
}
```

## Generated Methods

The `#[server]` macro generates the following methods on your struct:

- **`into_mcp_server(self) -> McpServer`** - Convert to a fully-configured MCP server (returns `turbomcp_wasm::wasm_server::McpServer`)
- **`server_info() -> (&'static str, &'static str)`** - Get (name, version) tuple
- **`get_tools_metadata() -> Vec<(&'static str, &'static str, &'static [&'static str], &'static str)>`** - Get tool metadata as (name, description, tags, version) tuples
- **`get_resources_metadata() -> Vec<(&'static str, &'static str, &'static [&'static str], &'static str)>`** - Get resource metadata as (uri_template, name, tags, version) tuples
- **`get_prompts_metadata() -> Vec<(&'static str, &'static str, &'static [&'static str], &'static str)>`** - Get prompt metadata as (name, description, tags, version) tuples
- **`get_tool_tags() -> Vec<(&'static str, Vec<String>)>`** - Tool name to tags mapping (for `VisibilityLayer` integration)
- **`get_resource_tags() -> Vec<(&'static str, Vec<String>)>`** - Resource URI to tags mapping
- **`get_prompt_tags() -> Vec<(&'static str, Vec<String>)>`** - Prompt name to tags mapping

## Attribute Syntax

### `#[server]`

```rust
#[server(name = "my-server", version = "1.0.0", description = "Optional description")]
impl MyServer { ... }
```

### `#[tool]`

```rust
#[tool("Description of the tool")]
async fn my_tool(&self, args: MyArgs) -> String { ... }

// Or without arguments
#[tool("Description")]
async fn no_args_tool(&self) -> String { ... }

// Full syntax with tags and version (tags feed VisibilityLayer)
#[tool(description = "Admin tool", tags = ["admin", "dangerous"], version = "2.0")]
async fn admin_op(&self, args: AdminArgs) -> Result<String, ToolError> { ... }

// Context injection: add `ctx: Arc<RequestContext>` before args
#[tool("Auth required")]
async fn auth_tool(
    &self,
    ctx: std::sync::Arc<RequestContext>,
    args: AuthArgs,
) -> Result<String, ToolError> { ... }
```

### `#[resource]`

```rust
#[resource("config://app")]
async fn config(&self, uri: String) -> ResourceResult { ... }

// With URI template
#[resource("file://{path}")]
async fn file(&self, uri: String) -> ResourceResult { ... }
```

### `#[prompt]`

```rust
#[prompt("Description of the prompt")]
async fn my_prompt(&self) -> PromptResult { ... }

// With optional arguments
#[prompt("Prompt with args")]
async fn prompt_with_args(&self, args: Option<MyArgs>) -> PromptResult { ... }
```

## Parameter Descriptions

Add descriptions to tool parameters using doc comments on your args struct fields:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
struct SearchArgs {
    /// The search query to execute
    query: String,
    /// Maximum number of results (default: 10)
    limit: Option<u32>,
}
```

Alternatively, use the `#[schemars(description = "...")]` attribute:

```rust
#[derive(Deserialize, schemars::JsonSchema)]
struct SearchArgs {
    #[schemars(description = "The search query to execute")]
    query: String,
}
```

These descriptions appear in the JSON schema and help LLMs understand how to use your tools.

## Requirements

Your struct must implement `Clone` for the generated code to work, as handlers need to clone the server instance.

## License

MIT
