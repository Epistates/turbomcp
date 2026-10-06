//! Real upstreams behind the gateway: the `hello_world` example as a child
//! process (and what it inherits of the proxy's environment), and served
//! over Streamable HTTP and WebSocket, under a network policy.
#![cfg(all(feature = "client", feature = "proxy"))]

mod support;

use serde_json::{Map, json};
use support::example_bin;
use turbomcp::client::ClientBuilder;
use turbomcp::proxy::{RemoteServer, Upstream};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stdio_upstream_serves_and_shuts_down() {
    let bin = example_bin("hello_world");
    let remote = RemoteServer::connect(Upstream::stdio(bin.to_string_lossy(), [] as [&str; 0]))
        .await
        .expect("spawn and handshake");
    let client =
        turbomcp::testing::connect(remote.clone().into_server(), ClientBuilder::new("a", "1"))
            .await
            .unwrap();
    let mut args = Map::new();
    args.insert("name".into(), json!("Ada"));
    let result = client.call_tool("hello", args).await.unwrap();
    assert!(result.text_content().unwrap().contains("Ada"), "{result:?}");
    tokio::time::timeout(std::time::Duration::from_secs(10), remote.shutdown())
        .await
        .expect("the child shut down in time");
}

/// What a stdio upstream launched with `inherit` sees of the environment:
/// a shell records it, then becomes the real server.
#[cfg(unix)]
async fn environment_seen_with(inherit: Option<turbomcp::proxy::Inherit>) -> String {
    let out = std::env::temp_dir().join(format!(
        "turbomcp-proxy-env-{}-{}",
        std::process::id(),
        inherit.map_or("default".into(), |i| format!("{i:?}"))
    ));
    let bin = example_bin("hello_world");
    let mut upstream = Upstream::stdio(
        "/bin/sh",
        [
            "-c".to_owned(),
            "env > \"$OUT\"; exec \"$0\"".to_owned(),
            bin.to_string_lossy().into_owned(),
        ],
    )
    .env("OUT", out.to_string_lossy());
    if let Some(inherit) = inherit {
        upstream = upstream.inherit_env(inherit);
    }
    let remote = RemoteServer::connect(upstream).await.expect("spawn");
    let seen = std::fs::read_to_string(&out).expect("the child recorded its environment");
    let _ = std::fs::remove_file(&out);
    remote.shutdown().await;
    seen
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stdio_upstream_inherits_only_what_a_program_needs() {
    // Cargo sets it for the test process: a stand-in for the proxy's secrets.
    assert!(std::env::var_os("CARGO_MANIFEST_DIR").is_some());
    let has = |env: &str, name: &str| env.lines().any(|l| l.starts_with(&format!("{name}=")));

    let safe = environment_seen_with(None).await;
    assert!(has(&safe, "PATH"), "{safe}");
    assert!(has(&safe, "OUT"), "what the operator set is passed: {safe}");
    assert!(!has(&safe, "CARGO_MANIFEST_DIR"), "{safe}");

    let all = environment_seen_with(Some(turbomcp::proxy::Inherit::All)).await;
    assert!(has(&all, "CARGO_MANIFEST_DIR"), "{all}");
}

/// `hello_world`'s server over HTTP on loopback, with a WebSocket route at
/// `/ws`; its address.
#[cfg(feature = "websocket")]
async fn served() -> std::net::SocketAddr {
    use turbomcp::http::{Http, HttpConfig, WebSocketConfig};
    use turbomcp::prelude::*;

    #[derive(Clone)]
    struct Hello;

    #[server(name = "hello", version = "1.0.0")]
    impl Hello {
        /// Say hello.
        #[tool]
        async fn hello(&self, name: String) -> String {
            format!("Hello, {name}!")
        }
    }

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let config = HttpConfig::new().with_websocket(WebSocketConfig::new("/ws"));
    tokio::spawn(
        Hello
            .into_server()
            .serve(Http::listener(listener).config(config)),
    );
    addr
}

#[cfg(feature = "websocket")]
async fn hello_through(remote: RemoteServer) -> String {
    let client = turbomcp::testing::connect(remote.into_server(), ClientBuilder::new("a", "1"))
        .await
        .unwrap();
    let mut args = Map::new();
    args.insert("name".into(), json!("Ada"));
    client
        .call_tool("hello", args)
        .await
        .unwrap()
        .text_content()
        .unwrap()
}

#[cfg(feature = "websocket")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_and_websocket_upstreams_connect_under_a_policy_that_allows_them() {
    use turbomcp::proxy::NetworkPolicy;
    let addr = served().await;
    // The default policy allows plaintext on loopback.
    for upstream in [
        Upstream::http(format!("http://{addr}/mcp")),
        Upstream::websocket(format!("ws://{addr}/ws")),
    ] {
        let remote = RemoteServer::builder(upstream.clone())
            .network_policy(NetworkPolicy::default())
            .connect()
            .await
            .unwrap_or_else(|e| panic!("{upstream:?}: {e}"));
        assert_eq!(hello_through(remote).await, "Hello, Ada!", "{upstream:?}");
    }
}

#[cfg(feature = "websocket")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_public_only_policy_refuses_an_internal_upstream() {
    use turbomcp::proxy::{NetworkPolicy, ProxyError};
    let addr = served().await;
    for upstream in [
        Upstream::http(format!("http://{addr}/mcp")),
        Upstream::websocket(format!("ws://{addr}/ws")),
        // By name, too: refused once resolved, whatever the scheme allows.
        Upstream::http(format!("https://localhost:{}/mcp", addr.port())),
        Upstream::websocket(format!("wss://localhost:{}/ws", addr.port())),
    ] {
        let err = RemoteServer::builder(upstream.clone())
            .network_policy(NetworkPolicy::public_only())
            .connect()
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                ProxyError::Config(_) | ProxyError::Dial { .. } | ProxyError::Connect { .. }
            ),
            "{upstream:?}: {err}"
        );
    }
}
