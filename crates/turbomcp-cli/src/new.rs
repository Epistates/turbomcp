//! New project command implementation.
//!
//! Creates new MCP server projects from templates with proper configuration
//! for various deployment targets including Cloudflare Workers.

use crate::cli::{NewArgs, ProjectTemplate};
use crate::error::{CliError, CliResult};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Execute the new project command.
pub fn execute(args: &NewArgs) -> CliResult<()> {
    // Determine output directory
    let output_dir = args
        .output
        .clone()
        .unwrap_or_else(|| PathBuf::from(&args.name));

    // Check if directory already exists
    if output_dir.exists() {
        return Err(CliError::Other(format!(
            "Directory '{}' already exists",
            output_dir.display()
        )));
    }

    println!("Creating new MCP server project '{}'...", args.name);
    println!("  Template: {}", args.template);

    // Create project directory
    fs::create_dir_all(&output_dir)
        .map_err(|e| CliError::Other(format!("Failed to create directory: {}", e)))?;

    // Generate project files based on template
    match args.template {
        ProjectTemplate::Minimal => generate_minimal(args, &output_dir)?,
        ProjectTemplate::Full => generate_full(args, &output_dir)?,
        ProjectTemplate::CloudflareWorkers => generate_cloudflare_workers(args, &output_dir)?,
        ProjectTemplate::CloudflareWorkersOauth => {
            generate_cloudflare_workers_oauth(args, &output_dir)?
        }
        ProjectTemplate::CloudflareWorkersDurableObjects => {
            generate_cloudflare_workers_do(args, &output_dir)?
        }
    }

    // Initialize git repository if requested
    if args.git {
        init_git(&output_dir)?;
    }

    println!("\nProject created successfully!");
    println!("\nNext steps:");
    println!("  cd {}", args.name);

    match args.template {
        ProjectTemplate::CloudflareWorkers
        | ProjectTemplate::CloudflareWorkersOauth
        | ProjectTemplate::CloudflareWorkersDurableObjects => {
            println!("  npx wrangler dev    # Start local development");
            println!("  npx wrangler deploy # Deploy to Cloudflare");
        }
        _ => {
            println!("  cargo build         # Build the server");
            println!("  cargo run           # Run the server");
        }
    }

    Ok(())
}

/// Generate a minimal MCP server project.
fn generate_minimal(args: &NewArgs, output_dir: &Path) -> CliResult<()> {
    let description = args
        .description
        .as_deref()
        .unwrap_or("A minimal MCP server");

    // Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
description = "{description}"
{author}

[dependencies]
turbomcp = "3.0"
tokio = {{ version = "1", features = ["full"] }}
serde = {{ version = "1", features = ["derive"] }}
schemars = "0.8"
"#,
        name = args.name,
        description = description,
        author = args
            .author
            .as_ref()
            .map(|a| format!("authors = [\"{}\"]", a))
            .unwrap_or_default(),
    );

    // src/main.rs
    let main_rs = format!(
        r#"//! {description}

use turbomcp::prelude::*;
use serde::Deserialize;

#[derive(Clone)]
struct {struct_name};

#[derive(Deserialize, schemars::JsonSchema)]
struct HelloArgs {{
    /// Name to greet
    name: String,
}}

#[server(name = "{name}", version = "0.1.0")]
impl {struct_name} {{
    /// Say hello to someone
    #[tool("Say hello to someone")]
    async fn hello(&self, args: HelloArgs) -> String {{
        format!("Hello, {{}}!", args.name)
    }}
}}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {{
    let server = {struct_name};
    server.serve_stdio().await?;
    Ok(())
}}
"#,
        description = description,
        struct_name = to_struct_name(&args.name),
        name = args.name,
    );

    // Write files
    write_file(output_dir, "Cargo.toml", &cargo_toml)?;
    fs::create_dir_all(output_dir.join("src"))?;
    write_file(&output_dir.join("src"), "main.rs", &main_rs)?;

    // .gitignore
    write_file(output_dir, ".gitignore", "/target\n")?;

    Ok(())
}

/// Generate a full-featured MCP server project.
fn generate_full(args: &NewArgs, output_dir: &Path) -> CliResult<()> {
    let description = args
        .description
        .as_deref()
        .unwrap_or("A full-featured MCP server");

    // Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
description = "{description}"
{author}

[dependencies]
turbomcp = {{ version = "3.0", features = ["http", "auth"] }}
tokio = {{ version = "1", features = ["full"] }}
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
schemars = "0.8"
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}
"#,
        name = args.name,
        description = description,
        author = args
            .author
            .as_ref()
            .map(|a| format!("authors = [\"{}\"]", a))
            .unwrap_or_default(),
    );

    // src/main.rs
    let main_rs = format!(
        r#"//! {description}

use turbomcp::prelude::*;
use serde::{{Deserialize, Serialize}};
use tracing_subscriber::{{layer::SubscriberExt, util::SubscriberInitExt}};

#[derive(Clone)]
struct {struct_name} {{
    config: ServerConfig,
}}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ServerConfig {{
    name: String,
}}

// Tool argument types
#[derive(Deserialize, schemars::JsonSchema)]
struct HelloArgs {{
    /// Name to greet
    name: String,
}}

#[derive(Deserialize, schemars::JsonSchema)]
struct CalculateArgs {{
    /// First number
    a: f64,
    /// Second number
    b: f64,
    /// Operation to perform
    operation: String,
}}

// Prompt argument types
#[derive(Deserialize, schemars::JsonSchema)]
struct GreetingArgs {{
    /// User's name
    name: String,
    /// Tone of greeting
    tone: Option<String>,
}}

#[server(
    name = "{name}",
    version = "0.1.0",
    description = "{description}"
)]
impl {struct_name} {{
    /// Say hello to someone
    #[tool("Say hello to someone")]
    async fn hello(&self, args: HelloArgs) -> String {{
        format!("Hello, {{}}!", args.name)
    }}

    /// Perform a calculation
    #[tool("Perform basic arithmetic")]
    async fn calculate(&self, args: CalculateArgs) -> Result<String, ToolError> {{
        let result = match args.operation.as_str() {{
            "add" => args.a + args.b,
            "subtract" => args.a - args.b,
            "multiply" => args.a * args.b,
            "divide" => {{
                if args.b == 0.0 {{
                    return Err(ToolError::new("Cannot divide by zero"));
                }}
                args.a / args.b
            }}
            _ => return Err(ToolError::new(format!(
                "Unknown operation: {{}}. Use: add, subtract, multiply, divide",
                args.operation
            ))),
        }};
        Ok(format!("{{}} {{}} {{}} = {{}}", args.a, args.operation, args.b, result))
    }}

    /// Server configuration
    #[resource("config://server")]
    async fn config(&self, _uri: String) -> ResourceResult {{
        ResourceResult::json(
            "config://server",
            &self.config,
        ).map_err(|e| ResourceError::new(e.to_string()))
         .unwrap_or_else(|e| ResourceResult::text("config://server", &format!("Error: {{}}", e)))
    }}

    /// Greeting prompt
    #[prompt("Generate a greeting")]
    async fn greeting(&self, args: GreetingArgs) -> PromptResult {{
        let tone = args.tone.as_deref().unwrap_or("friendly");
        PromptResult::user(format!(
            "Generate a {{}} greeting for {{}}.",
            tone,
            args.name
        ))
    }}
}}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {{
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let server = {struct_name} {{
        config: ServerConfig {{
            name: "{name}".to_string(),
        }},
    }};

    // Choose transport based on environment
    if std::env::var("MCP_HTTP").is_ok() {{
        tracing::info!("Starting HTTP server on port 8080...");
        server.serve_http(8080).await?;
    }} else {{
        tracing::info!("Starting STDIO server...");
        server.serve_stdio().await?;
    }}

    Ok(())
}}
"#,
        description = description,
        struct_name = to_struct_name(&args.name),
        name = args.name,
    );

    // Write files
    write_file(output_dir, "Cargo.toml", &cargo_toml)?;
    fs::create_dir_all(output_dir.join("src"))?;
    write_file(&output_dir.join("src"), "main.rs", &main_rs)?;

    // .gitignore
    write_file(output_dir, ".gitignore", "/target\n")?;

    Ok(())
}

/// Generate a Cloudflare Workers MCP server project.
fn generate_cloudflare_workers(args: &NewArgs, output_dir: &Path) -> CliResult<()> {
    let description = args
        .description
        .as_deref()
        .unwrap_or("An MCP server for Cloudflare Workers");

    // Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
description = "{description}"
{author}

[lib]
crate-type = ["cdylib"]

[dependencies]
turbomcp-wasm = {{ version = "3.0", features = ["macros", "streamable"] }}
worker = "0.5"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
schemars = "0.8"
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"

[profile.release]
opt-level = "s"
lto = true
"#,
        name = args.name,
        description = description,
        author = args
            .author
            .as_ref()
            .map(|a| format!("authors = [\"{}\"]", a))
            .unwrap_or_default(),
    );

    // wrangler.toml
    let wrangler_toml = format!(
        r#"name = "{name}"
main = "build/worker/shim.mjs"
compatibility_date = "2024-01-01"

[build]
command = "cargo install -q worker-build && worker-build --release"

# Uncomment for KV storage
# [[kv_namespaces]]
# binding = "MY_KV"
# id = "your-kv-namespace-id"

# Uncomment for Durable Objects
# [[durable_objects.bindings]]
# name = "MCP_STATE"
# class_name = "McpState"
"#,
        name = args.name,
    );

    // src/lib.rs
    let lib_rs = format!(
        r#"//! {description}

use turbomcp_wasm::prelude::*;
use serde::Deserialize;
use worker::*;

#[derive(Clone)]
struct {struct_name};

#[derive(Deserialize, schemars::JsonSchema)]
struct HelloArgs {{
    /// Name to greet
    name: String,
}}

#[server(name = "{name}", version = "0.1.0")]
impl {struct_name} {{
    /// Say hello to someone
    #[tool("Say hello to someone")]
    async fn hello(&self, args: HelloArgs) -> String {{
        format!("Hello, {{}}!", args.name)
    }}

    /// Get server status
    #[tool("Check server health")]
    async fn status(&self) -> String {{
        "OK".to_string()
    }}
}}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {{
    console_error_panic_hook::set_once();

    let server = {struct_name};
    let mcp = server.into_mcp_server();

    mcp.handle(req).await
}}
"#,
        description = description,
        struct_name = to_struct_name(&args.name),
        name = args.name,
    );

    // Write files
    write_file(output_dir, "Cargo.toml", &cargo_toml)?;
    write_file(output_dir, "wrangler.toml", &wrangler_toml)?;
    fs::create_dir_all(output_dir.join("src"))?;
    write_file(&output_dir.join("src"), "lib.rs", &lib_rs)?;

    // .gitignore
    write_file(output_dir, ".gitignore", "/target\n/build\n/node_modules\n")?;

    Ok(())
}

/// Generate a Cloudflare Workers MCP server with OAuth 2.1.
fn generate_cloudflare_workers_oauth(args: &NewArgs, output_dir: &Path) -> CliResult<()> {
    let description = args
        .description
        .as_deref()
        .unwrap_or("An MCP server for Cloudflare Workers with OAuth 2.1");

    // Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
description = "{description}"
{author}

[lib]
crate-type = ["cdylib"]

[dependencies]
turbomcp-wasm = {{ version = "3.0", features = ["macros", "streamable", "auth"] }}
worker = "0.5"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
schemars = "0.8"
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"

[profile.release]
opt-level = "s"
lto = true
"#,
        name = args.name,
        description = description,
        author = args
            .author
            .as_ref()
            .map(|a| format!("authors = [\"{}\"]", a))
            .unwrap_or_default(),
    );

    // wrangler.toml
    let wrangler_toml = format!(
        r#"name = "{name}"
main = "build/worker/shim.mjs"
compatibility_date = "2024-01-01"

[build]
command = "cargo install -q worker-build && worker-build --release"

# OAuth token storage
[[kv_namespaces]]
binding = "OAUTH_TOKENS"
id = "your-kv-namespace-id"

# Secrets (set via wrangler secret put)
# JWT_SECRET - Secret for signing JWT tokens
# OAUTH_CLIENT_SECRET - OAuth client secret
"#,
        name = args.name,
    );

    // src/lib.rs
    let lib_rs = format!(
        r#"//! {description}

use turbomcp_wasm::prelude::*;
use turbomcp_wasm::wasm_server::{{WithAuth, AuthExt}};
use serde::Deserialize;
use worker::*;
use std::sync::Arc;

#[derive(Clone)]
struct {struct_name};

#[derive(Deserialize, schemars::JsonSchema)]
struct HelloArgs {{
    /// Name to greet
    name: String,
}}

#[server(name = "{name}", version = "0.1.0")]
impl {struct_name} {{
    /// Say hello to someone (requires authentication)
    #[tool("Say hello to someone")]
    async fn hello(&self, ctx: Arc<RequestContext>, args: HelloArgs) -> Result<String, ToolError> {{
        // Check authentication
        if !ctx.is_authenticated() {{
            return Err(ToolError::new("Authentication required"));
        }}

        let user = ctx.user_id().unwrap_or("unknown");
        Ok(format!("Hello, {{}}! (authenticated as {{}})", args.name, user))
    }}

    /// Get server status (public)
    #[tool("Check server health")]
    async fn status(&self) -> String {{
        "OK".to_string()
    }}
}}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {{
    console_error_panic_hook::set_once();

    let server = {struct_name};
    let mcp = server
        .into_mcp_server()
        .with_jwt_auth(env.secret("JWT_SECRET")?.to_string());

    mcp.handle(req).await
}}
"#,
        description = description,
        struct_name = to_struct_name(&args.name),
        name = args.name,
    );

    // Write files
    write_file(output_dir, "Cargo.toml", &cargo_toml)?;
    write_file(output_dir, "wrangler.toml", &wrangler_toml)?;
    fs::create_dir_all(output_dir.join("src"))?;
    write_file(&output_dir.join("src"), "lib.rs", &lib_rs)?;

    // .gitignore
    write_file(output_dir, ".gitignore", "/target\n/build\n/node_modules\n")?;

    Ok(())
}

/// Generate a Cloudflare Workers MCP server with Durable Objects.
fn generate_cloudflare_workers_do(args: &NewArgs, output_dir: &Path) -> CliResult<()> {
    let description = args
        .description
        .as_deref()
        .unwrap_or("An MCP server for Cloudflare Workers with Durable Objects");

    // Cargo.toml
    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2024"
description = "{description}"
{author}

[lib]
crate-type = ["cdylib"]

[dependencies]
turbomcp-wasm = {{ version = "3.0", features = ["macros", "streamable"] }}
worker = "0.5"
serde = {{ version = "1", features = ["derive"] }}
serde_json = "1"
schemars = "0.8"
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"

[profile.release]
opt-level = "s"
lto = true
"#,
        name = args.name,
        description = description,
        author = args
            .author
            .as_ref()
            .map(|a| format!("authors = [\"{}\"]", a))
            .unwrap_or_default(),
    );

    // wrangler.toml
    let wrangler_toml = format!(
        r#"name = "{name}"
main = "build/worker/shim.mjs"
compatibility_date = "2024-01-01"

[build]
command = "cargo install -q worker-build && worker-build --release"

# Durable Objects for session and state management
[[durable_objects.bindings]]
name = "MCP_SESSIONS"
class_name = "McpSession"

[[durable_objects.bindings]]
name = "MCP_STATE"
class_name = "McpState"

[[durable_objects.bindings]]
name = "MCP_RATE_LIMIT"
class_name = "McpRateLimit"

[[migrations]]
tag = "v1"
new_classes = ["McpSession", "McpState", "McpRateLimit"]
"#,
        name = args.name,
    );

    // src/lib.rs
    let lib_rs = format!(
        r#"//! {description}

use turbomcp_wasm::prelude::*;
use turbomcp_wasm::wasm_server::{{
    DurableObjectSessionStore,
    DurableObjectStateStore,
    DurableObjectRateLimiter,
    RateLimitConfig,
    StreamableHandler,
}};
use serde::{{Deserialize, Serialize}};
use worker::*;
use std::sync::Arc;

#[derive(Clone)]
struct {struct_name} {{
    state_store: DurableObjectStateStore,
}}

#[derive(Deserialize, schemars::JsonSchema)]
struct HelloArgs {{
    /// Name to greet
    name: String,
}}

#[derive(Deserialize, schemars::JsonSchema)]
struct SaveArgs {{
    /// Key to save under
    key: String,
    /// Value to save
    value: String,
}}

#[derive(Deserialize, schemars::JsonSchema)]
struct LoadArgs {{
    /// Key to load
    key: String,
}}

#[server(name = "{name}", version = "0.1.0")]
impl {struct_name} {{
    /// Say hello to someone
    #[tool("Say hello to someone")]
    async fn hello(&self, args: HelloArgs) -> String {{
        format!("Hello, {{}}!", args.name)
    }}

    /// Save a value to persistent state
    #[tool("Save a value to persistent storage")]
    async fn save(&self, ctx: Arc<RequestContext>, args: SaveArgs) -> Result<String, ToolError> {{
        let session_id = ctx.session_id().unwrap_or("default");

        self.state_store
            .set(session_id, &args.key, &args.value)
            .await
            .map_err(|e| ToolError::new(format!("Failed to save: {{}}", e)))?;

        Ok(format!("Saved '{{}}' = '{{}}'", args.key, args.value))
    }}

    /// Load a value from persistent state
    #[tool("Load a value from persistent storage")]
    async fn load(&self, ctx: Arc<RequestContext>, args: LoadArgs) -> Result<String, ToolError> {{
        let session_id = ctx.session_id().unwrap_or("default");

        let value: Option<String> = self.state_store
            .get(session_id, &args.key)
            .await
            .map_err(|e| ToolError::new(format!("Failed to load: {{}}", e)))?;

        match value {{
            Some(v) => Ok(v),
            None => Ok(format!("Key '{{}}' not found", args.key)),
        }}
    }}
}}

#[event(fetch)]
async fn fetch(req: Request, env: Env, _ctx: Context) -> Result<Response> {{
    console_error_panic_hook::set_once();

    // Initialize Durable Object stores
    let session_store = DurableObjectSessionStore::from_env(&env, "MCP_SESSIONS")?;
    let state_store = DurableObjectStateStore::from_env(&env, "MCP_STATE")?;
    let rate_limiter = DurableObjectRateLimiter::from_env(&env, "MCP_RATE_LIMIT")?
        .with_config(RateLimitConfig::per_minute(100));

    let server = {struct_name} {{ state_store }};
    let mcp = server.into_mcp_server();

    // Use Streamable HTTP with session persistence
    let handler = StreamableHandler::new(mcp)
        .with_session_store(session_store);

    handler.handle(req).await
}}

// Durable Object implementations (minimal stubs - expand as needed)
// See turbomcp_wasm::wasm_server::durable_objects for protocol documentation

#[durable_object]
pub struct McpSession {{
    state: State,
    #[allow(dead_code)]
    env: Env,
}}

#[durable_object]
impl DurableObject for McpSession {{
    fn new(state: State, env: Env) -> Self {{
        Self {{ state, env }}
    }}

    async fn fetch(&mut self, req: Request) -> Result<Response> {{
        // Handle session storage requests
        // See DurableObjectSessionStore protocol documentation
        Response::ok("{{}}")
    }}
}}

#[durable_object]
pub struct McpState {{
    state: State,
    #[allow(dead_code)]
    env: Env,
}}

#[durable_object]
impl DurableObject for McpState {{
    fn new(state: State, env: Env) -> Self {{
        Self {{ state, env }}
    }}

    async fn fetch(&mut self, req: Request) -> Result<Response> {{
        // Handle state storage requests
        // See DurableObjectStateStore protocol documentation
        Response::ok("{{}}")
    }}
}}

#[durable_object]
pub struct McpRateLimit {{
    state: State,
    #[allow(dead_code)]
    env: Env,
}}

#[durable_object]
impl DurableObject for McpRateLimit {{
    fn new(state: State, env: Env) -> Self {{
        Self {{ state, env }}
    }}

    async fn fetch(&mut self, req: Request) -> Result<Response> {{
        // Handle rate limiting requests
        // See DurableObjectRateLimiter protocol documentation
        Response::ok("{{}}")
    }}
}}
"#,
        description = description,
        struct_name = to_struct_name(&args.name),
        name = args.name,
    );

    // Write files
    write_file(output_dir, "Cargo.toml", &cargo_toml)?;
    write_file(output_dir, "wrangler.toml", &wrangler_toml)?;
    fs::create_dir_all(output_dir.join("src"))?;
    write_file(&output_dir.join("src"), "lib.rs", &lib_rs)?;

    // .gitignore
    write_file(output_dir, ".gitignore", "/target\n/build\n/node_modules\n")?;

    Ok(())
}

/// Initialize a git repository in the project directory.
fn init_git(output_dir: &Path) -> CliResult<()> {
    let status = Command::new("git")
        .arg("init")
        .current_dir(output_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("  Initialized git repository");
            Ok(())
        }
        _ => {
            println!("  Warning: Failed to initialize git repository");
            Ok(()) // Don't fail the whole operation
        }
    }
}

/// Write a file to the specified directory.
fn write_file(dir: &Path, name: &str, content: &str) -> CliResult<()> {
    let path = dir.join(name);
    fs::write(&path, content)
        .map_err(|e| CliError::Other(format!("Failed to write {}: {}", path.display(), e)))
}

/// Convert a project name to a valid Rust struct name.
fn to_struct_name(name: &str) -> String {
    let name = name
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect::<String>();

    // Convert to PascalCase
    let mut result = String::new();
    let mut capitalize_next = true;

    for c in name.chars() {
        if c == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }

    // Ensure it starts with a letter
    if result.chars().next().is_none_or(|c| c.is_ascii_digit()) {
        result = format!("Server{}", result);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_to_struct_name() {
        assert_eq!(to_struct_name("my-server"), "MyServer");
        assert_eq!(to_struct_name("hello_world"), "HelloWorld");
        assert_eq!(to_struct_name("123test"), "Server123test");
        assert_eq!(to_struct_name("simple"), "Simple");
    }

    #[test]
    fn test_template_display() {
        assert_eq!(ProjectTemplate::Minimal.to_string(), "minimal");
        assert_eq!(
            ProjectTemplate::CloudflareWorkers.to_string(),
            "cloudflare-workers"
        );
    }
}
