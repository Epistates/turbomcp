# turbomcp-openapi

OpenAPI to MCP conversion for TurboMCP. Expose REST APIs as MCP tools and resources.

## Overview

This crate allows you to automatically convert an OpenAPI 3.0.x specification (via the `openapiv3` crate) into MCP (Model Context Protocol) tools and resources. This enables AI agents to interact with REST APIs without writing custom handlers.

**Default mapping:**
- `GET` endpoints → MCP Resources (readable content); those with path
  parameters, such as `/users/{id}`, → MCP Resource Templates
- `POST`, `PUT`, `PATCH`, `DELETE` endpoints → MCP Tools (callable operations)

## Quick Start

```rust
use std::path::Path;
use std::time::Duration;
use turbomcp_openapi::{OpenApiHandler, OpenApiProvider};
use turbomcp_server::{ServerBuilder, Transport};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load from URL
    let provider = OpenApiProvider::from_url("https://api.example.com/openapi.json")
        .await?
        .with_base_url("https://api.example.com")?
        .with_timeout(Duration::from_secs(30));  // Optional, 30s default

    // Or load from file
    let provider = OpenApiProvider::from_file(Path::new("openapi.yaml"))?
        .with_base_url("https://api.example.com")?;

    // Or load from string
    let spec_json = std::fs::read_to_string("openapi.json")?;
    let provider = OpenApiProvider::from_string(&spec_json)?
        .with_base_url("https://api.example.com")?;

    // Convert to an McpHandler and serve it with any transport
    let handler: OpenApiHandler = provider.into_handler();
    ServerBuilder::new(handler)
        .transport(Transport::stdio())
        .serve()
        .await?;
    Ok(())
}
```

## Security Features

### SSRF Protection

The provider includes built-in Server-Side Request Forgery (SSRF) protection that blocks requests to:

- **Localhost/loopback**: `127.0.0.0/8`, `::1`, `::`, `localhost`
- **Private networks**: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`, `fc00::/7`
- **Cloud metadata endpoints**: `169.254.169.254` and `169.254.0.0/16`
- **Link-local addresses**: `fe80::/10`
- **Addresses that embed IPv4**: IPv4-mapped (judged by the IPv4 rules),
  IPv4-compatible, NAT64 and 6to4
- **Other reserved ranges**: carrier-grade NAT, `0.0.0.0/8`, multicast,
  broadcast, documentation ranges, etc.

A hostname is refused if it does not resolve, or if any address it resolves
to is blocked. The built-in HTTP client connects only to the addresses it
validated, so a name cannot answer differently at connect time (DNS
rebinding), and every redirect is checked before it is followed.

A client supplied with `with_client` still has each URL and its resolved
addresses checked before the request is sent, but it resolves the name again
to connect and follows redirects by its own policy. The same applies to
requests sent through an HTTP proxy, which resolves names itself.

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

- **OpenAPI 3.0.x Support** - Parse both JSON and YAML specifications (via `openapiv3`)
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
    pub fn with_auth_provider(self, provider: Arc<dyn AuthProvider>) -> Self;
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

Implements `turbomcp_core::handler::McpHandler` for use with TurboMCP servers.
The handler exposes:

- `server_info()` — returns the spec's `info.title` / `info.version`
- `list_tools()` — one `Tool` per non-GET operation (subject to route mapping); `meta` includes the original `method`, `path`, and `operationId`
- `list_resources()` — one `Resource` per GET operation without path parameters (subject to route mapping); `mime_type` is `application/json`
- `list_resource_templates()` — one `ResourceTemplate` per GET operation with path parameters
- `list_prompts()` — always empty (OpenAPI has no prompt concept)
- `call_tool(name, args, ctx)` — dispatches an HTTP request for the matching tool, with SSRF validation
- `read_resource(uri, ctx)` — issues the GET for the matching resource URI, or for the template it instantiates, taking the path parameters from the URI
- `get_prompt(...)` — always returns `prompt_not_found`

Tool names use `operation_id` when present, otherwise `{method}_{path}` with `/`
replaced by `_` and `{}` stripped. Any character the MCP tool-name rules don't
allow (`A-Z a-z 0-9 _ - .`) becomes `_`, and names are capped at 128
characters. If several operations end up wanting one name, the first in spec
order keeps it and the others take the first free `_2`, `_3`, … suffix.

Resource URIs are `openapi://{method}{path}` (e.g. `openapi://get/users`); for
an operation with path parameters that is a URI template
(`openapi://get/users/{id}`), read as `openapi://get/users/42`. A concrete
resource wins over a template that also matches. Values containing `/`, `%` or
`..` do not match a template.

### Tool results

- The response body is returned as text: JSON re-indented, anything else as
  sent.
- A tool declares an `outputSchema` only when its first 2xx `application/json`
  response is an object schema, since MCP requires `outputSchema` to be an
  object. Such a tool also returns the body as `structuredContent`; if the
  upstream answers with something other than a JSON object, the call is a tool
  error carrying the body.
- A failed call is a tool result with `isError: true`, the way the model gets
  to see it: a non-2xx status (with the body), an unreachable upstream, a
  timeout, or a request the SSRF protection refused. A missing required or
  invalid argument is reported the same way, classified as invalid params
  (`io.turbomcp/errorCode: -32602` in `_meta`), and nothing is sent. Only a
  missing base URL is a JSON-RPC error.
- Path parameters are percent-encoded as a single path segment and appended to
  the base URL's own path, so `https://api.example.com/v1` stays the prefix.
  Header parameters are sent as headers and cookie parameters in one `Cookie`
  header. The request body is required only if `requestBody.required` says so.

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

**Resource templates:**
- `openapi://get/pets/{id}` (getPet)

**Tools:**
- `createPet` - Create a pet
- `deletePet` - Delete a pet

## Running the Example

```bash
cargo run -p turbomcp-openapi --example petstore
```

## Schema Handling Notes

- **JSON Schema 2020-12**: tool schemas are 2020-12 (MCP's default dialect),
  converted from OpenAPI 3.0: `nullable: true` becomes a `"null"` type (or an
  `anyOf` with `null` when there is no `type`), boolean
  `exclusiveMinimum`/`exclusiveMaximum` become numeric bounds, `example`
  becomes `examples`, and OpenAPI-only keywords (`discriminator`, `xml`,
  `externalDocs`, `x-*`) are dropped.
- **`$ref` resolution**: references into `components.schemas` are inlined, so
  clients that don't resolve `$ref` still see the whole shape. A recursive
  schema is written once into the root `$defs` and referenced as
  `#/$defs/Name`. A reference that resolves to nothing becomes the empty
  schema, so consumers never see dangling pointers.
- **Schema composition**: `allOf`, `oneOf` and `anyOf` pass through.
- **Parameter `content`**: only `schema`-form parameters are extracted; the
  content-type variant form is not yet converted.
- **Security schemes**: declared schemes are surfaced in each tool's and
  resource's `_meta`; install an `AuthProvider` with `with_auth_provider` to
  apply credentials to outgoing requests.

## License

MIT
