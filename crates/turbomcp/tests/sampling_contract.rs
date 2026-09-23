//! What a server may ask a client to sample, and what it may not.
//!
//! `sampling/createMessage` is the one request a server builds itself, so
//! nothing on the inbound path — not the capability negotiation, not the
//! version adapter — gets a look at it. Everything it has to satisfy has to be
//! checked here, at the point the request is assembled.

use std::sync::Arc;

use turbomcp_core::context::RequestContext;
use turbomcp_core::session::{McpSession, SessionFuture};
use turbomcp_types::{
    ClientCapabilities, IncludeContext, ProtocolVersion, Role, SamplingCapabilities,
    SamplingContent, SamplingMessage, ToolResultContent, ToolUseContent,
};
use turbomcp_types::{CreateMessageRequest, TaskMetadata};

/// A session that answers with whatever capabilities and wire the test needs,
/// and records the one request that reaches the transport.
#[derive(Debug)]
struct FakeClient {
    capabilities: Option<ClientCapabilities>,
    version: Option<ProtocolVersion>,
    sent: std::sync::Mutex<Vec<serde_json::Value>>,
}

impl FakeClient {
    fn new(capabilities: Option<ClientCapabilities>, version: ProtocolVersion) -> Arc<Self> {
        Arc::new(Self {
            capabilities,
            version: Some(version),
            sent: std::sync::Mutex::new(Vec::new()),
        })
    }

    fn sent_count(&self) -> usize {
        self.sent.lock().unwrap().len()
    }
}

impl McpSession for FakeClient {
    fn client_capabilities<'a>(&'a self) -> SessionFuture<'a, Option<ClientCapabilities>> {
        let capabilities = self.capabilities.clone();
        Box::pin(async move { Ok(capabilities) })
    }

    fn protocol_version<'a>(&'a self) -> SessionFuture<'a, Option<ProtocolVersion>> {
        let version = self.version.clone();
        Box::pin(async move { Ok(version) })
    }

    fn call<'a>(
        &'a self,
        _method: &'a str,
        params: serde_json::Value,
    ) -> SessionFuture<'a, serde_json::Value> {
        self.sent.lock().unwrap().push(params);
        Box::pin(async {
            Ok(serde_json::json!({
                "role": "assistant",
                "content": { "type": "text", "text": "ok" },
                "model": "fake",
            }))
        })
    }

    fn notify<'a>(&'a self, _method: &'a str, _params: serde_json::Value) -> SessionFuture<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

fn sampling_caps(sampling: SamplingCapabilities) -> ClientCapabilities {
    ClientCapabilities {
        sampling: Some(sampling),
        ..Default::default()
    }
}

fn tools_capable() -> ClientCapabilities {
    sampling_caps(SamplingCapabilities {
        tools: Some(std::collections::HashMap::new()),
        context: None,
    })
}

fn ctx_for(session: Arc<FakeClient>) -> RequestContext {
    RequestContext::new().with_session(session)
}

fn text(role: Role, body: &str) -> SamplingMessage {
    SamplingMessage {
        role,
        content: SamplingContent::text(body).into(),
        meta: None,
    }
}

fn tool_use(id: &str) -> SamplingContent {
    SamplingContent::ToolUse(ToolUseContent {
        id: id.to_string(),
        name: "get_weather".to_string(),
        input: std::collections::HashMap::new(),
        meta: None,
    })
}

fn tool_result(id: &str) -> SamplingContent {
    SamplingContent::ToolResult(ToolResultContent {
        tool_use_id: id.to_string(),
        content: Vec::new(),
        structured_content: None,
        is_error: None,
        meta: None,
    })
}

fn request(messages: Vec<SamplingMessage>) -> CreateMessageRequest {
    CreateMessageRequest {
        messages,
        max_tokens: 64,
        ..Default::default()
    }
}

// ── Message content constraints (§Message Content Constraints) ──────────

/// "When a user message contains tool results, it MUST contain ONLY tool
/// results." Provider APIs give tool results a dedicated role, so a mixed
/// message has nothing to translate into — the 400 comes back from Anthropic or
/// OpenAI three steps removed from the mistake that caused it.
#[tokio::test]
async fn a_tool_result_message_carrying_anything_else_is_refused() {
    let session = FakeClient::new(Some(tools_capable()), ProtocolVersion::V2025_11_25);
    let ctx = ctx_for(Arc::clone(&session));

    let error = ctx
        .sample(request(vec![
            text(Role::User, "weather?"),
            SamplingMessage {
                role: Role::Assistant,
                content: vec![tool_use("call_1")].into(),
                meta: None,
            },
            SamplingMessage {
                role: Role::User,
                content: vec![SamplingContent::text("here you go"), tool_result("call_1")].into(),
                meta: None,
            },
        ]))
        .await
        .expect_err("mixed tool-result content must be refused");

    assert_eq!(error.jsonrpc_error_code(), -32602, "{error}");
    assert!(error.to_string().contains("mixed"), "{error}");
    assert_eq!(session.sent_count(), 0, "nothing should reach the client");
}

/// "Every assistant message containing ToolUseContent blocks MUST be followed
/// by a user message that consists entirely of ToolResultContent blocks, with
/// each tool use matched by a corresponding tool result."
#[tokio::test]
async fn an_unanswered_tool_use_is_refused() {
    let session = FakeClient::new(Some(tools_capable()), ProtocolVersion::V2025_11_25);
    let ctx = ctx_for(Arc::clone(&session));

    let error = ctx
        .sample(request(vec![
            SamplingMessage {
                role: Role::Assistant,
                content: vec![tool_use("call_1"), tool_use("call_2")].into(),
                meta: None,
            },
            // Only one of the two answered.
            SamplingMessage {
                role: Role::User,
                content: vec![tool_result("call_1")].into(),
                meta: None,
            },
            text(Role::Assistant, "the weather is…"),
        ]))
        .await
        .expect_err("an unresolved tool use must be refused");

    assert_eq!(error.jsonrpc_error_code(), -32602, "{error}");
    assert!(
        error.to_string().contains("Tool result missing in request"),
        "the spec names this error verbatim: {error}"
    );
}

#[tokio::test]
async fn a_tool_result_naming_no_tool_use_is_refused() {
    let session = FakeClient::new(Some(tools_capable()), ProtocolVersion::V2025_11_25);
    let ctx = ctx_for(Arc::clone(&session));

    let error = ctx
        .sample(request(vec![SamplingMessage {
            role: Role::User,
            content: vec![tool_result("call_nobody_asked_for")].into(),
            meta: None,
        }]))
        .await
        .expect_err("an orphan tool result must be refused");

    assert_eq!(error.jsonrpc_error_code(), -32602, "{error}");
}

#[tokio::test]
async fn a_balanced_tool_loop_is_sent() {
    let session = FakeClient::new(Some(tools_capable()), ProtocolVersion::V2025_11_25);
    let ctx = ctx_for(Arc::clone(&session));

    ctx.sample(request(vec![
        text(Role::User, "weather in Paris and London?"),
        SamplingMessage {
            role: Role::Assistant,
            content: vec![tool_use("call_1"), tool_use("call_2")].into(),
            meta: None,
        },
        SamplingMessage {
            role: Role::User,
            content: vec![tool_result("call_2"), tool_result("call_1")].into(),
            meta: None,
        },
    ]))
    .await
    .expect("a balanced loop is valid, in any result order");

    assert_eq!(session.sent_count(), 1);
}

// ── Version representability ────────────────────────────────────────────

/// 2025-06-18's `SamplingMessage.content` is a single text/image/audio block.
/// A multi-version server running a tool loop would otherwise send
/// `content: [{type: "tool_use", …}]` to a client whose parser has no case for
/// it: strict clients reject the request, lenient ones drop the tool blocks and
/// answer the wrong question.
#[tokio::test]
async fn tool_content_is_refused_on_a_2025_06_18_session() {
    let session = FakeClient::new(Some(tools_capable()), ProtocolVersion::V2025_06_18);
    let ctx = ctx_for(Arc::clone(&session));

    // A perfectly balanced loop: nothing wrong with it on the 11-25 wire.
    let error = ctx
        .sample(request(vec![
            SamplingMessage {
                role: Role::Assistant,
                content: vec![tool_use("call_1")].into(),
                meta: None,
            },
            SamplingMessage {
                role: Role::User,
                content: vec![tool_result("call_1")].into(),
                meta: None,
            },
        ]))
        .await
        .expect_err("2025-06-18 cannot represent tool_use content");

    assert!(error.to_string().contains("2025-11-25"), "{error}");
    assert_eq!(session.sent_count(), 0);
}

/// Refusal is specific to shapes the wire cannot carry. Plain text sampling is
/// the same request on both wires and must keep working.
#[tokio::test]
async fn plain_text_sampling_still_works_on_2025_06_18() {
    let session = FakeClient::new(
        Some(sampling_caps(SamplingCapabilities::default())),
        ProtocolVersion::V2025_06_18,
    );
    let ctx = ctx_for(Arc::clone(&session));

    ctx.sample(request(vec![text(Role::User, "hello")]))
        .await
        .expect("plain text sampling is legal on both wires");

    assert_eq!(session.sent_count(), 1);
}

// ── Capability gates ────────────────────────────────────────────────────

/// `includeContext: thisServer | allServers` is soft-deprecated, and a server
/// SHOULD only use it against a client that declared `sampling.context`.
#[tokio::test]
async fn include_context_needs_the_context_capability_on_an_11_25_client() {
    let session = FakeClient::new(Some(tools_capable()), ProtocolVersion::V2025_11_25);
    let ctx = ctx_for(Arc::clone(&session));

    let error = ctx
        .sample(CreateMessageRequest {
            include_context: Some(IncludeContext::AllServers),
            ..request(vec![text(Role::User, "hi")])
        })
        .await
        .expect_err("a client that declared tools but not context opted out");

    assert!(error.to_string().contains("sampling.context"), "{error}");
    assert!(
        !ctx.client_supports_sampling_context().await.unwrap(),
        "and the handler can ask rather than discover it from the refusal"
    );
}

/// On a 2025-11-25 session a bare `sampling: {}` has not declared `context`
/// either. `sampling.tools` used to stand in for the negotiated version, so
/// this case was let through.
#[tokio::test]
async fn include_context_needs_the_context_capability_on_any_11_25_session() {
    let session = FakeClient::new(
        Some(sampling_caps(SamplingCapabilities::default())),
        ProtocolVersion::V2025_11_25,
    );
    let ctx = ctx_for(Arc::clone(&session));

    let error = ctx
        .sample(CreateMessageRequest {
            include_context: Some(IncludeContext::ThisServer),
            ..request(vec![text(Role::User, "hi")])
        })
        .await
        .expect_err("the client did not declare sampling.context");
    assert!(error.to_string().contains("sampling.context"), "{error}");
    assert_eq!(session.sent_count(), 0);
}

/// The refusal must not reach a 2025-06-18 client, where `sampling.context`
/// does not exist and both values are entirely legal.
#[tokio::test]
async fn include_context_is_untouched_for_a_client_with_bare_sampling() {
    let session = FakeClient::new(
        Some(sampling_caps(SamplingCapabilities::default())),
        ProtocolVersion::V2025_06_18,
    );
    let ctx = ctx_for(Arc::clone(&session));

    ctx.sample(CreateMessageRequest {
        include_context: Some(IncludeContext::ThisServer),
        ..request(vec![text(Role::User, "hi")])
    })
    .await
    .expect("a 06-18-shaped client still gets thisServer through");

    assert_eq!(
        session.sent.lock().unwrap()[0]["includeContext"],
        "thisServer"
    );
}

#[tokio::test]
async fn context_capable_clients_get_include_context() {
    let session = FakeClient::new(
        Some(sampling_caps(SamplingCapabilities {
            tools: Some(std::collections::HashMap::new()),
            context: Some(std::collections::HashMap::new()),
        })),
        ProtocolVersion::V2025_11_25,
    );
    let ctx = ctx_for(Arc::clone(&session));

    ctx.sample(CreateMessageRequest {
        include_context: Some(IncludeContext::AllServers),
        ..request(vec![text(Role::User, "hi")])
    })
    .await
    .expect("declaring context is exactly what permits this");

    assert!(ctx.client_supports_sampling_context().await.unwrap());
}

// ── Task augmentation ───────────────────────────────────────────────────

/// A task-augmented request returns `CreateTaskResult`, which has none of
/// `role`/`content`/`model`. Letting it through meant the client accepted and
/// started the task, `sample()` then failed on a deserialize that could never
/// succeed, and the task was orphaned until its TTL ran out.
#[tokio::test]
async fn task_augmented_sampling_is_refused_up_front() {
    let session = FakeClient::new(Some(tools_capable()), ProtocolVersion::V2025_11_25);
    let ctx = ctx_for(Arc::clone(&session));

    let error = ctx
        .sample(CreateMessageRequest {
            task: Some(TaskMetadata::default()),
            ..request(vec![text(Role::User, "hi")])
        })
        .await
        .expect_err("sample() cannot return a CreateTaskResult");

    assert!(error.to_string().contains("CreateTaskResult"), "{error}");
    assert_eq!(
        session.sent_count(),
        0,
        "the client must never be asked to start a task nobody will collect"
    );
}

/// The refusal fires even when capabilities are unknown — otherwise the failure
/// only surfaces on transports that captured them.
#[tokio::test]
async fn task_augmented_sampling_is_refused_without_known_capabilities() {
    let session = FakeClient::new(None, ProtocolVersion::V2025_11_25);
    let ctx = ctx_for(Arc::clone(&session));

    ctx.sample(CreateMessageRequest {
        task: Some(TaskMetadata::default()),
        ..request(vec![text(Role::User, "hi")])
    })
    .await
    .expect_err("unknown capabilities must not become permission");

    assert_eq!(session.sent_count(), 0);
}

/// The schema bounds every priority to `0..=1`; a client is free to reject
/// anything else, so the server refuses to send it.
#[tokio::test]
async fn out_of_range_model_priorities_are_refused_before_sending() {
    let session = FakeClient::new(
        Some(sampling_caps(SamplingCapabilities::default())),
        ProtocolVersion::V2025_11_25,
    );
    let ctx = ctx_for(Arc::clone(&session));

    let error = ctx
        .sample(CreateMessageRequest {
            model_preferences: Some(turbomcp_types::ModelPreferences {
                cost_priority: Some(1.5),
                ..Default::default()
            }),
            ..request(vec![text(Role::User, "hi")])
        })
        .await
        .expect_err("costPriority 1.5 is outside 0..=1");
    assert!(error.to_string().contains("costPriority"), "{error}");
    assert_eq!(session.sent_count(), 0);
}
