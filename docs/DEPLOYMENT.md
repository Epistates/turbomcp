# Deploying v4 and migrating during the alpha

v4 remains a prerelease. The supported core revisions are `2025-06-18`,
`2025-11-25`, and `2026-07-28`. Stdio and Streamable HTTP implement MCP
transports; WebSocket is a convenience transport, served as a route on the
HTTP endpoint (`HttpConfig::with_websocket`) so its authentication, Origin and
Host policy, and rate limits apply before the upgrade. Tasks is implemented;
Apps currently reserves an identifier and is not a supported implementation.

## Server contracts

Define a server with `#[server]`, build it with `into_server()`, add any
middleware with `.layer(…)`, and `.serve(stdio())` or (feature `http`)
`.serve(Http::bind(addr))`. The runtime wires the dispatcher's session
ownership backend, its supported revisions and graceful listen close on
every transport. `turbomcp::http::router(server, config)` does the same for an
axum router you mount yourself; pass it the `Server` (or the builder's
`.layer(…)`), not a bare dispatcher, or authenticated legacy sessions are
refused for want of a session terminator. Put TLS at the server or a trusted
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
required except configured loopback HTTP. The OAuth client engine
(`OAuthClient`) defaults to `NetworkPolicy::public_only()`, because discovery
follows URLs the MCP server chooses: it rejects private/reserved literals and
DNS results, uses the checked addresses for connection, disables proxies, and
disables loopback HTTP. A client of a trusted internal or local server opts out
with `with_network_policy(NetworkPolicy::default())`. Where a deployment only
has to reach one internal authorization server, name its range with
`with_allowed_ranges` rather than turning the policy off; the ranges are
consulted after the reserved-range check, so nothing else reopens.
`NetworkPolicy::default()` (private HTTPS allowed) remains the default for the
resource-server side, whose JWKS and issuer URLs are the operator's own. A custom reqwest client is an
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

`HttpConfig` defaults to 1,024 concurrent HTTP requests (`max_concurrent_requests`),
a 60-second deadline to response headers (`request_timeout`), and a 30-second
shutdown deadline. Admission precedes authentication. A request counts from
admission until its response has been sent, including a tool call whose
response streams; past the limit it receives `503` + `Retry-After`.

The header deadline does not cap how long a tool runs. A request still working
after 5 seconds (`sse_upgrade_after`) gets its headers then and an SSE
response, held open by keep-alive comments every 15 seconds (`sse_keepalive`)
until the result arrives; quicker requests get plain JSON. Keep both intervals
under the idle timeout of any proxy or load balancer in front (nginx's
`proxy_read_timeout` and an AWS ALB's idle timeout both default to 60 s), and
disable response buffering there for SSE (the endpoint sends
`X-Accel-Buffering: no` for nginx).

Long-lived streams (a legacy session's `GET` stream, a `subscriptions/listen`
stream) have their own budget and give their request slot back once open:
1,024 in all (`max_streams`, `503` past it) and 64 per caller
(`max_streams_per_client`, keyed by authenticated subject or else client IP;
`429` past it). A session has one `GET` stream at a time: a reconnecting `GET`
ends the previous one, and a session that is deleted or expires ends its
stream. Every long-lived stream ends at shutdown, so an open client connection
does not hold the drain for the full shutdown deadline.

## Running more than one replica

What a replica fleet needs depends on the wire a client speaks.

**`2026-07-28` is stateless.** Each request carries everything the server
needs, so round-robin load balancing works, with three things to set up:

- **MRTR state key.** A tool that asks the client something answers with an
  `InputRequiredResult` whose `requestState` the client sends back on the
  retry, possibly to another replica. That state is signed with a per-process
  random key unless every replica shares one: `ServerBuilder::with_state_key`.
- **Tasks extension.** A task lives in the process that created it. The client
  sends `Mcp-Name: <taskId>` on `tasks/get`, `tasks/update` and
  `tasks/cancel` so a load balancer can route polls to that replica (hash on
  the header), or implement a shared store behind the task seam.
- **Change notifications.** A `subscriptions/listen` stream is held by one
  replica, and `ServerNotifier` reaches the streams in its own process. Publish
  a change on every replica (each watching the same source of truth); there is
  no cross-replica notification bus yet.

**`2025-06-18` and `2025-11-25` need sticky sessions.** Elicitation and
sampling answers, cancellation, progress, the session's `GET` stream and its
resource subscriptions are bound to the process handling the session, so every
request of a session has to reach the same replica. The working recipe:

1. A shared `SessionBackend` (`ServerBuilder::with_session_backend`).
   `initialize` arrives without a session id, so it lands anywhere, and the
   replica the session later hashes to has to find the session that a
   different replica minted.
2. Consistent hashing on the `Mcp-Session-Id` header, so every later request
   of the session (POSTs, the `GET` stream, `DELETE`) reaches one replica.

nginx:

```nginx
upstream mcp {
    hash $http_mcp_session_id consistent;
    server 10.0.0.11:8080;
    server 10.0.0.12:8080;
}
```

Envoy (route action):

```yaml
hash_policy:
  - header:
      header_name: mcp-session-id
```

When a replica goes away, its sessions' requests hash elsewhere and find the
session in the shared store, but its in-memory routes are gone: in-flight
calls fail and resource subscriptions must be renewed. Clients reconnect their
`GET` stream on their own.

A session-store outage answers `503` + `Retry-After` (clients retry), not
`404` (which would send every client to re-`initialize` at once). The bundled
store refuses new sessions with `503` when full rather than evicting live
ones; idle sessions expire after an hour.

**Health.** `HttpConfig::with_health_check("/healthz")` answers `200` while
serving and `503` once shutdown begins, so a load balancer stops routing to a
draining replica. Long-lived streams end at shutdown and clients reconnect to
another replica.

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
`with_trusted_proxies` takes addresses or CIDR ranges (a pod network, a load
balancer subnet). Forwarded addresses are used only when the socket peer is
trusted; every `X-Forwarded-For` line is read, in order, and parsing stops at
the first untrusted hop. A malformed hop inside the trusted chain falls back
to the socket peer rather than skipping to attacker input. This HTTP
reverse-proxy support is distinct from a standalone MCP proxy product.

`with_rate_limiter` charges authenticated callers per subject, which needs a
verified token, so requests that fail authentication never reach it.
`with_ip_rate_limiter` is the cheap first gate: it charges every request per
client IP before the body is read or a token checked. Give it a generous
quota, since callers behind one NAT share it.
