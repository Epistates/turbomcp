//! `connect_child` smoke test: spawn the `hello_world` example as a real
//! subprocess, run the handshake over its stdio, exercise one tool call, and
//! tear the child down. This is the common local-MCP deployment shape — a
//! client that owns the server process — end to end.
#![cfg(feature = "client")]

use serde_json::{Map, json};
use tokio::process::Command;
use turbomcp::client::{ClientBuilder, connect_child};

/// The `hello_world` example binary.
///
/// `cargo test` builds examples alongside integration tests, so the artifact is
/// normally already there. Harnesses that select targets more narrowly do not —
/// `cargo llvm-cov --lib`/`--tests` builds no examples, and the test used to
/// fail with "example binary not built" against a path nobody had asked cargo
/// to produce. Cargo exposes `CARGO_BIN_EXE_*` for bins but has no equivalent
/// for examples, so build it on demand: the outer build has finished by the
/// time tests run, so the nested invocation takes the target-dir lock cleanly.
fn hello_world_bin() -> std::path::PathBuf {
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
        .join(format!("hello_world{}", std::env::consts::EXE_SUFFIX));
    if path.is_file() {
        return path;
    }

    target_dir.pop(); // the target dir cargo is actually using
    let mut cargo = std::process::Command::new(env!("CARGO"));
    cargo
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(["build", "--example", "hello_world", "--features", "client"])
        .arg("--target-dir")
        .arg(&target_dir);
    if profile == "release" {
        cargo.arg("--release");
    }
    let status = cargo.status().expect("spawn cargo to build the example");
    assert!(status.success(), "building the hello_world example failed");
    assert!(
        path.is_file(),
        "cargo reported success but no example at {}",
        path.display()
    );
    path
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_child_spawns_handshakes_and_calls() {
    let (client, mut child) = connect_child(
        ClientBuilder::new("child-smoke", "1.0.0"),
        Command::new(hello_world_bin()),
    )
    .await
    .expect("spawn + handshake");

    assert_eq!(client.server_info().expect("server info").name, "hello");

    let tools = client.list_tools(None).await.expect("list_tools");
    assert_eq!(tools.tools.len(), 1);
    assert_eq!(tools.tools[0].name, "hello");

    let mut args = Map::new();
    args.insert("name".into(), json!("world"));
    let result = client.call_tool("hello", args).await.expect("call_tool");
    match &result.content[0] {
        turbomcp::neutral::Content::Text { text, .. } => assert_eq!(text, "Hello, world!"),
        other => panic!("expected text content, got {other:?}"),
    }

    child.kill().await.expect("child teardown");
}
