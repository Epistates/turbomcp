# turbomcp-cli

The `turbomcp` command: serve the servers in an `mcpServers` configuration as
one MCP server, and run protocol operations against any MCP server.

```sh
cargo install turbomcp-cli
```

## The gateway

```sh
turbomcp proxy --config servers.json                  # over stdio
turbomcp proxy --config servers.json --http 127.0.0.1:8080
```

The configuration is the one MCP hosts already share: Claude Desktop's,
Cursor's and Windsurf's `mcpServers`, or VS Code's `servers`.

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
caused them.

- `${VAR}` and `${env:VAR}` in any string come from the environment, so
  secrets stay out of the file. An unset variable is an error, not an empty
  string.
- A command server inherits only what a program needs of the gateway's
  environment (`HOME`, `PATH`, `USER`, …, the official SDKs' list); give it
  more under `env`.
- An unreachable server is logged and left out (`--strict` makes it fatal);
  `"disabled": true` skips one; `--only a,b` serves a subset.
- `type: "sse"` (the 2024-11-05 HTTP+SSE transport) is refused with a reason:
  the current revisions replaced it with Streamable HTTP.
- `--http` serves with no authentication of its own, and warns when bound
  beyond loopback. To put the gateway behind OAuth, rate limits, visibility
  rules or your own tools, embed [`turbomcp-proxy`](../turbomcp-proxy) and
  compose it with the rest of TurboMCP.

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
revision; `--header 'Name: value'` adds a header; `--timeout` bounds each
request. A tool that reports an error makes `call` exit non-zero.

Logs go to stderr (`RUST_LOG` sets the level, default `warn`): under
`proxy` over stdio, stdout is the protocol.
