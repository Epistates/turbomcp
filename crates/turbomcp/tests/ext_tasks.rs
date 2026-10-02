//! Phase 9d exit criterion: the draft Tasks extension end-to-end through the
//! real [`Client`] over stdio — `server/discover` reports the extension, the
//! client opts in (declares the capability), a tasked `tools/call` returns a
//! `CreateTaskResult` it polls to completion, and Tasks composes with MRTR
//! elicitation on the same server.

#![cfg(all(feature = "client", feature = "ext-tasks"))]

use std::sync::Arc;
use std::time::Duration;

use serde_json::{Map, Value, json};
use turbomcp::client::{
    CallOptions, Client, ClientBuilder, ConnectMode, Detached, ElicitationHandler, TaskStatus,
    async_trait,
};
use turbomcp::ext_tasks::{EXTENSION_ID, TasksExtension};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Workshop;

#[server(name = "workshop", version = "1.0.0")]
impl Workshop {
    /// A slow report generator — run as a task.
    #[tool(description = "Generate a report")]
    async fn generate_report(&self, topic: String) -> McpResult<String> {
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(format!("Report: {topic}"))
    }

    /// A report generator that confirms mid-task — run as a task AND elicits
    /// while executing (SEP-2663 in-execution `input_required`).
    #[tool(description = "Generate a report after confirming")]
    async fn guarded_report(&self, ctx: &CallToolContext, topic: String) -> McpResult<String> {
        let outcome = ctx
            .client
            .elicit(
                "confirm_report",
                neutral::ElicitParams::new(
                    format!("Generate a report on {topic}?"),
                    json!({
                        "type": "object",
                        "properties": { "ok": { "type": "boolean" } },
                    }),
                ),
            )
            .await?;
        Ok(if outcome.accepted() {
            format!("Confirmed report: {topic}")
        } else {
            "declined".to_owned()
        })
    }

    /// Delete a path after eliciting confirmation (MRTR) — NOT a task.
    #[tool(description = "Delete a path after confirmation")]
    async fn delete(&self, ctx: &CallToolContext, path: String) -> McpResult<String> {
        let outcome = ctx
            .client
            .elicit(
                "confirm_delete",
                neutral::ElicitParams::new(
                    format!("Really delete {path}?"),
                    json!({
                        "type": "object",
                        "properties": { "ok": { "type": "boolean" } },
                        "required": ["ok"],
                    }),
                ),
            )
            .await?;
        if outcome.accepted() && outcome.content.get("ok").and_then(Value::as_bool) == Some(true) {
            Ok(format!("deleted {path}"))
        } else {
            Ok(format!("kept {path}"))
        }
    }
}

/// Confirms every elicitation (for the MRTR composition test).
struct Confirm;

#[async_trait]
impl ElicitationHandler for Confirm {
    async fn elicit(&self, _req: neutral::ElicitParams) -> neutral::ElicitOutcome {
        let mut content = Map::new();
        content.insert("ok".into(), json!(true));
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, content)
    }
}

/// A client (declaring both the tasks extension and elicitation) connected to a
/// Workshop server that runs `generate_report` as a task.
async fn connect() -> Client {
    turbomcp::testing::connect(
        Workshop.into_server().with_extension(Arc::new(
            TasksExtension::new()
                .task_tools(["generate_report", "guarded_report"])
                .poll_interval_ms(Some(10)),
        )),
        ClientBuilder::new("workshop-client", "1.0.0")
            .with_connect_mode(ConnectMode::Modern)
            .with_extension(EXTENSION_ID, json!({}))
            .with_elicitation(Confirm),
    )
    .await
    .expect("handshake")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn discover_reports_the_extension() {
    let client = connect().await;
    // The modern handshake ran `server/discover`; the extension is advertised.
    let caps = client.server_capabilities();
    assert!(
        caps.extensions.contains_key(EXTENSION_ID),
        "discover should advertise the tasks extension, got {caps:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tasked_tool_returns_a_task_handle_polled_to_completion() {
    let client = connect().await;

    // The client opted into the extension, so the server returns a task handle.
    let mut args = Map::new();
    args.insert("topic".into(), json!("uptime"));
    let created = client
        .request("tools/call", {
            let mut p = Map::new();
            p.insert("name".into(), json!("generate_report"));
            p.insert("arguments".into(), Value::Object(args));
            p
        })
        .await
        .expect("tools/call");
    assert_eq!(created["resultType"], "task", "got {created}");
    let task_id = created["taskId"].as_str().expect("taskId").to_owned();

    // Poll tasks/get until the task completes, then read its result.
    let mut terminal = None;
    for _ in 0..200 {
        let mut p = Map::new();
        p.insert("taskId".into(), json!(task_id));
        let got = client.request("tasks/get", p).await.expect("tasks/get");
        if got["status"] == "completed" {
            terminal = Some(got);
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let terminal = terminal.expect("task should complete");
    assert_eq!(terminal["resultType"], "complete");
    assert_eq!(terminal["result"]["content"][0]["text"], "Report: uptime");
    assert_eq!(terminal["result"]["isError"], false);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn call_tool_transparently_drives_a_task_to_completion() {
    // The typed `call_tool` keeps its fixed CallToolResult contract even when
    // the server answers with a task handle: it drives the polling flow
    // internally (SEP-2663 §Polymorphic Results guidance).
    let client = connect().await;
    let mut args = Map::new();
    args.insert("topic".into(), json!("latency"));
    let result = client
        .call_tool("generate_report", args)
        .await
        .expect("call_tool auto-drives the task");
    match &result.content[0] {
        neutral::Content::Text { text, .. } => assert_eq!(text, "Report: latency"),
        other => panic!("unexpected content {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mid_task_elicitation_flows_through_tasks_update() {
    // The Phase-12 exit: a taskified tool elicits WHILE EXECUTING. The typed
    // client observes `input_required` on a poll, answers through its
    // the elicitation handler via tasks/update, and the task completes.
    let client = connect().await;
    let mut args = Map::new();
    args.insert("topic".into(), json!("throughput"));
    let result = client
        .call_tool("guarded_report", args)
        .await
        .expect("mid-task elicitation should round-trip");
    match &result.content[0] {
        neutral::Content::Text { text, .. } => assert_eq!(text, "Confirmed report: throughput"),
        other => panic!("unexpected content {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn typed_task_cancel_reaches_the_server() {
    let client = connect().await;

    // Keep the task instead of driving it.
    let mut args = Map::new();
    args.insert("topic".into(), json!("never"));
    let Detached::Task(task) = client
        .call_tool_detached("guarded_report", args, &CallOptions::new())
        .await
        .expect("tools/call")
    else {
        panic!("expected a task");
    };

    task.cancel().await.expect("tasks/cancel");
    let got = task.get().await.expect("tasks/get");
    assert_eq!(got.status, TaskStatus::Cancelled);
    assert!(got.status.is_terminal());
}

/// "Clients SHOULD persist task IDs to durable storage so that polling can
/// resume after a crash or restart": a detached task's id is all it takes to
/// pick it up again, input requests and all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_detached_task_resumes_from_its_id() {
    let client = connect().await;
    for (tool, expected) in [
        ("generate_report", "Report: latency"),
        // Elicits mid-task: `wait` answers it through the client's handler.
        ("guarded_report", "Confirmed report: latency"),
    ] {
        let mut args = Map::new();
        args.insert("topic".into(), json!("latency"));
        let task_id = match client
            .call_tool_detached(tool, args, &CallOptions::new())
            .await
            .expect("tools/call")
        {
            Detached::Task(task) => task.id().to_owned(),
            Detached::Done(result) => panic!("expected a task, got {result:?}"),
        };

        let result = client.resume_task(&task_id).wait().await.expect("settles");
        match &result.content[0] {
            neutral::Content::Text { text, .. } => assert_eq!(text, expected),
            other => panic!("unexpected content {other:?}"),
        }
        let info = client.task_get(&task_id).await.expect("tasks/get");
        assert_eq!(info.status, TaskStatus::Completed);
        assert!(info.result.is_some());
    }
}

/// A tool the server runs inline comes back as a plain result.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_detached_call_that_runs_inline_is_done() {
    let client = connect().await;
    let mut args = Map::new();
    args.insert("path".into(), json!("/tmp/x"));
    let Detached::Done(result) = client
        .call_tool_detached("delete", args, &CallOptions::new())
        .await
        .expect("tools/call")
    else {
        panic!("`delete` is not a task tool");
    };
    match &result.content[0] {
        neutral::Content::Text { text, .. } => assert_eq!(text, "deleted /tmp/x"),
        other => panic!("unexpected content {other:?}"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mrtr_elicitation_composes_with_the_tasks_extension() {
    // On the SAME extension-enabled server, a non-tasked tool still drives MRTR
    // elicitation through the typed client + handler — Tasks and MRTR coexist.
    let client = connect().await;
    let mut args = Map::new();
    args.insert("path".into(), json!("/tmp/x"));
    let result = client.call_tool("delete", args).await.expect("call_tool");
    match &result.content[0] {
        neutral::Content::Text { text, .. } => assert_eq!(text, "deleted /tmp/x"),
        other => panic!("unexpected content {other:?}"),
    }
}
