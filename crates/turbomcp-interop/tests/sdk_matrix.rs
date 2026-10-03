//! The cross-SDK interop matrix over Streamable HTTP: turbomcp against the
//! official TypeScript (v2), Python and Go SDKs, in both directions and both
//! protocol eras (`2025-11-25` sessions and stateless `2026-07-28`).
//!
//! Each peer under `sdks/` is the smallest real program: a server with one
//! `add` tool that prints `READY <port>`, and a client that connects to a
//! URL, lists the tools, calls `add(2, 3)` and prints a JSON line.
//!
//! A peer whose toolchain (`node`, `uv`, `go`) is missing is skipped with a
//! note, so the suite runs anywhere; CI sets `TURBOMCP_INTEROP_REQUIRE=1`,
//! which turns a skip into a failure.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use turbomcp::client::{ClientBuilder, ConnectMode, connect_http};
use turbomcp::http::{Http, HttpConfig};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Adder;

#[server(name = "turbomcp-adder", version = "1.0.0")]
impl Adder {
    /// Add two integers.
    #[tool]
    async fn add(&self, a: i64, b: i64) -> String {
        (a + b).to_string()
    }
}

/// One peer SDK: how to run its server and its client.
#[derive(Clone, Copy, Debug)]
enum Sdk {
    TypeScript,
    Python,
    Go,
}

impl Sdk {
    fn dir(self) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("sdks")
            .join(match self {
                Self::TypeScript => "ts",
                Self::Python => "python",
                Self::Go => "go",
            })
    }

    fn toolchain(self) -> &'static str {
        match self {
            Self::TypeScript => "node",
            Self::Python => "uv",
            Self::Go => "go",
        }
    }

    /// Whether the peer can run here; installs the TypeScript packages on
    /// first use (the others resolve their own dependencies when run).
    async fn ready(self) -> bool {
        let found = Command::new(self.toolchain())
            // `go` takes `version`, not `--version`.
            .arg(if matches!(self, Self::Go) {
                "version"
            } else {
                "--version"
            })
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .is_ok_and(|s| s.success());
        if !found {
            assert!(
                std::env::var_os("TURBOMCP_INTEROP_REQUIRE").is_none(),
                "{self:?}: `{}` is required but missing",
                self.toolchain()
            );
            eprintln!("skipping {self:?}: `{}` not found", self.toolchain());
            return false;
        }
        if matches!(self, Self::TypeScript) && !self.dir().join("node_modules").exists() {
            let status = Command::new("npm")
                .args(["ci", "--silent"])
                .current_dir(self.dir())
                .status()
                .await
                .expect("npm runs");
            assert!(status.success(), "npm ci failed");
        }
        true
    }

    fn command(self, role: &str) -> Command {
        let mut command = match self {
            Self::TypeScript => {
                let mut c = Command::new("node");
                c.arg(format!("{role}.mjs"));
                c
            }
            Self::Python => {
                let mut c = Command::new("uv");
                c.args(["run", "--quiet", &format!("{role}.py")]);
                c
            }
            Self::Go => {
                let mut c = Command::new("go");
                c.args(["run", &format!("./{role}")]);
                c
            }
        };
        command.current_dir(self.dir()).kill_on_drop(true);
        command
    }

    /// Start the peer's server; its URL once it says it is serving.
    async fn serve(self) -> (PeerServer, String) {
        let mut command = self.command("server");
        // Its own process group: `uv run` and `go run` start the real server
        // as a grandchild, which killing the direct child leaves running
        // (holding the test's stdout open, so the run never ends).
        #[cfg(unix)]
        command.process_group(0);
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the peer server starts");
        let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
        let port = tokio::time::timeout(Duration::from_secs(120), async {
            while let Some(line) = lines.next_line().await.expect("server stdout") {
                if let Some(port) = line.strip_prefix("READY ") {
                    return port.trim().to_owned();
                }
            }
            panic!("{self:?} server exited before serving");
        })
        .await
        .expect("the peer server comes up");
        // Keep draining its stdout so a chatty server never blocks on it.
        tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
        (PeerServer(child), format!("http://127.0.0.1:{port}/mcp"))
    }

    /// Run the peer's client against `url` in `era`; what it printed.
    async fn call(self, url: &str, era: &str) -> Value {
        let out = tokio::time::timeout(
            Duration::from_secs(120),
            self.command("client")
                .args([url, era])
                .stderr(Stdio::inherit())
                .output(),
        )
        .await
        .expect("the peer client finishes")
        .expect("the peer client runs");
        assert!(
            out.status.success(),
            "{self:?} client failed in the {era} era"
        );
        let stdout = String::from_utf8(out.stdout).unwrap();
        let line = stdout.lines().last().expect("a JSON line");
        serde_json::from_str(line).unwrap_or_else(|e| panic!("{self:?} printed {line:?}: {e}"))
    }
}

/// A running peer server; dropping it ends the server's whole process group.
struct PeerServer(Child);

impl Drop for PeerServer {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.id() {
            let _ = std::process::Command::new("kill")
                .args(["-KILL", &format!("-{pid}")])
                .status();
        }
        let _ = self.0.start_kill();
    }
}

/// Serve [`Adder`] over HTTP on an ephemeral port.
async fn turbomcp_server() -> (String, turbomcp::CancellationToken) {
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let shutdown = turbomcp::CancellationToken::new();
    tokio::spawn(
        Adder.into_server().serve(
            Http::listener(listener).config(HttpConfig::new().with_shutdown(shutdown.clone())),
        ),
    );
    (url, shutdown)
}

/// The peer's client drives turbomcp's server, in both eras.
async fn peer_client_drives_turbomcp(sdk: Sdk) {
    if !sdk.ready().await {
        return;
    }
    let (url, shutdown) = turbomcp_server().await;
    for era in ["legacy", "modern"] {
        let seen = sdk.call(&url, era).await;
        assert_eq!(
            seen,
            json!({ "tools": ["add"], "text": "5", "isError": false }),
            "{sdk:?} client, {era} era"
        );
    }
    shutdown.cancel();
}

/// turbomcp's client drives the peer's server, in both eras.
async fn turbomcp_drives_peer_server(sdk: Sdk) {
    if !sdk.ready().await {
        return;
    }
    let (_child, url) = sdk.serve().await;
    for (mode, era) in [
        (ConnectMode::Legacy, ProtocolVersion::V2025_11_25),
        (ConnectMode::Modern, ProtocolVersion::V2026_07_28),
    ] {
        let client = connect_http(
            ClientBuilder::new("turbomcp-interop", "1.0.0").with_connect_mode(mode),
            &url,
        )
        .await
        .unwrap_or_else(|e| panic!("{sdk:?} server, {era}: connect: {e}"));
        assert_eq!(client.protocol_version(), &era, "{sdk:?} server");
        let tools = client.list_tools(None).await.expect("list");
        assert_eq!(tools.tools.len(), 1, "{sdk:?} server, {era}");
        assert_eq!(tools.tools[0].name, "add");
        let mut args = Map::new();
        args.insert("a".into(), json!(2));
        args.insert("b".into(), json!(3));
        let result = client.call_tool("add", args).await.expect("call");
        assert_eq!(
            result.text_content().as_deref(),
            Some("5"),
            "{sdk:?} server, {era}"
        );
        client.close().await;
    }
}

use turbomcp_core::ProtocolVersion;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typescript_client_drives_turbomcp() {
    peer_client_drives_turbomcp(Sdk::TypeScript).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turbomcp_drives_typescript_server() {
    turbomcp_drives_peer_server(Sdk::TypeScript).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn python_client_drives_turbomcp() {
    peer_client_drives_turbomcp(Sdk::Python).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turbomcp_drives_python_server() {
    turbomcp_drives_peer_server(Sdk::Python).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn go_client_drives_turbomcp() {
    peer_client_drives_turbomcp(Sdk::Go).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn turbomcp_drives_go_server() {
    turbomcp_drives_peer_server(Sdk::Go).await;
}
