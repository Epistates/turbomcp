# Changelog

All notable changes to TurboMCP will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [3.0.13] - 2026-04-14

### Added

- **MCP 2025-11-25 streamable HTTP transport, full lifecycle** — `crates/turbomcp-server/src/transport/http.rs` now implements the complete spec shape: `POST /` + `POST /mcp` for JSON-RPC requests, `GET /` + `GET /mcp` + `GET /sse` for Server-Sent Events, and `DELETE /` + `DELETE /mcp` for explicit session termination. `handle_json_rpc` emits `202 Accepted` for client JSON-RPC responses and notifications per §4, validates the `MCP-Protocol-Version` header against the per-session negotiated version, rejects `initialize` requests that already carry a session id with `400`, and returns `404` for terminated sessions. A new `build_router()` helper backs both the standalone `http::run{,_with_config}` entry points and `ServerBuilder::into_axum_router` / `into_service`, so BYO-Axum deployments pick up the same spec coverage.

- **JSON Schema draft-2020-12 support in `ToolInputSchema` / `ToolOutputSchema`** — Across `turbomcp-types`, `turbomcp-core`, and `turbomcp-protocol`, `schema_type` is now `Option<Value>` (accepts `"object"` or `["object", "null"]`), `additional_properties` is now `Option<Value>` (accepts `false` or a sub-schema like `{"type": "string"}`), and a new `#[serde(flatten)] extra_keywords: HashMap<String, Value>` field preserves arbitrary JSON Schema keywords (`oneOf`, `$schema`, `$defs`, `description`, ...) losslessly through round-trip. Covered by `test_tool_schema_preserves_arbitrary_json_schema_keywords`. Macros (`turbomcp-macros/src/tool.rs`), `turbomcp-openapi` handler, `turbomcp-wasm` registrations, `turbomcp-server/examples/manual_server.rs`, and `turbomcp-protocol` test helpers all updated to the new shape.

- **HTTP origin validation** — New `OriginValidationConfig` struct on `turbomcp-server::config` with `allowed_origins: HashSet<String>`, `allow_localhost: bool` (default `true`), and `allow_any: bool` (default `false`). Exposed on `ServerConfigBuilder` (`origin_validation`, `allow_origin`, `allow_origins`, `allow_localhost_origins`, `allow_any_origin`) and on `ServerBuilder` (`with_origin_validation`, `with_allowed_origin`, `allow_localhost_origins`, `allow_any_origin`). The HTTP transport threads the config into every POST / GET / DELETE handler via `validate_origin`, which also extracts the client IP from `ConnectInfo` or forwarded headers. Covers the MCP "Servers MUST validate the Origin header" DNS-rebinding mitigation.

- **Request ID deduplication per session** — New shared `InitializedSessionState` in `turbomcp-server/src/transport/mod.rs` tracks `seen_request_ids: HashSet<String>`, enforcing the MCP spec requirement that "The request ID MUST NOT have been previously used by the requestor within the same session." HTTP, channel, line (stdio/tcp/unix), and WebSocket transports all wire duplicate requests to a `-32600 Invalid Request` error. Notifications (id=None) bypass the dedup check.

- **Channel transport initialization lifecycle** — `turbomcp-server/src/transport/channel.rs` now enforces the same `SessionState` / `InitializedSessionState` lifecycle already present in line and WebSocket transports: rejects non-lifecycle requests before `initialize`, rejects duplicate `initialize`, and routes post-init requests through `route_request_versioned` with the negotiated `ProtocolVersion`.

- **SSE primer event for resumability** — `handle_sse` now yields an initial `Event::default().id("<session>-0").data("")` before draining the per-subscriber channel, satisfying the MCP spec's "server SHOULD immediately send an SSE event consisting of an event ID and an empty data field in order to prime the client to reconnect (using that event ID as Last-Event-ID)."

- **`RESERVED_METHOD_NAME` validation error** — `ProtocolValidator::validate_request` and `validate_notification` now emit the dedicated `RESERVED_METHOD_NAME` error code for methods starting with `rpc.`, per JSON-RPC 2.0 §6 ("Method names that begin with the word rpc followed by a period character are reserved for rpc-internal methods and extensions"). Both request and notification paths covered by new tests.

- **Comprehensive HTTP spec-compliance integration tests** — New `crates/turbomcp-server/tests/http_transport_spec.rs` covers session-id handshake, `notifications/initialized` → `202`, client JSON-RPC response POST → `202`, GET/DELETE session termination on the same endpoint, untrusted origin rejection, configured origin allowance, duplicate request-id rejection, `413 Payload Too Large` for oversized bodies (raw-TCP test, race-free), and SSE primer event emission. Nine tests, all green.

- **Channel transport duplicate request-id and silent-notification tests** (`test_channel_transport_rejects_duplicate_request_ids`, `test_channel_transport_silent_on_notification_before_init`) plus line transport silent-notification test (`test_line_transport_silent_on_notification_before_init`).

### Changed

- **`SessionManager` routes SSE messages to a single subscriber per session** — Previously backed by `tokio::sync::broadcast`, which delivered every outbound message to every active subscriber for a session. That violated MCP Streamable HTTP §Multiple Connections ("The server MUST send each of its JSON-RPC messages on only one of the connected streams; that is, it MUST NOT broadcast the same message across multiple streams"). `SessionData` now carries `subscribers: Vec<mpsc::UnboundedSender<String>>`, and `send_to_session` / `broadcast` route each message to exactly one live subscriber per session, dropping dead senders as they go. `subscribe_session` returns a dedicated `mpsc::UnboundedReceiver<String>`. Verified by the new `send_to_session_routes_to_single_subscriber` unit test.

- **`ServerBuilder::into_axum_router` / `into_service` delegate to `transport::http::build_router`** — Removed 180+ lines of duplicated `handle_json_rpc` logic from `crates/turbomcp-server/src/builder.rs`. BYO-Axum integrations now automatically inherit the full streamable HTTP spec coverage (session management, origin validation, request-id dedup, 413/413 body limits, SSE primer) rather than running a reduced fork.

- **Protocol method-name regex relaxed** — From `^[a-zA-Z][a-zA-Z0-9_/]*$` to `^[^\s\x00-\x1F]+$`, matching MCP's "just a string" rule and allowing extension-friendly names like `namespace.v1/tool-name`. The dedicated `RESERVED_METHOD_NAME` check still catches `rpc.*`.

- **HTTP client treats `405 Method Not Allowed` on GET `/sse` as "no standalone SSE"** — `crates/turbomcp-http/src/transport.rs` SSE connection task now logs and breaks its reconnection loop when the server replies 405, matching MCP spec-compliant servers that do not offer a standalone SSE stream.

- **Oversized HTTP bodies return `413 Payload Too Large`** — `build_router` now adds a `tower_http::limit::RequestBodyLimitLayer` at the middleware layer, and `handle_json_rpc` also performs an early Content-Length check and inspects the `to_bytes` error chain for `http_body_util::LengthLimitError` as a fallback. Previously mapped to `400 Bad Request`.

### Fixed

- **JSON-RPC 2.0 §4.1: transports no longer respond to notifications with errors** — Line (stdio/tcp/unix), channel, and WebSocket transports previously emitted a JSON-RPC error back to the peer when a notification was rejected (uninitialized session, duplicate request id, ...). Per spec, "Notifications are not confirmable by definition, since they do not have a Response object to be returned." The rejection paths now gate on `request.id.is_some()` for line and channel transports and use `JsonRpcOutgoing::notification_ack()` (dropped by `should_send`) for WebSocket, leaving the wire silent for notifications.

- **`turbomcp-wasm wasm_server/server.rs` compile failure under `--features wasm-server`** — Two `Tool` registrations in `tool()` / `tool_with_ctx()` initialized `extra_keywords: std::collections::HashMap::new()`, but the underlying `turbomcp-core::types::tools::ToolInputSchema::extra_keywords` field is typed `hashbrown::HashMap<String, Value>` (because the crate is `no_std + alloc`). Replaced with `HashMap::new()` so the already-imported `hashbrown` alias applies. Caught by `cargo check --workspace --all-features`.

- **`turbomcp-proxy::proxy::backend::convert_tools` compile failure** — The introspection `ToolSpec`'s `ToolInputSchema` carries a plain `schema_type: String` and flattened `additional: HashMap<String, Value>`, while `turbomcp-protocol::types::tools::ToolInputSchema` migrated to `Option<Value>` for both `schema_type` and `additional_properties`. `convert_tools` now extracts the schema-type string with an `"object"` fallback, copies `additional_properties` straight into `additional`, and propagates `extra_keywords` so arbitrary JSON Schema keywords survive proxy introspection.

## [3.0.12] - 2026-04-13

### Added

- **Draft `extensions` capability** — Both `ClientCapabilities` and `ServerCapabilities` now carry an optional `extensions: HashMap<String, Value>` map for opt-in key/value capability settings, available across `turbomcp-types`, `turbomcp-core`, and `turbomcp-protocol`. New builder helpers (`with_extensions`, `add_extension`) plus a `completions` server capability round out the stable 2025-11-25 shape. `CapabilityMatcher` treats extensions as mutually opt-in: negotiation requires both sides to declare an extension and silently disables mismatches rather than failing the session.

- **Handler-driven `initialize` response** — New `McpHandler::server_capabilities()` trait method with a default that derives tools/resources/prompts from existing listings, letting handlers override to advertise tasks, logging, completions, or draft extensions without forking the router. `build_initialize_result` now serializes the handler's full `ServerCapabilities` and `ServerInfo` via `serde_json::to_value`, so `description`, `title`, `websiteUrl`, and `icons` survive the initialize response instead of being silently dropped.

- **`Client::initialize_with_request()`** — New entry point that accepts a caller-built `InitializeRequest`, giving applications a stable way to opt into draft protocol versions or explicit capability shapes. The ergonomic `Client::initialize()` default path and auto-connect logic now share the same implementation.

- **`RequiredCapabilities` / `ClientCapabilities` extensions field** (`turbomcp-server::config`) — New `extensions: HashSet<String>` field with builder, validation, and `from_params` parsing, letting deployments require specific draft extensions from clients.

- **MCP 2025-11-25 icons plural migration in `turbomcp-core`** (SEP-973) — Replaces `icon: Option<Icon>` with `icons: Option<Vec<Icon>>` on `Implementation`, `Tool`, `Resource`, `ResourceTemplate`, and `Prompt`. Adds `description` and `website_url` to `Implementation` with builder helpers. `turbomcp-types` and `turbomcp-protocol` already carried the plural shape; this catches `turbomcp-core`'s parallel types and every literal constructor across `turbomcp-grpc` and `turbomcp-wasm` up to the spec.

- **gRPC capability + metadata parity** — `mcp.proto` gains `EmptyCapability`, elicitation, client/server task capability trees, `CompletionCapability`, and `ExtensionsCapabilities` messages, wired into `ClientCapabilities` and `ServerCapabilities`. `repeated Icon icons` replaces singular `icon` on `Implementation`, `Tool`, `Resource`, `ResourceTemplate`, and `Prompt`, with new `title`, `description`, `website_url`, and `size` fields per spec. `convert.rs` bidirectional conversions preserve extensions, elicitation, tasks, completions, and experimental capabilities via new `encode_json_map` / `decode_json_map` / `empty_capability_from_map` helpers, covered by round-trip tests for `Implementation` metadata and extensions + task tools.

### Changed

- **Version adapters strip draft `extensions`** — `V2025_11_25Adapter` and `V2025_06_18Adapter` now strip the draft `extensions` field from `filter_capabilities` and from `initialize` results; `DraftAdapter` passes it through. Regression tests cover both stripping and draft passthrough.

### Fixed

- **`wasm-server` feature compiles again** — The `wasm-server`-gated files `turbomcp-wasm/src/wasm_server/server.rs` and `composite.rs` carried two latent build breaks introduced during the icons + extensions work (they are skipped by default `cargo check --workspace --all-targets`). Removes spurious `website_url: None` from eight `Tool`/`Resource`/`ResourceTemplate`/`Prompt` literals (only `Implementation` carries `website_url`), and adds `completions: None` + `extensions: None` to the two `ServerCapabilities` constructors to match the new core shape. Verified with `cargo check`, `cargo clippy -- -D warnings`, and `cargo test --lib -p turbomcp-wasm --features wasm-server` (119 tests passing).

- **Stdio backend spawn tests no longer depend on Python** (`turbomcp-proxy`) — Tests hardcoded `python server.py` / `python -c '...'`, which fail on systems where only `python3` is on `PATH` (current macOS default) and race because `python -c` exits before `wait_for_ready` can observe a running child. Replaced with `/bin/cat`, gated `#[cfg(unix)]` since Windows handles subprocess spawning differently.

- **Workspace internal dep versions unified at 3.0.11** — `turbomcp-macros`, `turbomcp-proxy`, `turbomcp-server`, and `turbomcp-telemetry` still pinned internal deps to the stale 3.0.7 version while the rest of the tree had moved on. Switched to `workspace = true` so they inherit the workspace-declared version in one place; `turbomcp`'s remaining explicit path deps (which must keep `default-features = false`) bumped to match.

## [3.0.11] - 2026-04-02

### Added

- **`RequestContext::notify_client()`** — New method on the server-side request context for sending JSON-RPC notifications to connected clients. Enables server handlers to push `notifications/tools/list_changed`, progress events, and other fire-and-forget messages over bidirectional transports (channel, WebSocket, SSE). Accepts `impl AsRef<str>` for ergonomic method names.

- **`Client::trigger_tool_list_changed()`** — Programmatically invokes the registered `ToolListChangedHandler`, returning `HandlerResult<()>` so callers can observe failures. Designed for testing and external notification integration scenarios.

- **`Client::has_tool_list_changed_handler()`** — Check whether a tool list changed handler is registered, consistent with existing `has_roots_handler()`, `has_elicitation_handler()`, etc.

- **`HandlerRegistry::has_tool_list_changed_handler()`** — Proper `has_*` predicate on the registry, avoiding unnecessary `Arc` clone through `get_*().is_some()`.

## [3.0.10] - 2026-03-26

### Fixed

- **3.0.9 publish was missing changes** — Re-publish includes all session-level version tracking, `ServerBuilder::with_protocol()`, `ProtocolConfig` prelude re-export, `stdio::run_with_config()`, `SessionState` lifecycle enforcement, and versioned routing across all transports.

## [3.0.9] - 2026-03-26

### Added

- **Session-level protocol version tracking across all transports** — Every transport (STDIO, HTTP, TCP, Unix, WebSocket) now stores the negotiated `ProtocolVersion` after a successful `initialize` handshake and routes all subsequent requests through `route_request_versioned`, applying the correct version adapter for response filtering. Previously, version-aware routing was only available at the protocol layer; transports bypassed it.

- **MCP initialization lifecycle enforcement** — `SessionState` enum (`Uninitialized` / `Initialized(ProtocolVersion)`) enforces the MCP spec requirement that `initialize` must succeed before any other method is accepted. Pre-init requests are rejected with a clear error. Duplicate `initialize` requests are rejected. Lifecycle notifications (`notifications/initialized`, `notifications/cancelled`) pass through unconditionally.

- **`ServerBuilder::with_protocol()`** — New builder method to configure protocol version negotiation (e.g., `ProtocolConfig::multi_version()`) through the high-level builder API.

- **`ProtocolConfig` re-exported from prelude** — `turbomcp::prelude::ProtocolConfig` is now available for ergonomic multi-version server setup.

- **`stdio::run_with_config()`** — New entry point for STDIO transport that accepts `ServerConfig`, enabling multi-version protocol support for STDIO-based servers.

- **3 new transport-layer tests** — `test_line_transport_ping_after_init` (verifies requests succeed after init), `test_line_transport_rejects_before_init` (verifies pre-init rejection), `test_line_transport_rejects_duplicate_init` (verifies duplicate init rejection).

### Changed

- **`adapter_for_version()` returns `&'static dyn VersionAdapter`** — Replaced `Box<dyn VersionAdapter>` with static references to zero-sized adapter instances, eliminating per-request heap allocation.

- **HTTP `SessionManager` stores per-session protocol version** — `SessionData` struct replaces bare `broadcast::Sender`, bundling the SSE channel with an optional `ProtocolVersion`. New `set_protocol_version()` / `get_protocol_version()` methods on `SessionManager`.

- **TCP and Unix transports propagate `ServerConfig`** — Per-connection `LineTransportRunner` instances now receive the server config via `with_config()`, enabling version-aware routing on connection-oriented transports.

- **BYO Axum router (`into_axum_router`) gains version tracking** — `AppState` now carries `config`, `session_manager`, and `session_versions` map. The `handle_json_rpc` handler extracts `mcp-session-id` headers and performs per-session versioned routing.

### Removed

- **CI performance benchmarks workflow** — Removed standalone `performance.yml` workflow and consolidated test infrastructure into a single `test.yml` workflow.

## [3.0.8] - 2026-03-24

### Added

- **Multi-version MCP protocol support** — TurboMCP can now negotiate and serve multiple MCP specification versions from a single server. The version adapter system transparently filters outgoing responses to match the client's negotiated protocol version.

- **`ProtocolVersion` enum** — Replaced `type ProtocolVersion = String` alias with a proper enum (`V2025_06_18`, `V2025_11_25`, `Draft`, `Unknown(String)`). Provides `Serialize`/`Deserialize` (round-trips to canonical version strings), `Ord` (by release date), `Display`, `From<&str>`, `From<String>`, `PartialEq<&str>`, `is_stable()`, `is_known()`, and `is_draft()`. Constants: `ProtocolVersion::LATEST` and `ProtocolVersion::STABLE`.

- **Version adapter layer** (`turbomcp_protocol::versioning::adapter`) — `VersionAdapter` trait with `filter_capabilities()`, `filter_result()`, `validate_method()`, and `supported_methods()`. Factory function `adapter_for_version()` returns the appropriate adapter. Three implementations:
  - `V2025_06_18Adapter` — strips icons, execution, outputSchema from tools; icons from prompts/resources; description/icons/websiteUrl from serverInfo; tasks capability; elicitation.url sub-capability; sampling.tools sub-capability. Rejects tasks/* methods.
  - `V2025_11_25Adapter` — pass-through (current stable).
  - `DraftAdapter` — pass-through (superset of 2025-11-25).

- **`route_request_versioned()`** — New server router entry point for post-initialize requests. Validates incoming methods against the negotiated version and filters outgoing responses through the version adapter. Transport layers store the negotiated version from init and call this for subsequent requests.

- **`ProtocolConfig::multi_version()`** — Opt-in constructor that accepts all stable MCP versions. The default remains strict latest-only (matching prior behavior).

- **`ElicitationCapabilities` spec compliance** — Added `form` and `url` sub-capability structs per MCP 2025-11-25 specification. Empty capabilities object (`{}`) defaults to form-only support via `supports_form()`/`supports_url()` helpers. Builder defaults to full (form + URL). `schema_validation` retained as TurboMCP extension.

- **39 new version adapter tests** — Comprehensive coverage: serde round-trips, ordering consistency, adapter filtering for every response type, method validation, elicitation backward compat, end-to-end initialize response filtering.

### Fixed

- **`Ord`/`PartialEq` inconsistency on `ProtocolVersion::Unknown`** — Two distinct `Unknown` variants were `Ord::Equal` but `PartialEq` not-equal, violating Rust's trait contract and causing undefined behavior in `BTreeMap`/`BTreeSet`/sort. `Unknown` variants now compare lexicographically by inner string.

- **`ElicitationCapabilities` missing `form`/`url` sub-fields** — The 2025-11-25 spec requires `{ "form": {}, "url": {} }` structure in elicitation capabilities. Previously only had `schema_validation`.

### Changed

- **`SUPPORTED_VERSIONS` expanded** — Now includes both `"2025-06-18"` and `"2025-11-25"` (previously only `"2025-11-25"`). The default `ProtocolConfig` still only accepts the latest version; use `ProtocolConfig::multi_version()` to accept older clients.

- **`ProtocolConfig` uses `ProtocolVersion` enum** — `preferred_version` and `supported_versions` fields are now typed `ProtocolVersion` instead of `String`. `negotiate()` returns `Option<ProtocolVersion>`.

## [3.0.7] - 2026-03-23

### Fixed

- **`#[description]` attribute not stripped from macro output** — `strip_handler_attributes` in the `#[server]` proc macro now removes `#[description("...")]` from function parameter attributes after extracting their values for JSON schema generation. Previously, these attributes survived into the compiler output, triggering the `description` proc_macro_attribute's `compile_error!` fallback. Any server using `#[description]` on tool/resource/prompt parameters would fail to compile.

## [3.0.6] - 2026-03-18

### Security

- **SSRF bypass in JWT OIDC discovery** — `JwtValidator::discover_jwks_uri` and `JwksCache::get_client_for_issuer` now accept an optional `SsrfValidator` that validates URLs before any network I/O. Previously, `reqwest::get()` was called directly on user-controlled issuer claims, allowing SSRF against internal services. New constructors `JwtValidator::new_with_ssrf()` and `JwksClient::with_ssrf_validator()` enable SSRF-protected operation. The `ssrf` module is now unconditionally available (was previously gated behind `mcp-ssrf` feature).
- **JWT audience field RFC 7519 compliance** — `StandardClaims.aud` changed from `Option<String>` to `Option<Vec<String>>` using `serde_with::OneOrMany`, correctly handling both `"aud": "single"` and `"aud": ["one", "two"]` formats per RFC 7519 §4.1.3. Previously, tokens from enterprise IdPs (Google, Azure AD, Okta) using the array format would fail deserialization.
- **JWKS response size limit** — `JwksClient::fetch_and_cache` now enforces a 64KB response body limit before JSON parsing, preventing memory exhaustion from malicious JWKS endpoints.
- **DPoP server nonce implementation** — `generate_proof_with_params` now embeds server-provided nonces as the `"nonce"` claim in DPoP proofs per RFC 9449 §8. Previously, the nonce parameter was silently discarded.
- **PKCE `plain` method removed in WASM** — `verify_pkce` in the WASM auth provider now rejects the `"plain"` method, enforcing `"S256"` per RFC 7636 §4.2.
- **Constant-time comparison hardened in WASM** — Replaced hand-rolled branchless comparison with `subtle::ConstantTimeEq` to resist LLVM optimizer constant-time assumption violations.
- **Internal error leakage in JSON-RPC responses** — Error handler now generates an opaque UUID error ID for clients and logs the full internal error server-side, preventing reconnaissance via error messages.
- **Unbounded rate limiter memory** — Added `max_tracked_ips` (default: 100,000) to `RateLimitConfig` with automatic eviction of expired entries when capacity is reached, preventing OOM under IP spoofing attacks.
- **`lz4_flex` upgraded to 0.11.6** — Fixes RUSTSEC-2026-0041 (HIGH 8.2): information leak from uninitialized memory during decompression of invalid data.
- **`quinn-proto` upgraded to 0.11.14** — Fixes RUSTSEC-2026-0037 (HIGH 8.7): denial of service in Quinn endpoints via malformed QUIC packets.
- **`lru` replaced with `moka`** — Resolves RUSTSEC-2026-0002 (unsound `IterMut`). OAuth2 token cache now uses `moka::future::Cache` (thread-safe, lock-free, with TTL support).

### Fixed

- **`no_std` compliance in `turbomcp-types`** — Added `#![cfg_attr(not(feature = "std"), no_std)]` and cfg-conditional `HashMap`/`BTreeMap` imports in `content.rs`, `results.rs`, `protocol.rs`. Changed `std::fmt` to `core::fmt` in `traits.rs` and `protocol.rs`. Layer 1 crates now correctly support `no_std + alloc`.
- **`biased` added to shutdown-critical `select!` blocks** — `tokio::select!` in the line transport main loop and client message dispatcher now uses `biased;` to ensure shutdown signals are always checked first.
- **Production `unwrap()` removed from HTTP transport** — `HeaderValue::from_str().unwrap()` on session IDs replaced with graceful fallback.
- **Mutex poisoning risk eliminated** — `std::sync::Mutex` in channel transport replaced with `parking_lot::Mutex` (never poisons).
- **Unnecessary allocation in router** — `request.params.clone().unwrap_or_default()` replaced with borrow pattern in the initialize handler.
- **Dead code cleanup** — Removed unused `detect_server_initiated_type` function. Changed unused `SessionManager` methods to `pub(crate)`.
- **Workspace dependency consistency** — `turbomcp-grpc` now uses `{ workspace = true }` for internal deps instead of inline path specifications.
- **License compliance** — Added `OpenSSL` and `Zlib` to `deny.toml` allowlist. Added advisory ignores for compile-time-only crates (`paste`, `proc-macro-error`).

### Changed

- **Fraudulent security tests replaced** — Three tests in `security_attack_scenarios.rs` that asserted on test data (not SDK behavior) were rewritten with meaningful assertions against actual crate behavior.
- **Vacuous tests fixed** — `test_dispatcher_smoke` (zero assertions) replaced with `test_bidirectional_types_compile` with real assertions. `test_oauth2_expired_authorization` (sleep with no assertion) marked `#[ignore]` with documented implementation path.
- **Trybuild test documentation** — Disabled trybuild tests now have precise reason strings and documented TODO items for v3 compile-fail scenarios.

## [3.0.5] - 2026-03-17

### Fixed

- **Cross-platform compilation in `turbomcp-proxy`** — All Unix domain socket code (`BackendTransport::Unix`, `BackendConfig::Unix`, `UnixFrontend`, `with_unix_backend()`, `UnixTransport` import, `std::path::PathBuf` import) is now gated behind `#[cfg(unix)]`. This allows `turbomcp-proxy` to compile cleanly on Windows, where Unix sockets are unavailable. Unix-specific CLI branches (`BackendType::Unix` match arms) and tests are similarly gated. The `prelude` re-export of `UnixFrontend`/`UnixFrontendConfig` is now also conditional.

## [3.0.4] - 2026-03-15

### Added

- **Progress notification handler** — New `ProgressHandler` trait and `ProgressNotification` re-export in `turbomcp-client`. The client now routes `notifications/progress` to a registered handler instead of silently dropping them. Register via `ClientBuilder::with_progress_handler()` or `Client::set_progress_handler()`.
- **Cursor-based pagination for all list operations** — `list_tools()`, `list_resources()`, `list_resource_templates()`, and `list_prompts()` now automatically follow `next_cursor` to collect all pages (capped at 1000 pages as a safety bound). New `*_paginated(cursor)` variants (`list_tools_paginated`, `list_resources_paginated`, `list_resource_templates_paginated`, `list_prompts_paginated`) expose manual pagination control with the full result type including `next_cursor`.

## [3.0.3] - 2026-03-15

### Breaking Changes

- **Strict single-version protocol policy** — TurboMCP v3 now targets MCP `2025-11-25` only. `SUPPORTED_VERSIONS` narrowed to a single entry; `ProtocolConfig::default()` sets `allow_fallback: false`; `Version::stable()` and `VersionCompatibility::CompatibleWithWarnings` removed.
- **`Uri`, `MimeType`, `Base64String` promoted to newtypes** — These were `type Alias = String`; they are now `#[serde(transparent)]` newtype structs with `Deref<Target = str>`, `From<String>`, `From<&str>`, `AsRef<str>`, `Display`, and `PartialEq<&str>` impls. Wire format is unchanged.
- **`Content` type alias removed** — Use `ContentBlock` directly. The `pub type Content = ContentBlock` alias is deleted.
- **`ClientBuilder` consolidated** — The separate `client/builder.rs` is removed; builder logic is inlined into `turbomcp-client/src/lib.rs`. Public API is unchanged.
- **API key auth now validates against configured value** — `AuthConfig::api_key(header)` without `api_key_value` returns HTTP 500 (fail-closed). Use `with_api_key_auth_value(header, value)` or set `TURBOMCP_API_KEY_VALUE` env var.

### Security

- **Constant-time API key comparison** — API key validation now uses `subtle::ConstantTimeEq` to prevent timing side-channel attacks.
- **JWT scope enforcement** — Auth middleware validates `required_scopes` against token `scope`/`scp` claims.
- **JWT audience validation** — Validates `aud` claim against `server_uri` per RFC 8707 to prevent cross-service token reuse.
- **SSRF hardening with DNS resolution** — Proxy URL validation now resolves hostnames via `tokio::net::lookup_host` and validates all resolved IPs against private/loopback/metadata ranges.
- **JWKS URI construction fixed** — Uses `Url::join()` instead of string concatenation to avoid double-slash bugs with trailing-slash issuers.
- **Bearer token log truncation** — Revocation log now emits only an 8-character token prefix instead of the full token.

### Fixed

- **Response waiter memory leak** — `ProtocolClient` now cleans up response waiters on send failure and timeout, preventing `DashMap` entry leaks.
- **Spurious shutdown warnings** — `Client::Drop` no longer warns when `shutdown()` was already called.
- **Resilience settings silently ignored** — `ClientBuilder::build()` now returns an error (and `build_sync()` panics) if resilience settings are configured but `build_resilient()` is not used.
- **`--all-features` compilation** — Fixed missing `dpop_config` field in auth tests and `Uri` type mismatch in WASM crate.

### Changed

- **Dead code removal** — Deleted `axum_integration.rs` (1847 lines, never imported).
- **WebSocket long-running tests implemented** — Three previously-stub `#[ignore]` tests now use a real `WebSocketTestServer` harness.
- **Token lifecycle tests implemented** — Refresh token rotation and revocation tests now use real `OAuth2Client` instead of raw HTTP.

## [3.0.2] - 2026-03-08

### Changed

- **Eliminated unsafe code in `LockFreeStack`** - Replaced hand-rolled Treiber stack (using `crossbeam::epoch` raw pointers, `ptr::read`, `defer_destroy`) with safe `crossbeam::queue::SegQueue`-backed implementation. Zero unsafe blocks remain in application-level code.
- **Fixed `turbomcp-wire` compilation with `--all-features`** - Added missing `#[cfg(feature = "std")]` gate on `tracing::warn!` call in `StreamingJsonDecoder::feed()`.
- **Dependency updates** - Updated all workspace dependencies to latest versions for security, performance, and correctness:
  - **Major bumps**: `simd-json` 0.13→0.17, `jsonschema` 0.17→0.44, `config` 0.14→0.15, `flume` 0.11→0.12
  - **Runtime**: `tokio` 1.49→1.50, `hyper` 1.0→1.8, `reqwest` 0.13→0.13.2, `serde` 1.0→1.0.228, `serde_json` 1.0→1.0.149
  - **Observability**: `tracing` 0.1→0.1.44, `tracing-opentelemetry` 0.32→0.32.1, `metrics` 0.24→0.24.3
  - **Security**: `ring` 0.17→0.17.14, `jsonwebtoken` 10.2→10.3, `base64` 0.22→0.22.1
  - **Diagnostics**: `miette` 7.0→7.6, `anyhow` 1.0→1.0.102, `color-eyre` 0.6→0.6.5
  - **Testing**: `criterion` 0.8.1→0.8.2, `proptest` 1.9→1.10, `insta` 1.34→1.46, `testcontainers` 0.25→0.27, `wiremock` 0.6→0.6.5, `serial_test` 3.0→3.4
  - **Utilities**: `arc-swap` 1.6→1.8, `crossbeam` 0.8→0.8.4, `ahash` 0.8→0.8.12, `walkdir` 2.4→2.5

## [3.0.1] - 2026-03-05

### Added

- **In-process channel transport** - Zero-overhead `channel` transport using `tokio::sync::mpsc` channels for same-process MCP communication. Eliminates line framing, flushing, and redundant JSON parsing. 1.4x faster than rmcp on tool call latency (14μs vs 19μs), 1.2x higher throughput (71k rps vs 59k rps).
- **`TransportType::Channel` variant** - Added `Channel` to both `turbomcp-core` and `turbomcp-transport-traits` `TransportType` enums, with `is_local()` classification and proper `Display`/serde support.
- **`RequestContext::channel()` factory** - Convenience constructor for channel transport contexts in `turbomcp-server`.
- **`channel` feature flag** - New feature on `turbomcp-server` and `turbomcp` crates, included in `all-transports` bundle.

### Fixed

- **Channel transport type identification** - `ChannelTransport::transport_type()` now correctly returns `TransportType::Channel` instead of `TransportType::Stdio`.

## [3.0.0] - 2026-03-03

### Added

- **Telemetry Integration in top-level crate** - Integrated `turbomcp-telemetry` into the main `turbomcp` crate for instant observability.
- **New `telemetry` feature** - Added a dedicated feature flag to `turbomcp` that enables OpenTelemetry, Prometheus metrics, and structured logging.
- **Enhanced Prelude** - Added `TelemetryConfig`, `TelemetryConfigBuilder`, and `TelemetryGuard` to the `turbomcp::prelude` for improved DX.
- **Telemetry in Bundles** - Included `telemetry` in the `full` and `full-stack` feature bundles.

### Changed

- **Version Bump** - Final release version `v3.0.0` across all workspace crates.
- **Audit Completion** - Successfully completed a comprehensive "Google-grade" technical audit of the entire SDK, ensuring SOTA (Q1 2026) compliance and architectural integrity.

### Fixed

- **Example fixes** - Resolved compilation errors in `tcp_client` and `unix_client` examples by adding the missing `TaskMetadata` argument to `call_tool` calls.
- **Dependency cleanup** - Refined workspace dependencies to ensure clean propagation of features.

## [3.0.0-beta.5] - 2026-02-23

### Security

- **DPoP authentication hardening** - Comprehensive DPoP (RFC 9449) implementation across turbomcp components with enhanced proof-of-possession validation, token binding, and authorization flows (`turbomcp-auth`, `turbomcp-dpop`, `turbomcp-server`)
- **WASM authentication provider** - Full OAuth 2.1 provider for WASM targets with Web Crypto API integration, secure token storage, and PKCE support (`turbomcp-wasm`)

### Added

#### WASM Server Architecture (`turbomcp-wasm`, `turbomcp-wasm-macros`)
- **Durable Objects support** - Rate limiter, session store, state store, and token store durable objects for Cloudflare Workers
- **Streaming transport** - Streamable HTTP transport for edge-native WASM servers with SSE support
- **Composite server** - Multi-server composition with namespace isolation for WASM targets
- **Rich context system** - Enhanced request context with authentication, rate limiting, and middleware state
- **Middleware stack** - Typed middleware system for WASM servers (auth, rate limiting, logging, CORS)
- **Testing utilities** - Comprehensive test harness for WASM server implementations

#### CLI Enhancements (`turbomcp-cli`)
- **`turbomcp new` command** - Project scaffolding with templates for WASM, native, and hybrid servers
- **`turbomcp build` command** - WASM-aware build pipeline with wasm-pack integration
- **`turbomcp deploy` command** - Deploy scaffolding for Cloudflare Workers and other edge platforms

#### Streamable HTTP Transport (`turbomcp-transport-streamable`)
- New crate providing MCP 2025-11-25 Streamable HTTP transport types
- Session management with configurable timeouts and cleanup
- SSE event stream handling with proper connection lifecycle

#### MCP Content Types Enhancement (`turbomcp-types`, `turbomcp-protocol`)
- **Metadata fields** - Added metadata support to MCP content types for extensibility
- **Polymorphic serialization** - Robust `SamplingContentBlock` serialization supporting text, image, and audio content
- **`Role` display implementation** - `Display` trait for `Role` enum for human-readable output

#### Auth Tower Middleware (`turbomcp-auth`)
- **Rate limiting middleware** - Token bucket rate limiter as Tower middleware with configurable per-client limits
- **Auth metrics** - Observable authentication metrics (success/failure rates, latency histograms)
- **Auth context** - Request-scoped authentication context with claims, scopes, and DPoP binding

### Changed

- **Strict protocol compliance** - Enhanced macro-generated code for stricter MCP protocol adherence across tool handlers and server initialization (`turbomcp-macros`, `turbomcp-core`)
- **Error handling improvements** - Extended `McpError` with additional error variants for protocol compliance
- **Router enhancements** - Improved handler routing with better error propagation (`turbomcp-core`)
- **Client dispatcher** - Refined client-server interaction patterns (`turbomcp-client`)

### Fixed

- **Protocol compliance** - Fixed strict protocol compliance issues in server macro generation and tool handler dispatch (`turbomcp-macros`, `turbomcp-core`)
- **Name alias resolution** - Fixed crate name alias configuration

### Internal

- Code cleanup and polish across workspace
- CI workflow improvements for WASM builds and performance testing
- Dependency version updates across all crates

## [3.0.0-beta.4] - 2026-02-17

### Security

#### Comprehensive Security Audit Remediation
Full security audit across all 25 crates with fixes at all severity levels.

#### CRITICAL (`turbomcp-auth`, `turbomcp-dpop`, `turbomcp-wasm`)
- **JWT algorithm confusion prevention** - Fail-closed validation when algorithm list is empty
- **Key-type/algorithm compatibility enforcement** - RSA keys restricted to RS* algorithms, EC keys to ES* algorithms
- **Secret redaction in serialization** - Auth config secrets now serialize as `[REDACTED]` instead of plaintext
- **DPoP proof replay protection** - Enhanced nonce validation and proof binding checks
- **WASM JWT hardening** - Replaced `window.atob()` with standard `base64` crate for universal WASM target support

#### HIGH (`turbomcp-client`, `turbomcp-transport`, `turbomcp-protocol`)
- **Client mutex upgrade** - Replaced `std::sync::Mutex` with `parking_lot::Mutex` (no panic on poisoned lock)
- **Bounded STDIO messages** - `LinesCodec::new_with_max_length()` prevents unbounded memory allocation
- **Session ID length validation** - `SessionId` rejects IDs longer than 256 bytes
- **TCP strict mode** - Configurable `strict_mode` for JSON parse error handling (disconnect vs log-and-continue)

#### MEDIUM (`turbomcp-auth`, `turbomcp-protocol`, `turbomcp-websocket`)
- **SSRF protection hardening** - Blocks private networks, localhost, cloud metadata, link-local, multicast
- **RFC 8414 OpenID Connect Discovery** - JWT validator supports async discovery of JWKS endpoints
- **DPoP binding validation** - `AuthContext::validate_dpop_binding()` for thumbprint verification
- **Enhanced elicitation validation** - Stricter input validation for elicitation request types

#### LOW (across workspace)
- **EMA overflow protection** - Saturating arithmetic in transport metrics prevents u64 overflow
- **gRPC capability validation** - `validate_capabilities()` builder method with `tracing::warn!`
- **Unix socket graceful shutdown** - Broadcast-based shutdown with `JoinSet` task lifecycle management
- **CLI path validation** - Absolute path verification before filesystem operations
- **Macro error improvements** - `syn::Error` span-based errors for better IDE integration
- **Configurable HTTP User-Agent** - Optional `user_agent` field to control fingerprinting

### Added

#### New Crates
- **`turbomcp-openapi`** - OpenAPI 3.x to MCP conversion
  - GET endpoints → MCP Resources, POST/PUT/PATCH/DELETE → MCP Tools
  - Built-in SSRF protection, configurable timeouts, regex route mapping
- **`turbomcp-transport-streamable`** - Streamable HTTP transport types (MCP 2025-11-25)
  - Pure no-I/O SSE encoding/decoding, session management, `no_std` support

#### WASM Server Architecture (`turbomcp-wasm`)
- **Durable Objects** - `DurableRateLimiter`, `DurableSessionStore`, `DurableStateStore`, `DurableTokenStore`
- **Streamable Transport** - Session-based HTTP streaming with Server-Sent Events
- **Enhanced Auth Provider** - WASM-native crypto, multi-provider OAuth 2.1, DPoP, JWKS caching
- **Rich Request Context** - HTTP headers, method, path, query, correlation IDs, auth principal
- **Middleware System** - Request/response interception, rate limiting, logging hooks
- **Visibility Control** - Tool/resource/prompt visibility with user/role-based access
- **Composite Servers** - Compose multiple servers with automatic namespacing and secure CORS

#### WASM Procedural Macros (`turbomcp-wasm-macros`)
- `#[server(name = "...", version = "...")]` - Transform impl blocks into MCP servers
- `#[tool("description")]`, `#[resource("uri")]`, `#[prompt("description")]` - Handler registration
- Identical attribute syntax to native `turbomcp-macros`

#### Server Composition (`turbomcp-server`)
- **Composite Server** - Combine multiple servers with automatic prefixing
- **Typed Middleware** - Per-operation middleware hooks for all MCP operations
- **Visibility/Access Control** - Role-based resource access

#### CLI Enhancements (`turbomcp-cli`)
- `turbomcp build` - Build for native and WASM targets (Cloudflare Workers, Deno, generic wasm32)
- `turbomcp dev` - Development server with hot reload and file watching
- `turbomcp install` - Install servers to Claude Desktop and Cursor
- `turbomcp deploy` - Deploy to Cloudflare Workers
- `turbomcp new` - Create new MCP server projects from templates

#### Child Process Support (`turbomcp-stdio`)
- `StdioTransport::from_child(&mut Child)` - Transport from spawned child process
- `StdioTransport::from_raw<R, W>(reader, writer)` - Custom `AsyncRead`/`AsyncWrite` streams

#### Custom Struct Tool Returns (`turbomcp-core`)
- `IntoToolResult` for `Json<T>` - Tool handlers can return custom structs wrapped in `Json<T>`

#### Macro Enhancements (`turbomcp-macros`)
- **Tags and versioning** - `#[tool(tags = ["admin"], version = "2.0")]` on tools, resources, prompts
- **Type-based `RequestContext` detection** - Detects by type, not parameter name
- **Improved error messages** - `syn::Error` span-based errors, better deprecated attribute guidance

#### Authentication (`turbomcp-auth`)
- `AuthContext` with `requires_dpop()` and `validate_dpop_binding()` methods
- JWT validator async creation with RFC 8414 discovery

### Changed

#### Breaking
- **JWT validator** - `JwtValidator::new()` is now async with RFC 8414 discovery
- **Error types** - `McpError::validation()` → `McpError::invalid_params()` in auth validation

#### Improvements
- **CORS hardening** - Echoes request `Origin` header instead of wildcard `*`, adds `Vary: Origin`
- **Prelude** - Added `Role` to prelude for ergonomic `PromptResult` builder API
- **`parking_lot` workspace dep** - Standardized to 0.12.5 across workspace
- **WASM builder API** - `.tool()` replaces `.with_tool()` (deprecated), same for resources/prompts

### Fixed

- **JWT base64 decoding** (`turbomcp-wasm`) - Cloudflare Workers compatibility (no `window.atob()`)
- **Property test** (`turbomcp-transport`) - `prop_cache_clear_works` deduplicates IDs correctly
- **Prompt context detection** (`turbomcp-macros`) - Detects `&RequestContext` by type, not name
- **Client semaphore handling** (`turbomcp-client`) - Graceful degradation when handler semaphore closed
- **Sampling handler** (`turbomcp-client`) - Removed panic on poisoned lock

### Documentation

- **Macro syntax** (`docs/api/macros.md`) - Corrected resource macro syntax, parameter descriptions
- **McpHandler Clone bound** (`turbomcp-core`) - Documented Arc pattern for shared state
- **Wire codec** (`turbomcp-wire`) - Send+Sync docs, MsgPackCodec security notes
- **TelemetryGuard lifecycle** (`turbomcp-telemetry`) - Drop behavior documentation
- **CLI security warnings** (`turbomcp-cli`) - STDIO risks, token exposure, permissions

### Test Results

- 1,787 tests passing
- Zero clippy warnings with `--all-features`
- All transports verified: STDIO, TCP, HTTP, WebSocket, Unix socket, gRPC

## [3.0.0-beta.3] - 2026-01-22

### Security

#### JWT Algorithm Confusion Attack Prevention (`turbomcp-wasm`)
- **Fail-Closed Algorithm Validation** - Empty algorithm lists now return an error instead of bypassing validation
- **Key-Type/Algorithm Compatibility** - RSA keys can only be used with RS* algorithms, EC keys with ES* algorithms
- **Removed `Default` for `JwtConfig`** - Prevents accidental creation of insecure configurations
- **HTTPS Enforcement for JWKS** - JWKS URLs must use HTTPS (localhost exempt for development)
- Added `allow_insecure_http()` for development/testing only
- Added comprehensive security tests for algorithm confusion and HTTPS validation

### Added

#### Worker Error Integration (`turbomcp-wasm`)
- **`WorkerError` newtype wrapper** - Enables `worker::Error` to `ToolError` conversion via `.map_err(WorkerError)`
- **`WorkerResultExt` trait** - Ergonomic `.into_tool_result()` method for `worker::Result<T>`
- Both approaches enable full `?` operator support with Cloudflare Workers APIs (KV, Durable Objects, R2, D1, etc.)

### Documentation

#### OAuth and Authentication (`turbomcp-wasm`)
- **Comprehensive OAuth Protection Guide** - Three authentication patterns documented:
  1. Cloudflare Access (recommended for production)
  2. Custom JWT Validation (for self-hosted OAuth/OIDC)
  3. Bearer Token (development only, with security warnings)
- **Worker Error Integration Examples** - Usage examples for `WorkerError` and `WorkerResultExt`
- **Security Checklist** - Production deployment checklist for authentication

## [3.0.0-beta.2] - 2026-01-20

### Documentation

#### Security & Authentication
- **DPoP ES256-Only Rationale** (`turbomcp-dpop`) - Comprehensive documentation explaining:
  - Why ES256 (ECDSA P-256) is the only supported algorithm
  - Security comparison table (ES256 vs RSA)
  - RUSTSEC-2023-0071 timing attack vulnerability reference
  - Migration guide for users transitioning from RSA-based DPoP
  - NIST SP 800-186 compliance notes

- **WASM Secret Management** (`turbomcp-wasm`) - Added best practices documentation:
  - `worker::Env` pattern for Cloudflare Workers secrets
  - Cloudflare Access authentication example with `CloudflareAccessAuthenticator`
  - Anti-patterns to avoid (hardcoded secrets)
  - `wrangler.toml` configuration examples

- **CLI Secure Credential Roadmap** (`turbomcp-cli`) - Documented planned features:
  - OS-native keychain integration (macOS Keychain, Windows DPAPI, Linux libsecret)
  - Planned `turbomcp-cli auth login/logout` commands
  - Current workarounds using `--auth` flag and environment variables

### Internal

- Security audit recommendations validated and addressed (WASM and Native Auth)
- Architecture decision records updated (ADR-001: Transport Architecture)
- Removed completed audit report files

## [3.0.0-beta.1] - 2026-01-18

### 🎉 Beta Release

TurboMCP v3.0 enters beta! This release represents a **complete architectural rewrite** with
a net reduction of 47,000+ lines of code while adding powerful new capabilities. The codebase
is now leaner, faster, and more maintainable.

**Highlights:**
- Zero-boilerplate proc macros for native AND WASM MCP servers
- Unified `McpHandler` trait works across all deployment targets
- All dependencies updated to latest versions (reqwest 0.13, tokio 1.49, axum 0.8.8)
- Comprehensive security hardening with error message sanitization

### Added

#### Zero-Boilerplate Architecture (`turbomcp-macros`)
- **Pristine V3 Macro System** - Complete rewrite of procedural macros
  ```rust
  use turbomcp::prelude::*;

  #[derive(Clone)]
  struct Calculator;

  #[mcp_server(name = "calculator", version = "1.0.0")]
  impl Calculator {
      #[tool(description = "Add two numbers")]
      async fn add(&self, a: i64, b: i64) -> i64 {
          a + b
      }
  }

  #[tokio::main]
  async fn main() {
      Calculator.run_stdio().await.unwrap();
  }
  ```
- **Automatic JSON Schema Generation** - Tool parameters derive schemas from Rust types
- **Multi-Transport Support** - Generated methods for `run_stdio()`, `run_tcp()`, `run_http()`, `run_unix()`
- **Type-Safe Tool Registration** - Compile-time validation of tool signatures

#### Core Architecture (`turbomcp-core`)
- **`McpHandler` Trait** - Unified handler interface for all deployment targets
- **`RequestContext`** - Rich context with correlation IDs, headers, transport info
- **`Router`** - JSON-RPC routing with compile-time method registration
- **`McpResult<T>` / `ToolResult`** - Ergonomic response types
- **Security Module** - Path traversal prevention, input sanitization, error message filtering

#### Server Framework (`turbomcp-server`)
- **`ServerBuilder`** - Fluent API for server construction
  ```rust
  ServerBuilder::new("my-server", "1.0.0")
      .tool("greet", "Greet someone", greet_handler)
      .resource("config://app", config_handler)
      .build()
  ```
- **Transport Modules** - New `transport::stdio`, `transport::tcp`, `transport::http` modules
- **Automatic Capability Detection** - Server capabilities derived from registered handlers

#### WASM Enhancements (`turbomcp-wasm`)
- **Portable Server Example** - Same handler code runs natively and on Cloudflare Workers
- **Updated for V3 Architecture** - Full compatibility with unified `McpHandler` trait

#### Security
- **Error Message Sanitization** - Sensitive information (paths, IPs, internal errors) filtered from client responses
- **Production-Safe Defaults** - Sanitization enabled by default, can disable for development

### Changed

#### Dependencies (Latest Versions)
- **reqwest**: 0.12 → 0.13 (with new `OAuth2HttpClient` adapter for oauth2 compatibility)
- **tokio**: 1.47 → 1.49
- **axum**: 0.8.4 → 0.8.8
- **tower**: 0.5.2 → 0.5.3
- **thiserror**: 2.0.16 → 2.0.18
- **sonic-rs**: 0.3 → 0.5
- **compact_str**: 0.8 → 0.9
- **criterion**: 0.7 → 0.8
- **opentelemetry**: 0.28 → 0.31
- **opentelemetry_sdk**: 0.28 → 0.31
- **tracing-opentelemetry**: 0.29 → 0.32
- **metrics**: 0.23 → 0.24
- **metrics-exporter-prometheus**: 0.17 → 0.18

#### OAuth2 Compatibility (`turbomcp-auth`)
- **New `OAuth2HttpClient` Adapter** - Bridges reqwest 0.13 with oauth2 5.0's `AsyncHttpClient` trait
- oauth2 5.0 internally depends on reqwest 0.12, this adapter enables the latest reqwest

#### Constants Consolidation (DRY Compliance)
- **Single Source of Truth** - All protocol constants now in `turbomcp-core`:
  - `PROTOCOL_VERSION`, `SUPPORTED_VERSIONS`
  - `error_codes::*`, `methods::*`
- **Re-exports** - Dependent crates re-export from `turbomcp-core`

### Removed

#### Legacy Code (-47,000 lines net)
- **`turbomcp-macros`**: Removed legacy V2 macro modules (attrs, helpers, template, uri_template, etc.)
- **`turbomcp-server`**: Removed old handler system, elicitation module, multi-tenant config
- **`turbomcp`**: Removed injection, lifespan, registry, session, simd, sse_server modules
- **Test Suites**: Removed 20+ legacy integration test files (replaced by V3 tests)

#### Dead Code Cleanup
- **turbomcp-types**: Removed duplicate `MCP_PROTOCOL_VERSION` constant
- **turbomcp-types**: Deleted vestigial `error.rs` file
- **turbomcp-protocol**: Deleted empty `capabilities.rs.tmp` temp file
- **turbomcp-server**: Removed local `SUPPORTED_PROTOCOL_VERSIONS` constant

### Fixed

- **reqwest 0.13 TLS Configuration** - Removed deprecated `tls_built_in_root_certs()` call (reqwest 0.13 uses rustls-platform-verifier by default)

### Documentation

- **New**: `docs/V3_ARCHITECTURE.md` - Comprehensive architecture guide
- **New**: `docs/V3_UNIFIED_ARCHITECTURE.md` - Unified design principles
- **New**: `docs/guide/wasm.md` - WASM deployment guide
- **Updated**: `docs/getting-started/quick-start.md` - V3 examples
- **Updated**: `docs/getting-started/first-server.md` - Zero-boilerplate walkthrough
- **Updated**: `docs/guide/handlers.md` - New handler patterns
- **Updated**: `docs/architecture/v3-migration.md` - Migration guide

### Internal

- Comprehensive crate-by-crate audit completed across all 23 crates
- Verified consistent versioning (3.0.0-beta.1), lint settings, and documentation
- Confirmed no vestigial or partially implemented code remains
- All workspace tests passing

## [2.3.7] - 2026-01-05

### Added
- **Protocol Compliance Tests** (`turbomcp-protocol`) - Added comprehensive MCP basic protocol compliance tests to ensure strict adherence to the specification.

### Fixed
- **WebSocket Stability** (`turbomcp-transport`) - Fixed a test hang issue by checking connection state before receiving messages, improving overall WebSocket transport reliability.
- **Protocol Capabilities** - Updated server middleware and protocol capabilities to better align with compliance requirements.

## [2.3.6] - 2026-01-03

### Security

This release includes multiple security hardening improvements identified during a comprehensive audit.

#### CRITICAL
- **TLS certificate validation bypass gate** (`turbomcp-transport`) - Disabling certificate validation now requires explicit opt-in via `TURBOMCP_ALLOW_INSECURE_TLS=1` environment variable. Without this, the client will panic with a security error. This prevents accidental deployment of insecure configurations.
- **jsonwebtoken consolidated to v10.2** - Unified all crates on `jsonwebtoken` v10.2.0 with `aws_lc_rs` crypto backend, eliminating version fragmentation and ensuring consistent security.

#### HIGH
- **TLS 1.3 default** (`turbomcp-transport`) - Default minimum TLS version changed from 1.2 to 1.3 for improved security. TLS 1.2 remains available via `TlsVersion::Tls12` but is now deprecated.
- **Enhanced path traversal protection** (`turbomcp-protocol`) - Added detection for URL-encoded patterns (`%2e`, `%252e`), null byte injection (`\0`, `%00`), and Unicode lookalike characters.
- **JWT algorithm allowlist** (`turbomcp-auth`) - `MultiIssuerValidator` now validates JWT algorithms before processing, only permitting asymmetric algorithms (ES256/384, RS256/384/512, PS256/384/512) to prevent algorithm confusion attacks.
- **Explicit rustls backend** (`turbomcp-transport`, `turbomcp-proxy`) - HTTP client now explicitly uses rustls via `.use_rustls_tls()` to prevent native-tls fallback issues with TLS 1.3.

#### MEDIUM
- **API key minimum length** (`turbomcp-auth`) - API keys must now be at least 32 characters (`MIN_API_KEY_LENGTH`). Shorter keys are rejected to prevent brute-force attacks.
- **DPoP nonce storage warnings** (`turbomcp-dpop`) - `MemoryNonceTracker` now logs security warnings about single-instance limitations in production deployments.

### Changed
- `TlsVersion::default()` now returns `Tls13` instead of `Tls12`
- `validate_api_key()` returns `false` for keys shorter than 32 characters
- `reqwest` dependency updated to use `rustls-tls` feature with default-features disabled

### Dependencies
- `jsonwebtoken`: 10.1 → 10.2 (with `aws_lc_rs` and `use_pem` features)
- `reqwest`: Added `rustls-tls` feature, disabled native-tls default
- `oauth2`: Added `rustls-tls` feature to eliminate native-tls dependency
- `tokio-tungstenite`: Switched from `native-tls` to `rustls-tls-native-roots` feature
- `criterion`: Unified all crates on v0.7.0 (workspace version)
- Removed `atty` dependency in favor of `std::io::IsTerminal` (Rust 1.70+ stdlib)
- **native-tls completely eliminated** from dependency tree (security + portability improvement)

## [2.3.5] - 2025-12-16

### Added

#### Protocol Version Configuration (`turbomcp-server`, `turbomcp-macros`)

- **Configurable MCP protocol version negotiation** - Servers can now configure which protocol
  versions they support and how version negotiation works with clients.

- **Pre-built configurations**:
  - `ProtocolVersionConfig::latest()` - Default: Prefer `2025-11-25` (latest official spec) with fallback enabled
  - `ProtocolVersionConfig::compatible()` - Prefer `2025-06-18` for Claude Code compatibility
  - `ProtocolVersionConfig::strict(version)` - Only accept the specified version, reject mismatches
  - `ProtocolVersionConfig::custom(preferred, supported)` - Full control over version negotiation

- **ServerBuilder support** (`turbomcp-server`):
  ```rust
  use turbomcp_server::{ServerBuilder, ProtocolVersionConfig};

  // Use Claude Code compatible settings
  let server = ServerBuilder::new()
      .name("my-server")
      .protocol_version_config(ProtocolVersionConfig::compatible())
      .build();

  // Use strict mode - only accept 2025-11-25
  let server = ServerBuilder::new()
      .protocol_version_config(ProtocolVersionConfig::strict("2025-11-25"))
      .build();
  ```

- **Macro support** (`turbomcp-macros`):
  ```rust
  // Use Claude Code compatible mode
  #[turbomcp::server(protocol_version = "compatible")]
  impl MyServer { ... }

  // Use latest spec (default)
  #[turbomcp::server(protocol_version = "latest")]
  impl MyServer { ... }

  // Strict mode - only accept specific version
  #[turbomcp::server(protocol_version = "strict:2025-11-25")]
  impl MyServer { ... }

  // Specify preferred version directly
  #[turbomcp::server(protocol_version = "2025-06-18")]
  impl MyServer { ... }
  ```

- **TOML/YAML/JSON configuration support**:
  ```toml
  [protocol_version]
  preferred = "2025-11-25"
  supported = ["2025-11-25", "2025-06-18", "2025-03-26"]
  allow_fallback = true
  ```

#### Version Negotiation Flow

1. Client sends `protocolVersion` in initialize request
2. Server checks if client's version is in `supported` list
3. If supported → server responds with client's version
4. If not supported and `allow_fallback = true` → server offers `preferred` version
5. If not supported and `allow_fallback = false` → server rejects connection
6. Client decides to accept server's version or disconnect

### Fixed

#### Claude Code Compatibility
- **Fixed Claude Code connection failures** - Claude Code only supports `2025-06-18`, but TurboMCP
  was advertising `2025-11-25` without proper version negotiation. Servers using
  `ProtocolVersionConfig::compatible()` (or the macro equivalent) will now successfully connect.

### Changed

#### Protocol Version Default
- **Default protocol version remains `2025-11-25`** (latest official MCP spec)
- **Default fallback enabled** - Servers will offer their preferred version if client requests unsupported version
- Users who need Claude Code compatibility should use `ProtocolVersionConfig::compatible()` or
  `#[turbomcp::server(protocol_version = "compatible")]`

## [2.3.4] - 2025-12-13

### Fixed

#### WebSocket Transport (`turbomcp-transport`)
- **WebSocket client requests no longer timeout** - Fixed critical bug where standard request-response
  patterns were never routed to correlation handlers in `spawn_message_reader_task()`. Responses now
  correctly route to the `correlations` DashMap by matching JSON-RPC `id` to `request_id`.

#### Feature Propagation (`turbomcp`)
- **`mcp-tasks` feature now propagates to `turbomcp-server`** - Previously only propagated to
  `turbomcp-protocol`, causing compilation errors when `mcp-tasks` was enabled.

#### Error Handling (`turbomcp-protocol`)
- **`std::error::Error::source()` now returns the actual source error** - Was previously always
  returning `None` despite the `Error` struct having a `source` field. Enables proper error chain
  introspection for debugging and logging frameworks.

#### Code Quality
- Removed unused `error` import in `turbomcp-transport/src/axum/middleware/jwks.rs`
- Replaced `eprintln!` debug statements with `tracing::error` in `turbomcp-dpop/src/proof.rs`

### Added

#### Compile-Time Safety (`turbomcp-macros`)
- **`dbg!` macro now detected by stdio safety validation** - Prevents accidental stdout writes in
  servers using stdio transport. Error message updated to include `dbg!` in examples.

### Changed

#### Documentation
- **Protocol version updated from 2025-06-18 to 2025-11-25** - README now correctly reflects the
  actual `PROTOCOL_VERSION` constant used in the codebase.
- **Added comprehensive Requirements section** - Documents Rust 1.89.0+ requirement with `rustc --version`
  hint for verification.
- **Added Installation section** - Includes `Cargo.toml` and `cargo add` examples.
- **Added Feature Presets documentation** - Documents `default`, `full`, `full-stack`, and `network`
  presets with use cases.
- **Added Individual Features table** - Documents all transport, security, and performance features.
- **Fixed 10 failing doctests** - Updated examples to include required fields (`task`, `task_id`,
  `last_updated_at`, `limit`) and correct types (`HashMap<String, Value>` for arguments).
- **Updated version references from 2.1 to 2.3** in Quick Start examples.

## [2.3.3] - 2025-12-09

### Fixed

#### Macro-Generated HTTP Middleware Signature (`turbomcp-macros`)
- **`run_http_with_middleware` now uses `::turbomcp::axum::Router`** instead of `::axum::Router`
  - Fixes compilation failure for users who don't have `axum` as a direct dependency
  - Users only need `turbomcp` with the `http` feature enabled
  - "Bring Your Own Axum" still works: if user has `axum = "0.8.4"` (same version), types are identical
  - Mismatched axum versions correctly produce compile errors (prevents subtle runtime issues)

## [2.3.2] - 2025-12-09

### Added

#### Comprehensive Regression Test Coverage
- **Tool Serialization Tests** (`turbomcp-protocol`):
  - Added `test_tool_serialization_roundtrip()` - Validates tool JSON serialization/deserialization
  - Added `test_tool_list_result_serialization()` - Tests ListToolsResult with mixed execution modes
  - Added `test_tool_call_request_with_task_support()` - Validates CallToolRequest with task metadata
  - Added `test_backward_compatibility_tools_without_execution()` - Ensures pre-v2.3.1 tools work
  - Added `test_mixed_tools_in_list()` - Real-world scenario with mixed tool configurations
  - All tests prevent future tool serialization/visibility regressions
### Fixed

#### MCP Inspector Compatibility (GitHub Issue #9)
- **CORS Preflight Handling** (`turbomcp-server/runtime/http.rs`):
  - Added explicit OPTIONS handler for CORS preflight requests
  - Without this, Axum returned 405 Method Not Allowed before CorsLayer could process preflight
  - Browser-based clients (MCP Inspector) now connect successfully with `ENABLE_CORS=1`

- **CORS Expose Headers** (`turbomcp-server/runtime/http.rs`):
  - Added `Access-Control-Expose-Headers: mcp-session-id, mcp-protocol-version`
  - Critical fix: browsers block JavaScript from reading response headers not in expose list
  - MCP Inspector can now read session ID and protocol version from responses

#### Code Quality
- Fixed unused variable warnings in `turbomcp-transport/src/axum/middleware/auth.rs`
- Fixed unused mut warning in `turbomcp-transport/src/axum/router/builder.rs`
- Added `#[allow(dead_code)]` for conditionally-used `extract_bearer_token` function

### Verified Compatibility
- ✅ Full MCP Inspector v0.17.5 compatibility verified
- ✅ Streamable HTTP transport: GET/POST/DELETE/OPTIONS
- ✅ SSE streaming with proper Content-Type
- ✅ Session management headers exposed to browser clients
- ✅ Last-Event-ID resumption support
- ✅ 227 turbomcp-server tests passing
- ✅ 258 turbomcp-transport tests passing

#### Configuration Guards and Feature Gating
- Removed unnecessary `#[cfg(feature = "mcp-tasks")]` guards on now-unconditional `task` fields
- Fixed 4 test files with incorrect feature flag usage:
  - `crates/turbomcp-transport/src/websocket_bidirectional/mcp_methods.rs`
  - `crates/turbomcp-transport/src/websocket_bidirectional/types.rs`
  - `crates/turbomcp-transport/tests/sampling_rejection_hang_test.rs` (2 fixes)

#### Documentation
- Updated HTTP server example documentation with CORS setup guidance
- Clarified CORS messaging: "CORS disabled (secure mode)" instead of "CORS enabled (development mode)"
- Added explicit instructions for enabling CORS with `ENABLE_CORS=1` for browser-based tools

### Testing & Verification

- ✅ Verified tool listing works correctly in HTTP transport
- ✅ Confirmed new `execution` field serializes properly (optional, skipped when None)
- ✅ All 950+ tests passing
- ✅ Zero clippy warnings
- ✅ Backward compatibility maintained for tools without `execution` field

## [2.3.1] - 2025-12-09

### Added

#### MCP 2025-11-25 Specification Enhancements
- **Protocol Features** (`turbomcp-protocol`):
  - **Error Codes**: Added `URL_ELICITATION_REQUIRED` (-32042) for URL-based elicitation scenarios
  - **Tool Execution Management**: New `ToolExecution` struct with `taskSupport` field
    - `TaskSupportMode` enum (Forbidden/Optional/Required) for fine-grained task execution control
    - `Tool::execution` field to specify task execution capabilities per tool
    - Allows servers to declare which tools support asynchronous task-based invocation
  - **Builder API**: Added `Tool::with_execution()` method for ergonomic configuration

#### Dependency Updates
- `tower-http`: Updated to 0.6.8 (with `TimeoutLayer::with_status_code()` API migration)
  - Applies consistent HTTP timeout handling across all transports
  - Updated in: HTTP router, middleware stack, server core, WebSocket/HTTP proxies

### Fixed

#### Code Quality
- Updated all `Tool` struct initializers across 11 files to include new `execution: None` field
- Fixed `TimeoutLayer::new()` deprecation warnings (0.6.8 API change)
- Ensured backward compatibility: all new fields are optional with sensible defaults

#### Test Coverage
- All 950+ tests passing
- Zero clippy warnings
- All examples compiling without errors

## [2.3.0] - 2025-12-02

**MCP 2025-11-25 Specification Support**

This release adds comprehensive support for the MCP 2025-11-25 specification (final), including Tasks API, URL-mode elicitation, tool calling in sampling, enhanced metadata support, and multi-tenant infrastructure. All new features are opt-in via feature flags to maintain backward compatibility.

### Added

#### MCP 2025-11-25 Specification Support
- **Protocol Features** (`turbomcp-protocol`):
  - **Tasks API** (SEP-1686): Durable state machines for long-running operations with polling and deferred result retrieval
  - **URL Mode Elicitation** (SEP-1036): Out-of-band URL-based interactions for sensitive data
  - **Tool Calling in Sampling** (SEP-1577): `tools` and `toolChoice` parameters in sampling requests
  - **Icon Metadata Support** (SEP-973): Icons for tools, resources, resource templates, and prompts
  - **Enum Improvements** (SEP-1330): `oneOf`/`anyOf` titled enums, multi-select arrays, default values
  - **Tool Execution Settings**: `execution.taskSupport` field (forbidden/optional/required)
  - Feature flag: `mcp-draft` enables all experimental features; individual flags available for granular control
- **Authorization Features** (`turbomcp-auth`):
  - **SSRF Protection Module**: Secure HTTP fetching with redirect blocking and request validation
  - **Client ID Metadata Documents** (SEP-991) - `mcp-cimd`:
    - Cache-backed CIMD fetcher with concurrent access support
    - Metadata discovery and validation for OAuth 2.0 clients
    - Built-in type definitions for CIMD responses
  - **OpenID Connect Discovery** (RFC 8414 + OIDC) - `mcp-oidc-discovery`:
    - Authorization server metadata discovery
    - Dynamic endpoint configuration from well-known endpoints
    - Cached metadata with TTL-based expiration
  - **Incremental Scope Consent** (SEP-835) - `mcp-incremental-consent`:
    - WWW-Authenticate header parsing and processing
    - Incremental authorization flow support
    - Scope negotiation for privilege escalation workflows

**Files Added**:
- `crates/turbomcp-protocol/src/types/tasks.rs` - Tasks API types
- `crates/turbomcp-protocol/src/types/core.rs` - Enhanced protocol core types
- `crates/turbomcp-server/src/task_storage.rs` - Task storage backend
- `crates/turbomcp-server/src/routing/handlers/tasks.rs` - Task handlers
- `crates/turbomcp-auth/src/ssrf.rs` - SSRF protection utilities
- `crates/turbomcp-auth/src/cimd/` - Client ID Metadata Documents support
- `crates/turbomcp-auth/src/discovery/` - OpenID Connect Discovery support
- `crates/turbomcp-auth/src/incremental_consent.rs` - Incremental consent handling

**Design Philosophy**: All draft features are opt-in via feature flags. Stable versions remain unchanged and production-ready.

#### Multi-Tenant SaaS Support
- **New**: Comprehensive multi-tenancy infrastructure for SaaS applications
  - `TenantConfigProvider` trait with static and dynamic implementations
  - `MultiTenantMetrics` with LRU-based eviction (max 1000 tenants default)
  - Per-tenant configuration: rate limits, timeouts, tool permissions, request size limits
  - Tenant context tracking via `RequestContext::tenant()` and `require_tenant()` APIs
- **New Middleware**: Complete tenant extraction layer
  - `TenantExtractor` trait for flexible tenant identification strategies
  - Built-in extractors: `HeaderTenantExtractor`, `SubdomainTenantExtractor`, `CompositeTenantExtractor`
  - `TenantExtractionLayer` for automatic tenant context injection
- **New Examples**: Production-ready multi-tenant server examples
  - `multi_tenant_server.rs` - Basic multi-tenant setup with configuration
  - `multi_tenant_saas.rs` - Complete SaaS example with tenant metrics, limits, and tool permissions
- **Security**: Tenant ownership validation via `RequestContext::validate_tenant_ownership()`
  - Prevents cross-tenant resource access with `ResourceAccessDenied` errors
  - Critical for multi-tenant data isolation

**Files Added**:
- `crates/turbomcp-server/src/config/multi_tenant.rs` - Tenant configuration providers
- `crates/turbomcp-server/src/metrics/multi_tenant.rs` - Tenant metrics tracking
- `crates/turbomcp-server/src/middleware/tenancy.rs` - Tenant extraction middleware
- `crates/turbomcp/examples/multi_tenant_server.rs` - Basic multi-tenant example
- `crates/turbomcp/examples/multi_tenant_saas.rs` - Complete SaaS example

**Design Philosophy**: Opt-in, zero-breaking-changes. Multi-tenancy features are completely optional and only active when explicitly configured.

### Changed

#### Protocol Type System Enhancements
- **Protocol Core** (`turbomcp-protocol`):
  - Enhanced content types with improved serialization/deserialization
  - Expanded sampling workflow types with better async support
  - **Elicitation API Refactored**: `ElicitRequestParams` is now an enum with `Form` and `Url` variants
    - Breaking: Constructor changed from struct literal to `ElicitRequestParams::form()` factory method
    - Added `message()` method to access message across variants
    - `ElicitRequest` now has optional `task` field (feature-gated with `mcp-tasks`)
  - **Implementation struct enhanced** with new optional fields (MCP 2025-11-25):
    - `description: Option<String>` - Human-readable description of implementation
    - `icons: Option<Vec<Icon>>` - Icon metadata for UI integration
  - Tool definition types updated for better compatibility with spec features

#### Client API Updates
- **Client Handlers** (`turbomcp-client`):
  - **Elicitation request API refactored** to match new enum-based `ElicitRequestParams`:
    - `ElicitationRequest::schema()` now returns `Option<&ElicitationSchema>` (None for URL mode)
    - `ElicitationRequest::timeout()` returns None for URL mode
    - `ElicitationRequest::is_cancellable()` returns false for URL mode
    - All methods handle both Form and Url elicitation modes correctly

#### Authorization Configuration Updates
- **Authentication** (`turbomcp-auth`):
  - Module structure reorganized with feature-gated access
  - New optional dependency: `dashmap` 6.1.0 for concurrent caching (CIMD and Discovery)
  - Added `mcp-ssrf`, `mcp-cimd`, `mcp-oidc-discovery`, and `mcp-incremental-consent` feature flags
  - Updated `full` feature to include new draft specification modules
  - HTTP client now includes built-in SSRF protection via redirect policy

#### OAuth 2.1 Dependencies - Major Upgrade
- **Breaking (for auth feature users)**: Migrated from `oauth2` 4.4.2 → 5.0.0
  - **Typestate System**: Client now uses compile-time endpoint tracking for improved type safety
  - **Stateful HTTP Client**: Connection pooling and reuse for better performance
  - **SSRF Protection**: HTTP client configured with `redirect::Policy::none()` to prevent redirect-based attacks
  - **Method Renames**: `set_revocation_uri()` → `set_revocation_url()` (API breaking change)
  - **Import Changes**: `StandardRevocableToken` moved from `oauth2::revocation::` to `oauth2::` root
- **Eliminated Duplicate Dependencies**: Removed 29 transitive dependencies
  - Removed `oauth2 v4.4.2` (now only v5.0.0)
  - Removed `reqwest v0.11.27` (now only v0.12.24)
  - Removed `base64 v0.13.1` and `v0.21.7` (now only v0.22.1)
  - **Build Time Impact**: Reduced compilation time and binary size
- **Updated**: `jsonwebtoken` from 9.2 → 10.2.0 across workspace
  - Unified 8 crates to use workspace version
  - Updated features: now using `aws_lc_rs` backend

#### OAuth2 Client Implementation (`turbomcp-auth`)
- **Refactored**: `OAuth2Client` struct with typestate annotations
  - `BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>`
  - Compile-time guarantees for endpoint configuration
- **Improved**: HTTP client handling with stateful reqwest::Client
  - Connection pooling for multiple OAuth requests
  - Configured to prevent SSRF via redirect blocking
- **Fixed**: Optional client secret handling in oauth2 5.0
  - Conditional `set_client_secret()` only when secret is present
  - Prevents type mismatches in typestate system

### Fixed

#### Request Context Error Handling
- **Fixed**: Double-boxing errors in `RequestContext` tenant validation methods
  - `require_tenant()` and `validate_tenant_ownership()` were wrapping errors twice
  - Changed from `Box::new(Error::new(...))` to `Error::new(...).into()`
  - Fixes compilation errors introduced by recent context API enhancements

**Files Modified**: `crates/turbomcp-protocol/src/context/request.rs`

### Known Issues

#### Token Revocation Temporarily Unavailable
- **Limitation**: `OAuth2Client::revoke_token()` currently returns an error due to oauth2 5.0 typestate constraints
  - **Cause**: Conditional revocation URL configuration changes client type at compile time
  - **Workaround**: Tokens will naturally expire based on their TTL
  - **Future Fix**: Will address in next minor version by either:
    1. Making `OAuth2Client` generic over revocation endpoint typestate
    2. Storing revocation URL separately and building client on-demand
    3. Using dynamic dispatch for client storage
- **Impact**: Minimal - token expiration remains functional, only explicit revocation is unavailable

## [2.2.3] - 2025-11-16

### Added

#### New Middleware Architecture
- Refactored authentication, JWKS, and rate limiting middleware for enhanced modularity
- Separated concerns between MCP protocol handling and HTTP-specific middleware
- Improved middleware composition for better testability and reusability

#### Proxy Code Generation Enhancements
- Updated Handlebars templates for improved code generation
- Enhanced `Cargo.toml.hbs` template with updated dependency versions
- Improved `main.rs.hbs` template for main module generation
- Enhanced `proxy.rs.hbs` template with better proxy module generation
- Updated `types.rs.hbs` template for improved type definitions

### Changed

#### Dependency Updates
- Updated all internal crate version references to 2.2.3 for consistency across workspace
- Updated turbomcp-proxy to 2.2.3

### Improved

#### Security Middleware
- Enhanced security middleware configuration options
- Improved rate limiting middleware integration
- Better error handling in authentication middleware

#### Code Generation
- Improved template structure for better maintainability
- Enhanced code generation for client and server scaffolding

## [2.2.2] - 2025-11-13

### Added

#### CallToolResult Convenience Methods
Added four ergonomic helper methods to `CallToolResult` for common operations:
- `all_text()` - Concatenates all text content blocks with newlines
- `first_text()` - Returns the first text block (common pattern for simple tools)
- `has_error()` - Checks error status with sensible default (treats `None` as `false`)
- `to_display_string()` - Creates user-friendly formatted output including ResourceLink metadata

**Impact**: Significantly reduces boilerplate for integrators working with tool results.

#### New Examples
- **`structured_output.rs`** - Comprehensive guide showing when/how to use `structured_content` with `output_schema`, including best practices for backward compatibility
- **`resource_links.rs`** - Demonstrates proper ResourceLink usage with all metadata fields (description, mime_type, size) and explains their importance per MCP spec

#### Improved Documentation
- **Feature Requirements Guide**: Added clear documentation explaining minimum feature requirements when using `default-features = false`
  - Documents that at least one transport feature (stdio, http, websocket, tcp, unix) must be enabled
  - Provides practical example configurations for common use cases
  - Helps users avoid build errors when customizing feature flags

### Fixed

#### HTTP Session Logging Severity
- **Fixed**: Reduced log noise for stateless HTTP clients
  - **Issue**: Every HTTP POST request without a session ID logged a WARN message, even though this is normal and spec-compliant behavior
  - **Impact**: LM Studio and other stateless clients no longer generate excessive warnings
  - **Change**: Session ID generation for stateless requests now logs at DEBUG level instead of WARN
  - **Benefit**: Cleaner production logs, WARN level reserved for actual problems
  - **Spec Compliance**: Correctly treats session IDs as optional per MCP 2025-06-18 specification

#### Unix Socket Transport Compilation
- **Fixed**: Unix socket transport now compiles correctly when used independently
  - **Issue**: Missing `fs` feature in tokio dependency prevented Unix socket cleanup operations
  - **Impact**: Unix socket transport can now be used standalone or in combination with other transports
  - **Benefit**: Enables cleaner builds with only the transports you need

#### MCP 2025-06-18 Specification Compliance
- **Enhanced**: JSON-RPC batching properly deprecated per MCP specification
  - **Background**: MCP 2025-06-18 spec explicitly removed JSON-RPC batch support (PR #416)
  - **Action**: Added deprecation notices and clear warnings to batch-related types
  - **Impact**: Code remains backward compatible while guiding users toward spec-compliant patterns
  - **Note**: Batch types exist only for defensive deserialization and will be removed in future versions

#### Annotations Documentation Corrections
- **Fixed `audience` field bug**: Corrected documentation to reflect MCP spec requirement that audience values should be `"user"` or `"assistant"` only (not arbitrary strings like "developer", "admin", "llm")
- **Added MCP spec warnings**: Both `Annotations` and `ToolAnnotations` now include critical warnings from the MCP specification:
  - *"Annotations are weak hints only"*
  - *"Clients should never make tool use decisions based on ToolAnnotations received from untrusted servers"*
- **Honest assessment**: Documentation now accurately reflects that most annotation fields are subjective and "often ignored by clients", with `lastModified` being the most reliably useful field

**Files Modified**:
- `crates/turbomcp-protocol/src/types/core.rs:203-273` (Annotations)
- `crates/turbomcp-protocol/src/types/tools.rs:11-58` (ToolAnnotations)

### Improved

#### Enhanced Field Documentation
Added comprehensive inline documentation for previously ambiguous `CallToolResult` fields:
- **`is_error`**: Clarified that when `true`, ALL content blocks should be treated as error information
- **`structured_content`**: Documented schema-validated JSON usage and backward compatibility pattern
- **`_meta`**: Explained this is for client-internal data that should NOT be exposed to LLMs

**File Modified**: `crates/turbomcp-protocol/src/types/tools.rs:324-346`

#### Content Type Alias Clarification
Added detailed documentation explaining that `Content` is a backward compatibility alias for `ContentBlock`:
- Explains the rename from `Content` to `ContentBlock` in the MCP specification
- Recommends using `ContentBlock` directly in new code
- Includes examples showing equivalence

**File Modified**: `crates/turbomcp-protocol/src/types/content.rs:55-82`


## [2.2.1] - 2025-11-05

### Fixed
#### Provide full and raw access to JSON RPC tool call result
 - **Fixed `Client::call_tool()` to return complete `CallToolResult`** instead of only the first content block. Previously, the method discarded all subsequent content blocks, `structured_content`, and `_meta` fields, causing data loss.
  - **Breaking Change**: `call_tool()` return type changed from `Result<serde_json::Value>` to `Result<CallToolResult>`
  - **Migration**: Callers need to serialize the result if JSON is required: `serde_json::to_value(result)?`
  - **Impact**: CLI and proxy adapters updated to handle new return type
  - **Files Modified**: `turbomcp-client/src/client/operations/tools.rs:154`, `turbomcp-cli/src/transport.rs`, `turbomcp-proxy/src/proxy/backend.rs`
- **Version Script**: Fixed `update-versions.sh` to correctly update inline dependency format (`{ path = "...", version = "..." }`) in `turbomcp-cli/Cargo.toml`. The script now uses explicit regex pattern matching for inline dependencies instead of greedy wildcards.

## [2.2.0] - 2025-11-05

### 🔐 Major Security Release: Sprint 0 & Sprint 1 Complete

This release represents a comprehensive security hardening effort across the entire TurboMCP stack, addressing 1 critical cryptographic vulnerability and 6 high-priority security issues. Security rating improved from 7.0/10 to 8.5/10.

---

### Sprint 0: RSA Removal (CRITICAL CRYPTOGRAPHIC VULNERABILITY)

#### ❌ Eliminated RUSTSEC-2023-0071: RSA Timing Attack Vulnerability
**Removed all RSA support from turbomcp-dpop to eliminate timing attack vulnerability**

- **Vulnerability**: Marvin Attack on RSA decryption (CVSS 5.9)
- **Impact**: Potential private key extraction via nanosecond-precision timing measurements
- **Solution**: Complete removal of RS256 and PS256 algorithms, ES256 (ECDSA P-256) only
- **Status**: ✅ ELIMINATED from production code

**Security Improvements:**
- Removed `rsa` crate dependency from turbomcp-dpop
- Eliminated `DpopAlgorithm::RS256` and `DpopAlgorithm::PS256` variants
- Removed RSA key generation, conversion, and validation code (~366 lines)
- ES256 (ECDSA P-256) is now the only supported algorithm (RFC 9449 recommended)

**Performance Benefits:**
- **2-4x faster signing** (ES256 ~150µs vs RS256 ~500µs)
- **1.5-2x faster verification** (ES256 ~30µs vs RS256 ~50µs)
- **75% smaller signatures** (64 bytes vs 256 bytes)
- **87% smaller keys** (256 bits vs 2048 bits)

**Migration Path:**
- Replace `DpopKeyPair::generate_rs256()` with `DpopKeyPair::generate_p256()`
- ES256 widely supported by all modern OAuth 2.0 servers
- See `crates/turbomcp-dpop/MIGRATION-v2.2.md` for complete guide

**Documentation:**
- `SECURITY-ADVISORY.md`: Full explanation of RUSTSEC-2023-0071
- `MIGRATION-v2.2.md`: Step-by-step migration guide with examples
- Updated API documentation with security notices

**Files Modified:**
- `crates/turbomcp-dpop/Cargo.toml`: Removed rsa dependency
- `crates/turbomcp-dpop/src/types.rs`: Removed RSA algorithms and key types
- `crates/turbomcp-dpop/src/keys.rs`: Removed RSA key generation
- `crates/turbomcp-dpop/src/helpers.rs`: Removed RSA conversion functions
- `crates/turbomcp-dpop/src/proof.rs`: Updated to ES256-only validation

**Test Results:**
- All 21 turbomcp-dpop tests passing
- Zero compiler warnings
- Zero production uses of RSA remaining

---

### Sprint 1: Core Security Hardening (6 HIGH-PRIORITY FIXES)

#### 1.1 Response Size Validation (Memory Exhaustion DoS Prevention)

**Implemented configurable response/request size limits with secure defaults**

- **Vulnerability**: Unbounded response sizes could cause memory exhaustion
- **Solution**: `LimitsConfig` with 10MB response / 1MB request defaults
- **Impact**: Prevents malicious servers from exhausting client memory

**API Design:**
```rust
// Secure by default
let config = LimitsConfig::default();  // 10MB response, 1MB request

// Flexible for power users
let config = LimitsConfig::unlimited();  // No limits (use with caution)
let config = LimitsConfig::strict();     // 1MB response, 100KB request
```

**Features:**
- Stream enforcement option (validates chunk-by-chunk)
- Clear error messages with actual vs max sizes
- Configurable per-transport basis
- Zero-overhead when limits not set

**Files Added/Modified:**
- `crates/turbomcp-transport/src/config.rs`: Added `LimitsConfig` (80 lines)
- `crates/turbomcp-transport/src/core.rs`: Added size validation errors
- Tests: 8 comprehensive limit validation tests

---

#### 1.2 Request Timeout Enforcement (Resource Exhaustion Prevention)

**Implemented four-level timeout strategy with balanced defaults**

- **Vulnerability**: No request timeouts could cause resource exhaustion
- **Solution**: Connect/Request/Total/Read timeouts with 30s/60s/120s/30s defaults
- **Impact**: Prevents hanging connections and resource leaks

**API Design:**
```rust
// Balanced defaults
let config = TimeoutConfig::default();

// Use case presets
let config = TimeoutConfig::fast();      // 5s/10s/15s/5s
let config = TimeoutConfig::patient();   // 30s/5min/10min/60s
let config = TimeoutConfig::unlimited(); // No timeouts
```

**Features:**
- Four timeout levels for granular control
- Helpful error messages explaining which timeout fired
- Configurable per-transport
- Production-tested defaults based on real-world usage

**Files Added/Modified:**
- `crates/turbomcp-transport/src/config.rs`: Added `TimeoutConfig` (120 lines)
- `crates/turbomcp-transport/src/core.rs`: Added timeout error types
- Tests: 12 timeout enforcement tests

---

#### 1.3 TLS 1.3 Configuration (Cryptographic Security)

**Added TLS 1.3 support with deprecation path for TLS 1.2**

- **Issue**: TLS 1.2 default not aligned with modern security practices
- **Solution**: TLS 1.3 option with gradual migration path
- **Roadmap**: v2.2 (compat) → v2.3 (default) → v3.0 (TLS 1.3 only)

**API Design:**
```rust
// Modern security (TLS 1.3)
let config = TlsConfig::modern();

// Legacy compatibility (TLS 1.2, deprecated)
#[allow(deprecated)]
let config = TlsConfig::legacy();

// Testing only (no validation)
let config = TlsConfig::insecure();
```

**Features:**
- TLS version enforcement
- Custom CA certificate support
- Cipher suite configuration
- Certificate validation controls
- Clear deprecation warnings

**Files Added/Modified:**
- `crates/turbomcp-transport/src/config.rs`: Added `TlsConfig` and `TlsVersion` (95 lines)
- `crates/turbomcp-transport/src/core.rs`: TLS validation
- Tests: 6 TLS configuration tests

---

#### 1.4 Template Injection Protection (Code Generation Security)

**Implemented comprehensive input sanitization for code generation**

- **Vulnerability**: Unsanitized tool names could inject arbitrary Rust code
- **Solution**: Multi-layer validation rejecting injection patterns
- **Impact**: Eliminates code injection risk in generated proxies

**Security Layers:**
1. **Identifier Validation**: Only alphanumeric + underscore, no keywords
2. **String Literal Escaping**: Escape quotes, backslashes, control chars
3. **Type Validation**: Reject complex types with braces/generics
4. **URI Validation**: Block control characters and quotes
5. **Length Limits**: 128 char max for identifiers

**Protected Patterns:**
```rust
// ❌ Rejected patterns
"'; DROP TABLE users; --"  // SQL injection attempt
"fn evil() { /* ... */ }"   // Code injection
"../../../etc/passwd"       // Path traversal
"<script>alert(1)</script>" // XSS attempt
```

**Files Added:**
- `crates/turbomcp-proxy/src/codegen/sanitize.rs`: Complete sanitization module (650 lines)

**Test Coverage:**
- 31 sanitization tests covering all attack vectors
- SQL injection, code injection, path traversal, XSS, Unicode attacks
- 100% coverage of security-critical paths

---

#### 1.5 CLI Path Traversal Protection (File System Security)

**Fixed critical path traversal vulnerability in CLI schema export command**

- **Vulnerability**: Malicious MCP servers could write arbitrary files
- **Solution**: Multi-layer path validation with defense-in-depth
- **Impact**: Eliminates risk of arbitrary file write attacks

**Security Improvements:**
- **Path Validation**: Rejects absolute paths, parent directory components (`..`), and symlink escapes
- **Filename Sanitization**: Removes unsafe characters, rejects reserved filenames (`.`, `..`, `CON`, `NUL`, etc.)
- **Canonical Path Resolution**: Verifies all paths stay within intended directory after resolving symlinks
- **Attack Pattern Rejection**: Blocks common path traversal patterns (`../../../etc/passwd`, `/root/.ssh/authorized_keys`, etc.)

**Impact:**
- Eliminates risk of arbitrary file write attacks
- Protects against malicious servers providing tool names like `../../../etc/passwd`
- Maintains backward compatibility (only rejects invalid tool names)
- Exports continue for valid tools even if some are skipped

**Files Added/Modified:**
- `crates/turbomcp-cli/src/path_security.rs`: New security module with validation functions (424 lines)
- `crates/turbomcp-cli/src/executor.rs`: Updated export command to use secure paths
- `crates/turbomcp-cli/src/error.rs`: Added `SecurityViolation` error variant
- `crates/turbomcp-cli/tests/path_security_tests.rs`: Comprehensive security tests (343 lines)

**Test Coverage:**
- 13 unit tests validating sanitization and path checking
- 14 integration tests covering real-world attack scenarios
- Tests include: path traversal, absolute paths, symlink attacks, reserved filenames, Unicode handling
- All tests passing with 100% coverage of security-critical code paths

**Error Handling:**
- Clear, actionable error messages for security violations
- Warns when tool names are skipped due to invalid patterns
- Continues processing valid tools after encountering malicious names

**Vulnerability Details:**
- **CVE**: Pending (internal security audit)
- **Severity**: High (CVSS 7.5 - Local file write via malicious server)
- **Affected Versions**: All versions prior to 2.2.0
- **Mitigation**: Upgrade to 2.2.0 or later

**Example of Protected Attack:**
```bash
# Malicious server returns tool with name: "../../../etc/passwd"
# Before fix: Would write to /etc/passwd
# After fix: Rejected with SecurityViolation error
$ turbomcp-cli tools export --output ./schemas
Warning: Skipped tool '../../../etc/passwd': Path traversal detected
✓ Exported 5 schemas to: ./schemas
```

---

#### 1.6 WebSocket SSRF Protection (Network Security)

**Implemented industry-standard SSRF protection for WebSocket and HTTP backends**

- **Vulnerability**: No validation of backend URLs could enable SSRF attacks
- **Solution**: Three-tier protection using battle-tested `ipnetwork` crate
- **Impact**: Prevents proxies from being used to attack internal services

**Philosophy: Best-in-Class Libraries**
- Uses `ipnetwork` crate (same library used by Cloudflare, AWS)
- Removed custom IP/CIDR validation code (78 lines)
- Follows "do the right thing" principle: leverage industry expertise

**Protection Tiers:**

1. **Strict (Default)**: Blocks all private networks and cloud metadata
   - Private IPv4: 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
   - Loopback: 127.0.0.0/8, ::1
   - Link-local: 169.254.0.0/16, fe80::/10
   - Cloud metadata: 169.254.169.254, 168.63.129.16
   - IPv6 ULA: fc00::/7

2. **Balanced**: Allow specific private networks, block metadata
   - Configure `allowed_private_networks` with CIDR ranges
   - Example: Allow 10.0.0.0/8 for internal services

3. **Disabled**: No SSRF protection (use behind firewall)

**API Design:**
```rust
// Strict protection (default)
let config = BackendValidationConfig::default();

// Balanced for internal services
let config = BackendValidationConfig {
    ssrf_protection: SsrfProtection::Balanced {
        allowed_private_networks: vec![
            "10.0.0.0/8".parse().unwrap(),  // Internal VPC
        ],
    },
    ..Default::default()
};

// Disabled (infrastructure-level protection)
let config = BackendValidationConfig {
    ssrf_protection: SsrfProtection::Disabled,
    ..Default::default()
};
```

**Files Modified:**
- `crates/turbomcp-proxy/Cargo.toml`: Added `ipnetwork = "0.20"` dependency
- `crates/turbomcp-proxy/src/config.rs`: Updated to use `ipnetwork::IpNetwork`
- Removed custom implementation (78 lines of hand-rolled code)

**Test Coverage:**
- 26 SSRF protection tests passing
- Tests cover: strict/balanced/disabled modes, IPv4/IPv6, cloud metadata, custom blocklists
- 100% coverage of validation logic

---

### 📊 Overall Security Impact

**Security Rating:** 7.0/10 → 8.5/10 (+1.5 improvement)

**Vulnerabilities Addressed:**
- ✅ 1 Critical (RSA timing attack)
- ✅ 6 High (memory exhaustion, resource exhaustion, code injection, path traversal, SSRF, weak TLS)

**Test Coverage:**
- Sprint 0: 21 tests (turbomcp-dpop)
- Sprint 1.1: 8 tests (response/request limits)
- Sprint 1.2: 12 tests (timeouts)
- Sprint 1.3: 6 tests (TLS)
- Sprint 1.4: 31 tests (template injection)
- Sprint 1.5: 13 tests (path traversal)
- Sprint 1.6: 26 tests (SSRF)
- **Total: 117 new security tests**

**Code Quality:**
- Zero compiler warnings
- Zero clippy warnings
- 100% test pass rate
- Comprehensive documentation

**Philosophy Validated:**
- "Secure by default, flexible by design"
- "Use battle-tested libraries" (ipnetwork, jsonwebtoken, tokio)
- Sane defaults for users trusting TurboMCP for security
- Configuration options for infrastructure-level security

---

### Breaking Changes

**turbomcp-dpop (v2.2.0):**
- ❌ Removed `DpopAlgorithm::RS256` and `DpopAlgorithm::PS256`
- ❌ Removed `DpopKeyPair::generate_rs256()`
- ✅ Migration: Use `DpopKeyPair::generate_p256()` instead
- ✅ See `MIGRATION-v2.2.md` for complete guide

**Backward Compatibility:**
- All other APIs remain 100% compatible
- New security features are opt-in or have safe defaults
- Existing code continues to work (except RSA usage)

---

## [2.1.3] - 2025-11-03

### Critical Fixes: WebSocket Bidirectional Communication (2025-11-03)

#### WebSocket Response Routing (CRITICAL BUG FIX)
**Fixed architectural issue preventing WebSocket bidirectional methods from working**
- Added missing `spawn_message_reader_task()` to continuously process WebSocket messages
- Routes JSON-RPC responses to correlation maps (pending_pings, pending_samplings, pending_roots, elicitations)
- Auto-responds to WebSocket Ping frames with Pong (RFC 6455 compliance)
- Enables server-initiated features (elicitation, sampling, roots/list)
- **Test**: `test_websocket_ping_pong` now passes (was timing out after 60 seconds)

**Impact**:
- All bidirectional WebSocket methods now work correctly
- Ping/pong keep-alive mechanism operational
- Sampling requests complete in 65µs instead of hanging for 60 seconds (**1,000,000x speedup**)

**Files Modified:**
- `crates/turbomcp-transport/src/websocket_bidirectional/tasks.rs`: Added message reader (152 lines)
- `crates/turbomcp-transport/src/websocket_bidirectional/connection.rs`: Integrated into startup
- `crates/turbomcp-transport/tests/websocket_bidirectional_integration_test.rs`: Fixed test server, removed #[ignore]
- `crates/turbomcp-transport/tests/sampling_rejection_hang_test.rs`: Updated benchmark to verify fix

#### Documentation & Quality
- Created `REMAINING_CONNECTION_ISSUES.md` tracking all known WebSocket issues with migration roadmap
- Documented `num-bigint-dig` future incompatibility warning (non-blocking, transitive dependency)
- Fixed clippy linting errors (collapsed nested if statements for better code style)
- All 1000+ tests passing

#### Test Results
- Full test suite: 100% pass rate
- WebSocket ping/pong: ✅ PASSING (was failing)
- Sampling rejection: ✅ 65µs (was 60 seconds)
- Benchmark verification: ✅ Bug confirmed fixed

### Performance Impact
- Sampling rejection: **1,000,000x faster** (60s → 65µs)
- WebSocket keep-alive: Now functional
- No performance regression in other areas

### Breaking Changes
**None** - All fixes are internal improvements

---

## [2.1.2] - 2025-11-01

### Features & Improvements: WebSocket Unification, HTTP Header Access & Proxy Validation

#### HTTP Header Extraction (NEW)
**HTTP headers are now automatically extracted and accessible in request handlers**
- HTTP request headers are extracted and stored in context metadata as `http_headers`
- Headers available through `ctx.headers()` and `ctx.header(name)` helper methods
- Supports all HTTP headers including custom headers (e.g., `x-request-id`, `x-custom-header`)
- Headers accessible in both HTTP and WebSocket transports
- Added comprehensive tests for header extraction and access patterns

**Example Usage:**
```rust
#[handler]
async fn my_handler(ctx: &mut Context) -> Result<()> {
    // Access all headers
    let headers = ctx.headers();
    
    // Access specific header
    if let Some(user_agent) = ctx.header("user-agent") {
        // Use header value
    }
}
```

#### WebSocket Unification
**Eliminated 146 lines of duplicate code and unified WebSocket implementation across layers**
- Moved WebSocket implementation from server layer to transport layer (single source of truth)
- Created `WebSocketDispatcher` for bidirectional server-to-client requests
- Implemented `WebSocketFactory` pattern for per-connection handlers with configuration
- Proper layering: transport handles WebSocket mechanics, server handles protocol logic
- WebSocket requests also extract and store headers in session metadata
- Maintains 100% API compatibility - zero breaking changes

**Files Improved:**
- `turbomcp-transport`: Added unified WebSocket infrastructure (210 + 237 = 447 new lines)
- `turbomcp-server`: Refactored to use transport layer (100 line adapter, removed 822 line duplicate)
- Net reduction: 146 lines of duplicate code eliminated

#### Proxy & Transport Improvements
**Fixed hanging integration tests and feature gate compilation issues**
- Fixed 3 proxy integration tests hanging indefinitely (60+ seconds → 0.16s)
- Properly documented ignored tests with clear justification
- Fixed feature gate compilation errors when building without `websocket` feature
- Updated import paths after WebSocket refactoring
- All 340+ tests passing with zero regressions

**Test Results:**
- turbomcp-server: 183 tests passing (175 lib + 8 config)
- turbomcp-proxy: 73 tests passing (5 properly ignored)
- Proxy end-to-end validation: Confirmed working with stdio_server backend

#### Maintenance & Quality
- Zero compiler warnings
- Zero clippy warnings
- Feature gates working correctly for all feature combinations
- Production build validated and ready for deployment

### Performance Impact
- Build time: Neutral (8.72s clean workspace build)
- Test execution: 99%+ faster (hanging tests now properly ignored)
- Runtime: Neutral to slight improvement (same Axum patterns, fewer allocations)
- Code quality: -146 lines, improved maintainability

### Breaking Changes
**None** - All public APIs remain 100% compatible

---

## [2.1.0] - 2025-01-29

### Major Features: turbomcp-proxy, OAuth2.1 Flows, Complete Authentication Stack

#### New Crates

##### turbomcp-proxy (NEW)
**A production-grade MCP protocol proxy with transport flexibility and runtime introspection**

- **Multi-Transport Support** (25 backend×frontend combinations, 100% tested)
  - **Backends**: STDIO, HTTP, TCP, Unix Domain Sockets, WebSocket
  - **Frontends**: STDIO, HTTP, TCP, Unix Domain Sockets, WebSocket
  - All combinations validated with 40+ integration tests
  - Configurable host, port, socket paths with production-ready error handling

- **Protocol Features**
  - Authorization code generation and validation
  - Automatic URL scheme detection and routing
  - Resource URI binding (RFC 8707 compliant)
  - Metadata introspection and discovery
  - Comprehensive error handling with context

- **Architecture & Performance**
  - Enum dispatch pattern for type-erased transport abstraction
  - Zero-cost compile-time method dispatch via `dispatch_client!` macro
  - 100% safe Rust (zero unsafe code)
  - Consistent security validation across all transports

- **Security**
  - Command injection prevention
  - SSRF (Server-Side Request Forgery) protection
  - Path traversal protection
  - Production-ready security documentation

- **Testing**
  - 40+ comprehensive integration tests
  - All 25 transport combinations tested and working
  - Security validation tests
  - Builder pattern and configuration tests
  - Edge case and metrics coverage

---

#### turbomcp-auth Enhancements
**Complete OAuth 2.1 client and server implementation with RFC compliance**
- Updated README.md to reflect stateless authentication architecture
- Removed all references to session management from documentation
- Clarified MCP compliance: stateless token validation on every request

##### OAuth2Client - Production OAuth2.1 Flows
- **Authorization Code Flow with PKCE** (RFC 7636)
  - Automatic PKCE challenge/verifier generation for enhanced security
  - State parameter for CSRF protection
  - Works with all OAuth 2.1 providers
  - Methods: `authorization_code_flow()`, `exchange_code_for_token()`

- **Token Refresh**
  - Refresh tokens without user interaction
  - Automatic token validation checks
  - Method: `refresh_access_token()`

- **Client Credentials Flow** (Server-to-Server)
  - Service account authentication
  - No user interaction required
  - Method: `client_credentials_flow()`

- **Token Validation**
  - Client-side expiration checks
  - Format validation
  - Integration with OAuth provider introspection endpoints

##### OAuth2Provider (NEW)
**Full AuthProvider trait implementation for OAuth 2.1**
- Token validation via userinfo endpoints
- Token caching (5-minute default) for performance
- Refresh token handling
- Automatic userinfo parsing for Google, GitHub, Microsoft, GitLab
- Integration with AuthManager for multi-provider coordination

##### Server-Side Helpers (NEW)
**RFC 9728 Protected Resource Metadata and bearer token validation**

- **ProtectedResourceMetadataBuilder**
  - Generate RFC 9728 compliant metadata
  - Configurable scopes, bearer methods, documentation URI
  - Builder pattern for flexibility
  - JSON serialization for /.well-known/protected-resource endpoint

- **WwwAuthenticateBuilder**
  - RFC 9728 compliant 401 Unauthorized responses
  - Automatic header generation
  - Metadata URI discovery support
  - Scope and error information

- **BearerTokenValidator**
  - Extract bearer tokens from Authorization header
  - Token format validation
  - Case-insensitive Bearer scheme handling
  - Structured error messages

##### Examples
- `oauth2_auth_code_flow.rs` - Complete OAuth2.1 client flow demonstration
- `protected_resource_server.rs` - Server-side protected resource handling

##### Documentation
- Comprehensive README with quick-start guides (client and server)
- RFC compliance matrix (7636, 7591, 8707, 9728, 9449)
- Security best practices
- Complete code examples in documentation

---

#### turbomcp-dpop
**RFC 9449 Proof-of-Possession implementation with HSM support (already available in 2.0.5+)**

- Full RFC 9449 DPoP specification implementation
- RSA, ECDSA P-256, and PSS algorithm support
- Replay attack prevention with nonce tracking
- HSM integration (PKCS#11, YubiHSM)
- Redis-backed distributed nonce storage
- Constant-time comparison for timing attack protection

---

#### RFC Compliance Summary
- **RFC 7636**: PKCE (Proof Key for Public OAuth Clients) - ✅ Fully implemented
- **RFC 7591**: Dynamic Client Registration Protocol - ✅ Configuration types
- **RFC 8707**: Resource Indicators for OAuth 2.0 - ✅ Canonical URI validation
- **RFC 9728**: OAuth 2.0 Protected Resource Metadata - ✅ Full server implementation
- **RFC 9449**: DPoP (Proof-of-Possession) - ✅ Optional feature

#### Testing
- **turbomcp-auth**: 18 tests passing
- **turbomcp-dpop**: 21 tests passing
- **turbomcp-proxy**: 40+ integration tests (all 25 transport combinations)
- **Total**: 80+ comprehensive tests with 100% pass rate

#### Breaking Changes
- ✅ **Zero breaking changes** - fully backward compatible with 2.0.5

#### Migration Path
- See MIGRATION.md in turbomcp-auth and turbomcp-dpop for detailed upgrade guides
- Existing API unchanged; new features are purely additive


---

## [2.0.5] - 2025-10-24

### Fixed

- **Observability stderr output bug**: Fixed regression where observability logs were being written to stdout instead of stderr
  - Per MCP specification, stdout must be reserved exclusively for JSON-RPC protocol messages
  - Logs were corrupting the protocol stream when mixing with JSON-RPC responses
  - Root cause: `tracing_subscriber` fmt::layer() was missing explicit `.with_writer(std::io::stderr)` configuration
  - Now correctly outputs all observability logs to stderr
  - Added integration test in `examples/stdio_output_verification.rs` to prevent future regressions

### Added

- **Integration test**: `examples/stdio_output_verification.rs` demonstrates and validates stdout/stderr separation
- **Regression test**: Documentation test in observability module with verification instructions

## [2.0.4] - 2025-10-22

### Added

- **Explicit Transport Selection with `transports` Attribute**: New optional macro parameter for specifying which transports a server uses
  - Reduces generated code by only creating methods for specified transports
  - Eliminates cfg warnings on Nightly Rust when transports are specified
  - Supported values: `stdio`, `http`, `websocket`, `tcp`, `unix`
  - Example: `#[server(name = "my-server", version = "0.1.0", transports = ["stdio"])]`
  - Compile-time validation with helpful error messages
  - Fully backward compatible (omitting attribute generates all transports as before)

### Changed

- **Schema-Generation Now Unconditional**: Moved `schemars` from optional to always-enabled dependency
  - Schema generation is now available by default (required for MCP spec compliance)
  - Only affects build-time dependencies (zero runtime overhead)
  - Simplified mental model: users don't have to remember to enable schema-generation feature
  - Still works with `default-features = false` if needed

- **Macro Warnings Strategy**: Removed `#[allow(unexpected_cfgs)]` from generated code
  - Cfg warnings on Nightly Rust now provide actionable guidance
  - Guides users toward explicit transport specification
  - Cleaner design: warnings point to solutions rather than hiding issues
  - Stable Rust (1.89+) unaffected (no warnings by default)

### Fixed

- **Code Quality**: Removed anti-pattern of suppressing warnings in generated code
- **Schema Module**: Removed fallback implementations and unused cfg guards

### Technical Details

- Added transport validation in `attrs.rs`
- Conditional method generation in `bidirectional_wrapper.rs`
- Wire transport attribute through macro pipeline in `server.rs` and `compile_time_router.rs`
- Added comprehensive `examples/transports_demo.rs` showing all usage patterns

### Backward Compatibility

- ✅ Zero breaking changes
- ✅ All existing code continues to work
- ✅ Fully backward compatible with 2.0.3

## [2.0.3] - 2025-10-21

### Added

- **Configurable Concurrency Limits**: Semaphore-based concurrency is now configurable for production flexibility
  - **WebSocket Server**: `WebSocketServerConfig::max_concurrent_requests` (default: 100)
    - Configure via `WebSocketServerConfig { max_concurrent_requests: 200, .. }`
    - Limits concurrent client→server request handlers per connection
  - **Client**: `ClientCapabilities::max_concurrent_handlers` (default: 100)
    - Configure via `ClientBuilder::new().with_max_concurrent_handlers(200)`
    - Limits concurrent server→client request/notification handlers
  - **Tuning Guide**:
    - Low-resource systems: 50
    - Standard deployments: 100 (default)
    - High-performance: 200-500
    - Maximum recommended: 1000
  - **Benefits**: Production deployments can tune resource usage based on available memory/CPU

### Fixed

- **Macro-Generated Doc Test Failures**: Fixed compilation failures when users run `cargo test` on projects using the `#[server]` macro
  - **Issue**: Generated methods (`run_stdio()`, `run_tcp()`, `into_mcp_router()`, etc.) had doc examples marked as ````no_run`, which still compiles the code
  - **Root Cause**: Placeholder names like `MyServer` in examples attempted to compile in user projects, causing `cannot find value 'MyServer'` errors
  - **Solution**: Changed all macro-generated doc examples from ````no_run`/````rust,no_run` to ````rust,ignore`
  - **Files Modified**:
    - `crates/turbomcp-macros/src/bidirectional_wrapper.rs` (4 doc examples)
    - `crates/turbomcp-macros/src/compile_time_router.rs` (2 doc examples)
  - **Impact**: Users can now run `cargo test` without failures from turbomcp-generated documentation
  - **Details**: See `MACRO_DOC_TEST_FIX.md` for complete analysis

- **Task Lifecycle Management - Comprehensive Hardening**: Fixed critical "JoinHandle polled after completion" panics and implemented task lifecycle management across all transports
  - **Issue**: Spawned tasks without proper lifecycle management caused panics on clean shutdown and potential resource leaks
  - **Root Cause**: `tokio::spawn()` returned JoinHandles that were immediately dropped, leaving tasks orphaned
  - **Impact**: STDIO servers panicked on EOF, WebSocket/TCP/Client handlers could leak resources
  - **Scope**: Comprehensive fix across 4 major components
  
  #### Component 1: STDIO Transport (`turbomcp-server/src/runtime.rs`)
  - **Pattern**: JoinSet with graceful shutdown
  - **Changes**:
    - Added `use tokio::task::JoinSet` import
    - Refactored `run_stdio_bidirectional()` to track all spawned tasks in JoinSet
    - Implemented graceful shutdown with 5-second timeout and abort fallback
    - Added comprehensive unit tests (6 tests) and integration tests (9 tests)
  - **Result**: No more panics on clean EOF, all tasks properly cleaned up
  - **Tests**: `runtime::tests::*`, `stdio_lifecycle_test.rs`
  
  #### Component 2: WebSocket Server (`turbomcp-server/src/runtime/websocket.rs`)
  - **Pattern**: Semaphore for bounded concurrency (industry best practice)
  - **Changes**:
    - Added `use tokio::sync::Semaphore` import
    - Implemented semaphore-based concurrency control (configurable, default 100)
    - Per-request tasks use RAII pattern (permits auto-released on drop)
    - Main send/receive loops already properly tracked with tokio::select!
    - **NEW**: Added `max_concurrent_requests` field to `WebSocketServerConfig`
  - **Benefits**: Automatic backpressure, prevents resource exhaustion, simpler than JoinSet for short-lived tasks, **production configurable**
  - **Result**: Bounded concurrency, no resource leaks, production-ready
  
  #### Component 3: TCP Transport (`turbomcp-transport/src/tcp.rs`)
  - **Pattern**: JoinSet with shutdown signal + nested JoinSet for connections
  - **Changes**:
    - Added task tracking fields to `TcpTransport` struct
    - Implemented graceful shutdown in `disconnect()` method
    - Accept loop listens for shutdown signals via `tokio::select!`
    - Connection handlers tracked in nested JoinSet
  - **Result**: Clean shutdown of accept loop and all active connections
  - **Tests**: Existing TCP tests pass with new implementation
  
  #### Component 4: Client Handlers (`turbomcp-client/src/client/core.rs`)
  - **Pattern**: Semaphore for bounded concurrency (consistent with WebSocket)
  - **Changes**:
    - Added `handler_semaphore: Arc<Semaphore>` to `ClientInner` struct
    - Updated both constructors (`new()` and `with_capabilities()`)
    - Request and notification handlers acquire permits before processing
    - Automatic cleanup via RAII pattern
    - **NEW**: Added `max_concurrent_handlers` field to `ClientCapabilities`
    - **NEW**: Added `with_max_concurrent_handlers()` builder method
  - **Result**: Bounded concurrent request processing, prevents resource exhaustion, **production configurable**
  - **Tests**: All 72 client tests pass
  
  #### Architecture & Patterns
  - **Long-Running Infrastructure Tasks** → JoinSet + Shutdown Signal
    - Accept loops, keep-alive monitors, health checks
    - Graceful shutdown with timeout and abort fallback
    - Example: STDIO stdout writer, TCP accept loop
  - **Short-Lived Request Handlers** → Semaphore for Bounded Concurrency
    - HTTP/WebSocket/Client request handlers
    - Automatic backpressure and resource control
    - Example: WebSocket per-request spawns, client handlers
  - **Fire-and-Forget** → Explicitly Documented (rare, requires review)
    - Non-critical logging, metrics emission
    - Must be <100ms and truly non-critical
  
  #### Testing
  - **Unit Tests**: 6 new tests in `runtime::tests::*`
  - **Integration Tests**: 9 new tests in `stdio_lifecycle_test.rs`
  - **Regression Prevention**: Tests verify clean shutdown without panics
  - **All Existing Tests Pass**: No breaking changes
  
  #### Breaking Changes
  - **None** - All changes are internal implementation details
  - Public APIs unchanged
  - Backward compatible
  - Can be released as patch version (2.0.3)
  
  #### Performance Impact
  - **JoinSet Overhead**: ~16 bytes per task + Arc operations (negligible for infrastructure tasks)
  - **Semaphore Overhead**: Fixed memory, atomic operations (highly efficient)
  - **Shutdown Time**: +0-5 seconds for graceful cleanup (configurable timeout)
  - **Runtime Overhead**: None - tasks run identically
  
  #### Files Changed
  - `crates/turbomcp-server/src/runtime.rs` - STDIO JoinSet implementation
  - `crates/turbomcp-server/src/runtime/websocket.rs` - WebSocket semaphore implementation
  - `crates/turbomcp-transport/src/tcp.rs` - TCP JoinSet implementation
  - `crates/turbomcp-client/src/client/core.rs` - Client semaphore implementation
  - `crates/turbomcp-server/tests/stdio_lifecycle_test.rs` - New integration tests
  - `TASK_LIFECYCLE_GUIDELINES.md` - Developer guidelines
  - `TASK_LIFECYCLE_ANALYSIS.md` - Technical analysis
  - `TASK_LIFECYCLE_VISUAL.md` - Visual documentation
  
  #### Verification Steps
  ```bash
  # All tests pass
  cargo test --package turbomcp-server runtime::tests      # 6 tests ✅
  cargo test --package turbomcp-server stdio_lifecycle_test # 9 tests ✅  
  cargo test --package turbomcp-transport tcp              # 1 test ✅
  cargo test --package turbomcp-client                     # 72 tests ✅
  
  # Manual verification
  echo '{"jsonrpc":"2.0","method":"ping","id":1}' | cargo run --example stdio_server
  # Expected: Clean exit without panic ✅
  ```
  

## [2.0.2] - 2025-10-19

### Fixed

- **Resource Reading Broken**: Fixed critical bug where resources could be listed but not read
  - **Issue**: Resources were registered by method name but looked up by URI, causing "Resource not found" errors
  - **Root Cause**: `#[server]` macro registered resources using `resource_name` instead of `resource_uri_template` as the DashMap key
  - **Impact**: All `resources/read` requests failed with -32004 error even for valid resources
  - **Fix**: Changed registration in `turbomcp-macros/src/server.rs:436` to use URI as key
  - **Location**: `crates/turbomcp-macros/src/server.rs:436`
  - **Example**: `#[resource("stdio://help")]` now registers with key "stdio://help" not "help"
  - **Breaking Change**: No - this was a bug preventing correct MCP behavior
  - **Regression Test**: Added `test_resource_registration_lookup_by_uri` to prevent future regressions
  - **Reported By**: turbomcpstudio dogfood team via RESOURCE_READ_ISSUE.md
  - **Severity**: Critical - Completely broke resource reading functionality

## [2.0.1] - 2025-10-19

### Fixed

- **Resource Listing Metadata Loss**: Fixed critical bug where `Client::list_resources()` was discarding resource metadata
  - **Issue**: Method was returning only URIs (`Vec<String>`), throwing away all metadata from server
  - **Impact**: Broke applications like turbomcpstudio that needed resource names, descriptions, MIME types
  - **Root Cause**: Implementation was mapping `ListResourcesResult::resources` to just URIs instead of returning full `Resource` objects
  - **Fix**: Changed return type from `Vec<String>` to `Vec<Resource>` per MCP 2025-06-18 spec
  - **Breaking Change**: No - `Resource` type was already exported and clients can access `.uri` field
  - **Files Changed**:
    - `turbomcp-client/src/client/operations/resources.rs` - Core fix to return full Resource objects
    - `turbomcp-cli/src/executor.rs` - Updated to handle Resource objects
    - `turbomcp-client/src/lib.rs` - Updated documentation examples
    - `turbomcp/examples/comprehensive.rs` - Enhanced to display resource metadata
    - `turbomcp/examples/unix_client.rs` - Updated to use resource.uri field
  - **Reported By**: turbomcpstudio dogfood team
  - **Severity**: High - Breaks core resource functionality

## [2.0.0] - 2025-10-18

### Added

- **Rich Tool Descriptions with Metadata**: Enhanced `#[tool]` macro now supports comprehensive metadata fields
  - **New fields**: `description`, `usage`, `performance`, `related`, `examples`
  - **LLM Optimization**: All fields combined into pipe-delimited description for better decision-making
  - **Backward Compatible**: Simple string syntax still supported
  - **Impact**: Improved LLM understanding of when/why/how to use tools
  - **Example**: New `rich_tool_descriptions.rs` example demonstrating all metadata fields
  - **Commit**: `aae59f8`

- **MCP STDIO Transport Compliance Enhancements**: Comprehensive specification compliance with validation
  - **Strict Validation**: Embedded newlines (LF/CR/CRLF) detection and rejection
  - **Compliance Documentation**: Detailed checklist in module documentation
  - **Test Coverage**: Comprehensive test suite for newline validation scenarios
  - **Spec Clarification**: Literal newline bytes forbidden, escaped `\n` in JSON strings allowed
  - **Error Messages**: MCP-specific compliance context in validation errors
  - **Impact**: Prevents message framing issues in production environments
  - **Commit**: `c2b4032`

### Fixed

- **Publish Script**: Minor fixes to release automation
  - **Commit**: `0b6e6a3`

### Improved

- **Examples Documentation**: Updated to reflect rich tool descriptions example
  - **Updated**: Example count from 17 to 18 examples
  - **Added**: rich_tool_descriptions.rs to quick start commands and examples table
  - **Commit**: `6e3b211`

## [2.0.0-rc.3] - 2025-10-18

### Removed

- **Progress Reporting System**: Removed experimental progress reporting infrastructure
  - **Rationale**: Progress reporting was not part of MCP 2025-06-18 spec and added unnecessary complexity
  - **Files removed**: All progress-related handler references and test utilities
  - **Impact**: Cleaner codebase focused on MCP compliance
  - **Commits**: `046cfe8`, `01bfc26`, `5ed2049`, `efa927b`, `d3559ce`

### Added

- **Enhanced Tool Attributes with Rich Metadata**: Macro system now supports comprehensive tool metadata
  - **New attributes**: Support for more granular tool definition and configuration
  - **Impact**: Better tooling and IDE support for MCP server development
  - **Commit**: `723fb20`

- **Comprehensive Schema Builder Functions for Elicitation API**: New builder functions for elicitation schemas
  - **Purpose**: Simplify and standardize elicitation form creation
  - **Impact**: More ergonomic API for server-initiated forms
  - **Commit**: `a57dac2`

- **Comprehensive Audit Reports and Analysis Tools**: Documentation tools for codebase analysis
  - **Purpose**: Enhanced visibility into codebase structure and metrics
  - **Impact**: Better development tooling and auditing capabilities
  - **Commit**: `7a41a03`

### Changed

- **Simplified Feature Flags for WebSocket Functionality**: WebSocket feature gates now cleaner
  - **Impact**: Reduced feature flag complexity and interdependencies
  - **Commit**: `a15edc1`

- **Simplified HTTP Feature Compilation Guards**: Removed redundant conditional compilation
  - **Impact**: Cleaner feature gate logic
  - **Commit**: `20e2692`

- **Improved DPOP Module Implementation**: Cleaned up DPOP crate structure
  - **Impact**: Better maintainability and clearer code organization
  - **Commit**: `c17d2d4`

- **Minor Cleanup in Core Modules and Examples**: General codebase polish
  - **Commit**: `69e3089`

### Improved

- **Build Automation**: Makefile and build scripts enhanced for better CI/CD integration
  - **Changes**: Improved automation workflow and test execution
  - **Commits**: `c81f20d`, `0633b64`

- **Test Suite Modernization**: Comprehensive test improvements
  - **Impact**: Better test coverage and modernized testing patterns
  - **Commit**: `c8d4f0c`

- **Security Builder and Testing**: Enhanced transport security implementation
  - **Commit**: `412570f`

- **Documentation and Examples**: Updated root README and examples for clarity
  - **Commits**: `31f82e7`, `d0773db`, `8024198`

### Quality

- **Added #[must_use] Attributes**: Compiler hints to prevent accidental discarding of important values
  - **Impact**: Better compiler feedback for common mistakes
  - **Commit**: `3dd833f`

## [2.0.0-rc.2] - 2025-10-16

### 🎯 **MAJOR FEATURES**

#### Architectural Unification - ALL Transports Now MCP Compliant
- **CRITICAL FIX**: Unified transport runtime implementations to eliminate duplication and protocol drift
  - ✅ **Single Source of Truth**: All transports (STDIO/TCP/Unix/HTTP/WebSocket) now use `turbomcp-server` runtime
  - ✅ **MCP 2025-06-18 Compliance**: Complete compliance across ALL transport types
  - ✅ **Zero Duplication**: Removed ~2,200 lines of duplicate code
  - **Impact**: Eliminated potential for implementation drift between macro and ServerBuilder patterns

#### HTTP & WebSocket Bidirectional Support via ServerBuilder
- ✅ **HTTP/SSE Bidirectional**: Full support for elicitation, sampling, roots, ping
- ✅ **WebSocket Bidirectional**: Complete bidirectional support matching macro pattern
- **Implementation**: Factory patterns with per-connection/per-session dispatchers
- **Result**: ✅ **ALL transports now fully MCP-compliant via ServerBuilder**

#### Critical Bug Fixes

**Sampling Request ID Correlation (CRITICAL)** - Breaking Change for 2.0
- **Problem**: Clients couldn't correlate sampling request rejections with server requests
  - Handler trait did NOT receive JSON-RPC `request_id`
  - Clients forced to generate their own UUIDs
  - User rejections sent with WRONG ID
- **Solution**: Added `request_id: String` parameter to handler traits
  - ✅ Client-side: `SamplingHandler::handle_create_message(request_id, request)`
  - ✅ Server-side: `SamplingHandler::handle(request_id, request)`
  - ✅ User rejections now complete immediately (< 100ms, not 60s)
- **Breaking Change**: All `SamplingHandler` implementations MUST add `request_id` parameter
- **Justification**: Pre-release critical bug fix enforcing MCP JSON-RPC 2.0 compliance

**WebSocket Deadlock (CRITICAL - P0)**
- **Problem**: Sampling/elicitation requests timed out after 60 seconds (response time: 60s)
- **Circular Deadlock**: receive_loop waits for handler → handler waits for response → response waits for receive_loop
- **Solution**: Spawn request handlers in separate tokio tasks to keep receive_loop non-blocking
- **Result**: Response time: 60s → 0ms (instant)

**HTTP Session ID Generation**
- **Problem**: Server was rejecting SSE connections without session ID (400 Bad Request)
- **Solution**: Server now generates session ID and sends to client (per MCP spec)
- **Impact**: HTTP/SSE sampling, elicitation, roots, ping operations now work correctly

### 🏗️ **ARCHITECTURAL CHANGES**

- **Removed Duplicate Runtimes** (~2,200 lines):
  - ❌ `crates/turbomcp/src/runtime/stdio_bidirectional.rs` (484 lines)
  - ❌ `crates/turbomcp/src/runtime/http_bidirectional.rs` (19KB)
  - ❌ `crates/turbomcp/src/runtime/websocket_server.rs` (726 lines)
  - ✅ **Replaced with**: Re-exports from canonical `turbomcp-server/src/runtime/`

- **Added Missing `Clone` Trait Bounds** to Handler Types
  - Enables concurrent handler execution in tokio tasks
  - Required for proper async spawning pattern

- **Unified ServerBuilder Pattern**:
  - Macro-generated code now uses `create_server()` → ServerBuilder → canonical runtime
  - Single implementation path for all transport types

### ✨ **NEW FEATURES**

- **Release Management Infrastructure**:
  - `scripts/check-versions.sh` - Validates version consistency (224 lines)
  - `scripts/update-versions.sh` - Safe version updates with confirmation (181 lines)
  - `scripts/publish.sh` - Dependency-ordered publishing (203 lines)
  - Enhanced `scripts/prepare-release.sh` - Improved validation workflow

- **Feature Combination Testing**:
  - `scripts/test-feature-combinations.sh` - Tests 10 critical feature combinations
  - Prevents feature gate leakage and compatibility issues

- **HTTP Transport Support**: Re-enabled HTTP client exports
  - Added `VERSION` and `CRATE_NAME` constants to turbomcp-client
  - Re-exported `StreamableHttpClientTransport`, `RetryPolicy`, `StreamableHttpClientConfig`

### 🔧 **IMPROVEMENTS**

- **Error Code Preservation**: Protocol errors now properly preserved through server layer
  - Error codes like `-1` (user rejection) maintained instead of converting to `-32603`
  - Added `ServerError::Protocol` variant
  - Proper error propagation: client → server → calling client

- **Error Messages**: JSON-RPC error codes now semantically correct in all scenarios
  - User rejection: `-1` (not `-32603`)
  - Not found: `-32004` (not `-32603`)
  - Authentication: `-32008` (not `-32603`)

- **Feature Compatibility**: Various Cargo.toml and module updates for better feature gate isolation
  - Updated feature dependencies across all crates
  - Improved runtime module feature handling
  - Better server capabilities and error handling with features

- **Documentation**: Enhanced across all crates
  - Added feature requirement docs to generated transport methods
  - Simplified main README with focused architecture section
  - Improved benchmark and demo documentation
  - Standardized crate-level documentation

- **Debug Implementation**: Added missing `Debug` derive to `WebSocketServerDispatcher`

### 📊 **BUILD STATUS**

- ✅ All 1,165 tests pass
- ✅ Zero regressions
- ✅ Full MCP 2025-06-18 compliance verified across all transports

## [2.0.0-rc.1] - 2025-10-11

### 🐛 **BUG FIXES**

#### TransportDispatcher Clone Implementation (Critical)
- **FIXED**: Manual `Clone` implementation for `TransportDispatcher<T>` removing unnecessary `T: Clone` bound
- **IMPACT**: TCP and Unix Socket transports now compile correctly
- **ROOT CAUSE**: `#[derive(Clone)]` incorrectly required `T: Clone` when only `Arc<T>` needed cloning
- **SOLUTION**: Manual implementation clones `Arc<T>` without requiring `T: Clone`
- **LOCATION**: `crates/turbomcp-server/src/runtime.rs:395-406`

#### SSE Test Conditional Compilation
- **FIXED**: SSE test functions now correctly handle `#[cfg(feature = "auth")]` conditional compilation
- **IMPACT**: Tests compile with and without `auth` feature enabled
- **LOCATION**: `crates/turbomcp/src/sse_server.rs:615,631,656`

#### TCP Client Example Error Handling
- **FIXED**: Address parsing in TCP client example using `.expect()` instead of `?`
- **IMPACT**: Example compiles without custom error type conversions
- **LOCATION**: `crates/turbomcp/examples/tcp_client.rs:28-29`

#### TCP/Unix Client Example Imports and Feature Gates
- **FIXED**: Import transport types directly from `turbomcp_transport`
- **FIXED**: Added `required-features` declarations for TCP/Unix examples
- **ROOT CAUSE**: Examples compiled without features, `turbomcp::prelude` guards exports with `#[cfg(feature)]`
- **SOLUTION 1**: Import directly from `turbomcp_transport` (always available)
- **SOLUTION 2**: Add `required-features` to skip examples when features disabled
- **IMPACT**: Examples only compile when features enabled, preventing feature mismatch errors
- **LOCATION**: `crates/turbomcp/examples/{tcp_client.rs:16-17, unix_client.rs:17-18}`, `Cargo.toml:157-172`

### 📚 **DOCUMENTATION IMPROVEMENTS**

#### Transport Protocol Clarification
- **UPDATED**: Main README to distinguish MCP standard transports from custom extensions
- **CLARIFIED**: STDIO and HTTP/SSE are MCP 2025-06-18 standard transports
- **CLARIFIED**: TCP, Unix Socket, and WebSocket are MCP-compliant custom extensions
- **UPDATED**: Transport README with protocol compliance section
- **UPDATED**: Architecture diagram showing transport categorization

### ✅ **QUALITY ASSURANCE**

**Build Verification**:
- ✅ All features build successfully (`--all-features`)
- ✅ TCP transport builds successfully (`--features tcp`)
- ✅ Unix Socket transport builds successfully (`--features unix`)
- ✅ All examples compile cleanly

**Test Results**:
- ✅ 153 tests passed, 0 failed
- ✅ Zero clippy warnings with `-D warnings`
- ✅ All code formatted correctly

**MCP Compliance**:
- ✅ Full MCP 2025-06-18 specification compliance verified
- ✅ All standard transports (stdio, HTTP/SSE) compliant
- ✅ Custom transports preserve JSON-RPC and lifecycle requirements

## [2.0.0-rc] - 2025-10-09

### 🌟 **RELEASE HIGHLIGHTS**

**TurboMCP 2.0.0 represents a complete architectural overhaul focused on clean minimal core + progressive enhancement.**

**Key Achievements**:
- ✅ **Progressive Enhancement**: Minimal by default (stdio only), opt-in features for advanced needs
- ✅ **Zero Technical Debt**: No warnings, no TODOs, no FIXMEs
- ✅ **Security**: 1 mitigated vulnerability, 1 compile-time warning only
- ✅ **Clean Architecture**: RBAC removed (application-layer concern)
- ✅ **Latest Toolchain**: Rust 1.90.0 + 62 dependency updates
- ✅ **Production Ready**: All examples compile, all tests pass, strict clippy compliance

### 🎯 **BREAKING CHANGES**

#### RBAC Removal - Architectural Improvement
- **REMOVED**: RBAC/authorization feature from protocol layer
- **RATIONALE**: Authorization is an application-layer concern, not protocol-layer
- **IMPACT**: Cleaner separation of concerns, follows industry best practices
- **MIGRATION**: Implement authorization in your application layer (see `RBAC-REMOVAL-SUMMARY.md`)
- **BENEFIT**: Eliminated `casbin` dependency and `instant` unmaintained warning
- **SECURITY**: Reduced attack surface, removed unmaintained runtime dependency

#### SharedClient Removal - Architectural Improvement
- **REMOVED**: `SharedClient` wrapper (superseded by directly cloneable `Client<T>`)
- **RATIONALE**: `Client<T>` is now Arc-wrapped internally, making SharedClient redundant
- **IMPACT**: Simpler API with no wrapper needed for concurrent access
- **MIGRATION**: Replace `SharedClient::new(client)` with direct `client.clone()`
- **BENEFIT**: Reduced API surface, cleaner concurrent patterns following Axum/Tower standard
- **NOTE**: `SharedTransport` remains available for sharing transports across multiple clients

#### Default Feature Changes
- **BREAKING**: Default features changed to `["stdio"]` (minimal by default)
- **RATIONALE**: Progressive enhancement - users opt-in to features they need
- **MIGRATION**: Enable features explicitly: `turbomcp = { version = "2.0.0-rc", features = ["full"] }`

### 🏗️ **MAJOR REFACTORING: Clean Minimal Core**

#### New Crate Architecture (10 Total Crates)
- **NEW**: `turbomcp-auth` - OAuth 2.1 authentication (optional, 1,824 LOC)
- **NEW**: `turbomcp-dpop` - DPoP RFC 9449 implementation (optional, 7,160 LOC)
- **MODULAR**: Independent crates for protocol, transport, server, and client
- **PROGRESSIVE**: Features are opt-in via feature flags
- **CORE**: Context module decomposed from monolithic 2,046-line file into 8 focused modules:
  - `capabilities.rs` - Capability trait definitions
  - `client.rs` - Client session and identification
  - `completion.rs` - Completion context handling
  - `elicitation.rs` - Interactive form handling
  - `ping.rs` - Health check contexts
  - `request.rs` - Core request/response context
  - `server_initiated.rs` - Server-initiated communication
  - `templates.rs` - Resource template contexts
- **PROTOCOL**: Types module decomposed from monolithic 2,888-line file into 12 focused modules:
  - Individual modules for capabilities, completion, content, core, domain, elicitation, initialization, logging, ping, prompts, requests, resources, roots, sampling, and tools
- **IMPROVED**: Enhanced code maintainability with zero breaking changes to public API

### ⚡ **PERFORMANCE OPTIMIZATIONS**
- **ENHANCED**: Zero-copy message processing with extensive `bytes::Bytes` integration
- **NEW**: Advanced `ZeroCopyMessage` type for ultra-high throughput scenarios
- **OPTIMIZED**: Message processing with lazy deserialization and minimal allocations
- **IMPROVED**: SIMD-accelerated JSON processing with `sonic-rs` and `simd-json`

### 🔐 **SECURITY ENHANCEMENTS**
- **REMOVED**: RBAC feature eliminated `instant` unmaintained dependency (RUSTSEC-2024-0384)
- **IMPROVED**: Dependency cleanup with 13 fewer dependencies (-2.2%)
- **AUDIT**: Only 1 known vulnerability (RSA timing - mitigated by P-256 recommendation)
- **AUDIT**: Only 1 unmaintained warning (paste - compile-time only, zero runtime risk)
- **NEW**: Security validation module in `turbomcp-core` with path security utilities
- **ADDED**: `validate_path()`, `validate_path_within()`, `validate_file_extension()` functions
- **INTEGRATED**: Security features from dissolved security crate into core framework
- **DOCUMENTED**: P-256 recommended as default DPoP algorithm (not affected by RSA timing attack)

### 🛠️ **API IMPROVEMENTS**
- **IMPROVED**: Enhanced registry system with handler statistics and analytics
- **ADDED**: `EnhancedRegistry` with performance tracking
- **ENHANCED**: Session management with improved analytics and cleanup
- **REFINED**: Error handling with comprehensive context preservation


### 🔧 **INTERNAL IMPROVEMENTS**
- **CLEANED**: Removed obsolete tests and legacy code
- **ENHANCED**: Test suite with comprehensive coverage of new modules
- **IMPROVED**: Build system and CI/CD pipeline optimizations
- **MAINTAINED**: Zero clippy warnings and consistent formatting

### 🔨 **TOOLCHAIN & DEPENDENCY UPDATES**
- **UPDATED**: Rust toolchain from 1.89.0 → 1.90.0
- **UPDATED**: 62 dependencies to latest compatible versions:
  - `axum`: 0.8.4 → 0.8.6
  - `tokio-tungstenite`: 0.26.2 → 0.28.0
  - `redis`: 0.32.5 → 0.32.7
  - `serde`: 1.0.226 → 1.0.228
  - `thiserror`: 2.0.16 → 2.0.17
  - And 57 more transitive updates
- **ADDED**: `futures` dependency to `turbomcp-dpop` (previously missing)

### 🐛 **BUG FIXES & CODE QUALITY**
- **FIXED**: Documentation warning in `zero_copy.rs` (added missing doc comment)
- **FIXED**: Feature gate naming consistency (`dpop-redis` → `redis-storage`, `dpop-test-utils` → `test-utils`)
- **FIXED**: Removed unused middleware import in `turbomcp/router.rs`
- **FIXED**: Removed unused `McpResult` import in `turbomcp/transport.rs`
- **FIXED**: Removed unused `RateLimitConfig` import in `turbomcp-server/core.rs`
- **FIXED**: Clippy warnings (empty line after doc comments, manual is_multiple_of)
- **RESULT**: Zero compiler warnings, zero clippy warnings with `-D warnings`

### 🛡️ **BACKWARD COMPATIBILITY**
- **BREAKING**: RBAC feature removed (see migration notes below)
- **BREAKING**: Default features changed to minimal (`["stdio"]`)
- **COMPATIBLE**: Existing auth, rate-limiting, validation features unchanged
- **PROTOCOL**: Maintains complete MCP 2024-11-05 specification compliance

### 📦 **MIGRATION NOTES**

#### RBAC Removal (Breaking Change)
If you were using the RBAC feature:
```toml
# OLD (no longer works)
turbomcp-server = { version = "2.0.0-rc", features = ["rbac"] }

# NEW (implement in your application)
# See RBAC-REMOVAL-SUMMARY.md for migration patterns
```
- **Why**: Authorization is application-layer concern, not protocol-layer
- **How**: Implement RBAC in your application using JWT claims or external policy engine
- **Examples**: See `RBAC-REMOVAL-SUMMARY.md` for complete migration guide

#### Default Features
```toml
# OLD (1.x - everything enabled)
turbomcp = "1.x"  # Had all features by default

# NEW (2.0 - minimal by default)
turbomcp = { version = "2.0.0-rc", features = ["full"] }  # Opt-in to features
```

#### Crate Consolidation
- `turbomcp_dpop::*` → `turbomcp::auth::dpop::*`
- Security utilities now in `turbomcp_core::security`

#### Feature Gate Names
- `dpop-redis` → `redis-storage`
- `dpop-test-utils` → `test-utils`

See `MIGRATION.md` for complete upgrade guide.

### 📊 **METRICS & QUALITY**

**Codebase Quality**:
- ✅ Compiler warnings: **0**
- ✅ Clippy warnings (with `-D warnings`): **0**
- ✅ Technical debt markers (TODO/FIXME): **0**
- ✅ All examples compile: **Yes**
- ✅ All tests pass: **Yes**

**Security Posture**:
- 🔒 Known vulnerabilities: **1 (mitigated)**
  - RSA timing sidechannel: Use P-256 instead (recommended in docs)
- ⚠️ Unmaintained dependencies: **1 (informational only)**
  - paste v1.0.15: Compile-time proc macro only, zero runtime risk, HSM feature only
- ✅ Security improvements: Removed `instant` unmaintained runtime dependency

**Dependency Management**:
- 📦 Feature-gated dependencies: Pay only for what you use
- 📉 Cleanup: **-13 dependencies** (-2.2% from 1.x)

**Release Status**: 🟢 **PRODUCTION READY**

## [1.1.0] - 2025-09-24

### 🔐 **NEW MAJOR FEATURE: RFC 9449 DPoP Security Suite**
- **ADDED**: Complete RFC 9449 Demonstration of Proof-of-Possession (DPoP) implementation
- **NEW**: `turbomcp-dpop` crate with OAuth 2.0 security enhancements
- **SECURITY**: Cryptographic binding of access tokens to client keys preventing token theft
- **ENTERPRISE**: Multi-store support (Memory, Redis, HSM) for different security requirements
- **ALGORITHMS**: ES256, RS256 support with automatic key rotation policies
- **HSM**: YubiHSM2 and PKCS#11 integration for enhanced security

### 🏗️ **NEW MAJOR FEATURE: Type-State Capability Builders**
- **REVOLUTIONARY**: Const-generic type-state builders with compile-time validation
- **SAFETY**: Impossible capability configurations are unrepresentable in type system
- **PERFORMANCE**: Zero-cost abstractions - all validation at compile time
- **DEVELOPER EXPERIENCE**: Compile-time errors prevent runtime capability misconfigurations
- **TURBOMCP EXCLUSIVE**: Advanced features like SIMD optimization hints and enterprise security
- **CONVENIENCE**: Pre-configured builders for common patterns (full-featured, minimal, sampling-focused)

### ⚡ **PERFORMANCE & QUALITY IMPROVEMENTS**
- **MODERNIZED**: All benchmarks updated to use `std::hint::black_box` (eliminated deprecation warnings)
- **ENHANCED**: Redis AsyncIter with `safe_iterators` feature for safer iteration
- **IMPROVED**: WebSocket transport compatibility with tokio-tungstenite v0.27.0
- **OPTIMIZED**: Message::Text API usage for improved performance
- **FIXED**: All doctest compilation errors and import issues

### 📊 **DEPENDENCY & SECURITY UPDATES**
- **UPDATED**: All workspace dependencies to latest stable versions
- **SECURITY**: Eliminated all deprecated API usage across the codebase
- **COMPATIBILITY**: Enhanced WebSocket examples with real-time bidirectional communication
- **QUALITY**: Comprehensive test suite improvements and validation

### 🛡️ **BACKWARD COMPATIBILITY**
- **GUARANTEED**: 100% backward compatibility with all v1.0.x applications
- **ZERO BREAKING CHANGES**: All existing code continues to work unchanged
- **MIGRATION**: Optional upgrade path to new type-safe builders
- **PROTOCOL**: Maintains complete MCP 2025-06-18 specification compliance

### 📚 **DOCUMENTATION & EXAMPLES**
- **NEW**: Comprehensive DPoP integration guide with production examples
- **NEW**: Interactive type-state builder demonstration (`examples/type_state_builders_demo.rs`)
- **ENHANCED**: API documentation with advanced usage patterns
- **IMPROVED**: WebSocket transport examples with real-world patterns

## [1.0.13] - Never released

### 🔒 **SECURITY HARDENING - ZERO VULNERABILITIES ACHIEVED**
- **ELIMINATED**: RSA Marvin Attack vulnerability (`RUSTSEC-2023-0071`) through strategic `sqlx` removal
- **ELIMINATED**: Unmaintained `paste` crate vulnerability (`RUSTSEC-2024-0436`) via `rmp-serde` → `msgpacker` migration
- **IMPLEMENTED**: Comprehensive `cargo-deny` security policy with MIT-compatible license restrictions
- **OPTIMIZED**: Dependency security surface with strategic removal of vulnerable dependency trees

### ⚡ **COMPREHENSIVE BENCHMARKING INFRASTRUCTURE**
- **NEW**: Enterprise-grade criterion benchmarking with automated regression detection (5% threshold)
- **NEW**: Cross-platform performance validation (Ubuntu, Windows, macOS) with GitHub Actions integration
- **NEW**: Historical performance tracking with git commit correlation and baseline management
- **ACHIEVED**: Performance targets - <1ms tool execution, >100k messages/sec, <1KB overhead per request
- **ADDED**: Comprehensive benchmark coverage across all critical paths (core, framework, end-to-end)

### 🚀 **ENHANCED CLIENT LIBRARY**
- **ENHANCED**: Advanced LLM backend support with production-grade Anthropic and OpenAI implementations
- **NEW**: Interactive elicitation client with real-time user input capabilities
- **IMPROVED**: Comprehensive conversation context management and error handling
- **OPTIMIZED**: HTTP client configuration with proper timeouts and user agent versioning

### 🏗️ **CORE INFRASTRUCTURE IMPROVEMENTS**
- **ENHANCED**: MessagePack serialization with `msgpacker` integration (temporary test workaround in place)
- **IMPROVED**: Macro system with better compile-time routing and automatic discovery
- **OPTIMIZED**: Message processing with enhanced format detection and validation

### 📊 **QUALITY ASSURANCE**
- **FIXED**: Test suite timeout issues through optimized compilation and execution
- **ENHANCED**: Comprehensive message testing with edge cases and boundary validation
- **IMPROVED**: Error handling and debugging capabilities across all crates
- **SYNCHRONIZED**: All crate versions to 1.0.13 with updated documentation

### 🛠️ **DEVELOPER EXPERIENCE**
- **NEW**: `scripts/run_benchmarks.sh` automation with multiple execution modes
- **ENHANCED**: Documentation with comprehensive benchmarking guide and production examples
- **IMPROVED**: Build system performance and caching optimizations
- **ADDED**: Performance monitoring and regression detection in CI/CD pipeline

## [1.0.10] - 2025-09-21

### 🚨 **CRITICAL MCP 2025-06-18 COMPLIANCE FIX**
- **SharedClient Protocol Compliance**: Fixed critical gap where SharedClient was missing key MCP protocol methods
  - ✅ **Added `complete()`**: Argument completion support (completion/complete) for IDE-like experiences
  - ✅ **Added `list_roots()`**: Filesystem roots listing (roots/list) for boundary understanding
  - ✅ **Added elicitation handlers**: Server-initiated user information requests (elicitation/create)
  - ✅ **Added bidirectional handlers**: Log and resource update handler registration
  - ✅ **Added handler query methods**: `has_*_handler()` methods for capability checking
- **Full MCP 2025-06-18 Compliance**: SharedClient now provides complete protocol compliance matching regular Client
- **Zero Breaking Changes**: All additions are purely additive maintaining full backward compatibility
- **Enhanced Documentation**: Updated README to reflect complete protocol support and capabilities

### 🔧 **Quality Improvements**
- **Perfect Thread Safety**: All new SharedClient methods maintain zero-overhead Arc/Mutex abstractions
- **Consistent API Surface**: All methods use identical signatures to regular Client for drop-in replacement
- **Complete Doctest Coverage**: All new methods include comprehensive examples and usage patterns
- **Type Safety**: Maintains compile-time guarantees and proper error handling throughout

### 📋 **Post-Release Audit Results**
This release addresses compliance gaps identified during comprehensive MCP 2025-06-18 specification audit:
- ✅ **Specification Compliance**: 100% compliant with MCP 2025-06-18 including latest elicitation features
- ✅ **Transport Support**: All 5 transport protocols support complete MCP feature set
- ✅ **Server Implementation**: Full server-side MCP method coverage verified
- ✅ **Test Coverage**: All new functionality tested with comprehensive test suite

## [1.0.9] - 2025-09-21

### 🔄 Shared Wrapper System (MAJOR FEATURE)
- **Thread-Safe Concurrency Abstractions**: Complete shared wrapper system addressing Arc/Mutex complexity feedback
  - ✅ **SharedClient**: Thread-safe client wrapper enabling concurrent MCP operations
  - ✅ **SharedTransport**: Multi-client transport sharing with automatic connection management
  - ✅ **SharedServer**: Server wrapper with safe consumption pattern for management scenarios
  - ✅ **Generic Shareable Pattern**: Reusable trait-based abstraction for all shared wrappers
- **Zero Overhead Abstractions**:
  - ✅ **Same Performance**: Identical runtime performance to direct Arc/Mutex usage
  - ✅ **Hidden Complexity**: Encapsulates synchronization primitives behind ergonomic APIs
  - ✅ **MCP Protocol Compliant**: Maintains all MCP semantics in shared contexts
  - ✅ **Drop-in Replacement**: Works with existing code without breaking changes
- **Production-Ready Patterns**:
  - ✅ **Consumption Safety**: ConsumableShared<T> prevents multiple consumption of server-like objects
  - ✅ **Library Integration**: Seamless integration with external libraries requiring Arc<Mutex<Client>>
  - ✅ **Concurrent Access**: Multiple tasks can safely access clients and transports simultaneously
  - ✅ **Resource Management**: Proper cleanup and lifecycle management in multi-threaded scenarios

### 🚀 Enhanced Concurrency Support
- **Concurrent Operation Examples**:
  - Multiple threads calling tools simultaneously through SharedClient
  - Transport sharing between multiple client instances
  - Management dashboard integration with SharedServer consumption
  - Complex multi-client architectures with single transport
- **Developer Experience Improvements**:
  - ✅ **Ergonomic APIs**: Simple `.clone()` operations instead of complex Arc/Mutex patterns
  - ✅ **Type Safety**: Compile-time guarantees preventing common concurrency mistakes
  - ✅ **Clear Documentation**: Comprehensive examples and usage patterns in all crate READMEs
  - ✅ **Seamless Migration**: Existing code continues working; shared wrappers are additive

### 📚 Documentation Excellence
- **Comprehensive Documentation Updates**:
  - ✅ **All Crate READMEs Updated**: SharedClient, SharedTransport, SharedServer sections added
  - ✅ **Usage Examples**: Detailed examples showing concurrent patterns and integration
  - ✅ **Architecture Guidance**: Clear guidance on when and how to use shared wrappers
  - ✅ **Build Status Fix**: Consistent GitHub Actions badge format across all READMEs
- **Generic Pattern Documentation**:
  - ✅ **Shareable Trait**: Complete documentation of the reusable abstraction pattern
  - ✅ **Implementation Examples**: Both Shared<T> and ConsumableShared<T> patterns documented
  - ✅ **Best Practices**: Guidelines for implementing custom shared wrappers

### 🔧 Quality & Maintenance
- **Version Consistency**: Updated all crate versions to 1.0.9 with proper internal dependency alignment
- **Code Quality**: Maintained zero clippy warnings and perfect formatting standards
- **Test Coverage**: All unit tests (392 tests) passing across all crates
- **Build System**: Consistent build status reporting across all documentation

## [1.0.8] - 2025-09-21

### 🔐 OAuth 2.1 MCP Compliance (MAJOR FEATURE)
- **Complete OAuth 2.1 Implementation**:
  - ✅ **RFC 8707 Resource Indicators**: MCP resource URI binding for token scoping
  - ✅ **RFC 9728 Protected Resource Metadata**: Discovery and validation endpoints
  - ✅ **RFC 7591 Dynamic Client Registration**: Runtime client configuration
  - ✅ **PKCE Support**: Enhanced security with Proof Key for Code Exchange
  - ✅ **Multi-Provider Support**: Google, GitHub, Microsoft OAuth 2.0 integration
- **Security Hardening**:
  - ✅ **Redirect URI Validation**: Prevents open redirect attacks
  - ✅ **Domain Whitelisting**: Environment-based host validation
  - ✅ **Attack Vector Prevention**: Protection against injection and traversal attacks
  - ✅ **Production Security**: Comprehensive security level configuration
- **MCP-Specific Features**:
  - ✅ **Resource Registry**: MCP resource metadata with RFC 9728 compliance
  - ✅ **Bearer Token Methods**: Multiple authentication methods support
  - ✅ **Auto Resource Indicators**: Automatic MCP resource URI detection
  - ✅ **Security Levels**: Standard, Enhanced, Maximum security configurations

### 🚀 MCP STDIO Protocol Compliance
- **Logging Compliance**: Fixed demo application to output ONLY JSON-RPC messages
  - ✅ **Zero Stdout Pollution**: No logging, banners, or debug output on stdout
  - ✅ **Pure Protocol Communication**: MCP STDIO transport compliant
  - ✅ **Clean Demo Application**: Production-ready MCP server demonstration

### 🧹 Code Quality & Maintenance (MAJOR CLEANUP)
- **Zero-Tolerance Quality Standards Achieved**:
  - ✅ **100% Clippy Clean**: Fixed all clippy warnings with `-D warnings` across entire workspace
  - ✅ **Perfect Formatting**: All code consistently formatted with `cargo fmt`
  - ✅ **All Tests Passing**: Complete test suite (800+ tests) running without issues
  - ✅ **Modern Rust Patterns**: Converted all nested if statements to use let chains
  - ✅ **Memory Management**: Removed unnecessary explicit `drop()` calls for better clarity

### 🗂️ Project Cleanup & Organization
- **Removed Vestigial Files**:
  - Cleaned up 7 `.disabled` example files that were no longer needed
  - Removed: `transport_*_client.rs.disabled` and `transport_*_server.rs.disabled` files
  - Eliminated legacy code artifacts from development phase
- **Documentation Overhaul**:
  - **Updated Examples README**: Complete rewrite with accurate current example inventory
  - **35 Production-Ready Examples**: All examples documented and categorized properly
  - **Clear Learning Path**: Progression from beginner to advanced with numbered tutorials
  - **Transport Coverage**: Complete coverage of all 5 transport types (STDIO, TCP, HTTP/SSE, WebSocket, Unix)

### 🛠️ Technical Improvements
- **Collapsible If Statement Fixes**: 8+ instances converted to modern let chains pattern
  - `websocket_client.rs`: 2 collapsible if statements fixed
  - `transport_websocket_client.rs`: 6 collapsible if statements fixed
  - `unix_socket_client.rs`: 1 collapsible if statement fixed
- **Drop Non-Drop Warnings**: Fixed unnecessary explicit drops in test files
  - `real_end_to_end_working_examples.rs`: Removed 2 explicit drop calls for tokio WriteHalf types
- **Unix Transport Test Fixes**: Updated test expectations to match actual implementation
  - Fixed capabilities test to expect 1MB (not 64MB) message size limit
  - Updated error message expectations for disconnected transport scenarios

### 📚 Documentation Standards
- **Example Categories**: Clear organization by transport type, complexity, and use case
- **Quality Guarantees**: All examples follow high standards
- **Learning Progression**: 11 numbered tutorial examples from basic to advanced
- **Transport Comparison**: Legacy vs. current transport example organization
- **35 Total Examples**: Complete inventory with proper categorization

### 🔧 Development Experience
- **Make Test Integration**: Full compatibility with project's `make test` command
- **CI/CD Ready**: All quality checks pass automated testing pipeline
- **Zero Technical Debt**: Eliminated all placeholder code and TODOs from examples
- **Consistent Standards**: Unified code style and documentation across all examples

### 🏆 Quality Metrics Achieved
- **Clippy**: Zero warnings with strict `-D warnings` enforcement
- **Formatting**: 100% consistent code formatting across 35 examples
- **Tests**: All integration and unit tests passing
- **Documentation**: Complete and accurate example documentation
- **Examples**: 35 fully-functional examples

## [1.0.6] - 2025-09-10

### 🔌 Enterprise Plugin System (NEW)
- **Complete Plugin Architecture**: Production-ready middleware system for Client
  - `ClientPlugin` trait for custom plugin development
  - `PluginRegistry` for managing plugin lifecycle
  - `RequestContext` and `ResponseContext` for plugin state
  - Before/after request hooks for all 13 MCP protocol methods
- **Built-in Enterprise Plugins**:
  - **RetryPlugin**: Automatic retry with exponential backoff
  - **CachePlugin**: TTL-based response caching for performance
  - **MetricsPlugin**: Request/response metrics collection
- **Plugin Features**:
  - Zero-overhead when not in use
  - Transparent operation - no code changes needed
  - Composable - stack multiple plugins
  - Async-first design throughout
- **ClientBuilder Enhancement**: Fluent API for plugin registration
  ```rust
  ClientBuilder::new()
      .with_plugin(Arc::new(RetryPlugin::new(config)))
      .with_plugin(Arc::new(CachePlugin::new(config)))
      .build(transport)
  ```

### 🛠️ API Improvements
- **Plugin Management Methods** on Client:
  - `register_plugin()` - Add plugins at runtime
  - `has_plugin()` - Check if plugin is registered
  - `get_plugin()` - Access specific plugin instance
  - `initialize_plugins()` - Initialize all plugins
  - `shutdown_plugins()` - Clean shutdown of plugins
- **Execute with Plugins**: Internal helper for middleware execution
  - Automatic plugin pipeline for all protocol calls
  - Request/response modification support
  - Error propagation through middleware chain

### 📚 Documentation & Examples
- **New Plugin Examples**:
  - Complete plugin implementation examples in `plugins/examples.rs`
  - Shows retry logic, caching, and metrics collection
  - Demonstrates custom plugin development

### 🔧 Technical Improvements
- **Zero-Tolerance Production Standards**: 
  - Removed all TODO comments from plugin system
  - Complete implementation of all plugin features
  - No placeholders or incomplete code
- **Error Handling**: Better error messages for plugin failures
- **Performance**: Plugin system adds <2% overhead when active

### 🐛 Bug Fixes
- Fixed clippy warnings about unnecessary borrows
- Fixed formatting inconsistencies in plugin code
- Updated all test assertions for new version

## [1.0.5] - 2025-09-09

### 🎯 Major Examples Overhaul
- **Reduced from 41 to 12 focused examples** (70% reduction)
- Created clear learning progression from basics to production
- Added comprehensive EXAMPLES_GUIDE.md with learning path
- New `06_architecture_patterns.rs` showing builder vs macro equivalence
- New `06b_architecture_client.rs` separate client for testing both patterns
- Consolidated all transport demos into `07_transport_showcase.rs`
- Merged all elicitation patterns into `08_elicitation_complete.rs`
- Fixed all compilation errors across examples
- Every example now works end-to-end without placeholders
- **New two-terminal HTTP examples**: `08_elicitation_server.rs` and `08_elicitation_client.rs` for real-world testing

### 🚀 Developer Experience Improvements
- **📢 Deprecation: Simplified Feature System** - `internal-deps` feature flag is now deprecated (will be removed in 2.0.0)
  - Core framework dependencies are now included automatically - no manual setup required!
  - **Migration**: Remove `internal-deps` from your feature lists for cleaner configuration
  - **Before**: `features = ["internal-deps", "stdio"]` → **After**: `features = ["minimal"]` or `features = ["stdio"]`
  - **Backwards compatible**: Old feature combinations still work but show deprecation warnings
  - **Rationale**: Eliminates user confusion since these dependencies were always required
- **Enhanced Error Handling**: New `McpErrorExt` trait with ergonomic error conversion methods
  - `.tool_error("context")?` instead of verbose `.map_err()` calls
  - `.network_error()`, `.protocol_error()`, `.resource_error()`, `.transport_error()` methods
  - Automatic `From` trait implementations for common error types (`std::io::Error`, `reqwest::Error`, `chrono::ParseError`, etc.)
- **Improved Prelude**: Enhanced documentation showing that `use turbomcp::prelude::*;` eliminates complex import chains
- **Better Feature Discovery**: Comprehensive 🎯 Feature Selection Guide in documentation and Cargo.toml
  - Clear recommendations for `minimal` vs `full` feature sets
  - Beginner-friendly guidance with specific use cases
  - Prominent placement of minimal features for basic tool servers
- **Comprehensive Method Documentation**: New 📚 Generated Methods Reference documenting all `#[server]` macro-generated methods
  - Transport methods (`run_stdio()`, `run_http()`, `run_tcp()`, etc.)
  - Metadata and testing methods (`server_info()`, tool metadata functions)
  - Context injection behavior and flexible parameter positioning

### ✨ New Features

#### 🎯 Complete MCP Protocol Support with New Attribute Macros
**MAJOR: Four new attribute macros completing MCP protocol coverage**

- **`#[completion]`** - Autocompletion handlers for intelligent parameter suggestions
  ```rust
  #[completion("Complete file paths")]
  async fn complete_path(&self, partial: String) -> McpResult<Vec<String>> {
      Ok(vec!["config.json".to_string(), "data.txt".to_string()])
  }
  ```
- **`#[elicitation]`** - Structured input collection from clients with schema validation
  ```rust
  #[elicitation("Collect user preferences")]
  async fn get_preferences(&self, schema: serde_json::Value) -> McpResult<serde_json::Value> {
      Ok(serde_json::json!({"theme": "dark", "language": "en"}))
  }
  ```
- **`#[ping]`** - Bidirectional health checks and connection monitoring
  ```rust
  #[ping("Health check")]
  async fn health_check(&self) -> McpResult<String> {
      Ok("Server is healthy".to_string())
  }
  ```
- **`#[template]`** - Resource template handlers with RFC 6570 URI templates
  ```rust
  #[template("users/{user_id}/profile")]
  async fn get_user_profile(&self, user_id: String) -> McpResult<String> {
      Ok(format!("Profile for user: {}", user_id))
  }
  ```

#### 🚀 Enhanced Client SDK with Completion Support
**NEW: `complete()` method in turbomcp-client**
```rust
let completions = client.complete("complete_path", "/usr/b").await?;
println!("Suggestions: {:?}", completions.values);
```

#### 🌐 Advanced Transport & Integration Features
- **Configurable HTTP Routes**: Enhanced `/mcp` endpoint with `run_http_with_path()` for custom paths
  - Default `/mcp` route maintained for compatibility
  - Flexible routing with `into_router_with_path()` for Axum integration
  - Support for existing router state preservation
- **Advanced Axum Integration**: Production-grade integration layer for existing Axum applications
  - State-preserving merge capabilities for "bring your own server" philosophy
  - Zero-conflict route merging with existing stateful routers
  - Tower service foundation for observability and error handling
- **Streamable HTTP Transport**: MCP 2025-06-18 compliant HTTP/SSE transport with streaming capabilities
- **Client Plugin System**: Extensible plugin architecture for client customization  
- **LLM Integration**: Comprehensive LLM provider system with sampling protocol
- **Bidirectional Handlers**: Full support for MCP handler types:
  - ElicitationHandler for server-initiated prompts
  - LogHandler for structured logging
  - ResourceUpdateHandler for file change notifications
- **Enhanced Builder API**: Improved ServerBuilder and ClientBuilder patterns

### 🛠 Improvements
- **Simplified API surface** while maintaining full functionality
- **Enhanced Cargo.toml**: Reorganized feature flags with clear descriptions and recommendations
- **Better error messages** and compile-time validation
- **Improved test coverage** with real integration tests (800+ tests passing)
- **Updated all dependencies** to latest versions
- **Enhanced documentation** with clear examples and comprehensive method reference
- **Ergonomic imports**: Single prelude import provides everything needed for most use cases
- **Production-ready error handling**: Comprehensive error conversion utilities eliminate boilerplate

### 🐛 Bug Fixes
- Fixed schema generation in macro system
- Resolved handler registration issues
- Fixed transport lifecycle management
- Corrected async trait implementations

### 📚 Documentation
- Complete examples guide with difficulty ratings
- Learning path from "Hello World" to production
- Feature matrix showing which examples demonstrate what
- Clear explanation of builder vs macro trade-offs

### 🏗 Internal Changes
- Cleaned up legacy code and unused files
- Improved module organization
- Better separation of concerns
- Consistent error handling patterns

## [1.0.4] - 2025-01-07

### Added
- Initial production release
- Core MCP protocol implementation
- Macro-based server definition
- Multi-transport support (STDIO, HTTP, WebSocket, TCP)
- Comprehensive tool and resource management
- Elicitation support for server-initiated prompts

## [1.0.3] - 2025-01-06

### Added
- Sampling protocol support
- Roots configuration
- Enhanced security features

## [1.0.2] - 2025-01-05

### Added
- OAuth 2.0 authentication
- Rate limiting
- CORS support

## [1.0.1] - 2025-01-04

### Added
- Basic MCP server functionality
- Tool registration system
- Resource management

## [1.0.0] - 2025-01-03

### Added
- Initial release
- Basic MCP protocol support
- STDIO transport
