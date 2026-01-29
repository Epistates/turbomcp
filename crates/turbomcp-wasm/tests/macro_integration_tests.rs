//! Integration tests for the WASM server macros
//!
//! These tests verify that the `#[server]`, `#[tool]`, `#[resource]`, and `#[prompt]`
//! macros correctly generate code that uses the builder pattern.

#![cfg(feature = "macros")]
#![allow(dead_code)] // Methods are registered but not called directly in tests

use serde::Deserialize;
use turbomcp_wasm::prelude::*;
use turbomcp_wasm::server;

// Test struct with all handler types
#[derive(Clone)]
struct TestServer {
    prefix: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct GreetArgs {
    name: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
struct AddArgs {
    a: i64,
    b: i64,
}

#[server(
    name = "test-server",
    version = "1.0.0",
    description = "A test MCP server"
)]
impl TestServer {
    #[tool("Greet someone by name")]
    async fn greet(&self, args: GreetArgs) -> String {
        format!("{} {}!", self.prefix, args.name)
    }

    #[tool("Add two numbers")]
    async fn add(&self, args: AddArgs) -> String {
        format!("{}", args.a + args.b)
    }

    #[tool("Get server status")]
    async fn status(&self) -> String {
        "running".to_string()
    }

    #[resource("config://app")]
    async fn config(&self, uri: String) -> ResourceResult {
        ResourceResult::text(&uri, r#"{"theme": "dark"}"#)
    }

    #[prompt("Default greeting prompt")]
    async fn greeting(&self) -> PromptResult {
        PromptResult::user("Hello! How can I help?")
    }
}

#[test]
fn test_server_info() {
    let (name, version) = TestServer::server_info();
    assert_eq!(name, "test-server");
    assert_eq!(version, "1.0.0");
}

#[test]
fn test_tools_metadata() {
    let tools = TestServer::get_tools_metadata();
    assert_eq!(tools.len(), 3);

    // Check tool names and descriptions (tuple format: name, description, tags, version)
    let tool_names: Vec<_> = tools.iter().map(|(name, _, _, _)| *name).collect();
    assert!(tool_names.contains(&"greet"));
    assert!(tool_names.contains(&"add"));
    assert!(tool_names.contains(&"status"));

    // Check descriptions
    let greet_tool = tools
        .iter()
        .find(|(name, _, _, _)| *name == "greet")
        .unwrap();
    assert_eq!(greet_tool.1, "Greet someone by name");
}

#[test]
fn test_resources_metadata() {
    let resources = TestServer::get_resources_metadata();
    assert_eq!(resources.len(), 1);

    // Tuple format: uri, name, tags, version
    let (uri, name, _tags, _version) = resources[0];
    assert_eq!(uri, "config://app");
    assert_eq!(name, "config");
}

#[test]
fn test_prompts_metadata() {
    let prompts = TestServer::get_prompts_metadata();
    assert_eq!(prompts.len(), 1);

    // Tuple format: name, description, tags, version
    let (name, desc, _tags, _version) = prompts[0];
    assert_eq!(name, "greeting");
    assert_eq!(desc, "Default greeting prompt");
}

#[test]
fn test_into_mcp_server() {
    // Verify that into_mcp_server() compiles and returns the correct type
    let server = TestServer {
        prefix: "Hello".to_string(),
    };
    let _mcp_server: McpServer = server.into_mcp_server();
}

// Test with no tools/resources/prompts
#[derive(Clone)]
struct MinimalServer;

#[server(name = "minimal", version = "0.1.0")]
impl MinimalServer {}

#[test]
fn test_minimal_server() {
    let (name, version) = MinimalServer::server_info();
    assert_eq!(name, "minimal");
    assert_eq!(version, "0.1.0");

    let tools = MinimalServer::get_tools_metadata();
    assert!(tools.is_empty());

    let resources = MinimalServer::get_resources_metadata();
    assert!(resources.is_empty());

    let prompts = MinimalServer::get_prompts_metadata();
    assert!(prompts.is_empty());
}

// Test with only tools
#[derive(Clone)]
struct ToolsOnlyServer;

#[server(name = "tools-only", version = "1.0.0")]
impl ToolsOnlyServer {
    #[tool("A simple tool")]
    async fn simple(&self) -> String {
        "result".to_string()
    }
}

#[test]
fn test_tools_only_server() {
    let tools = ToolsOnlyServer::get_tools_metadata();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].0, "simple");

    let resources = ToolsOnlyServer::get_resources_metadata();
    assert!(resources.is_empty());

    let prompts = ToolsOnlyServer::get_prompts_metadata();
    assert!(prompts.is_empty());
}

// Test with multiple resources
#[derive(Clone)]
struct MultiResourceServer;

#[server(name = "multi-resource", version = "1.0.0")]
impl MultiResourceServer {
    #[resource("file://{path}")]
    async fn file(&self, uri: String) -> ResourceResult {
        ResourceResult::text(&uri, "file content")
    }

    #[resource("config://app")]
    async fn config(&self, uri: String) -> ResourceResult {
        ResourceResult::text(&uri, "config content")
    }

    #[resource("data://metrics")]
    async fn metrics(&self, uri: String) -> ResourceResult {
        ResourceResult::text(&uri, "metrics content")
    }
}

#[test]
fn test_multi_resource_server() {
    let resources = MultiResourceServer::get_resources_metadata();
    assert_eq!(resources.len(), 3);

    // Tuple format: uri, name, tags, version
    let uris: Vec<_> = resources.iter().map(|(uri, _, _, _)| *uri).collect();
    assert!(uris.contains(&"file://{path}"));
    assert!(uris.contains(&"config://app"));
    assert!(uris.contains(&"data://metrics"));
}
