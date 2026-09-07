//! Drives the official MCP conformance suite against TurboMCP's **client**.
//!
//! The mirror image of `conformance_server.rs`. There the harness connects to
//! our server; here it *is* the server — for each scenario it stands up a mock
//! with deliberately awkward behaviour, spawns the `conformance-client` binary
//! against it, and referees what appeared on the wire.
//!
//! This suite is why the client's standalone `GET` SSE stream exists. Both
//! halves of TurboMCP were only ever tested against each other, and our server
//! happens to deliver server→client requests inline on the POST's own stream —
//! so a client that never issued the `GET` looked perfectly correct in-repo
//! while hanging against any server that uses the standalone stream instead,
//! which is what the reference TypeScript SDK does.
//!
//! ## What is baselined, and why
//!
//! One entry: **`sse-retry`**, and it is an upstream defect that upstream has
//! already fixed.
//!
//! The scenario registers itself `introducedIn: "2025-11-25"`, but its mock
//! answers `initialize` with `protocolVersion: "2025-03-26"`. TurboMCP does not
//! serve that revision, and the lifecycle spec says to disconnect rather than
//! speak on in shapes the peer never agreed to, so the client refuses. Only the
//! `initialize` POST ever reaches the mock, and `client-sse-graceful-reconnect`
//! then reports that we never reconnected. Any client that dropped `2025-03-26`
//! is in the same position; the harness's own reference client passes because
//! the TypeScript SDK still speaks it.
//!
//! This is the same defect as [conformance#412], which was fixed for the
//! *server* runner. `src/scenarios/client/sse-retry.ts` on upstream `main` now
//! sends `2025-11-25`, but no release carries it yet: `0.2.0-alpha.11` is the
//! newest published and still sends `2025-03-26`. **When [`CONFORMANCE_PKG`] is
//! bumped past it, drop this baseline entry** — the suite fails on a stale
//! baseline, so a forgotten entry is caught rather than silently tolerated.
//!
//! [conformance#412]: https://github.com/modelcontextprotocol/conformance/issues/412
//!
//! Everything else passes: 280 checks on `2025-11-25` and 451 on `2026-07-28`,
//! including the full OAuth 2.1 surface — discovery and its metadata variants,
//! dynamic registration, PKCE, the RFC 9207 `iss` table (positive *and*
//! negative), scope step-up with union-on-reauth, the retry limit, and
//! re-registration when the resource moves to a different authorization server.
//!
//! ## What is skipped, and why that number matters
//!
//! A skipped check is one the harness had nothing to assert against, because
//! this runner never drove the path it watches. It is not a pass, and it used to
//! be counted as "info" — which hid eleven of them behind a summary reading
//! "0 failed". Closing that gap is what took the `2026-07-28` count from 442 to
//! 451: the SEP-2243 header mirror had only ever been measured on `tools/*`, and
//! three SEP-2575 capability declarations were never made at all.
//!
//! Two skips remain, both structural. `sep-2243-client-includes-standard-headers`
//! wants to see `Mcp-Method` on `initialize` and `notifications/initialized`,
//! and `2026-07-28` has neither: it replaced that handshake with a stateless
//! `server/discover`. Nothing this runner does can produce them.
//!
//! Requirements: `pnpm` on `PATH`; skipped without it unless
//! `TURBOMCP_CONFORMANCE_STRICT` is set. See `conformance_server.rs`.
//!
//! Run: `cd crates/turbomcp-conformance && cargo test --test conformance_client -- --nocapture`

use std::path::PathBuf;

use turbomcp_conformance::harness::{
    self, CONFORMANCE_PKG, CheckResult, assert_conformance, load_baseline,
};

/// The revisions the harness has client scenarios for. `2025-06-18` has a
/// handful, but no requirement set of its own, so its scenarios are covered
/// cumulatively by the `2025-11-25` run.
const SPEC_VERSIONS: &[&str] = &["2025-11-25", "2026-07-28"];

/// Floor on passing checks per revision — the same tripwire the server suite
/// carries, for the same reason: "0 failures" is also what a run that never
/// started reports. Today: 280 on `2025-11-25`, 442 on `2026-07-28`. The gap is
/// real (the draft has the header and MRTR scenarios on top of the shared auth
/// ones), so the floor sits below the smaller of the two with room for the
/// suite to be re-cut upstream. It is *not* to be lowered to green a build.
const MIN_PASSING_PER_VERSION: usize = 200;

/// The client binary the harness spawns. Cargo builds it for us and hands over
/// its path, so the suite can never score a stale binary — the failure mode of
/// building and locating it by hand.
const CLIENT_BIN: &str = env!("CARGO_BIN_EXE_conformance-client");

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("conformance-baseline-client.json")
}

/// Run the client suite at one spec version and return every check.
async fn run_harness(spec_version: &str) -> Vec<CheckResult> {
    let out_dir = harness::tempdir("client");
    let output = tokio::process::Command::new("pnpm")
        .arg("dlx")
        .arg(CONFORMANCE_PKG)
        .arg("client")
        .arg("--command")
        .arg(CLIENT_BIN)
        .arg("--suite")
        .arg("all")
        .arg("--spec-version")
        .arg(spec_version)
        .arg("--output-dir")
        .arg(&out_dir)
        .output()
        .await
        .expect("spawn pnpm dlx conformance client");

    let checks = harness::parse_checks_from_dir(&out_dir, spec_version);
    assert!(
        !checks.is_empty(),
        "conformance client harness produced no check results for {spec_version}.\n\
         --- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    checks
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conformance_client_suite() {
    if !harness::toolchain_ready("conformance_client_suite") {
        return;
    }

    let mut checks = Vec::new();
    for spec_version in SPEC_VERSIONS {
        checks.extend(run_harness(spec_version).await);
    }

    assert_conformance(
        "conformance (client)",
        SPEC_VERSIONS,
        &checks,
        &load_baseline(&baseline_path()),
        MIN_PASSING_PER_VERSION,
    );
}
