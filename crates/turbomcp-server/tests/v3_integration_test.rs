//! v3 Integration Tests
//!
//! Tests the v3 pristine architecture end-to-end.

use serde_json::json;
use std::future::Future;
use turbomcp_server::v3::{McpHandler, McpHandlerExt, RequestContext};
use turbomcp_types::{
    McpError, McpResult, Prompt, PromptResult, Resource, ResourceResult, ServerInfo, Tool,
    ToolResult,
};

/// A simple calculator server for testing.
#[derive(Clone)]
struct Calculator;

impl Calculator {
    fn new() -> Self {
        Self
    }
}

impl McpHandler for Calculator {
    fn server_info(&self) -> ServerInfo {
        ServerInfo::new("calculator", "1.0.0").with_description("A simple calculator server")
    }

    fn list_tools(&self) -> Vec<Tool> {
        vec![
            Tool::new("add", "Add two numbers"),
            Tool::new("multiply", "Multiply two numbers"),
            Tool::new("greet", "Greet someone by name"),
        ]
    }

    fn list_resources(&self) -> Vec<Resource> {
        vec![Resource::new("config://calculator", "config")]
    }

    fn list_prompts(&self) -> Vec<Prompt> {
        vec![Prompt::new("math_help", "Get help with math")]
    }

    fn call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
        _ctx: &RequestContext,
    ) -> impl Future<Output = McpResult<ToolResult>> + Send {
        let name = name.to_string();
        async move {
            match name.as_str() {
                "add" => {
                    let a = args.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                    let b = args.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
                    Ok(ToolResult::text(format!("{}", a + b)))
                }
                "multiply" => {
                    let a = args.get("a").and_then(|v| v.as_i64()).unwrap_or(0);
                    let b = args.get("b").and_then(|v| v.as_i64()).unwrap_or(0);
                    Ok(ToolResult::text(format!("{}", a * b)))
                }
                "greet" => {
                    let who = args.get("name").and_then(|v| v.as_str()).unwrap_or("World");
                    Ok(ToolResult::text(format!("Hello, {}!", who)))
                }
                _ => Err(McpError::tool_not_found(&name)),
            }
        }
    }

    fn read_resource(
        &self,
        uri: &str,
        _ctx: &RequestContext,
    ) -> impl Future<Output = McpResult<ResourceResult>> + Send {
        let uri = uri.to_string();
        async move {
            if uri == "config://calculator" {
                Ok(ResourceResult::json(
                    &uri,
                    &json!({
                        "precision": 10,
                        "mode": "standard"
                    }),
                )?)
            } else {
                Err(McpError::resource_not_found(&uri))
            }
        }
    }

    fn get_prompt(
        &self,
        name: &str,
        _args: Option<serde_json::Value>,
        _ctx: &RequestContext,
    ) -> impl Future<Output = McpResult<PromptResult>> + Send {
        let name = name.to_string();
        async move {
            if name == "math_help" {
                Ok(PromptResult::user("I need help with math calculations."))
            } else {
                Err(McpError::prompt_not_found(&name))
            }
        }
    }
}

#[tokio::test]
async fn test_calculator_server_info() {
    let calc = Calculator::new();
    let info = calc.server_info();
    assert_eq!(info.name, "calculator");
    assert_eq!(info.version, "1.0.0");
}

#[tokio::test]
async fn test_calculator_list_tools() {
    let calc = Calculator::new();
    let tools = calc.list_tools();
    assert_eq!(tools.len(), 3);
    assert!(tools.iter().any(|t| t.name == "add"));
    assert!(tools.iter().any(|t| t.name == "multiply"));
    assert!(tools.iter().any(|t| t.name == "greet"));
}

#[tokio::test]
async fn test_calculator_add() {
    let calc = Calculator::new();
    let ctx = RequestContext::new();
    let result = calc
        .call_tool("add", json!({"a": 5, "b": 3}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.first_text(), Some("8"));
}

#[tokio::test]
async fn test_calculator_multiply() {
    let calc = Calculator::new();
    let ctx = RequestContext::new();
    let result = calc
        .call_tool("multiply", json!({"a": 4, "b": 7}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.first_text(), Some("28"));
}

#[tokio::test]
async fn test_calculator_greet() {
    let calc = Calculator::new();
    let ctx = RequestContext::new();
    let result = calc
        .call_tool("greet", json!({"name": "Alice"}), &ctx)
        .await
        .unwrap();
    assert_eq!(result.first_text(), Some("Hello, Alice!"));
}

#[tokio::test]
async fn test_handle_request_initialize() {
    let calc = Calculator::new();
    let ctx = RequestContext::new();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });

    let response = calc.handle_request(request, ctx).await.unwrap();
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["serverInfo"]["name"], "calculator");
    assert!(response["result"]["capabilities"]["tools"].is_object());
}

#[tokio::test]
async fn test_handle_request_tools_list() {
    let calc = Calculator::new();
    let ctx = RequestContext::new();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });

    let response = calc.handle_request(request, ctx).await.unwrap();
    assert_eq!(response["jsonrpc"], "2.0");
    let tools = response["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);
}

#[tokio::test]
async fn test_handle_request_tools_call() {
    let calc = Calculator::new();
    let ctx = RequestContext::new();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "add",
            "arguments": {"a": 100, "b": 50}
        }
    });

    let response = calc.handle_request(request, ctx).await.unwrap();
    assert_eq!(response["jsonrpc"], "2.0");
    assert!(response.get("error").is_none());
    // Result contains ToolResult with content array
    let content = response["result"]["content"].as_array().unwrap();
    assert!(!content.is_empty());
}

#[tokio::test]
async fn test_handle_request_resources_list() {
    let calc = Calculator::new();
    let ctx = RequestContext::new();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "resources/list"
    });

    let response = calc.handle_request(request, ctx).await.unwrap();
    let resources = response["result"]["resources"].as_array().unwrap();
    assert_eq!(resources.len(), 1);
    assert_eq!(resources[0]["uri"], "config://calculator");
}

#[tokio::test]
async fn test_handle_request_prompts_list() {
    let calc = Calculator::new();
    let ctx = RequestContext::new();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "prompts/list"
    });

    let response = calc.handle_request(request, ctx).await.unwrap();
    let prompts = response["result"]["prompts"].as_array().unwrap();
    assert_eq!(prompts.len(), 1);
    assert_eq!(prompts[0]["name"], "math_help");
}

#[tokio::test]
async fn test_handle_request_error_unknown_method() {
    let calc = Calculator::new();
    let ctx = RequestContext::new();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "unknown/method"
    });

    let response = calc.handle_request(request, ctx).await.unwrap();
    assert!(response.get("error").is_some());
    assert_eq!(response["error"]["code"], McpError::METHOD_NOT_FOUND);
}
