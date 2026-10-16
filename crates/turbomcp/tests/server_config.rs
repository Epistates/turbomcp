//! `ServerConfig`: one configuration, made once, shared by servers; the
//! builder's settings write it; and the settings that came with it (an
//! inline-request timeout, a runtime to take a notifier from) behave.
#![cfg(feature = "client")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Map, Value, json};
use turbomcp::ServerConfig;
use turbomcp::client::{ClientBuilder, ConnectMode, ElicitationHandler, async_trait};
use turbomcp::neutral;
use turbomcp::prelude::*;

#[derive(Clone)]
struct Asker;

#[server(name = "asker", version = "1.0.0")]
impl Asker {
    /// Asks a question nobody answers in time.
    #[tool]
    async fn ask(&self, ctx: &CallToolContext) -> McpResult<String> {
        let schema = json!({ "type": "object", "properties": {} });
        ctx.client
            .elicit("q", neutral::ElicitParams::new("Anyone?", schema))
            .await?;
        Ok("answered".into())
    }
}

/// Never answers.
#[derive(Clone)]
struct Silent;

#[async_trait]
impl ElicitationHandler for Silent {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        std::future::pending().await
    }
}

/// An inline (`2025-*`) question waits as long as the config says, not the
/// fixed two minutes it used to.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_inline_request_timeout_is_configurable() {
    let config = ServerConfig::new().with_inline_request_timeout(Duration::from_millis(200));
    let client = turbomcp::testing::connect(
        Asker.into_server().with_config(config),
        ClientBuilder::new("t", "1")
            .with_connect_mode(ConnectMode::Legacy)
            .with_elicitation(Silent),
    )
    .await
    .unwrap();
    let started = std::time::Instant::now();
    let result = client.call_tool("ask", Map::new()).await;
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "{:?}",
        started.elapsed()
    );
    let text = match result {
        Ok(r) => {
            assert!(r.is_error, "{r:?}");
            r.text_content().unwrap_or_default()
        }
        Err(e) => e.to_string(),
    };
    assert!(text.contains("in time"), "{text}");
}

#[derive(Clone)]
struct Tools;

#[server(name = "tools", version = "1.0.0")]
impl Tools {
    /// A tool.
    #[tool]
    async fn t(&self) -> String {
        String::new()
    }
}

/// Heard notifications.
#[derive(Clone, Default)]
struct Heard(Arc<Mutex<Vec<String>>>);

#[async_trait]
impl turbomcp::client::NotificationHandler for Heard {
    async fn on_notification(&self, method: String, _params: Option<Value>) {
        self.0.lock().unwrap().push(method);
    }
}

/// `into_runtime` hands over the built server, whose notifier announces a
/// change from outside any request; one config serves two servers.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_runtime_announces_changes_and_a_config_is_shared() {
    let shared = ServerConfig::new().with_call_timeout(Duration::from_secs(5));
    let runtime = Tools
        .into_server()
        .with_config(shared.clone())
        .into_runtime();
    let notifier = runtime.notifier();
    let heard = Heard::default();
    let _client = turbomcp::testing::connect(
        runtime,
        ClientBuilder::new("t", "1")
            .with_connect_mode(ConnectMode::Legacy)
            .with_notifications(heard.clone()),
    )
    .await
    .unwrap();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while !heard
        .0
        .lock()
        .unwrap()
        .iter()
        .any(|m| m == "notifications/tools/list_changed")
    {
        notifier.tools_list_changed();
        assert!(
            tokio::time::Instant::now() < deadline,
            "the change was never heard"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let other = Asker.into_server().with_config(shared).build();
    drop(other);
}
