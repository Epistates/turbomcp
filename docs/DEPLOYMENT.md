# Deploying v4 and migrating during the alpha

v4 remains a prerelease. The supported core revisions are `2025-06-18`,
`2025-11-25`, and `2026-07-28`. Stdio and Streamable HTTP implement MCP
transports; WebSocket is a convenience transport. Tasks is implemented;
Apps currently reserves an identifier and is not a supported implementation.

## Server contracts

Define a server with `#[server]`, build it with `into_server()`, and serve
stdio or enable `http` and call `run_http`. The HTTP facade automatically wires
the dispatcher's session ownership backend. Applications assembling an axum
router themselves must set `HttpConfig::with_session_terminator` to the same
dispatcher's backend when using authenticated legacy sessions. Otherwise the
HTTP transport refuses that session mode. Put TLS at the server or a trusted
reverse proxy; configure allowed hosts/origins and trusted proxies explicitly.

A callable tool must resolve through `WithTools::lookup_tool`. Macro-generated
tools have a direct lookup with process-lifetime cached metadata; custom schema
derivation for those tools must be deterministic. Use a custom provider for
dynamic definitions. The default implementation for custom providers
walks their catalog, rejecting repeated cursors and excessive pagination.
Resources, templates, and prompts have corresponding lookup methods. A provider
may override lookup to use its own index. Lookup failures propagate; installed
visibility policies never turn absence or listing failure into permission.
Treat the provider's returned definition as the authoritative contract for that
invocation. Dynamic providers should update lookup and enumeration together.

Tool arguments are checked against the advertised JSON Schema before invocation,
including `schema_extend` constraints. Successful structured output is checked
against `outputSchema`. These checks also apply to task-augmented calls. Invalid
schemas fail calls without invoking the handler. Fix invalid schemas during
application tests before serving traffic. Validators are cached by schema value
in a bounded cache, so changed definitions do not reuse an old validator.

Legacy sessions bind to the creating issuer and subject, rather than an access
token; refreshing credentials preserves the session. Another principal cannot
POST, subscribe with GET, or DELETE that session. Anonymous sessions remain
anonymous. Custom `SessionBackend` implementations must persist the new
`SessionState::owner`; custom `SessionTerminator` implementations must implement
`owns` and check ownership during termination. Do not use session IDs as proof
of authentication. MRTR continuations also bind method, arguments, and principal. Stateless tasks
likewise bind reads, updates, cancellation, and subscriptions to their creator.
Custom extensions now receive the authenticated request context in `on_subscribe`
so they can enforce ownership before registering notification routes.

## Authenticated clients

Enable the facade's `client-oauth` feature. Create an `auth::client::OAuthClient`
with the resource URI, redirect URI, and registration strategy. Wrap it in
`client::oauth::OAuthSession` with an application implementation of
`AuthorizationHandler`. Attach an `Arc` of that session using
`HttpClientTransport::with_bearer_source`, then use `ClientBuilder::connect`.
The handler opens the authorization URL using the application's consent UI and
returns `CallbackParams`; the SDK verifies state, issuer, and PKCE.

`OAuthSession` coordinates concurrent authorization and refresh, rediscovers
issuer changes, and unions scopes for step-up. Failed refreshes are shared by
waiting callers; a subsequent resource challenge can initiate fresh consent. Each POST permits at most three
challenge retries; network errors and unrelated HTTP failures are not retried.
Choose a request timeout long enough for your consent UI. The timeout includes
queue admission and authorization. Call `client.connection().close().await`
when deterministic connection teardown is needed; this closes all clones.

`ClientError::Http` preserves the status, bounded decoded JSON-RPC error,
`WWW-Authenticate`, and `Retry-After`. `as_rpc`/`rpc_code` also see nested protocol
errors. Do not parse error text to decide whether to authorize or retry.
Authenticated HTTP response caching is disabled because a mutable bearer source
can change identity or granted scopes between calls.

OAuth discovery, registration, token exchange, and JWKS fetches use clients that
disable redirects and environment proxies, enforce deadlines, and cap response bodies at 1 MiB. HTTPS is
required except configured loopback HTTP. Internal HTTPS authorization servers
remain supported by default. Server-side clients accepting untrusted endpoint
URLs should select `NetworkPolicy::public_only()`: it rejects private/reserved
literals and DNS results, uses the checked addresses for connection, and disables
proxies. This policy also disables loopback HTTP. A custom reqwest client is an
explicit trust override: its owner must preserve redirect and DNS restrictions.
Use `with_network_policy` when those restrictions should be supplied by the SDK.

JWT validation checks signatures, issuer, audience, expiration, and `nbf` with
configured clock skew. `IssuerValidators` binds independent issuers to separate
validators and key sources. `JwtValidator::add_issuer` deliberately trusts every
key in that validator for every added issuer; use it only for shared key trust.
JWKS refreshes coalesce and failed refreshes establish a cooldown.

## Resource bounds and overload

The stream driver defaults to 1,024 concurrent application calls and an outbound
buffer of 1,024. New requests at capacity receive a protocol error; control
traffic has a separate bounded budget. A full cancellation queue closes the
client connection instead of spawning detached waiters. Stalled writes and
shutdown are deadline-bounded. Configure server stream limits with `ServeConfig`.

Client defaults bound pending requests and queued frames to 1,024 each, inbound
callbacks to 128, HTTP POSTs to 1,024, and HTTP JSON bodies/SSE events to 1 MiB. Configure HTTP budgets with
`HttpClientTransport::with_limits` before connecting.
Request admission uses the same deadline as response waiting. HTTP pumps and
standalone streams are owned by the transport and cancelled on drop/close.

`HttpConfig` defaults to 1,024 concurrent HTTP requests, including response
stream lifetimes, a 60-second deadline to response headers, and a 30-second
shutdown deadline. Admission precedes authentication. Excess requests receive
429. Configure these budgets for your deployment; long-lived SSE connections
consume slots. The stream lifetime is not limited by the header deadline.

## Evidence and release decisions

Use the locked workspace tests, Clippy, MSRV check, pinned conformance inventories,
and cross-SDK interoperability suite together. Inventory keys count distinct
scenario/check pairs; repeated probe requests do not add coverage. Review both
new skips and inventory changes. Corrected modern fixtures require discovery
headers; obsolete initialization methods are not sent to manufacture coverage.

Benchmarks must validate successful fixtures before timing. Publish the command,
commit/diff, toolchain, hardware, features, protocol, and raw results. A duplex
microbenchmark does not predict HTTP latency, memory under load, or deployment
throughput. Stable release and performance leadership additionally require
sustained load/fuzz evidence and feedback from real deployments.

The alpha client conformance gate uses [hash-pinned fixture corrections](../crates/turbomcp-conformance/fixtures/README.md).
It scores the legacy replay scenario on supported 2025-11-25 and requires modern
discovery headers rather than removed initialization messages. Corrected client
and unmodified server gates have zero failures, skips, or warnings; unmodified
upstream client results remain reproducible with their own explicit inventory.

Legacy POST SSE streams carrying event IDs resume through GET after graceful
closure, honoring retry delays up to 30 seconds and forwarding the
request's cursor, session, protocol version, and current bearer. The POST is not
replayed. Longer retry delays end recovery rather than reconnecting too early. Cancellation and the original request deadline cover recovery; a final
response ends pumping. Modern streams do not use this legacy recovery path.

Trusted reverse proxies must append or replace `X-Forwarded-For` correctly.
Forwarded addresses are used only when the socket peer is explicitly trusted;
parsing stops at the first untrusted hop. A malformed hop inside the trusted
chain falls back to the socket peer rather than skipping to attacker input.
This HTTP reverse-proxy support is distinct from a standalone MCP proxy product.
