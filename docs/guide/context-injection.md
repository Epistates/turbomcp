# Context & Shared State

How a handler gets at what it needs: the per-request `RequestContext`, and the
shared state that lives on your server struct.

## Overview

TurboMCP has no dependency-injection container. A handler has two sources of
everything it uses:

- **`&self`** — your server struct. `#[server]` requires it to be `Clone`, and
  every transport clones it, so shared services (configuration, caches,
  database pools, HTTP clients) live in it behind an `Arc`.
- **`ctx: &RequestContext`** — the one parameter the macro fills in for you. It
  carries per-request metadata and the operations that talk back to the client.

```rust
use std::sync::Arc;
use turbomcp::prelude::*;

#[derive(Clone)]
struct Config {
    api_base: String,
}

#[derive(Clone)]
struct MyServer {
    config: Arc<Config>,
}

#[server(name = "my-server", version = "1.0.0")]
impl MyServer {
    /// Describe where this request is going.
    #[tool]
    async fn my_handler(&self, path: String, ctx: &RequestContext) -> McpResult<String> {
        Ok(format!(
            "{}{} (request {})",
            self.config.api_base,
            path,
            ctx.request_id()
        ))
    }
}
```

On a tool, `ctx` may appear anywhere in the parameter list and is left out of
the input schema. Resource handlers take `(uri: String, ctx: &RequestContext)`,
and prompt handlers take `ctx` after their arguments.

## The Request Context

### Request Metadata

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Inspector;

#[server]
impl Inspector {
    /// Report what the server knows about this request.
    #[tool]
    async fn inspect(&self, ctx: &RequestContext) -> McpResult<String> {
        let request_id = ctx.request_id();         // JSON-RPC request ID
        let transport = ctx.transport();           // Stdio, Http, WebSocket, ...
        let session = ctx.session_id();            // Some(..) over Streamable HTTP
        let user_agent = ctx.header("user-agent"); // HTTP transports only
        let elapsed = ctx.elapsed();               // time since the request arrived

        Ok(format!(
            "{request_id} via {} (session {session:?}, agent {user_agent:?}, {elapsed:?})",
            transport.as_str()
        ))
    }
}
```

### Authentication

When the HTTP transport is configured with `HttpAuthorization` (see
[Authentication](authentication.md)), the validated principal is on the
context:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Admin;

#[server]
impl Admin {
    /// Only administrators may call this.
    #[tool]
    async fn purge(&self, ctx: &RequestContext) -> McpResult<String> {
        if !ctx.is_authenticated() {
            return Err(McpError::authentication("Sign in first"));
        }
        if !ctx.has_any_role(&["admin"]) {
            return Err(McpError::permission_denied("Requires the admin role"));
        }
        let who = ctx.subject().unwrap_or("unknown");
        Ok(format!("purged by {who}"))
    }
}
```

`ctx.principal()` returns the whole `Principal` (subject, issuer, audience,
expiry, email, roles).

### Cancellation and Progress

A client may cancel a request. Cancellation is cooperative: check
`ctx.is_cancelled()` at natural break points. Progress is sent only when the
client asked for it with a progress token; otherwise `report_progress` does
nothing:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Worker;

#[server]
impl Worker {
    /// Process `count` items.
    #[tool]
    async fn process(&self, count: u32, ctx: &RequestContext) -> McpResult<String> {
        for i in 0..count {
            if ctx.is_cancelled() {
                return Err(McpError::cancelled("Cancelled by client"));
            }
            // Do one unit of work, then report it
            if ctx.wants_progress() {
                ctx.report_progress(f64::from(i + 1), Some(f64::from(count)), None)
                    .await?;
            }
        }
        Ok(format!("processed {count} items"))
    }
}
```

### Talking Back to the Client

Over a transport with a session (STDIO, WebSocket, Streamable HTTP), the
context can send requests and notifications to the client:

| Method | Sends |
|---|---|
| `ctx.sample(request)` | `sampling/createMessage`: ask the client's LLM |
| `ctx.elicit_form(message, schema)` | `elicitation/create` (form mode) |
| `ctx.elicit_url(message, url, elicitation_id)` | `elicitation/create` (URL mode) |
| `ctx.list_roots()` | `roots/list` |
| `ctx.notify_resource_updated(uri)` | `notifications/resources/updated` |
| `ctx.notify_tools_list_changed()` (and resources/prompts) | `notifications/*/list_changed` |

Each fails when the client's declared capabilities show it does not support
the feature.

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Onboarding;

#[server]
impl Onboarding {
    /// Ask the user for their name.
    #[tool]
    async fn ask_name(&self, ctx: &RequestContext) -> McpResult<String> {
        let schema = serde_json::json!({
            "type": "object",
            "properties": { "name": { "type": "string" } },
            "required": ["name"]
        });
        let answer = ctx.elicit_form("What is your name?", schema).await?;
        let name = answer
            .content
            .as_ref()
            .and_then(|content| content["name"].as_str())
            .unwrap_or("stranger");
        Ok(format!("Hello, {name}!"))
    }
}
```

Sampling takes a `turbomcp_types::CreateMessageRequest`, so it needs
`turbomcp-types` as a direct dependency:

```rust
use turbomcp::prelude::*;
use turbomcp_types::{CreateMessageRequest, SamplingMessage};

#[derive(Clone)]
struct Summarizer;

#[server]
impl Summarizer {
    /// Summarize text with the client's model.
    #[tool]
    async fn summarize(&self, text: String, ctx: &RequestContext) -> McpResult<String> {
        let request = CreateMessageRequest {
            messages: vec![SamplingMessage::user(format!("Summarize:\n{text}"))],
            max_tokens: 200,
            ..Default::default()
        };
        let result = ctx.sample(request).await?;
        Ok(format!("{:?}", result.content))
    }
}
```

## Logging to the Client

`notifications/message` log messages come from the `RichContextExt` extension
trait in `turbomcp-protocol` (add it as a direct dependency). They are filtered
by the level the client chose with `logging/setLevel`, and rate limited per
session:

```rust
use turbomcp::prelude::*;
use turbomcp_protocol::RichContextExt;

#[derive(Clone)]
struct Chatty;

#[server]
impl Chatty {
    /// Log as it works.
    #[tool]
    async fn work(&self, ctx: &RequestContext) -> McpResult<String> {
        ctx.info("Starting operation").await?;
        ctx.warning("Unexpected value, continuing").await?;
        Ok("Done".to_string())
    }
}
```

For logs that stay on the server, use `tracing` (writing to stderr for STDIO
servers).

## Shared Services

Anything that outlives a request belongs on the server struct. Cloning the
struct clones the `Arc`s, not the services:

```rust
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use turbomcp::prelude::*;

#[derive(Clone)]
struct Services {
    http: reqwest::Client, // already Arc-backed
    cache: Arc<RwLock<HashMap<String, (String, Instant)>>>,
    ttl: Duration,
}

#[server]
impl Services {
    fn new() -> Self {
        Self {
            http: reqwest::Client::new(),
            cache: Arc::default(),
            ttl: Duration::from_secs(300),
        }
    }

    /// Fetch a URL, caching the body for five minutes.
    #[tool]
    async fn fetch(&self, url: String) -> McpResult<String> {
        if let Some((body, fetched)) = self.cache.read().await.get(&url) {
            if fetched.elapsed() < self.ttl {
                return Ok(body.clone());
            }
        }

        let body = self
            .http
            .get(&url)
            .send()
            .await
            .and_then(|response| response.error_for_status())
            .map_err(|e| McpError::external_service(e.to_string()))?
            .text()
            .await
            .map_err(|e| McpError::external_service(e.to_string()))?;

        self.cache
            .write()
            .await
            .insert(url, (body.clone(), Instant::now()));
        Ok(body)
    }
}
```

This uses `reqwest`; add it to your dependencies. To pick an implementation at
startup, store an `Arc<dyn Trait>`:

```rust
use std::sync::Arc;
use turbomcp::prelude::*;

trait Storage: Send + Sync {
    fn name(&self) -> &'static str;
}

struct InMemory;
impl Storage for InMemory {
    fn name(&self) -> &'static str {
        "memory"
    }
}

struct OnDisk;
impl Storage for OnDisk {
    fn name(&self) -> &'static str {
        "disk"
    }
}

#[derive(Clone)]
struct StorageServer {
    storage: Arc<dyn Storage>,
}

#[server]
impl StorageServer {
    fn from_env() -> Self {
        let storage: Arc<dyn Storage> = match std::env::var("STORAGE").as_deref() {
            Ok("disk") => Arc::new(OnDisk),
            _ => Arc::new(InMemory),
        };
        Self { storage }
    }

    /// Which storage backend is active?
    #[tool]
    async fn backend(&self) -> String {
        self.storage.name().to_string()
    }
}
```

## Session State

For state that belongs to one client session rather than the whole server,
either key your own map by `ctx.session_id()` (see
[Session-Scoped State](../examples/patterns.md#session-scoped-state)) or use
`RichContextExt`'s session store:

```rust
use turbomcp::prelude::*;
use turbomcp_protocol::RichContextExt;

#[derive(Clone)]
struct Counter;

#[server]
impl Counter {
    /// Count calls in this session.
    #[tool]
    async fn bump(&self, ctx: &RequestContext) -> McpResult<i64> {
        let count = ctx.get_state::<i64>("count").unwrap_or(0) + 1;
        if !ctx.set_state("count", &count) {
            return Err(McpError::invalid_request("This transport has no session"));
        }
        Ok(count)
    }
}
```

`get_state`/`set_state` need a session ID, and the store is a process-wide map
that is only cleared by `turbomcp_protocol::cleanup_session_state(id)` or a
`SessionStateGuard` being dropped. A long-running multi-client server must
arrange that cleanup itself.

## Request Correlation

Every request has an ID; put it on your `tracing` spans and errors so a
failure can be traced back to the request that caused it:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Traced;

#[server]
impl Traced {
    /// Fail with the request ID attached.
    #[tool]
    async fn fragile(&self, ctx: &RequestContext) -> McpResult<String> {
        let span = tracing::info_span!("fragile", request_id = %ctx.request_id());
        let _enter = span.enter();
        tracing::info!("starting");

        Err(McpError::internal("Operation failed")
            .with_request_id(ctx.request_id())
            .with_operation("fragile"))
    }
}
```

## Testing

Call tool methods directly — they are ordinary methods — passing a context you
build, or go through `McpTestClient` to exercise dispatch and argument
validation:

```rust
use turbomcp::prelude::*;

#[derive(Clone)]
struct Echo;

#[server]
impl Echo {
    /// Echo with the request ID.
    #[tool]
    async fn echo(&self, text: String, ctx: &RequestContext) -> String {
        format!("{text} ({})", ctx.request_id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn echo_directly() {
        let ctx = RequestContext::with_id("req-1");
        assert_eq!(Echo.echo("hi".into(), &ctx).await, "hi (req-1)");
    }

    #[tokio::test]
    async fn echo_through_dispatch() {
        let client = McpTestClient::new(Echo).with_session("session-1");
        let result = client
            .call_tool("echo", serde_json::json!({ "text": "hi" }))
            .await
            .unwrap();
        assert!(result.first_text().unwrap().starts_with("hi"));
    }
}
```

## Next Steps

- **[Transports Guide](transports.md)** - Configure multiple transports
- **[Authentication](authentication.md)** - Add OAuth and security
- **[Observability](observability.md)** - Logging and monitoring
- **[Examples](../examples/basic.md)** - Real-world usage patterns
