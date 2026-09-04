//! Shared plumbing for driving the official `@modelcontextprotocol/conformance`
//! CLI and scoring what it wrote.
//!
//! Both suites — the server one, where the harness connects to us, and the
//! client one, where it spawns us against its own mock servers — run the same
//! Node CLI, get the same `checks.json` shape back, and score it the same way
//! against a checked-in expected-failures baseline. Only the invocation
//! differs, so everything except the invocation lives here.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The conformance package + version both suites are pinned to. `pnpm dlx`
/// resolves (and caches) it; pinning keeps runs reproducible.
///
/// This is the `alpha` dist-tag, not `latest`. `latest` is still 0.1.16, which
/// predates the `2026-07-28` freeze entirely and has no requirement set for it
/// — pinning to it would mean the wire we actually serve as `LATEST` is the one
/// wire nothing checks. Revisit when a stable 0.2.x ships.
pub const CONFORMANCE_PKG: &str = "@modelcontextprotocol/conformance@0.2.0-alpha.11";

/// Set this to turn the "no Node toolchain" skip into a failure.
///
/// The skip exists for a developer who ran the whole crate incidentally, not
/// for a run that asked for conformance. CI and `just conformance` both set
/// this, because **a skip in CI is indistinguishable from a pass**.
pub const STRICT_ENV: &str = "TURBOMCP_CONFORMANCE_STRICT";

/// Is `pnpm` runnable? (Skip the suite gracefully if not.)
#[must_use]
pub fn pnpm_available() -> bool {
    std::process::Command::new("pnpm")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Enforce [`STRICT_ENV`] against a missing toolchain. Returns `false` when the
/// caller should skip.
///
/// # Panics
/// When `pnpm` is absent but [`STRICT_ENV`] is set — this run asked for
/// conformance and cannot deliver it.
#[must_use]
pub fn toolchain_ready(suite: &str) -> bool {
    if pnpm_available() {
        return true;
    }
    assert!(
        std::env::var_os(STRICT_ENV).is_none(),
        "{STRICT_ENV} is set but `pnpm` is not on PATH — this run asked for conformance \
         and cannot deliver it. Install pnpm, or unset {STRICT_ENV} to allow the skip.",
    );
    eprintln!("SKIP {suite}: `pnpm` not found on PATH (Node toolchain required).");
    false
}

/// The disposition of a single conformance check, from its `status` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    /// `SUCCESS` — a passing assertion.
    Pass,
    /// `FAILURE` — a failing assertion (counts against us unless baselined).
    Fail,
    /// `INFO` / `WARNING` — informational; neither pass nor fail.
    Info,
}

/// One conformance check outcome, projected from the harness's JSON output.
#[derive(Debug, Clone)]
pub struct CheckResult {
    /// The spec revision the harness was run against.
    pub spec_version: String,
    /// The scenario that produced this check.
    pub scenario: String,
    /// The check's own name/id.
    pub name: String,
    /// Pass, fail, or informational.
    pub disposition: Disposition,
    /// The harness's message, when it wrote one.
    pub message: Option<String>,
}

impl CheckResult {
    /// Baseline key. The spec version leads because the same check can be
    /// legitimately N/A on one revision and required on another.
    #[must_use]
    pub fn id(&self) -> String {
        format!("{}::{}::{}", self.spec_version, self.scenario, self.name)
    }

    /// Did this check fail?
    #[must_use]
    pub fn is_fail(&self) -> bool {
        self.disposition == Disposition::Fail
    }

    /// Did this check pass?
    #[must_use]
    pub fn is_pass(&self) -> bool {
        self.disposition == Disposition::Pass
    }
}

/// A unique temp dir for one harness run's `results/`.
///
/// # Panics
/// If the directory cannot be created.
#[must_use]
pub fn tempdir(tag: &str) -> PathBuf {
    let unique = format!(
        "turbomcp-conformance-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let dir = std::env::temp_dir().join(unique);
    std::fs::create_dir_all(&dir).expect("create temp results dir");
    dir
}

/// Walk a harness `--output-dir`, reading every `checks.json` under it.
///
/// The two suites lay results out differently — the server one writes
/// `server-<scenario>-<timestamp>/`, the client one writes
/// `<scenario>-<timestamp>/` and nests namespaced scenarios a level deeper
/// (`auth/<scenario>-<timestamp>/`) — so this recurses and derives the scenario
/// from whichever directory actually held the file.
#[must_use]
pub fn parse_checks_from_dir(dir: &Path, spec_version: &str) -> Vec<CheckResult> {
    let mut out = Vec::new();
    collect_dir(dir, spec_version, "", &mut out);
    out
}

fn collect_dir(dir: &Path, spec_version: &str, prefix: &str, out: &mut Vec<CheckResult>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let dir_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");

        let checks_json = path.join("checks.json");
        if checks_json.is_file() {
            let scenario = format!("{prefix}{}", strip_run_suffix(dir_name));
            if let Ok(text) = std::fs::read_to_string(&checks_json)
                && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
            {
                collect_checks(spec_version, &scenario, &value, out);
            }
            continue;
        }

        // An intermediate namespace directory (`auth/`), not a run.
        let nested = format!("{prefix}{dir_name}/");
        collect_dir(&path, spec_version, &nested, out);
    }
}

/// Strip the harness's `server-` prefix and `-<timestamp>` suffix from a run
/// directory, leaving the scenario name. Timestamps are what make an id
/// unstable across runs, so a baseline entry has to be free of them.
fn strip_run_suffix(dir_name: &str) -> &str {
    let body = dir_name.strip_prefix("server-").unwrap_or(dir_name);
    // `<scenario>-<ISO timestamp>`; the timestamp's own dashes mean splitting
    // from the right on the first `-` that starts a 4-digit year.
    body.rsplit_once("-20").map_or(body, |(head, _ts)| head)
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
        arr
    } else if let Some(arr) = value.get("checks").and_then(|c| c.as_array()) {
        arr
    } else {
        return;
    };

    for (i, check) in array.iter().enumerate() {
        let name = check
            .get("id")
            .or_else(|| check.get("name"))
            .or_else(|| check.get("description"))
            .and_then(|v| v.as_str())
            .map_or_else(|| format!("check-{i}"), std::string::ToString::to_string);

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
            .map(std::string::ToString::to_string);

        out.push(CheckResult {
            spec_version: spec_version.to_string(),
            scenario: scenario.to_string(),
            name,
            disposition,
            message,
        });
    }
}

/// Load an expected-failures baseline: a JSON array of
/// `"<spec>::<scenario>::<check>"` ids. A missing file is an empty baseline.
#[must_use]
pub fn load_baseline(path: &Path) -> BTreeSet<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    serde_json::from_str::<Vec<String>>(&text)
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Score a run against a baseline and report it, panicking on any regression.
///
/// `floor` guards the shape of failure a baseline cannot catch: **"0 failures"
/// is also what a suite that ran nothing reports**, so each revision must show
/// evidence it was actually exercised.
///
/// # Panics
/// On an unexpected failure, a stale baseline entry, or a revision that
/// produced fewer than `floor` passing checks.
pub fn assert_conformance(
    suite: &str,
    spec_versions: &[&str],
    checks: &[CheckResult],
    baseline: &BTreeSet<String>,
    floor: usize,
) {
    let passed: Vec<&CheckResult> = checks.iter().filter(|c| c.is_pass()).collect();
    let failed: Vec<&CheckResult> = checks.iter().filter(|c| c.is_fail()).collect();
    let info = checks.len() - passed.len() - failed.len();

    let unexpected: Vec<&&CheckResult> = failed
        .iter()
        .filter(|c| !baseline.contains(&c.id()))
        .collect();

    // Stale baseline entries: listed as expected-failure but now passing.
    let failing_ids: BTreeSet<String> = failed.iter().map(|c| c.id()).collect();
    let stale: Vec<&String> = baseline
        .iter()
        .filter(|id| !failing_ids.contains(*id))
        .collect();

    eprintln!(
        "\n=== TurboMCP {suite} ({}) ===\n  checks: {} total, {} passed, {} failed ({} expected, {} unexpected), {info} info\n  baseline entries: {} ({} stale)",
        spec_versions.join(" + "),
        checks.len(),
        passed.len(),
        failed.len(),
        failed.len() - unexpected.len(),
        unexpected.len(),
        baseline.len(),
        stale.len(),
    );

    // Per revision, because the totals hide a version that contributed nothing.
    let per_version: Vec<(&&str, usize)> = spec_versions
        .iter()
        .map(|v| (v, passed.iter().filter(|c| c.spec_version == **v).count()))
        .collect();
    for (spec_version, n) in &per_version {
        eprintln!("  {spec_version}: {n} passed (floor {floor})");
    }

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
        "{} unexpected conformance failure(s) in {suite} — see the list above. Add them to \
         the baseline only after confirming they are optional/unimplemented features, not \
         spec-compliance bugs.",
        unexpected.len(),
    );
    assert!(
        stale.is_empty(),
        "stale {suite} baseline entries (now passing — remove them): {stale:?}",
    );
    for (spec_version, n) in &per_version {
        assert!(
            *n >= floor,
            "only {n} passing {suite} checks for {spec_version} (floor {floor}) — \
             the harness ran but produced almost nothing, which is a broken run, not a pass",
        );
    }
}
