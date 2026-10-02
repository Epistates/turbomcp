//! A [`TaskSnapshot`] from the task backend, in the extension's wire shapes.
//! Kept out of [`wire`](crate::wire) so the wire types stay free of server
//! types for a client to use.

use turbomcp_server::{TaskSnapshot, TaskStatus};

use crate::wire::{self, DetailedTask, Task};

pub(crate) fn status(status: TaskStatus) -> wire::TaskStatus {
    match status {
        TaskStatus::Working => wire::TaskStatus::Working,
        TaskStatus::InputRequired => wire::TaskStatus::InputRequired,
        TaskStatus::Completed => wire::TaskStatus::Completed,
        TaskStatus::Failed => wire::TaskStatus::Failed,
        TaskStatus::Cancelled => wire::TaskStatus::Cancelled,
    }
}

pub(crate) fn task(snapshot: &TaskSnapshot) -> Task {
    Task {
        task_id: snapshot.task_id.clone(),
        status: status(snapshot.status),
        status_message: snapshot.status_message.clone(),
        created_at: snapshot.created_at.clone(),
        last_updated_at: snapshot.last_updated_at.clone(),
        ttl_ms: snapshot.ttl_ms,
        poll_interval_ms: snapshot.poll_interval_ms,
    }
}

/// The `tasks/get` result: the task, with what its status calls for inlined
/// (every outstanding input request while `input_required`, the result once
/// `completed`, the error once `failed`).
pub(crate) fn detailed(snapshot: TaskSnapshot) -> DetailedTask {
    let mut detailed = DetailedTask::new(task(&snapshot));
    match (snapshot.status, snapshot.outcome) {
        (TaskStatus::InputRequired, _) => {
            detailed.input_requests = Some(snapshot.input_requests);
        }
        (TaskStatus::Completed | TaskStatus::Failed, Some(Ok(result))) => {
            detailed.result = Some(result);
        }
        (TaskStatus::Failed, Some(Err(error))) => {
            detailed.error = serde_json::to_value(error).ok();
        }
        _ => {}
    }
    detailed
}
