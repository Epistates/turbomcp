# Critical behavior and test coverage

Use behavioral regressions and independent peers together. Passing TurboMCP
client/server tests alone does not establish MCP compliance. Supported revisions
are 2025-06-18, 2025-11-25, and 2026-07-28; conformance and rmcp interoperability
currently score the latter two. The first has generated conversion and protocol
coverage, not an equivalent external conformance score.

| Critical area | Executable evidence |
| --- | --- |
| JSON-RPC malformed envelopes and null response fidelity | core `jsonrpc` unit tests, codec tests, codec/differential fuzz targets |
| Pagination, visibility, schema enforcement, lookup failures | facade `audit_regressions`, `macro_server`, server capability/composition tests |
| Dynamic metadata and header mirrors | facade `mcp_header`, `response_cache`, server dispatcher tests |
| Cancellation, pending-request cleanup, overload and shutdown | facade `audit_regressions`, `client_robustness`, service `serve_concurrency` |
| HTTP auth, cross-principal sessions, rate limits, origin/host | HTTP `auth_http`, `post_sse`, transport unit tests |
| Trusted proxy chain and malformed boundaries | HTTP `client_ip_*` unit tests |
| JWT nbf, JWKS coalescing/backoff, OAuth URLs and PKCE | auth `audit_security`, `oauth_flow`, network policy unit tests |
| Public OAuth concurrent authorization, stale challenges, refresh rotation and shared refresh failure | facade `oauth_coordination` (24 concurrent callers) |
| Stateful and stateless task ownership/cancellation | server `legacy_tasks`, ext-tasks `augmentation`, facade `ext_tasks` |
| MRTR argument/principal binding, elicitation and subscriptions | server `mrtr`, facade `client_elicitation*`, `client_subscriptions` |
| Renamed macro dependencies and metadata isolation | `renamed_dependency` fixture, facade `macro_server` |
| Independent peers | strict conformance inventories, excluded rmcp interop suite |

Run `just test` for the local aggregate gate, then excluded conformance and
interop suites. CI has additional MSRV/platform/feature checks. `just docs-links`
checks local Markdown file destinations; it does not validate remote URLs,
anchors, or compile Markdown examples. Executable crate doctests and examples
provide the compile checks. Historical reports in audits/ are not active API docs.

Conformance failure baselines are empty. The corrected client fixture gate and
unmodified server gate have zero skips or warnings. The unmodified upstream
client mode retains two modern-header skips and two legacy SSE warnings; see
[fixture corrections](../crates/turbomcp-conformance/fixtures/README.md). The
`client_sse_resume` regressions additionally cover concurrent replay cursors,
retry timing, auth/session headers, no repeated POST, and cancellation during
backoff. Optional server-side replay remains unimplemented. Finite conformance
and regression tests are not blanket certification; see the [benchmark guide](../benches/README.md)
for performance methodology and remaining evidence.

No standalone v4 proxy or CLI is present. The unpublished code generator has a
CLI, tested by regenerating all three schemas with `just codegen-check`.
Shell publication entry points are dry-run by default and share the justfile's
package order and failure handling. A future MCP proxy/CLI needs independent
end-to-end tests for upstream errors, auth isolation, pagination, notifications,
cancellation, startup failure, and shutdown before entering supported scope.
