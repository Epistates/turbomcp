# turbomcp-proxy

The embeddable MCP gateway for [TurboMCP](https://github.com/Epistates/turbomcp) v4.

A `RemoteServer` connects to an upstream MCP server (a command over stdio, or
a Streamable HTTP / WebSocket endpoint) and serves it as if it were local: it
implements the same capability traits a `#[server]` does by forwarding to the
upstream. So everything that applies to a local server applies to a remote
one:

- **Bridge** a server across transports and revisions: a stdio-only server on
  HTTP, a `2025-11-25` server to `2026-07-28` clients and back.
- **Aggregate** many behind one endpoint: mount remotes beside your own tools
  in a `Composite`, under a prefix (`github__create_issue`) or flat.
- **Govern** them with the stack you already run: authentication, rate
  limits, per-caller visibility (a hidden upstream tool is indistinguishable
  from a nonexistent one), interceptors, OpenTelemetry.

```rust
use turbomcp::prelude::*;
use turbomcp::proxy::{OutboundAuth, RemoteServer, Upstream};
use turbomcp::http::{Http, HttpConfig};

let github = RemoteServer::builder(Upstream::http("https://api.githubcopilot.com/mcp/"))
    .auth(OutboundAuth::bearer(std::env::var("GITHUB_TOKEN")?))
    .connect()
    .await?;
let files = RemoteServer::connect(Upstream::stdio(
    "npx", ["-y", "@modelcontextprotocol/server-filesystem", "/srv/docs"],
))
.await?;

let gateway = Composite::new(Implementation::new("gateway", "1.0.0"))
    .mount("github", github.clone().into_server())?
    .mount("files", files.clone().into_server())?
    .into_server()
    .layer(tower::layer::util::Identity::new());
github.forward_changes_to(gateway.notifier());
files.forward_changes_to(gateway.notifier());
gateway.serve(Http::bind(addr).config(HttpConfig::new())).await?;
```

What a remote advertises is what its upstream advertised in the handshake.
The caller's token is never passed upstream: the proxy authenticates as
itself, with a static token or OAuth client credentials at the upstream's
own authorization server (`OutboundAuth`, feature `oauth`), stepping up
there when the upstream asks for more scope. A stdio upstream inherits only
what a program needs of the proxy's environment, and HTTP and WebSocket
upstreams can be held to a `NetworkPolicy` (SSRF and DNS rebinding checked
at every connect). Cancellation, progress and trace context cross the hop, and a stdio
upstream gets the specification's shutdown sequence (stdin closed, then
`SIGTERM`, then `SIGKILL`, to its whole process group).

An upstream that asks for input (elicitation, sampling, roots) asks the
downstream caller whose call caused it, whatever revision each side speaks:
a `2025-*` caller is asked inline, a `2026-07-28` caller gets an
`InputRequiredResult` and retries. A request the proxy can't attribute to a
caller is refused, never shown to someone it may not belong to.

Which upstream connection serves a call is its `UpstreamKey`: one shared
connection, one per authenticated caller, or one per downstream session.
The default follows the upstream. A `2026-07-28` or Streamable HTTP upstream
says whose each request for input is, so one connection serves everyone; a
`2025-*` stdio upstream doesn't, so each caller gets its own (and its own
child process, which also keeps per-user upstream state apart). Idle
connections close, and a dead one is replaced on the next call.

```rust,ignore
let remote = RemoteServer::builder(Upstream::stdio("my-server", ["--stdio"]))
    .key(UpstreamKey::Session)
    .idle_timeout(Duration::from_secs(300))
    .connect()
    .await?;
// Close each session's upstream when the session ends.
let server = remote.clone().into_server().observe_sessions(Arc::new(remote));
```

Use it through the facade: `turbomcp = { version = "4", features = ["proxy", "http"] }`.
