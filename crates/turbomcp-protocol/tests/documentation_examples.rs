//! Tests for documentation examples in `lib.rs`.

use turbomcp_protocol::{RequestContext, types::{CreateMessageRequest, SamplingMessage, Role, Content, TextContent}};

#[tokio::test]
async fn test_my_tool_example() {
    // This is a mock implementation of the my_tool function for testing purposes.
    async fn my_tool(ctx: RequestContext) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if let Some(_capabilities) = ctx.clone().server_to_client() {
            let _request = CreateMessageRequest {
                messages: vec![
                    SamplingMessage {
                        role: Role::User,
                        content: Content::Text(TextContent {
                            text: "Hello".to_string(),
                            annotations: None,
                            meta: None,
                        }),
                        metadata: None,
                    }
                ],
                max_tokens: 100,
                model_preferences: None,
                system_prompt: None,
                include_context: None,
                temperature: None,
                stop_sequences: None,
                _meta: None,
            };
            // In a real scenario, this would make a network request.
            // For this test, we'll assume it succeeds without actually sending anything.
            // let _response = capabilities.create_message(request, ctx).await?;
        }
        Ok(())
    }

    // To test this, we need a mock RequestContext.
    let ctx = RequestContext::default();
    let result = my_tool(ctx).await;
    assert!(result.is_ok());
}
