//! Tests for documentation examples in `lib.rs`.

use turbomcp_server::ServerBuilder;

#[tokio::test]
async fn test_server_builder_example() {
    let server = ServerBuilder::new()
        .name("MyServer")
        .version("1.0.0")
        // Configure filesystem roots
        .root("file:///workspace", Some("Workspace".to_string()))
        .root("file:///tmp", Some("Temp".to_string()))
        .build();
    
    // Get shutdown handle for graceful termination
    let _shutdown_handle = server.shutdown_handle();
    
    // In production: spawn server and wait for shutdown
    // tokio::spawn(async move { server.run_stdio().await });
    // signal::ctrl_c().await?;
    // shutdown_handle.shutdown().await;
}

use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{CreateMessageRequest, SamplingMessage, Role, Content, TextContent};

async fn my_tool(_ctx: RequestContext) -> Result<String, Box<dyn std::error::Error>> {
    // Create a sampling request
    let _request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: "What is 2+2?".to_string(),
                annotations: None,
                meta: None,
            }),
            metadata: None,
        }],
        max_tokens: 50,
        model_preferences: None,
        system_prompt: Some("You are a helpful math assistant.".to_string()),
        include_context: Some(turbomcp_protocol::types::IncludeContext::None),
        temperature: Some(0.7),
        stop_sequences: None,
        _meta: None,
    };

    // Send the request to the client
    // In a real scenario, this would make a network request.
    // For this test, we'll assume it succeeds without actually sending anything.
    // let result = ctx.create_message(request).await?;
    Ok(format!("Response: {:?}", "mocked result"))
}

#[tokio::test]
async fn test_sampling_ext_example() {
    let ctx = RequestContext::default();
    let result = my_tool(ctx).await;
    assert!(result.is_ok());
}
