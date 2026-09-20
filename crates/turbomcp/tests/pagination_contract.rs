//! Cursor handling on the four paginated list methods.
//!
//! Pagination is optional for servers, so the default is to return the whole
//! catalogue and mint no `nextCursor` — switching paging on underneath a client
//! that does not follow cursors would silently truncate what it can see. What
//! is *not* optional is handling a cursor correctly when one arrives: before
//! 3.5.0 a cursor was ignored entirely, so a client walking pages was served
//! page one forever.

use turbomcp::prelude::*;
use turbomcp_core::handler::McpHandler;

#[derive(Clone)]
struct Catalogue;

#[server(name = "catalogue", version = "1.0.0")]
impl Catalogue {
    #[tool]
    async fn a(&self) -> String {
        String::new()
    }
    #[tool]
    async fn b(&self) -> String {
        String::new()
    }
    #[tool]
    async fn c(&self) -> String {
        String::new()
    }
}

/// Same tools, but opts into paging two at a time.
#[derive(Clone)]
struct Paged;

#[server(name = "paged", version = "1.0.0", page_size = 2)]
impl Paged {
    #[tool]
    async fn a(&self) -> String {
        String::new()
    }
    #[tool]
    async fn b(&self) -> String {
        String::new()
    }
    #[tool]
    async fn c(&self) -> String {
        String::new()
    }
}

async fn list<H: McpHandler>(handler: &H, cursor: Option<&str>) -> serde_json::Value {
    let mut params = serde_json::json!({});
    if let Some(cursor) = cursor {
        params["cursor"] = serde_json::Value::String(cursor.to_string());
    }
    handler
        .handle_request(
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": params
            }),
            RequestContext::stdio(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn default_returns_everything_and_mints_no_cursor() {
    let response = list(&Catalogue, None).await;
    let result = &response["result"];

    assert_eq!(result["tools"].as_array().unwrap().len(), 3);
    assert!(
        result.get("nextCursor").is_none(),
        "a server that does not paginate must not mint a cursor: {result}"
    );
}

/// The actual defect: a cursor used to be ignored, so a client following pages
/// received page one again and could loop forever.
#[tokio::test]
async fn an_invalid_cursor_is_rejected_rather_than_ignored() {
    for bad in [
        "garbage",
        "tools:999",
        "prompts:0",
        "tools:",
        ":0",
        "tools:-1",
    ] {
        let response = list(&Catalogue, Some(bad)).await;
        assert_eq!(
            response["error"]["code"], -32602,
            "cursor {bad:?} should be invalid params, got {response}"
        );
    }
}

/// A cursor minted for one list must not be honoured by another — it would be
/// reinterpreted as an offset into a different collection.
#[tokio::test]
async fn a_cursor_from_another_list_is_rejected() {
    let response = list(&Catalogue, Some("prompts:1")).await;
    assert_eq!(response["error"]["code"], -32602, "got {response}");
}

/// Offset zero is the start of the list and is always valid.
#[tokio::test]
async fn the_zero_cursor_round_trips() {
    let response = list(&Catalogue, Some("tools:0")).await;
    assert_eq!(response["result"]["tools"].as_array().unwrap().len(), 3);
}

// ── Opting in with #[server(page_size = N)] ────────────────────────────────

/// Walking the pages must reach every entry exactly once and then stop.
#[tokio::test]
async fn paging_walks_the_whole_catalogue_and_terminates() {
    let mut seen: Vec<String> = Vec::new();
    let mut cursor: Option<String> = None;
    let mut pages = 0;

    loop {
        let response = list(&Paged, cursor.as_deref()).await;
        let result = &response["result"];
        assert!(response["error"].is_null(), "got {response}");

        let page = result["tools"].as_array().unwrap();
        assert!(
            page.len() <= 2,
            "page exceeded the configured size: {result}"
        );
        seen.extend(page.iter().map(|t| t["name"].as_str().unwrap().to_string()));

        pages += 1;
        assert!(pages < 10, "pagination did not terminate");

        match result.get("nextCursor").and_then(|c| c.as_str()) {
            Some(next) => cursor = Some(next.to_string()),
            None => break,
        }
    }

    assert_eq!(pages, 2, "3 tools at 2 per page is two pages");
    seen.sort();
    assert_eq!(seen, ["a", "b", "c"], "every tool appears exactly once");
}

/// The last page carries no cursor — that absence is the end-of-results signal.
#[tokio::test]
async fn the_final_page_mints_no_cursor() {
    let first = list(&Paged, None).await;
    let cursor = first["result"]["nextCursor"].as_str().unwrap().to_string();

    let second = list(&Paged, Some(&cursor)).await;
    assert_eq!(second["result"]["tools"].as_array().unwrap().len(), 1);
    assert!(
        second["result"].get("nextCursor").is_none(),
        "got {}",
        second["result"]
    );
}
