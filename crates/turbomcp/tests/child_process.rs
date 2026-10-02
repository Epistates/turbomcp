//! `connect_child` smoke test: spawn the `hello_world` example as a real
//! subprocess, run the handshake over its stdio, exercise one tool call, and
//! tear the child down. This is the common local-MCP deployment shape — a
//! client that owns the server process — end to end.
#![cfg(feature = "client")]

use serde_json::{Map, json};
use tokio::process::Command;
use turbomcp::client::{ClientBuilder, connect_child};

/// An example binary, `name`.
///
/// `cargo test` builds examples alongside integration tests, so the artifact is
/// normally already there. Harnesses that select targets more narrowly do not —
/// `cargo llvm-cov --lib`/`--tests` builds no examples, and the test used to
/// fail with "example binary not built" against a path nobody had asked cargo
/// to produce. Cargo exposes `CARGO_BIN_EXE_*` for bins but has no equivalent
/// for examples, so build it on demand: the outer build has finished by the
/// time tests run, so the nested invocation takes the target-dir lock cleanly.
fn example_bin(name: &str) -> std::path::PathBuf {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_child_spawns_handshakes_and_calls() {
    let (client, mut child) = connect_child(
        ClientBuilder::new("child-smoke", "1.0.0"),
        Command::new(example_bin("hello_world")),
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

/// A token-triggered shutdown ends the process promptly even while the client
/// holds stdin open. tokio's own stdin reads on the blocking pool with a read
/// that can't be cancelled, and dropping the runtime waits for it: `serve`
/// returned, then `main` hung until the client wrote again, so clients had to
/// kill the server.
#[cfg(unix)]
#[test]
fn a_signalled_stdio_server_exits_with_stdin_still_open() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut child = Command::new(example_bin("graceful_shutdown"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the example");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    // A round trip first: the server is up and its signal handler installed,
    // so the signal below is handled rather than killing the process outright.
    let init = json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "t", "version": "1" },
        }
    });
    writeln!(stdin, "{init}").unwrap();
    let mut line = String::new();
    stdout.read_line(&mut line).unwrap();
    assert!(line.contains("\"result\""), "initialize answered: {line}");

    let status = Command::new("kill")
        .args(["-INT", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());

    // `stdin` is still open here.
    let deadline = Instant::now() + Duration::from_secs(5);
    let exit = loop {
        if let Some(exit) = child.try_wait().unwrap() {
            break exit;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the server did not exit with stdin held open");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert!(exit.success(), "a clean, handled shutdown: {exit:?}");
    drop(stdin);
}
