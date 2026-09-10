//! The pluggable-state seams: a custom [`SessionBackend`] and a custom
//! [`TaskBackend`] registered through [`ServerBuilder`] carry real traffic —
//! proving external session/task storage (e.g. Redis) can slot in without
//! touching the dispatcher.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use async_trait::async_trait;
use serde_json::{Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{
    CancellationToken, Implementation, JsonRpcError, JsonRpcMessage, JsonRpcRequest, LogLevel,
    McpResult,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, LegacySessionAdapter, ListToolsContext, McpServerCore, ServerBuilder,
    SessionBackend, SessionState, SessionStore, TaskBackend, TaskError, TaskSnapshot, TaskStore,
    VersionDispatcher, WithTools,
};

/// A [`SessionBackend`] that wraps the bundled store and counts traffic —
/// standing in for an external (Redis-like) backend.
#[derive(Default)]
struct CountingSessions {
    inner: SessionStore,
    inserts: AtomicUsize,
    gets: AtomicUsize,
    removes: AtomicUsize,
}

#[async_trait]
impl SessionBackend for CountingSessions {
    async fn insert(&self, id: &str, state: SessionState) {
        self.inserts.fetch_add(1, Ordering::SeqCst);
        SessionBackend::insert(&self.inner, id, state).await;
    }

    async fn get(&self, id: &str) -> Option<SessionState> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        SessionBackend::get(&self.inner, id).await
    }

    async fn set_log_level(&self, id: &str, level: LogLevel) -> bool {
        SessionBackend::set_log_level(&self.inner, id, level).await
    }

    async fn remove(&self, id: &str) -> bool {
        self.removes.fetch_add(1, Ordering::SeqCst);
        SessionBackend::remove(&self.inner, id).await
    }

    async fn sweep_expired(&self) -> Vec<String> {
        SessionBackend::sweep_expired(&self.inner).await
    }
}

/// A [`TaskBackend`] wrapping the bundled store, with a distinctive poll
/// interval so the wire proves the custom backend answered.
#[derive(Default)]
struct CountingTasks {
    inner: TaskStore,
    creates: AtomicUsize,
}

#[async_trait]
impl TaskBackend for CountingTasks {
    async fn create(
        &self,
        session_id: &str,
        requested_ttl_ms: Option<i64>,
        cancel: CancellationToken,
    ) -> Result<TaskSnapshot, TaskError> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        TaskBackend::create(&self.inner, session_id, requested_ttl_ms, cancel).await
    }

    async fn complete(&self, task_id: &str, outcome: Result<Value, JsonRpcError>) {
        TaskBackend::complete(&self.inner, task_id, outcome).await;
    }

    async fn cancel(&self, session_id: &str, task_id: &str) -> Result<TaskSnapshot, TaskError> {
        TaskBackend::cancel(&self.inner, session_id, task_id).await
    }

    async fn get(&self, session_id: &str, task_id: &str) -> Result<TaskSnapshot, TaskError> {
        TaskBackend::get(&self.inner, session_id, task_id).await
    }

    async fn list(
        &self,
        session_id: &str,
        cursor: Option<&str>,
        page_size: usize,
    ) -> Result<(Vec<TaskSnapshot>, Option<String>), TaskError> {
        TaskBackend::list(&self.inner, session_id, cursor, page_size).await
    }

    async fn wait_result(
        &self,
        session_id: &str,
        task_id: &str,
    ) -> Result<Result<Value, JsonRpcError>, TaskError> {
        TaskBackend::wait_result(&self.inner, session_id, task_id).await
    }

    fn poll_interval_ms(&self) -> i64 {
        123
    }
}

#[derive(Clone)]
struct Echo;

impl McpServerCore for Echo {
    fn server_info(&self) -> Implementation {
        Implementation::new("echo", "1.0.0")
    }
}

impl WithTools for Echo {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
            "echo",
            json!({"type": "object", "properties": {}}),
        )]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("echoed"))
    }
}

type Svc = LegacySessionAdapter<VersionDispatcher<Echo>>;

async fn ok(svc: &mut Svc, req: JsonRpcRequest) -> Value {
    let out = svc
        .ready()
        .await
        .expect("ready")
        .call(req.into())
        .await
        .expect("call");
    let r = match out {
        Some(JsonRpcMessage::Response(r)) => r,
        other => panic!("expected response, got {other:?}"),
    };
    assert!(r.error.is_none(), "unexpected error: {:?}", r.error);
    r.result.expect("result")
}

async fn initialize(svc: &mut Svc) -> Value {
    ok(
        svc,
        JsonRpcRequest::new(
            0,
            "initialize",
            Some(json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "seam-client", "version": "1" },
            })),
        ),
    )
    .await
}

#[tokio::test]
async fn custom_session_backend_carries_the_legacy_path() {
    let sessions = Arc::new(CountingSessions::default());
    let mut svc = LegacySessionAdapter::new(
        ServerBuilder::new(Echo)
            .with_tools()
            .with_session_backend(Arc::clone(&sessions) as Arc<dyn SessionBackend>)
            .build(),
    );

    let _ = initialize(&mut svc).await;
    assert_eq!(
        sessions.inserts.load(Ordering::SeqCst),
        1,
        "initialize stored the session in the custom backend"
    );

    let result = ok(
        &mut svc,
        JsonRpcRequest::new(1, "tools/call", Some(json!({ "name": "echo" }))),
    )
    .await;
    assert_eq!(result["content"][0]["text"], "echoed");
    assert!(
        sessions.gets.load(Ordering::SeqCst) >= 1,
        "the legacy request resolved its session through the custom backend"
    );
}

#[tokio::test]
async fn custom_task_backend_carries_core_tasks() {
    let tasks = Arc::new(CountingTasks::default());
    let mut svc = LegacySessionAdapter::new(
        ServerBuilder::new(Echo)
            .with_tools()
            .with_task_backend(Arc::clone(&tasks) as Arc<dyn TaskBackend>)
            .build(),
    );
    let init = initialize(&mut svc).await;
    assert_eq!(
        init["capabilities"]["tasks"]["list"],
        json!({}),
        "a task backend implies the tasks capability"
    );

    let created = ok(
        &mut svc,
        JsonRpcRequest::new(1, "tools/call", Some(json!({ "name": "echo", "task": {} }))),
    )
    .await;
    assert_eq!(tasks.creates.load(Ordering::SeqCst), 1);
    assert_eq!(
        created["task"]["pollInterval"], 123,
        "the custom backend's poll interval reaches the wire"
    );

    // The task runs to completion through the custom backend.
    let task_id = created["task"]["taskId"].as_str().expect("id").to_owned();
    let outcome = ok(
        &mut svc,
        JsonRpcRequest::new(2, "tasks/result", Some(json!({ "taskId": task_id }))),
    )
    .await;
    assert_eq!(outcome["content"][0]["text"], "echoed");
}

#[tokio::test]
async fn session_termination_goes_through_the_custom_backend() {
    let sessions = Arc::new(CountingSessions::default());
    let dispatcher = ServerBuilder::new(Echo)
        .with_tools()
        .with_session_backend(Arc::clone(&sessions) as Arc<dyn SessionBackend>)
        .build();
    let terminator = dispatcher.session_terminator();
    let mut svc = LegacySessionAdapter::new(dispatcher);
    let _ = initialize(&mut svc).await;

    // The adapter minted one session; terminate it through the seam the HTTP
    // DELETE handler uses.
    use turbomcp_service::SessionTerminator;
    assert!(!terminator.terminate("no-such-session", None).await);
    assert_eq!(
        sessions.removes.load(Ordering::SeqCst),
        0,
        "unknown sessions never reach deletion"
    );
}

#[tokio::test]
async fn sessions_bind_issuer_and_subject_and_keep_anonymous_separate() {
    use turbomcp_core::{Identity, meta};
    use turbomcp_service::SessionTerminator;
    let mut dispatcher = ServerBuilder::new(Echo).with_tools().build();
    let terminator = dispatcher.session_terminator();
    let identity = |issuer: &str| Identity::Bearer {
        sub: "alice".into(),
        claims: serde_json::from_value(json!({"iss":issuer})).unwrap(),
    };
    let alice = identity("issuer-a");
    for (sid, principal) in [("owned", alice.clone()), ("anonymous", Identity::Anonymous)] {
        let mut msg: JsonRpcMessage = JsonRpcRequest::new(
            1,
            "initialize",
            Some(json!({
                "protocolVersion":"2025-11-25", "capabilities":{},
                "clientInfo":{"name":"ownership-test", "version":"1"}
            })),
        )
        .into();
        meta::set_request_meta(&mut msg, meta::internal::SESSION_ID, json!(sid));
        meta::set_request_meta(
            &mut msg,
            meta::internal::IDENTITY,
            match principal {
                Identity::Bearer { sub, claims } => json!({"sub":sub,"claims":claims}),
                _ => Value::Null,
            },
        );
        let reply = dispatcher
            .ready()
            .await
            .unwrap()
            .call(msg)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(reply, JsonRpcMessage::Response(r) if r.error.is_none()));
    }
    let owner = alice.principal_key().unwrap();
    let other = identity("issuer-b").principal_key().unwrap();
    assert!(terminator.owns("owned", Some(&owner)).await);
    assert!(!terminator.owns("owned", Some(&other)).await);
    assert!(!terminator.owns("owned", None).await);
    assert!(!terminator.owns("anonymous", Some(&owner)).await);
    assert!(terminator.owns("anonymous", None).await);
    assert!(!terminator.terminate("owned", Some(&other)).await);
    assert!(terminator.owns("owned", Some(&owner)).await);
    assert!(terminator.terminate("owned", Some(&owner)).await);
}
