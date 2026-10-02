//! Typed interceptors: one rule over the neutral request and result, on every
//! revision and every path a call takes.
#![cfg(feature = "client")]

use std::sync::{Arc, Mutex};

use serde_json::{Map, Value, json};
use turbomcp::client::{ClientBuilder, ConnectMode, ElicitationHandler, async_trait};
use turbomcp::intercept::{Interceptor, NextCallTool, on_call_tool, on_list_tools};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Vault;

#[server(name = "vault", version = "1.0.0")]
impl Vault {
    /// Reveal a secret.
    #[tool]
    async fn reveal(&self) -> String {
        "the code is 1234".into()
    }

    /// Reveal a secret once the user agrees (MRTR on 2026-07-28).
    #[tool]
    async fn reveal_after_asking(&self, ctx: &CallToolContext) -> McpResult<String> {
        let outcome = ctx
            .client
            .elicit(
                "ok",
                neutral::ElicitParams::new(
                    "Reveal?",
                    json!({ "type": "object", "properties": {} }),
                ),
            )
            .await?;
        Ok(if outcome.accepted() {
            "the code is 1234".into()
        } else {
            "kept".into()
        })
    }
}

/// Redacts digits in every text block of a tool result.
fn redactor() -> Arc<dyn Interceptor> {
    on_call_tool(|ctx, params, next| async move {
        let mut result = next.run(ctx, params).await?;
        for block in &mut result.content {
            if let neutral::Content::Text { text, .. } = block {
                *text = text
                    .chars()
                    .map(|c| if c.is_ascii_digit() { '#' } else { c })
                    .collect();
            }
        }
        Ok(result)
    })
}

struct Agree;

#[async_trait]
impl ElicitationHandler for Agree {
    async fn elicit(&self, _req: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, Map::new())
    }
}

async fn text_of(client: &turbomcp::client::Client, tool: &str) -> String {
    client
        .call_tool(tool, Map::new())
        .await
        .expect("call")
        .text_content()
        .expect("text")
}

#[tokio::test]
async fn a_result_rule_holds_on_every_revision_and_through_mrtr() {
    for mode in [ConnectMode::Modern, ConnectMode::Legacy] {
        let client = turbomcp::testing::connect(
            Vault.into_server().intercept(redactor()),
            ClientBuilder::new("t", "1.0.0")
                .with_connect_mode(mode)
                .with_elicitation(Agree),
        )
        .await
        .expect("handshake");
        assert_eq!(
            text_of(&client, "reveal").await,
            "the code is ####",
            "{mode:?}"
        );
        // On 2026-07-28 the handler aborts for input and runs again: the
        // interceptor passes the abort through and redacts the final round.
        assert_eq!(
            text_of(&client, "reveal_after_asking").await,
            "the code is ####",
            "{mode:?}"
        );
    }
}

#[tokio::test]
async fn interceptors_nest_in_the_order_added() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let tracer = |name: &'static str| {
        let order = Arc::clone(&order);
        on_call_tool(move |ctx, params, next| {
            let order = Arc::clone(&order);
            async move {
                order.lock().unwrap().push(format!("{name} in"));
                let result = next.run(ctx, params).await;
                order.lock().unwrap().push(format!("{name} out"));
                result
            }
        })
    };
    let client = turbomcp::testing::connect(
        Vault
            .into_server()
            .intercept(tracer("outer"))
            .intercept(tracer("inner")),
        ClientBuilder::new("t", "1.0.0"),
    )
    .await
    .expect("handshake");
    text_of(&client, "reveal").await;
    assert_eq!(
        *order.lock().unwrap(),
        ["outer in", "inner in", "inner out", "outer out"]
    );
}

/// Refuses every call without running the tool, as a tool error the model
/// can read.
struct Deny;

#[async_trait]
impl Interceptor for Deny {
    async fn call_tool(
        &self,
        _ctx: CallToolContext,
        params: neutral::CallToolParams,
        _next: NextCallTool,
    ) -> McpResult<neutral::CallToolResult> {
        Err(McpError::tool_execution_failed(
            &params.name,
            "denied by policy",
        ))
    }
}

#[tokio::test]
async fn an_interceptor_can_answer_without_the_handler() {
    let client = turbomcp::testing::connect(
        Vault.into_server().intercept(Arc::new(Deny)),
        ClientBuilder::new("t", "1.0.0"),
    )
    .await
    .expect("handshake");
    let result = client
        .call_tool("reveal", Map::new())
        .await
        .expect("a result");
    assert!(result.is_error);
    assert!(result.text_content().unwrap().contains("denied by policy"));
}

#[tokio::test]
async fn a_list_rule_shapes_every_page() {
    let client = turbomcp::testing::connect(
        Vault
            .into_server()
            .intercept(on_list_tools(|ctx, params, next| async move {
                let mut page = next.run(ctx, params).await?;
                page.tools.retain(|t| t.name != "reveal_after_asking");
                for tool in &mut page.tools {
                    tool.meta
                        .insert("com.example/reviewed".into(), Value::Bool(true));
                }
                Ok(page)
            })),
        ClientBuilder::new("t", "1.0.0"),
    )
    .await
    .expect("handshake");
    let tools = client.list_tools(None).await.expect("list").tools;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].name, "reveal");
    assert_eq!(tools[0].meta["com.example/reviewed"], true);
}

/// A call that becomes a task runs through the interceptors too: the task's
/// result is what the rule made of it.
#[cfg(feature = "ext-tasks")]
#[tokio::test]
async fn a_task_result_is_intercepted_too() {
    use turbomcp::ext_tasks::{EXTENSION_ID, TasksExtension};
    let client = turbomcp::testing::connect(
        Vault
            .into_server()
            .with_extension(Arc::new(
                TasksExtension::new()
                    .task_tools(["reveal"])
                    .poll_interval_ms(Some(5)),
            ))
            .intercept(redactor()),
        ClientBuilder::new("t", "1.0.0")
            .with_connect_mode(ConnectMode::Modern)
            .with_extension(EXTENSION_ID, json!({})),
    )
    .await
    .expect("handshake");
    match client
        .call_tool_detached("reveal", Map::new(), &Default::default())
        .await
        .expect("call")
    {
        turbomcp::client::Detached::Task(task) => {
            let result = task.wait().await.expect("the task completes");
            assert_eq!(result.text_content().as_deref(), Some("the code is ####"));
        }
        turbomcp::client::Detached::Done(_) => panic!("expected a task"),
    }
}
