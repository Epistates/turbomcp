//! # Elicitation (v4)
//!
//! A tool that asks the user for confirmation before acting, via
//! `ctx.client.elicit_as::<T>(…)` (the form from `T`'s schema, the answer
//! parsed into `T`; `ctx.client.elicit(…)` takes a schema by hand). The same handler works on **both protocol
//! versions** without changes:
//!
//! - `2026-07-28` (MRTR, SEP-2322): the first call answers
//!   `InputRequiredResult`; the client gathers the user's answer and retries
//!   the call with `inputResponses`, re-running the handler from the top —
//!   keep elicit keys stable and pre-elicit work idempotent.
//! - `2025-11-25`: the call blocks while a real `elicitation/create` request
//!   goes to the client inline; no re-execution happens.
//!
//! Run with: `cargo run -p turbomcp --example elicitation`

use turbomcp::prelude::*;

/// The form: its schema is generated from the type, and the answer parses
/// back into it.
#[derive(serde::Deserialize, schemars::JsonSchema)]
struct Confirm {
    /// Delete it?
    ok: bool,
}

#[derive(Clone)]
struct FileManager;

#[server(name = "file-manager", version = "1.0.0")]
impl FileManager {
    /// Delete a path — but only after the user confirms.
    #[tool]
    async fn delete(&self, ctx: &CallToolContext, path: String) -> McpResult<String> {
        // Stable key: on `2026-07-28` the re-executed call finds its answer
        // under it.
        let answer = ctx
            .client
            .elicit_as::<Confirm>("confirm_delete", format!("Really delete {path}?"))
            .await?;
        match answer {
            // Side effects belong AFTER the elicit: on `2026-07-28` this line
            // runs exactly once, in the retry execution.
            Elicited::Accepted(Confirm { ok: true }) => Ok(format!("deleted {path}")),
            _ => Ok(format!("kept {path}")),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), turbomcp::ProtocolError> {
    // Logs MUST go to stderr — stdout carries the MCP protocol framing.
    FileManager.run_stdio().await
}
