//! A notification bus carries a change from the replica that saw it to the
//! replica holding the subscriber's stream.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::mpsc;
use tower::{Service, ServiceExt};
use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpRequest, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, Change, LegacySessionAdapter, ListToolsContext, LocalBus, McpServerCore,
    NotificationBus, ServerBuilder, WithTools,
};
use turbomcp_service::Peer;

#[derive(Clone)]
struct Tools;

impl McpServerCore for Tools {
    fn server_info(&self) -> Implementation {
        Implementation::new("tools", "1.0.0")
    }
}

impl WithTools for Tools {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text(""))
    }
}

/// Replica B holds the session's stream; replica A sees the change.
#[tokio::test]
async fn a_change_on_one_replica_reaches_a_stream_on_another() {
    let bus: Arc<dyn NotificationBus> = Arc::new(LocalBus::default());
    let replica = || {
        ServerBuilder::new(Tools)
            .with_tools()
            .with_notification_bus(Arc::clone(&bus))
            .build()
    };
    let (a, b) = (replica(), replica());

    let (tx, mut stream) = mpsc::channel(8);
    let mut session = LegacySessionAdapter::new(b);
    let init = McpRequest::new(JsonRpcRequest::new(
        0,
        "initialize",
        Some(json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "c", "version": "1" },
        })),
    ))
    .with(Peer::new("client", &tx));
    session.ready().await.unwrap().call(init).await.unwrap();

    a.notifier().tools_list_changed();
    let note = tokio::time::timeout(Duration::from_secs(5), stream.recv())
        .await
        .expect("the change crosses replicas")
        .expect("stream open");
    let JsonRpcMessage::Notification(note) = note else {
        panic!("expected a notification, got {note:?}");
    };
    assert_eq!(note.method, "notifications/tools/list_changed");
}

#[test]
fn changes_serialize_for_any_bus() {
    assert_eq!(
        serde_json::to_value(Change::ResourceUpdated {
            uri: "file:///a".into()
        })
        .unwrap(),
        json!({ "change": "resource_updated", "uri": "file:///a" })
    );
    assert_eq!(
        serde_json::from_value::<Change>(json!({ "change": "tools_list_changed" })).unwrap(),
        Change::ToolsListChanged
    );
}
