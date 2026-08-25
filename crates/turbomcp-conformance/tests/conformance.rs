//! Drives the official MCP conformance suite
//! (`@modelcontextprotocol/conformance`, a Node CLI) against the in-process
//! [`Everything`] TurboMCP server over Streamable HTTP.
//!
//! The harness connects as an MCP client to `--url <addr>/mcp`, runs each
//! *server* scenario, and reports per-check results. We stand the server up on
//! an ephemeral port, then shell out to `npx @modelcontextprotocol/conformance
//! server …`, parse its JSON, and assert against an expected-failures baseline
//! checked in beside this test (`conformance-baseline.json`).
//!
//! Requirements: Node/`npx` on `PATH`. If neither is available the test is
//! skipped (logged), not failed — this crate is `exclude`d from the main gate
//! precisely so a missing Node toolchain never breaks `just test`.
//!
//! Run: `cd crates/turbomcp-conformance && cargo test -- --nocapture`

mod common;

use std::collections::BTreeSet;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::time::Duration;

use common::Everything;
use turbomcp::CancellationToken;
use turbomcp::http::{HttpConfig, ServeHttp};

/// The conformance package + version this suite is pinned to. `npx` resolves
/// (and caches) it; pinning keeps runs reproducible.
/// The conformance package + version this suite is pinned to. `npx` resolves
/// (and caches) it; pinning keeps runs reproducible.
///
/// This is the `alpha` dist-tag, not `latest`. `latest` is still 0.1.16, which
/// predates the `2026-07-28` freeze entirely and has no requirement set for it
/// — pinning to it would mean the wire we actually serve as `LATEST` is the one
/// wire nothing checks. Revisit when a stable 0.2.x ships.
const CONFORMANCE_PKG: &str = "@modelcontextprotocol/conformance@0.2.0-alpha.11";

/// Every protocol revision we advertise gets its own full harness run — a
/// server that answers three revisions has to be conformant on each of them,
/// and a regression on one is invisible from the others.
///
/// `2025-06-18` is absent because the harness has no requirement set for it;
/// it is covered by `turbomcp-protocol`'s step-down conversion tests instead.
const SPEC_VERSIONS: &[&str] = &["2025-11-25", "2026-07-28"];

fn baseline_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("conformance-baseline.json")
}

/// Is `npx` runnable? (Skip the suite gracefully if not.)
fn npx_available() -> bool {
    std::process::Command::new("npx")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The disposition of a single conformance check, from its `status` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Disposition {
    /// `SUCCESS` — a passing assertion.
    Pass,
    /// `FAILURE` — a failing assertion (counts against us unless baselined).
    Fail,
    /// `INFO` / `WARNING` — informational; neither pass nor fail.
    Info,
}

/// One conformance check outcome, projected from the harness's JSON output.
#[derive(Debug, Clone)]
struct CheckResult {
    spec_version: String,
    scenario: String,
    name: String,
    disposition: Disposition,
    message: Option<String>,
}

impl CheckResult {
    /// Baseline key. The spec version leads because the same check can be
    /// legitimately N/A on one revision and required on another.
    fn id(&self) -> String {
        format!("{}::{}::{}", self.spec_version, self.scenario, self.name)
    }
    fn is_fail(&self) -> bool {
        self.disposition == Disposition::Fail
    }
    fn is_pass(&self) -> bool {
        self.disposition == Disposition::Pass
    }
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
    let out_dir = tempdir();
    let output = tokio::process::Command::new("npx")
        .arg("--yes")
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
        .expect("spawn npx conformance");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let checks = parse_checks_from_dir(&out_dir, spec_version);
    if checks.is_empty() {
        // Surface the harness output so a wiring failure is diagnosable.
        panic!(
            "conformance harness produced no check results for {spec_version}.\n\
             --- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
        );
    }
    checks
}

/// A unique temp dir for one harness run's `results/`.
fn tempdir() -> PathBuf {
    let base = std::env::temp_dir();
    let unique = format!(
        "turbomcp-conformance-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let dir = base.join(unique);
    std::fs::create_dir_all(&dir).expect("create temp results dir");
    dir
}

/// Walk the harness `--output-dir`, reading every `checks.json` it wrote. The
/// harness lays results out as `<dir>/server-<scenario>-<timestamp>/checks.json`.
fn parse_checks_from_dir(dir: &PathBuf, spec_version: &str) -> Vec<CheckResult> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let checks_json = path.join("checks.json");
        let Ok(text) = std::fs::read_to_string(&checks_json) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        // Scenario name from the directory: server-<scenario>-<timestamp>.
        let dir_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
        let scenario = dir_name
            .strip_prefix("server-")
            .and_then(|s| s.rsplit_once('-').map(|(head, _ts)| head))
            .unwrap_or(dir_name)
            .to_string();
        collect_checks(spec_version, &scenario, &value, &mut out);
    }
    out
}

/// Extract check objects from a `checks.json` payload. The harness writes a
/// top-level JSON array of check objects, each with an uppercase `status`
/// (`SUCCESS` / `FAILURE` / `INFO` / `WARNING`), an `id`/`name`, and an
/// optional `errorMessage`.
fn collect_checks(
    spec_version: &str,
    scenario: &str,
    value: &serde_json::Value,
    out: &mut Vec<CheckResult>,
) {
    let array = if let Some(arr) = value.as_array() {
        arr.clone()
    } else if let Some(arr) = value.get("checks").and_then(|c| c.as_array()) {
        arr.clone()
    } else {
        return;
    };

    for (i, check) in array.iter().enumerate() {
        let name = check
            .get("name")
            .or_else(|| check.get("id"))
            .or_else(|| check.get("description"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("check-{i}"));

        let status = check
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_ascii_uppercase();
        let disposition = match status.as_str() {
            "SUCCESS" | "PASS" | "PASSED" | "OK" => Disposition::Pass,
            "FAILURE" | "FAIL" | "FAILED" | "ERROR" => Disposition::Fail,
            _ => Disposition::Info, // INFO / WARNING / anything else: not scored.
        };

        let message = check
            .get("errorMessage")
            .or_else(|| check.get("message"))
            .or_else(|| check.get("error"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        out.push(CheckResult {
            spec_version: spec_version.to_string(),
            scenario: scenario.to_string(),
            name,
            disposition,
            message,
        });
    }
}

/// Load the expected-failures baseline: a JSON array of `"scenario::check"` ids.
fn load_baseline() -> BTreeSet<String> {
    let path = baseline_path();
    let Ok(text) = std::fs::read_to_string(&path) else {
        return BTreeSet::new();
    };
    let ids: Vec<String> = serde_json::from_str(&text).unwrap_or_default();
    ids.into_iter().collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn conformance_server_suite() {
    if !npx_available() {
        eprintln!(
            "SKIP conformance_server_suite: `npx` not found on PATH (Node toolchain required)."
        );
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

    let baseline = load_baseline();

    let mut passed = Vec::new();
    let mut failed = Vec::new();
    let mut info = 0usize;
    for c in &checks {
        if c.is_pass() {
            passed.push(c);
        } else if c.is_fail() {
            failed.push(c);
        } else {
            info += 1;
        }
    }

    // Group failures into expected (in baseline) vs unexpected (regressions).
    let mut unexpected = Vec::new();
    for c in &failed {
        if !baseline.contains(&c.id()) {
            unexpected.push(*c);
        }
    }

    // Stale baseline entries: listed as expected-failure but now passing.
    let failing_ids: BTreeSet<String> = failed.iter().map(|c| c.id()).collect();
    let stale: Vec<&String> = baseline
        .iter()
        .filter(|id| !failing_ids.contains(*id))
        .collect();

    eprintln!(
        "\n=== TurboMCP conformance ({}) ===\n  checks: {} total, {} passed, {} failed ({} expected, {} unexpected), {} info\n  baseline entries: {} ({} stale)",
        SPEC_VERSIONS.join(" + "),
        checks.len(),
        passed.len(),
        failed.len(),
        failed.len() - unexpected.len(),
        unexpected.len(),
        info,
        baseline.len(),
        stale.len(),
    );

    if !failed.is_empty() {
        eprintln!("\n--- failing checks ---");
        for c in &failed {
            let tag = if baseline.contains(&c.id()) {
                "expected"
            } else {
                "UNEXPECTED"
            };
            eprintln!(
                "  [{tag}] {}  {}",
                c.id(),
                c.message.as_deref().unwrap_or("")
            );
        }
    }

    assert!(
        unexpected.is_empty(),
        "{} unexpected conformance failure(s) — see the list above. Add them to \
         conformance-baseline.json only after confirming they are optional/unimplemented \
         features, not spec-compliance bugs.",
        unexpected.len(),
    );
    assert!(
        stale.is_empty(),
        "stale baseline entries (now passing — remove them): {stale:?}",
    );
}
