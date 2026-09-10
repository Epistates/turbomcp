#![cfg(feature = "client")]
#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };
    use tokio::sync::{Notify, mpsc};
    use turbomcp_core::{JsonRpcMessage, JsonRpcRequest};
    use turbomcp_service::{ProtocolError, ServeConfig, Transport, serve_with};

    struct InputTransport {
        rx: mpsc::Receiver<JsonRpcMessage>,
        read: Arc<AtomicUsize>,
    }
    impl Transport for InputTransport {
        type Error = std::io::Error;
        async fn send(&mut self, _: JsonRpcMessage) -> Result<(), Self::Error> {
            Ok(())
        }
        async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
            let msg = self.rx.recv().await;
            if msg.is_some() {
                self.read.fetch_add(1, Ordering::SeqCst);
            }
            Ok(msg)
        }
        async fn close(self) -> Result<(), Self::Error> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn saturated_driver_obeys_shutdown_deadline() {
        let (tx, rx) = mpsc::channel(8);
        let read = Arc::new(AtomicUsize::new(0));
        let config = ServeConfig {
            max_in_flight: 1,
            drain_timeout: Duration::from_millis(10),
            ..Default::default()
        };
        let shutdown = config.shutdown.clone();
        let svc = tower::service_fn(|_: JsonRpcMessage| async {
            std::future::pending::<Result<Option<JsonRpcMessage>, ProtocolError>>().await
        });
        let mut job = tokio::spawn(serve_with(
            InputTransport {
                rx,
                read: read.clone(),
            },
            svc,
            config,
        ));
        for i in 1..=2 {
            tx.send(JsonRpcRequest::new(i, "tools/call", None).into())
                .await
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            while read.load(Ordering::SeqCst) < 2 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        shutdown.cancel();
        let outcome = tokio::time::timeout(Duration::from_millis(150), &mut job).await;
        job.abort();
        assert!(
            outcome.is_ok(),
            "shutdown must honor the drain deadline under saturation"
        );
    }
    struct BlockedSend {
        entered: Arc<Notify>,
    }
    impl Transport for BlockedSend {
        type Error = std::io::Error;
        async fn send(&mut self, _: JsonRpcMessage) -> Result<(), Self::Error> {
            self.entered.notify_one();
            std::future::pending().await
        }
        async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, Self::Error> {
            std::future::pending().await
        }
        async fn close(self) -> Result<(), Self::Error> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn client_queue_wait_obeys_timeout_and_cleans_pending_on_drop() {
        let entered = Arc::new(Notify::new());
        let conn = turbomcp_client::Connection::with_timeout(
            BlockedSend {
                entered: entered.clone(),
            },
            Duration::from_millis(10),
        );
        conn.notify("first", None).await.unwrap();
        entered.notified().await;
        for _ in 0..1024 {
            conn.notify("fill", None).await.unwrap();
        }
        let result =
            tokio::time::timeout(Duration::from_millis(100), conn.request("tools/list", None))
                .await;
        assert!(matches!(
            result,
            Ok(Err(turbomcp_client::ClientError::Timeout))
        ));
        let debug = format!("{conn:?}");
        assert!(debug.contains("in_flight: 0"), "{debug}");
    }
}
#[cfg(test)]
mod catalog_tests {
    use serde_json::json;
    use std::sync::Arc;
    use tower::ServiceExt;
    use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpResult};
    use turbomcp_protocol::neutral;
    use turbomcp_server::{
        CallToolContext, ListToolsContext, McpServerCore, ServerBuilder, Visibility,
        VisibleComponent, WithTools,
    };
    #[derive(Clone)]
    struct Paged;
    impl McpServerCore for Paged {
        fn server_info(&self) -> Implementation {
            Implementation::new("paged", "1")
        }
    }
    impl WithTools for Paged {
        async fn list_tools(
            &self,
            _: &ListToolsContext,
            p: neutral::ListParams,
        ) -> McpResult<neutral::ListToolsResult> {
            let second = p.cursor.is_some();
            let mut t = neutral::Tool::new(
                if second { "secret" } else { "public" },
                json!({"type":"object"}),
            );
            if second {
                t.meta
                    .insert("io.turbomcp/tags".into(), json!(["internal"]));
            }
            let mut r = neutral::ListToolsResult::new(vec![t]);
            if !second {
                r.next_cursor = Some("page2".into());
            }
            Ok(r)
        }
        async fn call_tool(
            &self,
            _: &CallToolContext,
            _: neutral::CallToolParams,
        ) -> McpResult<neutral::CallToolResult> {
            Ok(neutral::CallToolResult::text("SECRET EXECUTED"))
        }
    }
    fn request(method: &str, mut params: serde_json::Value) -> JsonRpcMessage {
        params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}});
        JsonRpcRequest::new(1, method, Some(params)).into()
    }
    #[tokio::test]
    async fn hidden_page_two_tool_cannot_be_called() {
        let dispatcher = ServerBuilder::new(Paged)
            .with_tools()
            .with_visibility(Arc::new(Visibility::new().hiding_tagged(["internal"])))
            .build();
        let listed = dispatcher
            .clone()
            .oneshot(request("tools/list", json!({"cursor":"page2"})))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(listed).unwrap()["result"]["tools"],
            json!([])
        );
        let result = dispatcher
            .oneshot(request(
                "tools/call",
                json!({"name":"secret","arguments":{}}),
            ))
            .await
            .unwrap()
            .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["result"]["isError"], true);
    }
    #[tokio::test]
    async fn deny_all_visibility_rejects_later_page_tool() {
        let dispatcher = ServerBuilder::new(Paged)
            .with_tools()
            .with_visibility(Arc::new(|_: &VisibleComponent<'_>| false))
            .build();
        let result = dispatcher
            .oneshot(request(
                "tools/call",
                json!({"name":"secret","arguments":{}}),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(result).unwrap()["result"]["isError"],
            true
        );
    }
    #[tokio::test]
    async fn flat_composite_calls_later_page_tools() {
        let composite = turbomcp_server::Composite::new(Implementation::new("composed", "1"))
            .mount_flat(ServerBuilder::new(Paged).with_tools())
            .unwrap()
            .build();
        let reply = composite
            .into_server()
            .build()
            .oneshot(request(
                "tools/call",
                json!({"name":"secret","arguments":{}}),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(reply).unwrap()["result"]["content"][0]["text"],
            "SECRET EXECUTED"
        );
    }

    #[derive(Clone)]
    struct Unavailable;
    impl McpServerCore for Unavailable {
        fn server_info(&self) -> Implementation {
            Implementation::new("unavailable", "1")
        }
    }
    impl WithTools for Unavailable {
        async fn list_tools(
            &self,
            _: &ListToolsContext,
            _: neutral::ListParams,
        ) -> McpResult<neutral::ListToolsResult> {
            Err(turbomcp_core::McpError::internal("catalog unavailable"))
        }
        async fn call_tool(
            &self,
            _: &CallToolContext,
            _: neutral::CallToolParams,
        ) -> McpResult<neutral::CallToolResult> {
            panic!("catalog failure must not invoke a tool")
        }
    }
    #[tokio::test]
    async fn catalog_failure_never_bypasses_policy() {
        let reply = ServerBuilder::new(Unavailable)
            .with_tools()
            .with_visibility(Arc::new(|_: &VisibleComponent<'_>| true))
            .build()
            .oneshot(request("tools/call", json!({"name":"secret"})))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(reply).unwrap()["error"]["code"],
            -32603
        );
    }

    #[tokio::test]
    async fn dispatch_benchmark_fixture_is_rejected_before_handler() {
        let dispatcher = ServerBuilder::new(Paged).with_tools().build();
        let msg: JsonRpcMessage = JsonRpcRequest::new(
            1,
            "tools/call",
            Some(json!({
            "name":"add", "arguments":{"a":2.0,"b":40.0},
            "_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}
            })),
        )
        .into();
        let value = serde_json::to_value(dispatcher.oneshot(msg).await.unwrap().unwrap()).unwrap();
        assert!(value.get("error").is_some(), "{value}");
    }
}

#[cfg(test)]
mod schema_tests {
    use serde_json::json;
    use tower::ServiceExt;
    use turbomcp::prelude::*;
    use turbomcp::{JsonRpcMessage, JsonRpcRequest};
    #[derive(Clone)]
    struct Adults;
    #[server(name = "adults", version = "1")]
    impl Adults {
        #[tool(schema_extend = r#"{"properties":{"age":{"type":"integer","minimum":18}}}"#)]
        async fn adult(&self, age: i32) -> String {
            format!("accepted {age}")
        }
    }
    fn request(method: &str, mut params: serde_json::Value) -> JsonRpcMessage {
        params["_meta"] = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}});
        JsonRpcRequest::new(1, method, Some(params)).into()
    }
    #[tokio::test]
    async fn advertised_schema_constraint_rejects_invalid_arguments() {
        let dispatcher = Adults.into_server().build();
        let listed = dispatcher
            .clone()
            .oneshot(request("tools/list", json!({})))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            serde_json::to_value(listed).unwrap()["result"]["tools"][0]["inputSchema"]["properties"]
                ["age"]["minimum"],
            18
        );
        let result = dispatcher
            .oneshot(request(
                "tools/call",
                json!({"name":"adult","arguments":{"age":1}}),
            ))
            .await
            .unwrap()
            .unwrap();
        let value = serde_json::to_value(result).unwrap();
        assert_eq!(value["result"]["isError"], true);
    }
}
