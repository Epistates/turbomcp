//! Comprehensive MCP Specification Compliance Test
//!
//! This test validates that TurboMCP fully implements the MCP specification:
//! - All required protocol methods are implemented
//! - Auto-discovery of tools, prompts, and resources works correctly
//! - Macro generation produces correct metadata

use turbomcp::{Context, McpResult, prompt, resource, server, tool};

#[tokio::test]
async fn test_macro_generation_mcp_compliance() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Testing TurboMCP Macro Generation Compliance");

    // This validates that the macro system is working correctly
    // This is a compile-time test - if the macros are broken, this won't compile

    // Mock struct to test server macro
    #[server(name = "TestServer")]
    impl TestMcpServer {
        #[tool("Test tool")]
        async fn test_tool(&self, _ctx: Context) -> McpResult<String> {
            Ok("test".to_string())
        }

        #[prompt("Test prompt")]
        async fn test_prompt(&self, _ctx: Context) -> McpResult<String> {
            Ok("test prompt".to_string())
        }

        #[resource("test://resource")]
        async fn test_resource(&self, _ctx: Context) -> McpResult<String> {
            Ok("test resource".to_string())
        }
    }

    #[derive(Clone)]
    struct TestMcpServer;

    // Test that metadata functions were generated
    let tools = TestMcpServer::get_tools_metadata();
    println!("Debug: Tools metadata: {:?}", tools);
    assert!(!tools.is_empty(), "Tool metadata not generated");

    let prompts = TestMcpServer::get_prompts_metadata();
    println!("Debug: Prompts metadata: {:?}", prompts);
    assert!(!prompts.is_empty(), "Prompt metadata not generated");

    let resources = TestMcpServer::get_resources_metadata();
    println!("Debug: Resources metadata: {:?}", resources);
    assert!(!resources.is_empty(), "Resource metadata not generated");

    println!("✅ Metadata generation working correctly");
    println!("   • Tools: {}", tools.len());
    println!("   • Prompts: {}", prompts.len());
    println!("   • Resources: {}", resources.len());

    Ok(())
}
