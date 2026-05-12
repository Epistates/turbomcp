# TurboMCP vs RMCP Audit - 2026-05-11

This note compares TurboMCP against the local official Rust MCP SDK checkout at
`../reference/rust-sdk` and records the dogfood benchmark/interop harness added in
`../dogfood/benchmarks`.

## Baseline

- TurboMCP: local workspace, crate versions `3.1.5`
- RMCP: `../reference/rust-sdk`, workspace version `1.6.0`
- Protocol target: both implement MCP `2025-11-25`
- Dogfood command: `../dogfood/benchmarks/run-comparison.sh rust`

## Findings

### Correctness and Protocol Surface

Both SDKs cover the core MCP protocol surface:

- initialization and capability negotiation
- JSON-RPC request, response, notification, and error shapes
- tools, resources, prompts, completion, logging, ping, progress, cancellation
- server-to-client requests for roots, sampling, and elicitation
- `2025-11-25` content variants including text, image, audio, embedded resources, and resource links
- structured tool output and task-related protocol types

TurboMCP has broader product surface than RMCP in this checkout:

- additional native transports: TCP, Unix socket, WebSocket, streamable HTTP, gRPC
- WASM/browser/WASI support
- OpenTelemetry, auth, DPoP, proxy, and OpenAPI crates
- version adapters for `2025-06-18` and `2025-11-25`
- fuzz targets and schema-method parity checks

RMCP has one compatibility advantage:

- it explicitly models `2025-03-26`, `2025-06-18`, and `2025-11-25`; TurboMCP currently models stable `2025-06-18` and `2025-11-25`

### Test Coverage Gaps to Track

TurboMCP should keep parity pressure on the RMCP areas with dedicated tests:

- cross-SDK stdio interop in CI
- streamable HTTP interop against RMCP and JS/Python MCP peers
- legacy `2025-03-26` behavior decision: either support it or document it as intentionally out of scope
- conformance-run artifacts comparable to RMCP's `conformance/results`
- explicit resource-link, structured-output, elicitation-defaults, and sampling-tool-use interop cases

## Dogfood Harness Added

The dogfood benchmark crate now includes:

- `interop-turbomcp-server`: TurboMCP stdio server
- `interop-rmcp-server`: RMCP stdio server
- `interop-check`: cross-SDK checker
- `interop-http-check`: cross-SDK Streamable HTTP checker
- `run-comparison.sh rust`: TurboMCP channel benchmark, RMCP duplex benchmark, and stdio + Streamable HTTP interop checks

The stdio interop checker validates both directions:

- TurboMCP client -> RMCP server
- RMCP client -> TurboMCP server

The Streamable HTTP checker validates both directions:

- RMCP client -> TurboMCP Streamable HTTP server
- TurboMCP client -> RMCP Streamable HTTP server

Each direction exercises initialize, `tools/list`, `tools/call`, `resources/list`,
`resources/read`, `prompts/list`, and `prompts/get`.

The HTTP checker also asserts TurboMCP's standalone SSE startup shape:

- the first record is an SSE comment
- no `data:` field is emitted before a real JSON-RPC message
- no synthetic `id:`/`retry:` event is emitted before a real JSON-RPC message

RMCP's current server emits SEP-1699-style priming events and RMCP's current
client skips empty SSE data. The Codex 0.130.0 bug report against the initial
TurboMCP 3.1.5 patch still shows worker closure when TurboMCP emits an empty
standalone GET primer event, so TurboMCP's default compatibility profile avoids
dispatching any non-JSON-RPC data event at startup. Server-initiated JSON-RPC
messages still carry resumable event IDs.

## Latest Local Results

Generated with `../dogfood/benchmarks/run-comparison.sh rust` on 2026-05-11.

| Metric | TurboMCP channel mean | RMCP duplex mean | Ratio |
| --- | ---: | ---: | ---: |
| `list_tools` | 26.26 us | 38.83 us | 1.48x |
| `list_resources` | 14.09 us | 18.96 us | 1.35x |
| `tool_call_ping` | 14.47 us | 37.43 us | 2.59x |
| `tool_call_add` | 14.51 us | 38.48 us | 2.65x |
| `tool_call_echo` | 14.76 us | 38.00 us | 2.58x |
| Throughput | 68,623 rps | 27,464 rps | 2.50x |

Interop status:

- stdio: pass
- Streamable HTTP: pass

## Recommended Next Steps

1. Add `../dogfood/benchmarks/run-comparison.sh interop` to CI if dogfood is part of the validation pipeline.
2. Decide on `2025-03-26` support. If unsupported, add an explicit compatibility note and a rejection test.
3. Expand the interop server fixture to include resource links, structured tool output, elicitation defaults/enums, and sampling tool-use content.
4. Add JS/Python Streamable HTTP peer checks so TurboMCP continuously validates the most common non-Rust MCP clients.
