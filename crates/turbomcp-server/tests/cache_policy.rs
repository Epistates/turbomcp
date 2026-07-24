//! Server cache policy (SEP-2549): configured per-capability defaults surface
//! as the draft wire's `ttlMs`/`cacheScope`, a handler-set policy wins over
//! the default, the unconfigured default stays `0`/`"private"`, and the
//! legacy `2025-11-25` wire (which has no cache fields) is untouched.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::mpsc;
use turbomcp_core::{Implementation, JsonRpcMessage, McpResult};
use turbomcp_protocol::neutral::{self, CachePolicy};
use turbomcp_server::{
    CachePolicies, CallToolContext, ListToolsContext, McpServerCore, MethodRouter,
    ReadResourceContext, VersionDispatcher, WithResources, WithTools,
};
use turbomcp_service::{ServeConfig, Transport, serve_with};

struct MockTransport {
    inbound: mpsc::Receiver<JsonRpcMessage>,
    outbound: mpsc::UnboundedSender<JsonRpcMessage>,
}

impl Transport for MockTransport {
    type Error = std::io::Error;

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), Self::Error> {
        self.outbound
            .send(msg)
            .map_err(|_| std::io::Error::other("outbound closed"))
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
        Ok(self.inbound.recv().await)
    }

    async fn close(self) -> Result<(), Self::Error> {
        Ok(())
    }
}

/// `override_cache` makes `list_tools` set its own policy (handler wins).
#[derive(Clone)]
struct Catalog {
    override_cache: bool,
}

impl McpServerCore for Catalog {
    fn server_info(&self) -> Implementation {
        Implementation::new("catalog", "0.1.0")
    }
}

impl WithTools for Catalog {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        let result = neutral::ListToolsResult::new(vec![]);
        Ok(if self.override_cache {
            result.with_cache(CachePolicy::public(Duration::from_secs(300)))
        } else {
            result
        })
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("ok"))
    }
}

impl WithResources for Catalog {
    async fn list_resources(
        &self,
        _ctx: &turbomcp_server::ListResourcesContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        Ok(neutral::ListResourcesResult::new(vec![]))
    }

    async fn read_resource(
        &self,
        _ctx: &ReadResourceContext,
        _params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::text("mem://a", "hi"))
    }
}

/// Run `frames` through a serve loop over a dispatcher built with `cache` and
/// collect everything the server writes.
async fn run(server: Catalog, cache: Option<CachePolicies>, frames: Vec<Value>) -> Vec<Value> {
    let (in_tx, in_rx) = mpsc::channel(8);
    let (out_tx, mut out_rx) = mpsc::unbounded_channel();
    let router = MethodRouter::new().with_tools().with_resources();
    let mut dispatcher = VersionDispatcher::new(server, router);
    if let Some(cache) = cache {
        dispatcher = dispatcher.with_cache_policy(cache);
    }
    let service = turbomcp_server::LegacySessionAdapter::new(dispatcher);
    let transport = MockTransport {
        inbound: in_rx,
        outbound: out_tx,
    };
    let task = tokio::spawn(serve_with(transport, service, ServeConfig::default()));

    for frame in frames {
        in_tx
            .send(serde_json::from_value(frame).expect("valid frame"))
            .await
            .unwrap();
    }
    drop(in_tx);
    task.await.unwrap().expect("serve loop exits cleanly");

    let mut out = Vec::new();
    while let Ok(msg) = out_rx.try_recv() {
        out.push(serde_json::to_value(&msg).unwrap());
    }
    out
}

fn draft_request(id: u64, method: &str, mut params: Value) -> Value {
    params.as_object_mut().expect("object params").insert(
        "_meta".into(),
        json!({ "io.modelcontextprotocol/protocolVersion": "2026-07-28" }),
    );
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

fn result_of(frames: &[Value], id: u64) -> &Value {
    frames
        .iter()
        .find(|f| f["id"] == json!(id))
        .unwrap_or_else(|| panic!("no response for id {id}: {frames:#?}"))
        .get("result")
        .expect("success result")
}

#[tokio::test]
async fn configured_policy_fills_draft_cacheable_results() {
    let cache = CachePolicies::uniform(CachePolicy::public(Duration::from_secs(60)));
    let frames = run(
        Catalog {
            override_cache: false,
        },
        Some(cache),
        vec![
            draft_request(1, "tools/list", json!({})),
            draft_request(2, "resources/list", json!({})),
            draft_request(3, "resources/read", json!({ "uri": "mem://a" })),
            // `server/discover` lost its cache fields in the 2026-07-28 RC.
            draft_request(4, "server/discover", json!({})),
        ],
    )
    .await;

    for id in 1..=3 {
        let result = result_of(&frames, id);
        assert_eq!(result["ttlMs"], json!(60_000), "id {id}: {result:#?}");
        assert_eq!(result["cacheScope"], json!("public"), "id {id}");
    }
    let discover = result_of(&frames, 4);
    assert!(discover.get("ttlMs").is_none(), "{discover:#?}");
    assert!(discover.get("cacheScope").is_none(), "{discover:#?}");
}

#[tokio::test]
async fn handler_set_policy_wins_over_the_default() {
    let cache = CachePolicies::uniform(CachePolicy::private(Duration::from_secs(1)));
    let frames = run(
        Catalog {
            override_cache: true,
        },
        Some(cache),
        vec![draft_request(1, "tools/list", json!({}))],
    )
    .await;

    let result = result_of(&frames, 1);
    assert_eq!(result["ttlMs"], json!(300_000));
    assert_eq!(result["cacheScope"], json!("public"));
}

#[tokio::test]
async fn unconfigured_default_is_no_cache() {
    let frames = run(
        Catalog {
            override_cache: false,
        },
        None,
        vec![draft_request(1, "tools/list", json!({}))],
    )
    .await;

    let result = result_of(&frames, 1);
    assert_eq!(result["ttlMs"], json!(0));
    assert_eq!(result["cacheScope"], json!("private"));
}

#[tokio::test]
async fn legacy_wire_carries_no_cache_fields() {
    let cache = CachePolicies::uniform(CachePolicy::public(Duration::from_secs(60)));
    let frames = run(
        Catalog {
            override_cache: false,
        },
        Some(cache),
        vec![
            json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "legacy", "version": "1" },
                }
            }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} }),
        ],
    )
    .await;

    let result = result_of(&frames, 2);
    assert!(
        result.get("ttlMs").is_none(),
        "legacy has no ttlMs: {result:#?}"
    );
    assert!(result.get("cacheScope").is_none());
}

/// Per-capability granularity: only the configured surface changes.
#[tokio::test]
async fn per_capability_policies_apply_independently() {
    let cache = CachePolicies::default().tools_list(CachePolicy::public(Duration::from_secs(30)));
    let frames = run(
        Catalog {
            override_cache: false,
        },
        Some(cache),
        vec![
            draft_request(1, "tools/list", json!({})),
            draft_request(2, "resources/list", json!({})),
        ],
    )
    .await;

    let tools = result_of(&frames, 1);
    assert_eq!(tools["ttlMs"], json!(30_000));
    assert_eq!(tools["cacheScope"], json!("public"));

    let resources = result_of(&frames, 2);
    assert_eq!(resources["ttlMs"], json!(0));
    assert_eq!(resources["cacheScope"], json!("private"));
}
