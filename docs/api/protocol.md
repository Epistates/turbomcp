# Protocol API Reference

Complete reference for the MCP protocol implementation in `turbomcp-protocol`.

`turbomcp-protocol` serves MCP `2025-11-25` and `2025-06-18`. Its MCP types are
re-exports of `turbomcp-types`, its error type is `turbomcp-core`'s `McpError`,
and its `RequestContext` is `turbomcp-core`'s — so values move between the
crates without conversion.

## Core Types

### Request/Response Types

The JSON-RPC 2.0 envelope types live in `turbomcp_protocol::jsonrpc` and are
re-exported at the crate root. Abridged; see
[docs.rs](https://docs.rs/turbomcp-protocol) for the full definitions:

```rust,ignore
pub struct JsonRpcRequest {
    pub jsonrpc: JsonRpcVersion,   // always serializes as "2.0"
    pub method: String,
    pub params: Option<Value>,
    pub id: RequestId,             // RequestId = MessageId (string or number)
}

pub struct JsonRpcResponse {
    pub jsonrpc: JsonRpcVersion,
    pub payload: JsonRpcResponsePayload,  // Success { result } | Error { error }
    pub id: ResponseId,                   // null only for a parse error
}

pub struct JsonRpcNotification {
    pub jsonrpc: JsonRpcVersion,
    pub method: String,
    pub params: Option<Value>,
}

pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    pub data: Option<Value>,
}
```

A response deserializes only if it has exactly one of `result` and `error`.

### Message Types

`JsonRpcMessage` is the union of the three envelopes. JSON-RPC batches are not
part of MCP, and `parse_message_typed` reports a top-level array as
`BatchUnsupported` so a server can answer `-32600`:

```rust
use turbomcp_protocol::jsonrpc::utils::{ParseMessageError, parse_message_typed};
use turbomcp_protocol::jsonrpc::JsonRpcMessage;

fn classify(json: &str) {
    match parse_message_typed(json) {
        Ok(JsonRpcMessage::Request(request)) => println!("request {}", request.method),
        Ok(JsonRpcMessage::Notification(note)) => println!("notification {}", note.method),
        Ok(JsonRpcMessage::Response(response)) => println!("response to {:?}", response.id),
        Err(ParseMessageError::BatchUnsupported) => println!("batch: answer -32600"),
        Err(ParseMessageError::Json(e)) => println!("parse error: {e}"),
    }
}
```

The MCP method names are constants in `turbomcp_protocol::methods`
(`methods::CALL_TOOL == "tools/call"`, …), and the request/result payloads are
typed in `turbomcp_protocol::types` (`CallToolRequest`, `CallToolResult`,
`ReadResourceResult`, `GetPromptResult`, `InitializeRequest`, …).

## Handler Registration

Servers do not register handlers with the protocol layer. A server is a type
implementing `turbomcp_core::handler::McpHandler` — which `#[server]` generates
— and `turbomcp-server` routes each JSON-RPC request to its `call_tool`,
`read_resource`, `get_prompt`, and list methods. See the
[Server API](server.md) and [Macros](macros.md) references.

### Resource URI Templates

Resources may be declared with RFC 6570 URI templates. The matcher the
generated dispatch uses is public, and `captures` returns what each variable
took, for a handler that needs the values:

```rust
use turbomcp_core::uri_template::UriTemplate;

let template = UriTemplate::parse("users://{user_id}/posts/{post_id}");

assert!(template.matches("users://42/posts/7"));
assert!(!template.matches("users://42/comments/7"));
assert_eq!(template.captures("users://42/posts/7"), Some(vec!["42", "7"]));
```

## Session Management

### Capability Negotiation

`ClientCapabilities` and `ServerCapabilities` are exchanged during
`initialize`. The type-state builders only offer a sub-capability once its
parent is enabled:

```rust
use turbomcp_protocol::{ClientCapabilitiesBuilder, ServerCapabilitiesBuilder};

let server_caps = ServerCapabilitiesBuilder::new()
    .enable_tools()
    .enable_tool_list_changed()   // only available after enable_tools()
    .enable_resources()
    .enable_resources_subscribe() // only available after enable_resources()
    .enable_prompts()
    .build();

// Clients start with every capability enabled; minimal() starts with none
let client_caps = ClientCapabilitiesBuilder::minimal()
    .enable_sampling()
    .enable_roots()
    .build();

assert!(server_caps.tools.is_some());
assert!(client_caps.sampling.is_some());
```

A server built with `#[server]` does not use these: it advertises exactly the
capabilities its handlers serve, plus `logging`.

### Session State

`SessionManager` tracks client sessions with bounded memory (LRU eviction, an
idle timeout, and a cap on per-session request history):

```rust
use std::time::Duration;
use turbomcp_protocol::{SessionConfig, SessionManager};

let manager = SessionManager::new(SessionConfig {
    max_sessions: 1_000,
    session_timeout: chrono::Duration::hours(1),
    cleanup_interval: Duration::from_secs(60),
    ..Default::default()
});
```

The Streamable HTTP server in `turbomcp-server` keeps its own sessions,
configured through `ServerConfig::http_sessions`.

## Error Handling

### Error Types

There is one error type: `McpError` (re-exported as `turbomcp_protocol::Error`,
with `Result<T> = Result<T, McpError>`). It is a struct with a classification
(`kind: ErrorKind`), a message, and optional context, not an enum. Constructors
pick the right kind, and the kind decides the JSON-RPC code:

```rust
use turbomcp_protocol::{ErrorKind, McpError};

let err = McpError::invalid_params("`city` is required");
assert_eq!(err.kind, ErrorKind::InvalidParams);
assert_eq!(err.jsonrpc_code(), -32602);

let err = McpError::resource_not_found("file:///missing.txt");
assert_eq!(err.kind, ErrorKind::ResourceNotFound);

let err = McpError::internal("database unavailable")
    .with_operation("user_lookup")
    .with_component("storage");
println!("retryable: {}", err.is_retryable());
```

On the wire the error is a `JsonRpcError`, which has constructors for the
standard codes:

```rust
use turbomcp_protocol::JsonRpcError;

let error = JsonRpcError::method_not_found("tools/destroy");
assert_eq!(error.code(), -32601);
assert!(JsonRpcError::invalid_params("missing name").is_invalid_params());
```

## Serialization

### JSON-RPC 2.0 Compliance

All messages use JSON-RPC 2.0 format:

```json
// Request with ID (expects response)
{
  "jsonrpc": "2.0",
  "method": "tools/call",
  "params": {"name": "get_weather", "arguments": {}},
  "id": 1
}

// Response
{
  "jsonrpc": "2.0",
  "result": {"content": [{"type": "text", "text": "72°F"}]},
  "id": 1
}

// Notification (no response expected)
{
  "jsonrpc": "2.0",
  "method": "notifications/resources/updated",
  "params": {"uri": "config://app"}
}
```

### SIMD-Accelerated Processing

The `simd` feature (on by default) pulls in `simd-json` and `sonic-rs` for
faster JSON parsing. It needs no code changes.

## Context API

### RequestContext

Handlers receive a `RequestContext` (defined in `turbomcp-core`, re-exported
here). It carries request metadata and the server-to-client operations:

```rust
use turbomcp_protocol::{McpResult, RequestContext};

async fn describe(ctx: &RequestContext) -> McpResult<String> {
    // Metadata
    let who = ctx.subject().unwrap_or("anonymous");
    let session = ctx.session_id().unwrap_or("none");

    // Cooperative cancellation
    if ctx.is_cancelled() {
        return Err(turbomcp_protocol::McpError::cancelled("cancelled by client"));
    }

    // Progress, when the client asked for it
    ctx.report_progress(1.0, Some(1.0), Some("done")).await?;

    Ok(format!(
        "request {} over {:?} from {who} in session {session}",
        ctx.request_id(),
        ctx.transport(),
    ))
}
```

`RichContextExt` adds session state (`get_state` / `set_state`) and logging to
the client (`info`, `warning`, `log`, …), filtered by the level the client set
with `logging/setLevel`:

```rust
use turbomcp_protocol::{McpResult, RequestContext, RichContextExt};

async fn log(ctx: &RequestContext) -> McpResult<()> {
    ctx.info("starting").await?;
    ctx.set_state("step", &1);
    Ok(())
}
```

## Elicitation

### Elicitation Request

A server asks the user for input with `elicitation/create`, through
`RequestContext::elicit_form` (a JSON Schema form) or `elicit_url` (an
out-of-band URL, for clients that declared URL mode):

```rust
use turbomcp_protocol::types::{ElicitAction, ElicitRequestParams};
use turbomcp_protocol::{McpResult, RequestContext};

async fn ask(ctx: &RequestContext) -> McpResult<Option<String>> {
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "name": { "type": "string" } },
        "required": ["name"]
    });
    let result = ctx.elicit_form("What is your name?", schema).await?;

    Ok(match result.action {
        ElicitAction::Accept => result
            .content
            .and_then(|c| c["name"].as_str().map(str::to_string)),
        _ => None, // Decline or Cancel
    })
}

// The request parameters, as the client receives them
let form = ElicitRequestParams::form("What is your name?", serde_json::json!({"type": "object"}));
let url = ElicitRequestParams::url("Authorize GitHub", "https://example.com/auth", "elicit-1");
```

## Sampling

### Model Sampling API

A server asks the client's model for a completion with
`sampling/createMessage`, through `RequestContext::sample`:

```rust
use turbomcp_protocol::types::{CreateMessageRequest, SamplingMessage};
use turbomcp_protocol::{McpResult, RequestContext};

async fn summarize(ctx: &RequestContext, text: &str) -> McpResult<()> {
    let request = CreateMessageRequest {
        messages: vec![SamplingMessage::user(format!("Summarize: {text}"))],
        max_tokens: 200,
        temperature: Some(0.2),
        ..Default::default()
    };
    let result = ctx.sample(request).await?;
    println!("model {} answered", result.model);
    Ok(())
}
```

## Validation

### Schema Validation

`ProtocolValidator` checks messages and definitions against the specification:

```rust
use turbomcp_protocol::JsonRpcRequest;
use turbomcp_protocol::validation::ProtocolValidator;

fn check(request: &JsonRpcRequest) {
    let validator = ProtocolValidator::new();
    let result = validator.validate_request(request);
    if result.is_invalid() {
        for error in result.errors() {
            eprintln!("{error:?}");
        }
    }
}
```

Tool arguments are checked by the server against the tool's input schema
before dispatch; an unexpected argument is refused because the generated schema
declares `additionalProperties: false`.

## Versioning

### Protocol Version

Supported versions: **2025-11-25** (latest) and **2025-06-18**
(`turbomcp_protocol::SUPPORTED_VERSIONS`).

A server answers each client in the version it negotiated. If the client asks
for a version the server does not support, the server offers its preferred one,
as the lifecycle spec requires, and the client decides whether to continue.
Responses are filtered through a per-version adapter so a `2025-06-18` client
never sees fields that version does not define:

```rust
use turbomcp_protocol::adapter_for_version;
use turbomcp_protocol::types::ProtocolVersion;

let adapter = adapter_for_version(&ProtocolVersion::V2025_06_18);
assert_eq!(adapter.version(), &ProtocolVersion::V2025_06_18);
```

## Performance

### Optimizations

- Zero-copy message handling with `Bytes`
- SIMD-accelerated JSON parsing (default `simd` feature)
- Lazy deserialization

## Integration Examples

### Using the Protocol Layer

Parsing a request, routing on its method, and building the response:

```rust
use turbomcp_protocol::jsonrpc::ResponseId;
use turbomcp_protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse, methods};

fn handle(json: &str) -> Result<JsonRpcResponse, serde_json::Error> {
    // Parse incoming JSON-RPC message
    let request: JsonRpcRequest = serde_json::from_str(json)?;

    // Route to handler
    let response = match request.method.as_str() {
        "ping" => JsonRpcResponse::success(serde_json::json!({}), request.id.clone()),
        methods::CALL_TOOL => {
            let result = serde_json::json!({ "content": [{ "type": "text", "text": "done" }] });
            JsonRpcResponse::success(result, request.id.clone())
        }
        other => JsonRpcResponse::error_response(
            JsonRpcError::method_not_found(other),
            request.id.clone(),
        ),
    };
    assert_eq!(response.id, ResponseId::from_request(request.id));
    Ok(response)
}
```

For a whole server, route through `turbomcp_server::route_request` or run a
transport rather than doing this by hand.

## See Also

- **[Full Documentation](https://docs.rs/turbomcp-protocol)** on docs.rs
- **[Source Code](../../../crates/turbomcp-protocol/src/lib.rs)**
- **[Protocol Compliance](../architecture/protocol-compliance.md)** guidelines
- **[Examples](../examples/basic.md)** using protocol APIs
