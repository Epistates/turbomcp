//! Scores the public client against the pinned official harness with reviewed
//! fixture corrections. No assertion is removed or weakened. Set
//! TURBOMCP_CONFORMANCE_UPSTREAM=1 to reproduce the unmodified upstream results.
//! See fixtures/README.md for source hashes, corrections, and raw limitations.

use std::path::PathBuf;

use turbomcp_conformance::harness::{
    self, CONFORMANCE_PKG, CheckResult, assert_conformance, load_baseline,
};

/// The revisions the harness has client scenarios for. `2025-06-18` has a
/// handful, but no requirement set of its own, so its scenarios are covered
/// cumulatively by the `2025-11-25` run.
const SPEC_VERSIONS: &[&str] = &["2025-11-25", "2026-07-28"];

/// A secondary floor; the exact inventory is the primary coverage gate.
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
    let mut command = tokio::process::Command::new("pnpm");
    if harness::raw_upstream() {
        command.arg("dlx").arg(CONFORMANCE_PKG);
    } else {
        command
            .arg(format!("--package={CONFORMANCE_PKG}"))
            .arg("dlx")
            .arg("node")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/corrected-client.mjs"));
    }
    let output = command
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
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("=== SUITE SUMMARY ===")
            && stdout.lines().any(|line| line.starts_with("Total:")),
        "harness did not complete: {stdout}"
    );
    // alpha.11 exits 1 on WARNING as well as FAILURE. Permit that exit only
    // for an otherwise complete run with warnings; the exact warning inventory
    // below is independently pinned and reviewed.
    assert!(
        output.status.success()
            || (harness::raw_upstream()
                && output.status.code() == Some(1)
                && !checks.iter().any(CheckResult::is_fail)
                && checks
                    .iter()
                    .any(|c| c.disposition == harness::Disposition::Warning)),
        "harness exited {}: {stdout}",
        output.status
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

    if !harness::raw_upstream() {
        // Method-specific coverage must not collapse behind the shared check id.
        let labels: std::collections::BTreeSet<_> = checks
            .iter()
            .filter(|c| {
                c.spec_version == "2026-07-28"
                    && c.scenario == "http-standard-headers"
                    && c.is_pass()
            })
            .filter_map(|c| c.label.as_deref())
            .collect();
        for method in [
            "server_discover",
            "tools_list",
            "tools_call",
            "resources_list",
            "resources_read",
            "prompts_list",
            "prompts_get",
        ] {
            assert!(
                labels.contains(format!("ClientMcpMethodHeader_{method}").as_str()),
                "unexercised Mcp-Method for {method}"
            );
        }
        for method in ["tools_call", "resources_read", "prompts_get"] {
            assert!(
                labels.contains(format!("ClientMcpNameHeader_{method}").as_str()),
                "unexercised Mcp-Name for {method}"
            );
        }
        eprintln!(
            "Client fixture corrections v1 active; unmodified upstream mode: TURBOMCP_CONFORMANCE_UPSTREAM=1"
        );
    }
    for version in SPEC_VERSIONS {
        harness::assert_inventory("client", version, &checks);
    }
    assert_conformance(
        "conformance (client)",
        SPEC_VERSIONS,
        &checks,
        &load_baseline(&baseline_path()),
        MIN_PASSING_PER_VERSION,
    );
}
