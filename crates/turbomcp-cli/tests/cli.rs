//! The `turbomcp` binary end to end, against the facade's `hello_world`
//! example run over stdio, and against itself serving a configuration.

use std::path::PathBuf;
use std::process::{Command, Output};

use serde_json::Value;

/// The `hello_world` example, built if this run didn't build it.
fn hello_world() -> PathBuf {
    let mut dir = std::env::current_exe().expect("test binary path");
    dir.pop(); // …/<profile>/deps
    dir.pop(); // …/<profile>
    let path = dir
        .join("examples")
        .join(format!("hello_world{}", std::env::consts::EXE_SUFFIX));
    if !path.is_file() {
        let profile = dir
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("debug")
            .to_owned();
        let mut target = dir.clone();
        target.pop();
        let mut cargo = Command::new(env!("CARGO"));
        cargo
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "build",
                "-p",
                "turbomcp",
                "--example",
                "hello_world",
                "--features",
                "client",
            ])
            .arg("--target-dir")
            .arg(&target);
        if profile == "release" {
            cargo.arg("--release");
        }
        assert!(
            cargo.status().expect("cargo").success(),
            "building hello_world"
        );
    }
    path
}

fn turbomcp(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_turbomcp"))
        .args(args)
        .output()
        .expect("running turbomcp")
}

fn stdout(output: &Output) -> String {
    assert!(
        output.status.success(),
        "turbomcp failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout.clone()).expect("UTF-8 output")
}

fn hello() -> String {
    hello_world().to_string_lossy().into_owned()
}

#[test]
fn tools_are_listed_for_people_and_for_scripts() {
    let human = stdout(&turbomcp(&["tools", &hello()]));
    assert!(human.starts_with("hello"), "{human}");
    assert!(human.contains("Say hello"), "{human}");

    let json: Value =
        serde_json::from_str(&stdout(&turbomcp(&["tools", "--json", &hello()]))).expect("JSON");
    assert_eq!(json[0]["name"], "hello");
    assert_eq!(json[0]["inputSchema"]["required"][0], "name");
}

#[test]
fn a_tool_is_called_with_its_arguments() {
    let out = stdout(&turbomcp(&["call", "hello", "-a", "name=Ada", &hello()]));
    assert_eq!(out.trim(), "Hello, Ada!");
    let out = stdout(&turbomcp(&[
        "call",
        "hello",
        "--args",
        r#"{"name": "Bob"}"#,
        "--json",
        &hello(),
    ]));
    let json: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(json["content"][0]["text"], "Hello, Bob!");
}

#[test]
fn a_failed_call_fails_the_command_and_says_why() {
    let output = turbomcp(&["call", "nope", &hello()]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("nope"), "{stderr}");
}

#[test]
fn a_probe_reports_every_revision_the_server_speaks() {
    let json: Value =
        serde_json::from_str(&stdout(&turbomcp(&["probe", "--json", &hello()]))).unwrap();
    assert_eq!(json["server"]["name"], "hello");
    assert_eq!(json["negotiated"], "2026-07-28");
    assert_eq!(json["stateful"], false);
    assert_eq!(json["revisions"]["2026-07-28"]["negotiated"], "2026-07-28");
    assert_eq!(json["revisions"]["2025-*"]["negotiated"], "2025-11-25");
    assert_eq!(json["counts"]["tools"], 1);
    assert!(json["capabilities"]["tools"].is_object());
}

/// A configuration file with `body`, unique to `test`.
fn config(test: &str, body: &str) -> PathBuf {
    let path =
        std::env::temp_dir().join(format!("turbomcp-cli-{test}-{}.json", std::process::id()));
    std::fs::write(&path, body).unwrap();
    path
}

#[test]
fn the_proxy_serves_a_configuration_as_one_server() {
    let hello = serde_json::to_string(&hello()).unwrap();
    let path = config(
        "proxy",
        &format!(
            r#"{{"mcpServers": {{
                "left": {{ "command": {hello} }},
                "right": {{ "command": {hello} }},
                "gone": {{ "command": "/nonexistent/server" }},
                "off": {{ "command": "/nonexistent/server", "disabled": true }}
            }}}}"#
        ),
    );
    let me = env!("CARGO_BIN_EXE_turbomcp");
    let config = path.to_string_lossy().into_owned();
    // The CLI's own client, against the CLI's own gateway, over stdio.
    let json: Value = serde_json::from_str(&stdout(&turbomcp(&[
        "tools", "--json", me, "proxy", "--config", &config,
    ])))
    .unwrap();
    let mut names: Vec<&str> = json
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["left__hello", "right__hello"]);

    let out = stdout(&turbomcp(&[
        "call",
        "right__hello",
        "-a",
        "name=Cy",
        me,
        "proxy",
        "--config",
        &config,
    ]));
    assert_eq!(out.trim(), "Hello, Cy!");

    // `--strict`: an unreachable server is fatal, not skipped.
    let strict = turbomcp(&["tools", me, "proxy", "--config", &config, "--strict"]);
    assert!(!strict.status.success());
    let _ = std::fs::remove_file(path);
}

#[test]
fn a_configuration_the_proxy_cannot_serve_is_refused_with_a_reason() {
    let path = config(
        "sse",
        r#"{"mcpServers": {"old": {"type": "sse", "url": "https://example.com/sse"}}}"#,
    );
    let output = turbomcp(&["proxy", "--config", &path.to_string_lossy()]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Streamable HTTP"), "{stderr}");
    let _ = std::fs::remove_file(path);
}

/// A raw HTTP/1.1 exchange with `addr`: the status line and the rest.
fn http(addr: std::net::SocketAddr, request: &str) -> (String, String) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(addr).expect("connect");
    stream
        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    let (status, rest) = response.split_once("\r\n").unwrap_or((&response, ""));
    (status.to_owned(), rest.to_owned())
}

#[test]
fn the_http_gateway_can_require_bearer_tokens() {
    let hello = serde_json::to_string(&hello()).unwrap();
    let path = config(
        "auth",
        &format!(r#"{{"mcpServers": {{"hi": {{"command": {hello}}}}}}}"#),
    );
    let addr = {
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap()
    };
    let mut gateway = Command::new(env!("CARGO_BIN_EXE_turbomcp"))
        .args([
            "proxy",
            "--config",
            &path.to_string_lossy(),
            "--http",
            &addr.to_string(),
        ])
        .args(["--auth-issuer", "https://auth.example.com"])
        .args(["--auth-jwks", "https://auth.example.com/jwks.json"])
        .stdin(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while std::net::TcpStream::connect(addr).is_err() {
        assert!(
            std::time::Instant::now() < deadline,
            "the gateway never listened"
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
    }

    let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
    let (status, rest) = http(
        addr,
        &format!(
            "POST /mcp HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\n\
             Accept: application/json, text/event-stream\r\nContent-Length: {}\r\n\
             Connection: close\r\n\r\n{body}",
            body.len()
        ),
    );
    assert!(status.contains("401"), "{status}");
    let metadata_url = format!("http://{addr}/.well-known/oauth-protected-resource/mcp");
    assert!(rest.contains(&metadata_url), "{rest}");

    let (status, rest) = http(
        addr,
        &format!(
            "GET /.well-known/oauth-protected-resource/mcp HTTP/1.1\r\nHost: {addr}\r\n\
             Connection: close\r\n\r\n"
        ),
    );
    assert!(status.contains("200"), "{status}");
    assert!(rest.contains("https://auth.example.com"), "{rest}");
    assert!(rest.contains(&format!("http://{addr}/mcp")), "{rest}");

    let _ = gateway.kill();
    let _ = gateway.wait();
    let _ = std::fs::remove_file(path);
}
