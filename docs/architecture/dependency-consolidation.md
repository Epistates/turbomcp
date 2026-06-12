# Dependency Version Consolidation

This document records the dependency-deduplication work done in this PR: what was
duplicated, why, what changed, and the measured effect. It is written to double as
the PR description.

## Problem

`cargo tree --duplicates` showed the workspace compiling **multiple versions of the
same crate** side by side. For the `turbomcp-proxy` release binary (the fattest
binary in the workspace — it pulls in client, server, all transports, and the full
auth stack), the duplicate set before this PR was:

| Crate | Versions compiled | Root cause |
|---|---|---|
| `reqwest` | **0.12.28 + 0.13.3** | `oauth2 5.0`'s default `reqwest` feature |
| `sha2` | 0.10.9 + 0.11.0 | workspace pinned 0.11; `p256`/`oauth2` use the digest-0.10 stack |
| `digest` | 0.10.7 + 0.11.3 | follows sha2 |
| `block-buffer` | 0.10.4 + 0.12.0 | follows sha2 |
| `crypto-common` | 0.1.7 + 0.2.1 | follows sha2 |
| `rand` | **0.8.6 + 0.9.4 + 0.10.1** | workspace pinned 0.10; `governor`/`tungstenite` use 0.9; `oauth2` uses 0.8 |
| `rand_core` | 0.6.4 + 0.9.5 + 0.10.1 | follows rand |
| `getrandom` | 0.2.17 + 0.3.4 + 0.4.2 | follows rand (+ `ring` on 0.2, `uuid` on 0.4) |
| `thiserror` (+impl) | 1.0.69 + 2.0.18 | `oauth2 5.0` still on 1.x |
| `hashbrown` | 0.14.5 + 0.16.1 + 0.17.0 | `dashmap 6.1` / `governor`+`halfbrown` / `indexmap 2.14` |
| `cpufeatures` | 0.2.17 + 0.3.0 | sha2-0.10 stack vs blake3/aws-lc stack |
| `untrusted` | 0.7.1 + 0.9.0 | `aws-lc-rs` (rustls, jsonwebtoken) vs `ring` |

Each duplicated version is compiled, optimized, and linked separately: it costs
build time, binary size, and (for stateful crates) the risk of two versions not
sharing types.

## Changes

### 1. Drop oauth2's bundled reqwest 0.12 — `crates/turbomcp-auth/Cargo.toml`

The single biggest win, and effectively free.

`oauth2 = { version = "5.0", default-features = false, features = ["reqwest", "rustls-tls"] }`
pulled in **reqwest 0.12 and a second copy of the HTTP client stack**
(hyper-rustls config, connection pools, redirect/proxy/compression machinery)
purely as a side effect of the feature flag.

turbomcp-auth never uses oauth2's bundled client: every token request already goes
through our own `OAuth2HttpClient` adapter
(`crates/turbomcp-auth/src/oauth2/http_client.rs`), which implements oauth2's
`AsyncHttpClient` trait on top of the **workspace reqwest 0.13** (added precisely
because the oauth2-bundled types were incompatible with 0.13). The bundled 0.12
stack was dead weight; `oauth2::reqwest` appears nowhere in the source.

**Change:** `features = ["reqwest", "rustls-tls"]` → no features.
**Effect:** reqwest 0.12 is gone from `Cargo.lock` entirely.

### 2. Consolidate `sha2` on 0.10 — workspace `Cargo.toml`

The workspace pinned `sha2 = "0.11"` (digest-0.11 stack) while `p256` (turbomcp-dpop)
and `oauth2` (turbomcp-auth) are on the digest-0.10 stack and have no 0.11-based
releases yet. Both stacks were compiled in full: sha2, digest, block-buffer,
crypto-common, cpufeatures ×2 each.

All our usage is the version-portable `Sha256`/`Digest` API, so pinning the
workspace to `sha2 = "0.10"` collapses the whole RustCrypto stack to one
generation with **zero code changes**.

**Revisit when:** `p256` and `oauth2` publish digest-0.11-based releases — then
move the workspace to 0.11 instead.

### 3. Consolidate `rand` on 0.9 — workspace `Cargo.toml`

The workspace pinned `rand = "0.10"` while `governor` (rate limiting) and
`tungstenite` (websockets) are on 0.9. Our own usage (`rand::rng()`,
`rand::distr`, `rand::random`) is identical across 0.9/0.10, so pinning the
workspace to 0.9 drops the rand 0.10 / rand_core 0.10 / getrandom 0.4(rand) copies
with zero code changes. (`rand 0.8` remains — unconditional `oauth2` dep.)

### 4. Dead-dependency cleanup

* `crates/turbomcp-protocol/Cargo.toml`: `rand = "0.10"` was pinned directly,
  bypassing the workspace entry → now `rand = { workspace = true }`.
* `crates/turbomcp-dpop/Cargo.toml`: removed two **unused** dependencies:
  * `rand` — dpop only uses `p256::elliptic_curve::rand_core::OsRng` (p256's
    re-export); the direct `rand` dep was never imported.
  * `signature = "3.0"` — never imported, and wrong-generation anyway: the
    p256 0.13 stack implements the `signature 2.x` traits, so the direct 3.0 dep
    was a guaranteed duplicate that nothing used.

## Not fixable in this PR (upstream-constrained)

| Duplicate | Blocked on |
|---|---|
| `thiserror` 1+2, `rand` 0.8+0.9 | `oauth2 5.0` (unconditional deps; no newer release as of 2026-06) |
| `hashbrown` ×3 | `dashmap 6.1` (0.14), `governor`/`halfbrown` (0.16), `indexmap` (0.17). dashmap 7 is still rc. |
| `untrusted`, `getrandom 0.2` | `ring` vs `aws-lc-rs` split — rustls and jsonwebtoken default to aws-lc-rs while turbomcp-dpop/`ring` use ring. Unifying crypto backends is real surgery and out of scope here. |
| `getrandom 0.4` | `uuid` — harmless, version follows uuid's MSRV policy |

## Downstream compatibility

These are all **private-dependency changes** — no breakage for projects consuming
turbomcp crates:

* `sha2` and `rand` are never re-exported and appear in no public signature; all
  usage is internal (hashing, jitter). The oauth2 crate *version* is unchanged
  (5.0) — only its unused optional features were disabled — so its types are
  identical too.
* The requirements stay ordinary caret ranges (no `=` pins), and both version
  moves align with what downstream trees already contain: dpop consumers already
  compile sha2 0.10 via `p256`, and transport/auth consumers already compile
  rand 0.9 via `tungstenite`/`governor`. Downstream lockfiles get *smaller*
  after upgrading, not more constrained.
* One standard Cargo caveat: a downstream that depends directly on `oauth2 5.x`
  and uses `oauth2::reqwest::Client` **without declaring the `reqwest` feature
  itself** (relying on turbomcp-auth enabling it transitively) would need to add
  `features = ["reqwest"]` to its own oauth2 dependency. Cargo documents relying
  on transitively-enabled features as a consumer bug; feature unification means
  correctly-declared dependents are unaffected.

## Measured results

Methodology: `cargo build --release -p turbomcp-proxy --bin turbomcp-proxy`
(the release profile: fat LTO, `codegen-units = 1`, `panic = "abort"`, stripped)
on `rustc 1.94.1`, Windows x86_64. Same machine, same toolchain, before and after.

| Metric | Before | After | Delta |
|---|---|---|---|
| Duplicated package-versions in proxy build graph (`cargo tree --duplicates`) | 33 | 21 | **−12** |
| Unique packages compiled for the proxy release binary (`cargo tree -e normal`) | 393 | 374 | **−19 crates** |
| Package stanzas in `Cargo.lock` | 676 | 665 | −11 |
| `reqwest` versions **compiled** | 2 (0.12.28 + 0.13.3) | 1 (0.13.3) | −1 full HTTP client stack |
| `turbomcp-proxy.exe` size (release) | 8,790,016 B | 8,793,600 B | **≈ unchanged** (+3.5 KB noise) |

### Why the binary size did not change (and why that's expected)

The release profile uses **fat LTO + `codegen-units = 1` + strip**. Under fat LTO
the linker already dead-code-eliminates anything unreachable — including the
entire unused reqwest 0.12 stack — so the duplicates were never costing binary
bytes *in this profile*. What they cost is:

* **Compile time**: 19 extra crates built (and fed into LTO) on every clean
  release build of the proxy, and similar overhead across every other binary,
  test, and downstream consumer build.
* **Downstream binary size**: consumers who build with the default cargo profiles
  (thin/no LTO, 16 codegen units) do **not** get this dead-code elimination and
  were linking two HTTP stacks.
* **Correctness risk**: two versions of the same crate have incompatible types;
  keeping one generation per crate prevents an entire class of trait/type
  mismatch errors at API boundaries.

Note: `Cargo.lock` still contains an inert `reqwest 0.12.28` stanza — it is an
*optional* dependency of oauth2 and Cargo locks optional deps even when no
feature activates them. `cargo tree -i reqwest@0.12.28` confirms it matches no
package in any build graph; it is never downloaded or compiled.

Removed from the compiled graph entirely (verified via `Cargo.lock` diff):
`block-buffer 0.12`, `chacha20 0.10`, `const-oid 0.10`, `crypto-common 0.2`,
`digest 0.11`, `hybrid-array 0.4`, `rand 0.10`, `rand_core 0.10`, `sha2 0.11`,
`signature 3.0`, `webpki-roots 1.0` — plus `reqwest 0.12` and its private
subtree dropped from all build graphs.

### Test evidence

`cargo test --workspace --no-fail-fast` after the changes: **exit code 0 — zero
failures**. The tail of the log captured 82 suites / 1,577 tests passed,
148 ignored, 0 failed (earlier suites scrolled past the capture window; the
`--no-fail-fast` + exit-0 combination guarantees no suite anywhere failed).
All affected crates were additionally type-checked with `--all-features`
(`turbomcp-auth`, `turbomcp-dpop`, `turbomcp-protocol`, `turbomcp-transport`)
to cover feature-gated code paths the default test build wouldn't touch.
