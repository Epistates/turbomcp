//! The `#[server]` dispatch futures must not carry handler bodies inline.
//!
//! `call_tool`, `read_resource`, and `get_prompt` are each generated as a
//! single `async` block with one `match` arm per handler. Every arm is part of
//! one state machine, so an un-boxed arm makes that machine at least as large
//! as the fattest handler body — and the whole thing is returned by value,
//! moved into tasks, and held for the life of the call. A 30-tool server whose
//! largest tool held a 10 KiB local measured a 10776-byte `call_tool` future;
//! boxing each arm brought it to 192 bytes.
//!
//! These tests pin the decoupling: the servers below differ only in how much
//! state their handlers hold, and their dispatch futures must stay the same
//! small size.

use turbomcp::prelude::*;

/// Big enough that an inline body is unmistakable in the measurement, and far
/// beyond any plausible fixed dispatch overhead.
const FAT_LOCAL: usize = 64 * 1024;

/// Ceiling for every dispatch future. Boxed arms measure a few hundred bytes;
/// a single inlined `FAT_LOCAL` body would blow past this by ~30x.
const MAX_DISPATCH_FUTURE: usize = 2048;

async fn tick() {
    tokio::task::yield_now().await
}

#[derive(Clone)]
struct Thin;

#[server(name = "thin", version = "1.0.0")]
impl Thin {
    #[tool]
    async fn only(&self, a: i64) -> McpResult<String> {
        Ok(a.to_string())
    }

    #[prompt]
    async fn p(&self, topic: String, _ctx: &RequestContext) -> McpResult<PromptResult> {
        Ok(PromptResult::user(topic))
    }

    #[resource("mem://{name}")]
    async fn r(&self, uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok(uri)
    }
}

#[derive(Clone)]
struct Fat;

#[server(name = "fat", version = "1.0.0")]
impl Fat {
    #[tool]
    async fn only(&self, a: i64) -> McpResult<String> {
        // Held across an await, so it must live in whichever frame runs it.
        let buf = [7u8; FAT_LOCAL];
        tick().await;
        Ok(format!("{a}{}", buf[FAT_LOCAL - 1]))
    }

    #[prompt]
    async fn p(&self, topic: String, _ctx: &RequestContext) -> McpResult<PromptResult> {
        let buf = [7u8; FAT_LOCAL];
        tick().await;
        Ok(PromptResult::user(format!("{topic}{}", buf[FAT_LOCAL - 1])))
    }

    #[resource("mem://{name}")]
    async fn r(&self, uri: String, _ctx: &RequestContext) -> McpResult<String> {
        let buf = [7u8; FAT_LOCAL];
        tick().await;
        Ok(format!("{uri}{}", buf[FAT_LOCAL - 1]))
    }
}

fn tool_future_size<H: McpHandler>(handler: &H, ctx: &RequestContext) -> usize {
    std::mem::size_of_val(&handler.call_tool("only", serde_json::json!({ "a": 1 }), ctx))
}

fn prompt_future_size<H: McpHandler>(handler: &H, ctx: &RequestContext) -> usize {
    let args = Some(serde_json::json!({ "topic": "x" }));
    std::mem::size_of_val(&handler.get_prompt("p", args, ctx))
}

fn resource_future_size<H: McpHandler>(handler: &H, ctx: &RequestContext) -> usize {
    std::mem::size_of_val(&handler.read_resource("mem://x", ctx))
}

#[test]
fn a_fat_handler_body_does_not_inflate_the_dispatch_future() {
    let ctx = RequestContext::stdio();

    for (what, thin, fat) in [
        (
            "call_tool",
            tool_future_size(&Thin, &ctx),
            tool_future_size(&Fat, &ctx),
        ),
        (
            "get_prompt",
            prompt_future_size(&Thin, &ctx),
            prompt_future_size(&Fat, &ctx),
        ),
        (
            "read_resource",
            resource_future_size(&Thin, &ctx),
            resource_future_size(&Fat, &ctx),
        ),
    ] {
        eprintln!("{what}: thin={thin} bytes, fat={fat} bytes");
        assert_eq!(
            thin, fat,
            "{what} future grew from {thin} to {fat} bytes when the handler body \
             gained a {FAT_LOCAL}-byte local: the body is inlined into the \
             dispatch state machine instead of being boxed"
        );
        assert!(
            fat <= MAX_DISPATCH_FUTURE,
            "{what} future is {fat} bytes (limit {MAX_DISPATCH_FUTURE})"
        );
    }
}

#[tokio::test]
async fn boxed_arms_still_dispatch() {
    let ctx = RequestContext::stdio();

    let result = Fat
        .call_tool("only", serde_json::json!({ "a": 7 }), &ctx)
        .await
        .unwrap();
    assert_eq!(result.first_text(), Some("77"));

    let prompt = Fat
        .get_prompt("p", Some(serde_json::json!({ "topic": "hi" })), &ctx)
        .await
        .unwrap();
    assert_eq!(prompt.messages[0].content.as_text(), Some("hi7"));

    let resource = Fat.read_resource("mem://anything", &ctx).await.unwrap();
    assert_eq!(resource.first_text(), Some("mem://anything7"));
}
