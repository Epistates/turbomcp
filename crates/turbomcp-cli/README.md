# turbomcp-cli

The `turbomcp` command: serve the servers in an `mcpServers` configuration as
one MCP server, and run protocol operations against any MCP server.

```sh
cargo binstall turbomcp-cli     # the prebuilt binary for your platform
cargo install turbomcp-cli      # or build it
```

Each [GitHub release](https://github.com/Epistates/turbomcp/releases) also
carries the archives themselves (macOS, Linux glibc and static musl, Windows),
their checksums, and shell and PowerShell installers; `gh attestation verify
<file> --repo Epistates/turbomcp` checks that a download was built by this
repository's release workflow. While 4.0 is in prerelease, name the version:
`cargo binstall turbomcp-cli@<version>`.

## The gateway

```sh
turbomcp proxy --config servers.json                  # over stdio
turbomcp proxy --config servers.json --http 127.0.0.1:8080
```

The configuration is the one MCP hosts already share: Claude Desktop's,
Cursor's and Windsurf's `mcpServers`, VS Code's `servers` (`mcp.json`), or
its `mcp.servers` (`settings.json`).

```json
{
  "mcpServers": {
    "files": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/srv/docs"]
    },
    "issues": {
      "url": "https://mcp.example.com/mcp",
      "headers": { "Authorization": "Bearer ${ISSUES_TOKEN}" }
    }
  }
}
```

Every server's tools and prompts are served under its name (`files__read_file`,
`issues__create`; `--separator` changes the joiner); resource URIs pass through
untouched. One entry in a host's own configuration (`turbomcp proxy --config
…` over stdio) then stands for all of them. The servers can speak any
revision, and so can the host: the gateway translates between `2025-06-18`,
`2025-11-25` and `2026-07-28` in both directions, and passes a server's
requests for input (elicitation, sampling, roots) to the caller whose call
caused them. Elicitations reach the caller named after the server that
asks (`files: Choose a folder`); sampling (which runs the caller's model on
the server's prompt) and roots (the caller's files) are passed on only for
servers whose entry allows them.

- `${VAR}` and `${env:VAR}` in any string come from the environment, so
  secrets stay out of the file; `${VAR:-default}` falls back, and
  `${workspaceFolder}`, `${userHome}` and `${pathSeparator}` work as in VS
  Code. An unset variable with no fallback is an error, not an empty string.
- A command server inherits only what a program needs of the gateway's
  environment (`HOME`, `PATH`, `USER`, …, the official SDKs' list); give it
  more under `env`, or set `"inherit": "all"`.
- Each entry is read on its own: one the gateway can't use (`type: "sse"`,
  the 2024-11-05 transport the current revisions replaced with Streamable
  HTTP; a typo) is skipped with its reason, and `--only a,b` never reads the
  others. `"disabled": true` skips one.
- A server that can't be reached at startup is retried for `--retry-for`
  seconds (default 30: one still starting, or `npx` still downloading it),
  then left out until the next start. `--strict` makes either fatal.
- `--http` beyond loopback requires authentication: `--auth-issuer` and
  `--auth-jwks` check each token's signature, `iss`, `exp` and `aud` (the
  endpoint's URL, or `--auth-audience`), `--auth-scopes` adds a baseline,
  and the RFC 9728 metadata clients need to find the issuer is served at
  the well-known path. Without them the gateway refuses to start unless
  given `--insecure`. `--allow-origin` lets a browser client in;
  `--allow-host` names the host clients reach it by behind a proxy. `--http`
  is plain HTTP: terminate TLS in front of it.
- Upstream connections are capped (`--max-connections`, and
  `--max-connections-per-caller` for servers keyed per caller or session):
  past a cap a call is refused, never another caller's connection closed.
- `SIGTERM` or Ctrl-C stops the gateway: in-flight `--http` requests get
  `--drain` seconds, and every command server gets the stdio shutdown
  sequence.

An entry can also say how the gateway treats its server:

```json
{
  "url": "https://mcp.example.com/mcp",
  "timeout": 120,
  "key": "principal",
  "forwardSampling": true,
  "forwardRoots": false,
  "tags": ["internal"],
  "auth": {
    "clientCredentials": {
      "issuer": "https://auth.example.com",
      "clientId": "gateway",
      "clientSecret": "${GATEWAY_SECRET}"
    }
  }
}
```

`timeout` is how long the server may go silent on a call (progress restarts
the clock). `key` is `global` (one connection for every caller),
`principal` (one per authenticated caller) or `session`. `auth` is
`{"bearer": token}`, `{"clientCredentials": account}` (the gateway calls as
itself), or `{"tokenExchange": account}`, which exchanges each caller's own
token for one issued for the server and so needs `--auth-issuer`. For rate
limits, per-caller visibility or your own tools beside the remote ones,
embed [`turbomcp-proxy`](../turbomcp-proxy) and compose it with the rest of
TurboMCP.

```sh
turbomcp proxy --config servers.json --http 0.0.0.0:8080 \
  --auth-issuer https://auth.example.com \
  --auth-jwks https://auth.example.com/.well-known/jwks.json \
  --auth-audience https://mcp.example.com/mcp
```

## Protocol operations

A server is a URL (`http(s)://`, `ws(s)://`), a command and its arguments run
over stdio, or `--config FILE --server NAME`. Options go before it.

```sh
turbomcp tools https://mcp.example.com/mcp --bearer "$TOKEN"
turbomcp call read_file -a path=/srv/docs/README.md npx -y @modelcontextprotocol/server-filesystem /srv/docs
turbomcp resources --config servers.json --server files
turbomcp read config://app ./my-server
turbomcp prompt summarize -a text="…" ./my-server
turbomcp probe ./my-server
```

`probe` reports which revisions a server negotiates (`2026-07-28`, and the
newest of `2025-*`), whether the session is stateful, who the server says it
is, what it declares, and how much it offers. `--json` on any command prints
the protocol's own shapes for scripts. `--mode modern|legacy` pins the
revision; `--header 'Name: value'` and `--bearer` authenticate to an HTTP or
WebSocket server (refused for a command, which takes credentials from its
environment); `--inherit-env safe|all|none` says what a command inherits;
`--timeout` bounds each request. Text a server sent is printed with control
characters escaped.

The exit status tells failures apart: `1` a tool that reported an error,
`2` a usage or configuration error, `3` a server that couldn't be reached
or started, `4` a server that answered with an error.

Logs go to stderr (`RUST_LOG` sets the level, default `warn`): under
`proxy` over stdio, stdout is the protocol.
