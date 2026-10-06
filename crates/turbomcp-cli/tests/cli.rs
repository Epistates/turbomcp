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
