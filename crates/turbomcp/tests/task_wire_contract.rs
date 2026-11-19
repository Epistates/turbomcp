//! Drift tripwire: the typed client's `call_tool` auto-drive reads task fields
//! as raw JSON (deliberately — the client crate has no dependency on
//! `turbomcp-ext-tasks`). That decoupling means a rename on the extension's
//! wire types would silently break the client with no compile error.
//!
//! This test couples the two: it serializes the **real** ext-tasks wire shapes
//! and asserts every JSON path the client's `drive_task` navigates still
//! resolves. If SEP-2663 (or our wire types) renames `taskId`, `pollIntervalMs`,
//! `inputRequests`, a status string, etc., a serialization assertion here fails
//! — a loud signal to update the client's raw reads in lock-step.
//!
//! The client-side contract (crates/turbomcp-client/src/client.rs `drive_task`)
//! reads: `taskId`, `status` ∈ {working, input_required, completed, failed,
//! cancelled}, `ttlMs`, `pollIntervalMs`, `result` (completed),
//! `error.{code,message,data}` (failed), `inputRequests.<key>.{method,params}`,
//! and `resultType == "task"` on the create result.

#![cfg(all(feature = "client", feature = "ext-tasks"))]

use serde_json::{Map, Value, json};
use turbomcp::ext_tasks::wire::{
    CreateTaskResult, DetailedTask, RESULT_TYPE_TASK, Task, TaskStatus,
};

/// A `Task` carrying the polling fields the client depends on.
fn task(status: TaskStatus) -> Task {
    let mut task = Task::working("task-123", "2026-01-01T00:00:00Z");
    task.status = status;
    task.ttl_ms = Some(60_000);
    task.poll_interval_ms = Some(250);
    task
}

#[test]
fn create_result_exposes_the_fields_the_client_polls_on() {
    // What `tools/call` returns and the client recognizes as a task handle.
    let v = serde_json::to_value(CreateTaskResult::new(task(TaskStatus::Working))).unwrap();

    // The client's task-handle discriminator (client.rs `RESULT_TYPE_TASK`).
    assert_eq!(v["resultType"], RESULT_TYPE_TASK);
    assert_eq!(v["resultType"], "task");
    // The id the client polls with, and the fields it reads each round.
    assert_eq!(v["taskId"], "task-123");
    assert_eq!(v["status"], "working");
    assert_eq!(v["ttlMs"], 60_000);
    assert_eq!(v["pollIntervalMs"], 250);
}

#[test]
fn completed_task_carries_result_where_the_client_reads_it() {
    let v = serde_json::to_value(
        DetailedTask::new(task(TaskStatus::Completed))
            .with_result(json!({ "content": [{ "type": "text", "text": "ok" }] })),
    )
    .unwrap();
    assert_eq!(v["status"], "completed");
    assert!(
        v.get("result").is_some(),
        "client returns `result` verbatim"
    );
    assert_eq!(v["result"]["content"][0]["text"], "ok");
}

#[test]
fn failed_task_carries_error_code_message_data() {
    let v = serde_json::to_value(
        DetailedTask::new(task(TaskStatus::Failed))
            .with_error(json!({ "code": -32000, "message": "boom", "data": { "x": 1 } })),
    )
    .unwrap();
    assert_eq!(v["status"], "failed");
    // The client maps these three into a ClientError::Rpc.
    assert_eq!(v["error"]["code"], -32000);
    assert_eq!(v["error"]["message"], "boom");
    assert_eq!(v["error"]["data"]["x"], 1);
}

#[test]
fn cancelled_status_string_matches() {
    let v = serde_json::to_value(DetailedTask::new(task(TaskStatus::Cancelled))).unwrap();
    assert_eq!(v["status"], "cancelled");
}

#[test]
fn input_required_task_exposes_the_request_envelope_the_client_dispatches() {
    let mut requests = Map::new();
    requests.insert(
        "confirm#1".into(),
        json!({
            "method": "elicitation/create",
            "params": { "message": "ok?" },
        }),
    );
    let v = serde_json::to_value(
        DetailedTask::new(task(TaskStatus::InputRequired)).with_input_requests(requests),
    )
    .unwrap();

    assert_eq!(v["status"], "input_required");
    // The client iterates `inputRequests`, reading `method` + `params` from
    // each and answering via tasks/update keyed by the same key.
    let requests = v["inputRequests"]
        .as_object()
        .expect("inputRequests object");
    let (key, req) = requests.iter().next().expect("one request");
    assert_eq!(key, "confirm#1");
    assert_eq!(req["method"], "elicitation/create");
    assert!(req.get("params").is_some());
}

#[test]
fn unlimited_ttl_serializes_as_null_not_absent() {
    // The client reads `ttlMs` as `Option<u64>`; a null (unlimited) must poll
    // forever, and its absence-vs-null must stay stable.
    let mut task = Task::working("t", "2026-01-01T00:00:00Z");
    task.ttl_ms = None;
    let v = serde_json::to_value(DetailedTask::new(task)).unwrap();
    assert!(v["ttlMs"].is_null());
    assert_eq!(v.get("ttlMs"), Some(&Value::Null));
}
