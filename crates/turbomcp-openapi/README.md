# turbomcp-openapi

OpenAPI to MCP conversion for TurboMCP. Expose REST APIs as MCP tools and resources.

## Overview

This crate allows you to automatically convert an OpenAPI 3.x specification into MCP (Model Context Protocol) tools and resources. This enables AI agents to interact with REST APIs without writing custom handlers.

**Default mapping:**
- `GET` endpoints → MCP Resources (readable content)
- `POST`, `PUT`, `PATCH`, `DELETE` endpoints → MCP Tools (callable operations)

## Quick Start

```rust
use turbomcp_openapi::{OpenApiProvider, OpenApiHandler};
use turbomcp_server::McpServer;
use std::time::Duration;

// Load from URL
let provider = OpenApiProvider::from_url("https://api.example.com/openapi.json")
    .await?
    .with_base_url("https://api.example.com")?
    .with_timeout(Duration::from_secs(30));  // Optional, 30s default

// Or load from file
let provider = OpenApiProvider::from_file(Path::new("openapi.yaml"))?
    .with_base_url("https://api.example.com")?;

// Or load from string
let provider = OpenApiProvider::from_string(spec_json)?
    .with_base_url("https://api.example.com")?;

// Convert to MCP handler
let handler = provider.into_handler();

// Use with TurboMCP server
let server = McpServer::from_handler(handler);
```

## Security Features

### SSRF Protection

The provider includes built-in Server-Side Request Forgery (SSRF) protection that blocks requests to:

- **Localhost/loopback**: `127.0.0.0/8`, `::1`, `localhost`
- **Private networks**: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
- **Cloud metadata endpoints**: `169.254.169.254` and `169.254.0.0/16`
- **Link-local addresses**: `fe80::/10`
- **Other reserved ranges**: Multicast, broadcast, etc.

This prevents malicious API specs from making requests to internal infrastructure.

### Request Timeouts

All HTTP requests have a configurable timeout (default: 30 seconds) to prevent:
- Slowloris attacks
- Indefinite hangs on unresponsive servers
- Resource exhaustion

```rust
use std::time::Duration;

let provider = OpenApiProvider::from_string(spec)?
    .with_base_url("https://api.example.com")?
    .with_timeout(Duration::from_secs(10));  // 10 second timeout

// Check current timeout
println!("Timeout: {:?}", provider.timeout());
```

## Custom Route Mapping

You can customize how OpenAPI operations map to MCP types:

```rust
use turbomcp_openapi::{OpenApiProvider, RouteMapping, McpType};

let mapping = RouteMapping::new()
    // Default: GET -> Resource
    .map_method("GET", McpType::Resource)
    // Custom: All /admin/* paths are skipped
    .map_pattern(r"/admin/.*", McpType::Skip)?
    // Custom: Force specific paths to be tools
    .map_rule(["GET"], r"/api/search.*", McpType::Tool, 10)?;

let provider = OpenApiProvider::from_string(spec)?
    .with_route_mapping(mapping)
    .with_base_url("https://api.example.com")?;
```

## Route Mapping Rules

Rules are evaluated in priority order (highest first):

```rust
use turbomcp_openapi::{RouteRule, McpType};

// Create a rule matching POST/PUT to /users/* paths
let rule = RouteRule::new(McpType::Tool)
    .methods(["POST", "PUT"])
    .pattern(r"/users/\d+")?
    .priority(100);  // Higher priority = checked first
```

### McpType Variants

- `McpType::Tool` - Expose as MCP tool (callable operation)
- `McpType::Resource` - Expose as MCP resource (readable content)
- `McpType::Skip` - Don't expose via MCP

## Features

- **OpenAPI 3.x Support** - Parse both JSON and YAML specifications
- **Multiple Loading Methods** - From URL, file path, or string
- **Configurable Mapping** - Customize how operations map to MCP types
- **Regex Pattern Matching** - Route rules support regex path patterns
- **Parameter Handling** - Path, query, header, and cookie parameters
- **Request Body Support** - JSON request bodies converted to tool inputs
- **HTTP Client Integration** - Built-in reqwest client for API calls
- **Custom Client Support** - Provide your own configured reqwest::Client
- **SSRF Protection** - Built-in protection against server-side request forgery
- **Request Timeouts** - Configurable timeouts (default: 30 seconds)

## API Reference

### OpenApiProvider

The main entry point for loading and configuring OpenAPI specs:

```rust
impl OpenApiProvider {
    // Loading methods
    pub fn from_spec(spec: OpenAPI) -> Self;
    pub fn from_string(content: &str) -> Result<Self>;
    pub fn from_file(path: &Path) -> Result<Self>;
    pub async fn from_url(url: &str) -> Result<Self>;

    // Configuration
    pub fn with_base_url(self, base_url: &str) -> Result<Self>;
    pub fn with_route_mapping(self, mapping: RouteMapping) -> Self;
    pub fn with_client(self, client: reqwest::Client) -> Self;
    pub fn with_timeout(self, timeout: Duration) -> Self;

    // Inspection
    pub fn title(&self) -> &str;
    pub fn version(&self) -> &str;
    pub fn timeout(&self) -> Duration;
    pub fn operations(&self) -> &[ExtractedOperation];
    pub fn tools(&self) -> impl Iterator<Item = &ExtractedOperation>;
    pub fn resources(&self) -> impl Iterator<Item = &ExtractedOperation>;

    // Conversion
    pub fn into_handler(self) -> OpenApiHandler;
}
```

### OpenApiHandler

Implements `McpHandler` for use with TurboMCP servers:

```rust
impl McpHandler for OpenApiHandler {
    fn server_info(&self) -> ServerInfo;
    fn list_tools(&self) -> Vec<Tool>;
    fn list_resources(&self) -> Vec<Resource>;
    fn list_prompts(&self) -> Vec<Prompt>;
    async fn call_tool(&self, name: &str, args: Value, ctx: &RequestContext) -> McpResult<ToolResult>;
    async fn read_resource(&self, uri: &str, ctx: &RequestContext) -> McpResult<ResourceResult>;
    async fn get_prompt(&self, name: &str, args: Option<Value>, ctx: &RequestContext) -> McpResult<PromptResult>;
}
```

## Error Types

```rust
pub enum OpenApiError {
    FetchError(reqwest::Error),     // Failed to fetch spec from URL
    ParseError(String),              // Failed to parse spec (JSON/YAML)
    IoError(std::io::Error),        // Failed to read file
    InvalidUrl(url::ParseError),     // Invalid URL
    InvalidPattern(regex::Error),    // Invalid regex pattern
    ApiError(String),                // API call returned error
    MissingParameter(String),        // Required parameter missing
    InvalidParameter(String, String), // Invalid parameter value
    OperationNotFound(String),       // Operation not found
    NoBaseUrl,                       // Base URL not configured
    SsrfBlocked(String),            // SSRF protection blocked request
    Timeout(u64),                    // Request timed out
}
```

## Example

Given this OpenAPI spec:

```yaml
openapi: 3.0.0
info:
  title: Pet Store
  version: 1.0.0
paths:
  /pets:
    get:
      operationId: listPets
      summary: List all pets
    post:
      operationId: createPet
      summary: Create a pet
  /pets/{id}:
    get:
      operationId: getPet
      summary: Get a pet by ID
    delete:
      operationId: deletePet
      summary: Delete a pet
```

The handler exposes:

**Resources:**
- `openapi://get/pets` (listPets)
- `openapi://get/pets/{id}` (getPet)

**Tools:**
- `createPet` - Create a pet
- `deletePet` - Delete a pet

## Running the Example

```bash
cargo run -p turbomcp-openapi --example petstore
```

## License

MIT
