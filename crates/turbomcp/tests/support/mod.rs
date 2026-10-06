//! Helpers shared by the integration tests that run example binaries.

/// An example binary, `name`.
///
/// `cargo test` builds examples alongside integration tests, so the artifact is
/// normally already there. Harnesses that select targets more narrowly do not —
/// `cargo llvm-cov --lib`/`--tests` builds no examples, and the test used to
/// fail with "example binary not built" against a path nobody had asked cargo
/// to produce. Cargo exposes `CARGO_BIN_EXE_*` for bins but has no equivalent
/// for examples, so build it on demand: the outer build has finished by the
/// time tests run, so the nested invocation takes the target-dir lock cleanly.
pub fn example_bin(name: &str) -> std::path::PathBuf {
    let mut target_dir = std::env::current_exe().expect("test binary path");
    target_dir.pop(); // …/<profile>/deps
    target_dir.pop(); // …/<profile>
    let profile = target_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("debug")
        .to_string();
    let path = target_dir
        .join("examples")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if path.is_file() {
        return path;
    }

    target_dir.pop(); // the target dir cargo is actually using
    let mut cargo = std::process::Command::new(env!("CARGO"));
    cargo
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["build", "--example", name, "--features", "client"])
        .arg("--target-dir")
        .arg(&target_dir);
    if profile == "release" {
        cargo.arg("--release");
    }
    let status = cargo.status().expect("spawn cargo to build the example");
    assert!(status.success(), "building the {name} example failed");
    assert!(
        path.is_file(),
        "cargo reported success but no example at {}",
        path.display()
    );
    path
}
