//! The client response cache (SEP-2549) end-to-end: a server-declared
//! `ttlMs > 0` lets the client serve repeat `*/list` calls from memory; the
//! unconfigured server default (`ttlMs: 0`) and the legacy path are never
//! cached; `notifications/tools/list_changed` invalidates; the cache can be
//! disabled or cleared.

#![cfg(feature = "client")]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{Map, Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, split};
use turbomcp::client::{Client, ClientBuilder, ConnectMode};
use turbomcp::neutral::CachePolicy;
use turbomcp::prelude::*;
use turbomcp::{LegacySessionAdapter, SerdeJsonCodec, serve};
use turbomcp_transport_stdio::LineTransport;

#[derive(Clone)]
struct Demo;

#[server(name = "demo", version = "1.0.0")]
impl Demo {
    /// A tool so the `tools` capability is advertised.
    #[tool(description = "noop")]
    async fn noop(&self) -> McpResult<String> {
        Ok("ok".into())
    }
}

/// Serve `Demo` (optionally with a cache policy) over a duplex pipe and
/// connect a typed client in `mode` with the cache `enabled` or not.
async fn connect(policy: Option<CachePolicy>, mode: ConnectMode, cache_enabled: bool) -> Client {
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    let (s_rd, s_wr) = split(server_io);
    let transport = LineTransport::new(BufReader::new(s_rd), s_wr, SerdeJsonCodec);
    let mut builder = Demo.into_server();
    if let Some(policy) = policy {
        builder = builder.cache_policy(policy);
    }
    let service = LegacySessionAdapter::new(builder.build());
    tokio::spawn(serve(transport, service));

    let (c_rd, c_wr) = split(client_io);
    let client_transport = LineTransport::new(BufReader::new(c_rd), c_wr, SerdeJsonCodec);
    ClientBuilder::new("cache-test", "1.0.0")
        .with_connect_mode(mode)
        .with_response_cache(cache_enabled)
        .connect(client_transport)
        .await
        .expect("handshake succeeds")
}

#[tokio::test]
async fn declared_ttl_serves_repeat_lists_from_cache() {
    let client = connect(
        Some(CachePolicy::public(Duration::from_secs(60))),
        ConnectMode::Modern,
        true,
    )
    .await;

    // The raw wire carries the configured policy (`request` skips the cache).
    let raw = client.request("tools/list", Map::new()).await.unwrap();
    assert_eq!(raw.get("ttlMs"), Some(&json!(60_000)));
    assert_eq!(raw.get("cacheScope"), Some(&json!("public")));

    // Two typed calls, one wire round-trip: mutate nothing server-side, but
    // verify the second result is identical and instant (cache hit).
    let first = client.list_tools(None).await.expect("list 1");
    let second = client.list_tools(None).await.expect("list 2");
    assert_eq!(first.tools.len(), second.tools.len());
    // The decoded neutral result surfaces the server's policy to callers.
    assert_eq!(
        first.cache.map(|c| c.ttl_ms),
        Some(60_000),
        "neutral result carries the cache policy"
    );

    // Clearing forces the next call back to the wire (observable only as a
    // successful refetch here; the scripted test asserts wire counts).
    client.clear_response_cache();
    client.list_tools(None).await.expect("list 3");
}

#[tokio::test]
async fn unconfigured_server_and_legacy_path_are_never_cached() {
    // Draft path, no policy: ttlMs 0 on the wire.
    let client = connect(None, ConnectMode::Modern, true).await;
    let raw = client.request("tools/list", Map::new()).await.unwrap();
    assert_eq!(raw.get("ttlMs"), Some(&json!(0)));
    let first = client.list_tools(None).await.unwrap();
    assert_eq!(first.cache.map(|c| c.ttl_ms), Some(0));

    // Legacy path: no cache fields at all; the neutral result has no policy.
    let client = connect(
        Some(CachePolicy::public(Duration::from_secs(60))),
        ConnectMode::Legacy,
        true,
    )
    .await;
    let raw = client.request("tools/list", Map::new()).await.unwrap();
    assert!(raw.get("ttlMs").is_none());
    let listed = client.list_tools(None).await.unwrap();
    assert!(listed.cache.is_none());
}

// ---- scripted server: exact wire counts + notification invalidation ----------

/// A hand-scripted draft server on the far end of the pipe: answers
/// `server/discover`, `tools/list` and `resources/read` (counting each), and
/// `prompts/list` — and, when armed, writes an invalidating notification
/// *before* the next `prompts/list` response, so `list_prompts().await` is a
/// deterministic barrier.
///
/// `prompts/list` is the barrier because it is an uncounted request that
/// exists on this wire; `ping` would be simpler but `2026-07-28` removed it.
fn spawn_scripted_server(
    server_io: tokio::io::DuplexStream,
    list_hits: Arc<AtomicUsize>,
    read_hits: Arc<AtomicUsize>,
    notify_on_ping: Arc<AtomicUsize>,
    update_uri_on_ping: Arc<std::sync::Mutex<Option<String>>>,
) {
    tokio::spawn(async move {
        let (rd, mut wr) = split(server_io);
        let mut lines = BufReader::new(rd).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let frame: Value = serde_json::from_str(&line).expect("valid json from client");
            let id = frame.get("id").cloned().unwrap_or(Value::Null);
            let result = match frame.get("method").and_then(Value::as_str) {
                Some("server/discover") => json!({
                    "capabilities": {
                        "tools": { "listChanged": true },
                        "resources": { "listChanged": true },
                        "prompts": { "listChanged": true },
                    },
                    "supportedVersions": ["2026-07-28"],
                    "resultType": "complete", "cacheScope": "private", "ttlMs": 0
                }),
                Some("tools/list") => {
                    list_hits.fetch_add(1, Ordering::SeqCst);
                    json!({
                        "tools": [], "resultType": "complete",
                        "cacheScope": "private", "ttlMs": 60_000
                    })
                }
                Some("resources/read") => {
                    read_hits.fetch_add(1, Ordering::SeqCst);
                    let uri = frame["params"]["uri"].clone();
                    json!({
                        "contents": [{ "uri": uri, "text": "hi" }],
                        "resultType": "complete",
                        "cacheScope": "private", "ttlMs": 60_000
                    })
                }
                Some("prompts/list") => {
                    if notify_on_ping.load(Ordering::SeqCst) > 0 {
                        notify_on_ping.fetch_sub(1, Ordering::SeqCst);
                        let note = json!({
                            "jsonrpc": "2.0",
                            "method": "notifications/tools/list_changed"
                        });
                        wr.write_all(format!("{note}\n").as_bytes()).await.unwrap();
                    }
                    let pending_update = update_uri_on_ping.lock().unwrap().take();
                    if let Some(uri) = pending_update {
                        let note = json!({
                            "jsonrpc": "2.0",
                            "method": "notifications/resources/updated",
                            "params": { "uri": uri }
                        });
                        wr.write_all(format!("{note}\n").as_bytes()).await.unwrap();
                    }
                    // Uncounted and uncached (`ttlMs: 0`) so the barrier never
                    // perturbs what the test is measuring.
                    json!({
                        "prompts": [], "resultType": "complete",
                        "cacheScope": "private", "ttlMs": 0
                    })
                }
                other => panic!("unexpected method from client: {other:?}"),
            };
            let reply = json!({ "jsonrpc": "2.0", "id": id, "result": result });
            wr.write_all(format!("{reply}\n").as_bytes()).await.unwrap();
        }
    });
}

#[tokio::test]
async fn cache_hits_skip_the_wire_and_list_changed_invalidates() {
    let list_hits = Arc::new(AtomicUsize::new(0));
    let notify_on_ping = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_scripted_server(
        server_io,
        Arc::clone(&list_hits),
        Arc::new(AtomicUsize::new(0)),
        Arc::clone(&notify_on_ping),
        Arc::new(std::sync::Mutex::new(None)),
    );

    let (c_rd, c_wr) = split(client_io);
    let transport = LineTransport::new(BufReader::new(c_rd), c_wr, SerdeJsonCodec);
    let client = ClientBuilder::new("scripted", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(transport)
        .await
        .expect("handshake");

    // First call hits the wire; the next two are cache hits.
    client.list_tools(None).await.unwrap();
    client.list_tools(None).await.unwrap();
    client.list_tools(None).await.unwrap();
    assert_eq!(list_hits.load(Ordering::SeqCst), 1, "one wire round-trip");

    // Arm the notification; ping is the ordered barrier (the notification is
    // written before the ping response on the same pipe).
    notify_on_ping.store(1, Ordering::SeqCst);
    client.list_prompts(None).await.unwrap();

    client.list_tools(None).await.unwrap();
    assert_eq!(
        list_hits.load(Ordering::SeqCst),
        2,
        "list_changed invalidated the cached page"
    );

    // Manual clear also forces the wire.
    client.clear_response_cache();
    client.list_tools(None).await.unwrap();
    assert_eq!(list_hits.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn disabled_cache_always_hits_the_wire() {
    let list_hits = Arc::new(AtomicUsize::new(0));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_scripted_server(
        server_io,
        Arc::clone(&list_hits),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(AtomicUsize::new(0)),
        Arc::new(std::sync::Mutex::new(None)),
    );

    let (c_rd, c_wr) = split(client_io);
    let transport = LineTransport::new(BufReader::new(c_rd), c_wr, SerdeJsonCodec);
    let client = ClientBuilder::new("scripted", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .with_response_cache(false)
        .connect(transport)
        .await
        .expect("handshake");

    client.list_tools(None).await.unwrap();
    client.list_tools(None).await.unwrap();
    assert_eq!(list_hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn read_cache_is_per_uri_and_resources_updated_invalidates_only_that_uri() {
    let read_hits = Arc::new(AtomicUsize::new(0));
    let update_uri = Arc::new(std::sync::Mutex::new(None));
    let (client_io, server_io) = tokio::io::duplex(64 * 1024);
    spawn_scripted_server(
        server_io,
        Arc::new(AtomicUsize::new(0)),
        Arc::clone(&read_hits),
        Arc::new(AtomicUsize::new(0)),
        Arc::clone(&update_uri),
    );

    let (c_rd, c_wr) = split(client_io);
    let transport = LineTransport::new(BufReader::new(c_rd), c_wr, SerdeJsonCodec);
    let client = ClientBuilder::new("scripted", "1.0.0")
        .with_connect_mode(ConnectMode::Modern)
        .connect(transport)
        .await
        .expect("handshake");

    // Repeat reads of one URI are one round-trip; a second URI is its own entry.
    client.read_resource("mem://a").await.unwrap();
    client.read_resource("mem://a").await.unwrap();
    assert_eq!(read_hits.load(Ordering::SeqCst), 1, "mem://a cached");
    client.read_resource("mem://b").await.unwrap();
    assert_eq!(read_hits.load(Ordering::SeqCst), 2, "mem://b is distinct");

    // `resources/updated { uri: mem://a }` drops ONLY that entry.
    *update_uri.lock().unwrap() = Some("mem://a".to_owned());
    client.list_prompts(None).await.unwrap();

    client.read_resource("mem://b").await.unwrap();
    assert_eq!(read_hits.load(Ordering::SeqCst), 2, "mem://b still cached");
    client.read_resource("mem://a").await.unwrap();
    assert_eq!(read_hits.load(Ordering::SeqCst), 3, "mem://a refetched");
}
