//! A server sees a stateful client arrive and may turn it away
//! (`#[on_initialize]`), hears its requests cancelled (`#[on_cancelled]`),
//! and is told when each session starts as well as when it ends.
#![cfg(feature = "client")]

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Map;
use turbomcp::client::{ClientBuilder, ConnectMode};
use turbomcp::prelude::*;
use turbomcp::{EndedSession, SessionObserver, StartedSession};

#[derive(Clone, Default)]
struct Gatekeeper {
    cancelled: Arc<Mutex<Vec<String>>>,
}

#[server(name = "gatekeeper", version = "1.0.0")]
impl Gatekeeper {
    #[on_initialize]
    async fn admit(&self, ctx: &InitializeContext) -> McpResult<()> {
        match ctx.base.client_info.as_ref().map(|c| c.name.as_str()) {
            Some("blocked") => Err(McpError::permission_denied("this client is not welcome")),
            _ => Ok(()),
        }
    }

    #[on_cancelled]
    fn note(&self, id: &RequestId, _reason: Option<&str>) {
        self.cancelled.lock().unwrap().push(format!("{id:?}"));
    }

    /// Waits for a minute.
    #[tool]
    async fn wait(&self, ctx: &CallToolContext) -> String {
        tokio::select! {
            () = ctx.base.cancellation.cancelled() => "stopped".into(),
            () = tokio::time::sleep(Duration::from_secs(60)) => "done".into(),
        }
    }
}

#[derive(Default)]
struct Sessions {
    started: Mutex<Vec<String>>,
}

impl SessionObserver for Sessions {
    fn session_started(&self, session: &StartedSession<'_>) {
        self.started
            .lock()
            .unwrap()
            .push(session.client_info.name.clone());
    }

    fn session_ended(&self, _session: &EndedSession<'_>) {}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_admits_watches_and_hears_cancellations() {
    let sessions = Arc::new(Sessions::default());
    let server = Gatekeeper::default();
    let cancelled = Arc::clone(&server.cancelled);
    let build = || {
        server
            .clone()
            .into_server()
            .observe_sessions(Arc::clone(&sessions) as Arc<dyn SessionObserver>)
    };

    let refused = turbomcp::testing::connect(
        build(),
        ClientBuilder::new("blocked", "1").with_connect_mode(ConnectMode::Legacy),
    )
    .await
    .unwrap_err();
    assert!(refused.to_string().contains("not welcome"), "{refused}");

    let client = turbomcp::testing::connect(
        build(),
        ClientBuilder::new("welcome", "1")
            .with_connect_mode(ConnectMode::Legacy)
            .with_timeout(Duration::from_millis(300)),
    )
    .await
    .unwrap();
    assert_eq!(
        *sessions.started.lock().unwrap(),
        ["welcome"],
        "only the admitted client started a session"
    );

    assert!(client.call_tool("wait", Map::new()).await.is_err());
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while cancelled.lock().unwrap().is_empty() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "on_cancelled never ran"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}
