# Advanced Examples

The workspace examples cover advanced server composition, visibility filtering,
middleware, in-memory testing, OpenAPI conversion, auth helpers, and proxy
introspection. This page points to runnable examples and avoids advertising
sample flows that are not shipped in the repository.

## Composition

`crates/turbomcp/examples/composition.rs` mounts several independent handlers
behind a single `CompositeHandler`:

```bash
cargo run -p turbomcp --example composition
```

It demonstrates prefixed tool names such as `weather_get_current`, resource
aggregation, duplicate-prefix handling, and direct handler invocation in tests or
metadata inspection tools.

## Progressive Disclosure

`crates/turbomcp/examples/visibility.rs` demonstrates `VisibilityLayer`:

```bash
cargo run -p turbomcp --example visibility
```

Use it when one handler should expose different tools or resources based on
tags, exact allow/deny names, read-only tool annotations, or session-specific
grants. For AI-facing deployments, prefer a small allowlist plus read-only
annotations so `tools/list` stays focused and unsafe operations remain hidden.

## Middleware

`crates/turbomcp/examples/middleware.rs` demonstrates typed middleware:

```bash
cargo run -p turbomcp --example middleware
```

The example wraps a server with logging, metrics, and access-control layers and
then calls through the layered handler directly.

## In-Memory Testing

`crates/turbomcp/examples/test_client.rs` uses `McpTestClient` to exercise tools,
resources, prompts, and session IDs without starting TCP, HTTP, or STDIO
transports:

```bash
cargo run -p turbomcp --example test_client
```

This is the preferred pattern for fast unit tests around handler behavior.

## Type-State Builders

`crates/turbomcp/examples/type_state_builders_demo.rs` shows compile-time setup
guards:

```bash
cargo run -p turbomcp --example type_state_builders_demo
```

Use this pattern when a public builder must require fields or configuration
steps before `build()` is available.

## OpenAPI Conversion

`crates/turbomcp-openapi/examples/petstore.rs` converts an OpenAPI document into
MCP tools and resources:

```bash
cargo run -p turbomcp-openapi --example petstore
```

The example covers operation extraction, route mapping, SSRF checks, request
timeouts, and metadata surfaced on generated tools/resources.

## Auth Helpers

The auth crate includes self-contained examples for OAuth 2.1 URL generation,
protected-resource metadata, and Tower rate limiting:

```bash
cargo run -p turbomcp-auth --example oauth2_auth_code_flow
cargo run -p turbomcp-auth --example protected_resource_server
cargo run -p turbomcp-auth --example tower_rate_limiting --features middleware
```

These examples print configuration and validation behavior. They do not start a
full OAuth authorization server.

## Proxy Introspection And Schema Export

The proxy examples cover runtime proxy construction, backend introspection, and
schema export:

```bash
cargo run -p turbomcp-proxy --example runtime_proxy
cargo run -p turbomcp-proxy --example schema_export
```

`schema_export` runs with a built-in mock spec by default. TCP and Unix backend
examples require a live MCP backend listening at the documented address or
socket path.

## Client-Mediated Sampling And Elicitation

TurboMCP includes protocol types for client-mediated flows such as sampling and
elicitation, but this workspace does not currently ship standalone runnable
sampling or elicitation examples. Add a dedicated example before documenting a
copy-paste workflow for those flows.

## Verification

Compile all example targets:

```bash
cargo check --workspace --examples --all-features
```

Run focused examples that print and exit:

```bash
cargo run -p turbomcp --example composition
cargo run -p turbomcp --example visibility
cargo run -p turbomcp --example middleware
cargo run -p turbomcp --example test_client
cargo run -p turbomcp-proxy --example schema_export
```
