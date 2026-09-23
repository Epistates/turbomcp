//! End-to-end: the `turbomcp-proxy` binary serving a stdio frontend.
//!
//! With a stdio frontend, stdout *is* the MCP channel. These tests drive the
//! real binary, with verbose logging on, in front of a scripted upstream, and
//! hold stdout to carrying nothing but JSON-RPC.

#![cfg(all(unix, feature = "cli"))]

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

use serde_json::{Value, json};

/// A minimal MCP server in POSIX sh: one tool, `echo`, answering whatever id
/// the client used.
const FAKE_UPSTREAM: &str = r#"
while IFS= read -r line; do
  id=$(printf '%s\n' "$line" | sed -E 's/.*"id":("[^"]*"|[0-9]+).*/\1/')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"fake","version":"1.0.0"}}}\n' "$id" ;;
    *'"method":"tools/list"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"tools":[{"name":"echo","inputSchema":{"type":"object"}}]}}\n' "$id" ;;
    *'"method":"tools/call"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"content":[{"type":"text","text":"echoed"}]}}\n' "$id" ;;
  esac
done
"#;

fn send(stdin: &mut impl Write, message: &Value) {
    writeln!(stdin, "{message}").expect("proxy stdin is open");
}

/// PX-V1: the CLI logged to stdout, so `-v` against a stdio frontend
/// interleaved log lines with protocol messages and broke every client. The
/// handshake itself also has to be a real one: `initialize` returns a
/// `protocolVersion` and `serverInfo`, `ping` gets `{}`, and the
/// `notifications/initialized` in between gets nothing.
#[test]
fn stdout_carries_only_json_rpc_even_when_verbose() {
    let mut proxy = Command::new(env!("CARGO_BIN_EXE_turbomcp-proxy"))
        .args(["-vv", "serve", "--backend", "stdio", "--cmd", "sh"])
        .arg("--args=-c")
        .arg(format!("--args={FAKE_UPSTREAM}"))
        .args(["--frontend", "stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the proxy binary starts");

    let mut stdin = proxy.stdin.take().expect("stdin");
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": { "name": "e2e", "version": "1.0.0" }
            }
        }),
    );
    send(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    );
    send(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "ping" }),
    );
    send(
        &mut stdin,
        &json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "echo", "arguments": {} }
        }),
    );

    let mut stdout = BufReader::new(proxy.stdout.take().expect("stdout")).lines();
    let mut responses = Vec::new();
    for _ in 0..3 {
        let line = stdout
            .next()
            .expect("the proxy answers")
            .expect("stdout is readable");
        let message: Value = serde_json::from_str(&line)
            .unwrap_or_else(|e| panic!("stdout carried a non-JSON line ({e}): {line}"));
        responses.push(message);
    }

    drop(stdin);
    let status = proxy.wait().expect("the proxy exits");
    // Anything left on stdout after EOF must be JSON-RPC too.
    for line in stdout {
        let line = line.expect("stdout is readable");
        assert!(
            serde_json::from_str::<Value>(&line).is_ok(),
            "stdout carried a non-JSON line: {line}"
        );
    }
    let mut logs = String::new();
    std::io::Read::read_to_string(&mut proxy.stderr.take().expect("stderr"), &mut logs)
        .expect("stderr is readable");

    assert!(status.success(), "proxy failed: {logs}");
    assert!(!logs.is_empty(), "-vv logs should land on stderr");

    assert_eq!(responses[0]["id"], 1);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "fake-proxy");
    assert_eq!(
        responses[1],
        json!({ "jsonrpc": "2.0", "id": 2, "result": {} })
    );
    assert_eq!(responses[2]["id"], 3);
    assert_eq!(responses[2]["result"]["content"][0]["text"], "echoed");
}
