//! The pluggable-state seams: a custom [`SessionBackend`] and a custom
//! [`TaskBackend`] registered through [`ServerBuilder`] carry real traffic —
//! proving external session/task storage (e.g. Redis) can slot in without
//! touching the dispatcher.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{Map, Value, json};
use tower::{Service, ServiceExt};
use turbomcp_core::{
    CancellationToken, Implementation, JsonRpcError, JsonRpcMessage, JsonRpcRequest, LogLevel,
    McpRequest, McpResult, SessionId,
};
use turbomcp_protocol::neutral;
use turbomcp_server::{
    CallToolContext, InputWaiter, LegacySessionAdapter, ListToolsContext, McpServerCore, NewTask,
    ServerBuilder, SessionBackend, SessionError, SessionState, TaskBackend, TaskError, TaskOutcome,
    TaskOwner, TaskSnapshot, TaskStore, TaskUpdate, VersionDispatcher, WithTools,
};

/// A [`SessionBackend`] that keeps sessions as bytes, the way a Redis or SQL
/// backend does, so it has to rebuild [`SessionState`] on every read. It
/// counts traffic, and can be switched off to stand in for an outage.
#[derive(Default)]
struct ByteSessions {
    rows: Mutex<HashMap<String, Vec<u8>>>,
    down: AtomicBool,
    inserts: AtomicUsize,
    gets: AtomicUsize,
    removes: AtomicUsize,
}

impl ByteSessions {
    fn up(&self) -> Result<(), SessionError> {
        if self.down.load(Ordering::SeqCst) {
            Err(SessionError::Unavailable("connection refused".into()))
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl SessionBackend for ByteSessions {
    async fn insert(&self, id: &str, state: SessionState) -> Result<(), SessionError> {
        self.up()?;
        self.inserts.fetch_add(1, Ordering::SeqCst);
        let bytes = serde_json::to_vec(&state).expect("session state serializes");
        self.rows.lock().unwrap().insert(id.to_owned(), bytes);
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Option<Arc<SessionState>>, SessionError> {
        self.up()?;
        self.gets.fetch_add(1, Ordering::SeqCst);
        Ok(self.rows.lock().unwrap().get(id).map(|bytes| {
            Arc::new(serde_json::from_slice(bytes).expect("session state deserializes"))
        }))
    }

    async fn set_log_level(&self, id: &str, level: LogLevel) -> Result<bool, SessionError> {
        self.up()?;
        let mut rows = self.rows.lock().unwrap();
        let Some(bytes) = rows.get_mut(id) else {
            return Ok(false);
        };
        let state: SessionState = serde_json::from_slice(bytes).unwrap();
        *bytes = serde_json::to_vec(&state.with_log_level(Some(level))).unwrap();
        Ok(true)
    }

    async fn remove(&self, id: &str) -> Result<bool, SessionError> {
        self.up()?;
        self.removes.fetch_add(1, Ordering::SeqCst);
        Ok(self.rows.lock().unwrap().remove(id).is_some())
    }

    async fn sweep_expired(&self) -> Result<Vec<String>, SessionError> {
        Ok(Vec::new())
    }
}

/// A [`TaskBackend`] wrapping the bundled store, with a distinctive poll
/// interval so the wire proves the custom backend answered.
struct CountingTasks {
    inner: TaskStore,
    creates: AtomicUsize,
}

impl Default for CountingTasks {
    fn default() -> Self {
        Self {
            inner: TaskStore::default().with_poll_interval_ms(Some(123)),
            creates: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl TaskBackend for CountingTasks {
    async fn create(
        &self,
        owner: &TaskOwner,
        task: NewTask,
        cancel: CancellationToken,
    ) -> Result<TaskSnapshot, TaskError> {
        self.creates.fetch_add(1, Ordering::SeqCst);
        self.inner.create(owner, task, cancel).await
    }

    async fn complete(&self, task_id: &str, outcome: TaskOutcome) {
        self.inner.complete(task_id, outcome).await;
    }

    async fn update(&self, task_id: &str, update: TaskUpdate) -> Result<bool, TaskError> {
        self.inner.update(task_id, update).await
    }

    async fn get(&self, owner: &TaskOwner, task_id: &str) -> Result<TaskSnapshot, TaskError> {
        self.inner.get(owner, task_id).await
    }

    async fn list(
        &self,
        owner: &TaskOwner,
        cursor: Option<&str>,
        page_size: usize,
    ) -> Result<(Vec<TaskSnapshot>, Option<String>), TaskError> {
        self.inner.list(owner, cursor, page_size).await
    }

    async fn cancel(&self, owner: &TaskOwner, task_id: &str) -> Result<TaskSnapshot, TaskError> {
        self.inner.cancel(owner, task_id).await
    }

    async fn wait_result(
        &self,
        owner: &TaskOwner,
        task_id: &str,
    ) -> Result<Result<Value, JsonRpcError>, TaskError> {
        self.inner.wait_result(owner, task_id).await
    }

    async fn request_input(
        &self,
        task_id: &str,
        key: &str,
        request: Value,
    ) -> Result<InputWaiter, TaskError> {
        self.inner.request_input(task_id, key, request).await
    }

    async fn provide_input(
        &self,
        owner: &TaskOwner,
        task_id: &str,
        responses: &Map<String, Value>,
    ) -> Result<bool, TaskError> {
        self.inner.provide_input(owner, task_id, responses).await
    }

    async fn end_session(&self, session_id: &str) {
        self.inner.end_session(session_id).await;
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
    let sessions = Arc::new(ByteSessions::default());
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
    let sessions = Arc::new(ByteSessions::default());
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
    assert!(!terminator.terminate("no-such-session", None).await.unwrap());
    assert_eq!(
        sessions.removes.load(Ordering::SeqCst),
        0,
        "unknown sessions never reach deletion"
    );
}

#[tokio::test]
async fn sessions_bind_issuer_and_subject_and_keep_anonymous_separate() {
    use turbomcp_core::Identity;
    use turbomcp_service::SessionTerminator;
    let mut dispatcher = ServerBuilder::new(Echo).with_tools().build();
    let terminator = dispatcher.session_terminator();
    let identity = |issuer: &str| Identity::Bearer {
        sub: "alice".into(),
        claims: serde_json::from_value(json!({"iss":issuer})).unwrap(),
    };
    let alice = identity("issuer-a");
    for (sid, principal) in [("owned", alice.clone()), ("anonymous", Identity::Anonymous)] {
        let msg: JsonRpcMessage = JsonRpcRequest::new(
            1,
            "initialize",
            Some(json!({
                "protocolVersion":"2025-11-25", "capabilities":{},
                "clientInfo":{"name":"ownership-test", "version":"1"}
            })),
        )
        .into();
        let request = McpRequest::new(msg)
            .with(SessionId::new(sid))
            .with(principal);
        let reply = dispatcher
            .ready()
            .await
            .unwrap()
            .call(request)
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(reply, JsonRpcMessage::Response(r) if r.error.is_none()));
    }
    let owner = alice.principal_key().unwrap();
    let other = identity("issuer-b").principal_key().unwrap();
    assert!(terminator.owns("owned", Some(&owner)).await.unwrap());
    assert!(!terminator.owns("owned", Some(&other)).await.unwrap());
    assert!(!terminator.owns("owned", None).await.unwrap());
    assert!(!terminator.owns("anonymous", Some(&owner)).await.unwrap());
    assert!(terminator.owns("anonymous", None).await.unwrap());
    assert!(!terminator.terminate("owned", Some(&other)).await.unwrap());
    assert!(terminator.owns("owned", Some(&owner)).await.unwrap());
    assert!(terminator.terminate("owned", Some(&owner)).await.unwrap());
}

/// A store outage is the store failing, not the session being gone: the
/// request fails as `Unavailable` (`503` over HTTP), not `UnknownSession`
/// (`404`), which would send every client to re-`initialize` at once. And a
/// handshake whose session can't be stored fails rather than handing out an
/// id every later request would find unknown.
#[tokio::test]
async fn a_session_store_outage_is_unavailable_not_unknown() {
    let sessions = Arc::new(ByteSessions::default());
    let mut svc = LegacySessionAdapter::new(
        ServerBuilder::new(Echo)
            .with_tools()
            .with_session_backend(Arc::clone(&sessions) as Arc<dyn SessionBackend>)
            .build(),
    );
    let _ = initialize(&mut svc).await;
    sessions.down.store(true, Ordering::SeqCst);

    let call: JsonRpcMessage =
        JsonRpcRequest::new(1, "tools/call", Some(json!({ "name": "echo" }))).into();
    let err = svc
        .ready()
        .await
        .unwrap()
        .call(McpRequest::new(call))
        .await
        .expect_err("the store is down");
    assert!(
        matches!(err, turbomcp_service::ProtocolError::Unavailable(_)),
        "{err:?}"
    );

    let mut fresh = LegacySessionAdapter::new(
        ServerBuilder::new(Echo)
            .with_tools()
            .with_session_backend(Arc::clone(&sessions) as Arc<dyn SessionBackend>)
            .build(),
    );
    let init: JsonRpcMessage = JsonRpcRequest::new(
        0,
        "initialize",
        Some(json!({
            "protocolVersion": "2025-11-25",
            "capabilities": {},
            "clientInfo": { "name": "t", "version": "1" },
        })),
    )
    .into();
    let err = fresh
        .ready()
        .await
        .unwrap()
        .call(McpRequest::new(init))
        .await
        .expect_err("the session couldn't be stored");
    assert!(
        matches!(err, turbomcp_service::ProtocolError::Unavailable(_)),
        "{err:?}"
    );
}
