# turbomcp-auth

TurboMCP v4 auth: OAuth 2.1 resource-server validation (JWT/JWKS bearer tokens, RFC 8707 audience binding, RFC 9728 protected-resource metadata) and, behind the `oauth-client` feature, the OAuth 2.1 client flow (authorization-code + PKCE, discovery, dynamic registration, RFC 9207 issuer validation).

Part of [TurboMCP](https://github.com/Epistates/turbomcp), a Rust SDK for the
[Model Context Protocol](https://modelcontextprotocol.io). Most users should
depend on the [`turbomcp`](https://crates.io/crates/turbomcp) facade, which
re-exports this crate's surface behind one dependency and its feature flags.

## Contracts and validation

See [deployment and migration](https://github.com/Epistates/turbomcp/blob/main/docs/DEPLOYMENT.md)
for authoritative catalog lookup, runtime schema validation, identity ownership,
OAuth network policy, trusted proxies, and default resource limits. See
[critical test coverage](https://github.com/Epistates/turbomcp/blob/main/docs/TESTING.md)
for the relevant regression and independent interoperability checks.

## License

MIT
