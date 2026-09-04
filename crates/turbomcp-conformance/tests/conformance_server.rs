//! Drives the official MCP conformance suite
//! (`@modelcontextprotocol/conformance`, a Node CLI) against the in-process
//! [`Everything`] TurboMCP server over Streamable HTTP.
//!
//! The harness connects as an MCP client to `--url <addr>/mcp`, runs each
//! *server* scenario, and reports per-check results. We stand the server up on
//! an ephemeral port, then shell out to `pnpm dlx
//! @modelcontextprotocol/conformance server …`, parse its JSON, and assert
//! against an expected-failures baseline checked in beside this test
//! (`conformance-baseline-server.json`).
//!
//! The mirror image — the harness driving *our client* — is
//! `conformance_client.rs`. Everything they share is in
//! [`turbomcp_conformance::harness`].
//!
//! Requirements: `pnpm` on `PATH`. If it is absent the test is skipped
//! (logged), not failed — this crate is `exclude`d from the main gate precisely
//! so a missing Node toolchain never breaks `just test`. That graceful skip is
//! exactly how a gate comes to measure nothing, so it is opt-out: set
//! `TURBOMCP_CONFORMANCE_STRICT` and a missing toolchain is a failure instead.
//!
//! Run: `cd crates/turbomcp-conformance && cargo test -- --nocapture`

mod common;

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use common::Everything;
use turbomcp::CancellationToken;
use turbomcp::http::{HttpConfig, ServeHttp};
use turbomcp_conformance::harness::{
    self, CONFORMANCE_PKG, CheckResult, assert_conformance, load_baseline,
};

/// Every protocol revision we advertise gets its own full harness run — a
/// server that answers three revisions has to be conformant on each of them,
/// and a regression on one is invisible from the others.
///
/// `2025-06-18` is absent because the harness has no requirement set for it;
/// it is covered by `turbomcp-protocol`'s step-down conversion tests instead.
const SPEC_VERSIONS: &[&str] = &["2025-11-25", "2026-07-28"];

/// Floor on passing checks per revision. Not a coverage target — a tripwire for
/// the suite silently degrading to nothing, which is how this gate ran green
/// against a harness that had no requirement set for the wire we serve as
/// `LATEST`. Today: 80 on `2025-11-25`, 147 on `2026-07-28`. This only fires if
/// a run collapses, and it is *not* to be lowered to make a red build green.
const MIN_PASSING_PER_VERSION: usize = 60;

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("conformance-baseline-server.json")
}

/// Bind an ephemeral port, run the [`Everything`] server on it, and return its
/// `/mcp` URL plus a shutdown handle.
async fn spawn_server() -> (String, CancellationToken, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("bind ephemeral port");
    let addr: SocketAddr = listener.local_addr().unwrap();
    drop(listener); // run_http rebinds; this just reserves a free port number.

    let shutdown = CancellationToken::new();
    // Pin the Origin AND Host to this server's real `host:port` so the
    // DNS-rebinding scenario's spoofed `evil.example.com` Host/Origin are
    // rejected (4xx) while the harness's legitimate `127.0.0.1:<port>` requests
    // pass. (The SDK client sends no Origin, so ordinary scenarios are
    // unaffected — an absent Origin is allowed.) `with_logging` advertises the
    // `logging` capability and answers `logging/setLevel`.
    let authority = addr.to_string(); // 127.0.0.1:<port>
    let config = HttpConfig::new()
        .with_shutdown(shutdown.clone())
        .allow_origin(format!("http://{authority}"))
        .allow_host(authority);
    let handle = tokio::spawn(async move {
        let _ = Everything
            .into_server()
            .with_logging()
            .run_http(addr, config)
            .await;
    });

    // Give axum a moment to bind before the harness connects.
    tokio::time::sleep(Duration::from_millis(300)).await;
    (format!("http://{addr}/mcp"), shutdown, handle)
}

/// Run the harness against `url` at one spec version and return every check.
async fn run_harness(url: &str, spec_version: &str) -> Vec<CheckResult> {
    let out_dir = harness::tempdir("server");
    let output = tokio::process::Command::new("pnpm")
        .arg("dlx")
        .arg(CONFORMANCE_PKG)
        .arg("server")
        .arg("--url")
        .arg(url)
        .arg("--suite")
        .arg("all")
        .arg("--spec-version")
        .arg(spec_version)
        .arg("--verbose")
        .arg("--output-dir")
        .arg(&out_dir)
        .output()
        .await
        .expect("spawn pnpm dlx conformance");

    let checks = harness::parse_checks_from_dir(&out_dir, spec_version);
    assert!(
        !checks.is_empty(),
        "conformance harness produced no check results for {spec_version}.\n\
         --- stdout ---\n{}\n--- stderr ---\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    checks
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conformance_server_suite() {
    if !harness::toolchain_ready("conformance_server_suite") {
        return;
    }

    // One server, every revision: the same dispatcher answers all of them, and
    // running each suite against the same process is what proves that.
    let (url, shutdown, handle) = spawn_server().await;
    let mut checks = Vec::new();
    for spec_version in SPEC_VERSIONS {
        checks.extend(run_harness(&url, spec_version).await);
    }
    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;

    assert_conformance(
        "conformance (server)",
        SPEC_VERSIONS,
        &checks,
        &load_baseline(&baseline_path()),
        MIN_PASSING_PER_VERSION,
    );
}
