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
    let (target_dir, profile) = target_dir_and_profile();
    let path = target_dir
        .join(&profile)
        .join("examples")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if path.is_file() {
        return path;
    }

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

/// The target dir this test binary was built in, and the profile directory
/// under it (`debug`, `release`).
///
/// The target dir is the nearest ancestor cargo marked with `CACHEDIR.TAG`.
/// Counting levels up from the binary stopped working in Rust 1.100, which
/// moved test binaries from `<target>/<profile>/deps/` to
/// `<target>/<profile>/build/<package>/<hash>/out/`; examples are still copied
/// to `<target>/<profile>/examples/`.
fn target_dir_and_profile() -> (std::path::PathBuf, String) {
    let exe = std::env::current_exe().expect("test binary path");
    let target_dir = exe
        .ancestors()
        .find(|dir| dir.join("CACHEDIR.TAG").is_file())
        .expect("the test binary is inside a cargo target dir");
    let profile = exe
        .strip_prefix(target_dir)
        .ok()
        .and_then(|rest| rest.components().next())
        .and_then(|dir| dir.as_os_str().to_str())
        .unwrap_or("debug")
        .to_owned();
    (target_dir.to_path_buf(), profile)
}
