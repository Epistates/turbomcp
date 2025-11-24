//! # Elicitation - Requesting User Input
//!
//! Demonstrates how an MCP server can request structured input from the user
//! through the client UI using the elicitation protocol.
//!
//! Run with: `cargo run --example elicitation`

use std::collections::HashMap;
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, Content, ElicitRequest, ElicitRequestParams,
    ElicitationAction, ElicitationSchema, PrimitiveSchemaDefinition, TextContent, Tool,
    ToolInputSchema,
};
use turbomcp_server::sampling::SamplingExt;
use turbomcp_server::{ServerBuilder, ServerError, handlers::FunctionToolHandler};

/// Request user's name via elicitation
async fn get_user_name(
    _req: CallToolRequest,
    ctx: RequestContext,
) -> Result<CallToolResult, ServerError> {
    // Create simple text input schema
    let mut schema = ElicitationSchema::new();
    schema.properties.insert(
        "name".to_string(),
        PrimitiveSchemaDefinition::String {
            title: None,
            description: Some("Your full name".to_string()),
            format: None,
            min_length: Some(2),
            max_length: Some(100),
            enum_values: None,
            enum_names: None,
        },
    );
    schema.required = Some(vec!["name".to_string()]);

    let request = ElicitRequest {
        params: ElicitRequestParams::form(
            "Please enter your name".to_string(),
            schema,
            Some(60000),
            Some(true),
        ),
        _meta: None,
        #[cfg(feature = "mcp-tasks")]
        task: None,
    };

    // Send elicitation request to client
    let result = ctx.elicit(request).await?;

    // Handle user response
    let response_text = match result.action {
        ElicitationAction::Accept => {
            let name = result
                .content
                .as_ref()
                .and_then(|c| c.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            format!("✅ Hello, {}! Nice to meet you.", name)
        }
        ElicitationAction::Decline => "❌ User declined to provide name".to_string(),
        ElicitationAction::Cancel => "🚫 User cancelled the request".to_string(),
    };

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: response_text,
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        _meta: None,
        #[cfg(feature = "mcp-tasks")]
        task_id: None,
    })
}

/// Request model configuration from user
async fn configure_model(
    _req: CallToolRequest,
    ctx: RequestContext,
) -> Result<CallToolResult, ServerError> {
    // Create schema with multiple field types
    let mut schema = ElicitationSchema::new();

    // Model selection (enum)
    schema.properties.insert(
        "model".to_string(),
        PrimitiveSchemaDefinition::String {
            title: None,
            description: Some("LLM model to use".to_string()),
            format: None,
            min_length: None,
            max_length: None,
            enum_values: Some(vec![
                "gpt-4o".to_string(),
                "claude-3-5-sonnet".to_string(),
                "claude-3-haiku".to_string(),
            ]),
            enum_names: Some(vec![
                "GPT-4o (Most Capable)".to_string(),
                "Claude 3.5 Sonnet (Best)".to_string(),
                "Claude 3 Haiku (Fastest)".to_string(),
            ]),
        },
    );

    // Temperature (number)
    schema.properties.insert(
        "temperature".to_string(),
        PrimitiveSchemaDefinition::Number {
            title: None,
            description: Some("Sampling temperature (0.0-1.0)".to_string()),
            minimum: Some(0.0),
            maximum: Some(1.0),
        },
    );

    // Max tokens (integer)
    schema.properties.insert(
        "maxTokens".to_string(),
        PrimitiveSchemaDefinition::Integer {
            title: None,
            description: Some("Maximum response tokens".to_string()),
            minimum: Some(1),
            maximum: Some(4096),
        },
    );

    schema.required = Some(vec!["model".to_string(), "temperature".to_string()]);

    let request = ElicitRequest {
        params: ElicitRequestParams::form(
            "Configure your LLM preferences".to_string(),
            schema,
            Some(120000),
            Some(true),
        ),
        _meta: None,
        #[cfg(feature = "mcp-tasks")]
        task: None,
    };

    let result = ctx.elicit(request).await?;

    let response_text = match result.action {
        ElicitationAction::Accept => {
            let config = result
                .content
                .as_ref()
                .map(|c| serde_json::to_string_pretty(c).unwrap_or_default())
                .unwrap_or_else(|| "No configuration".to_string());
            format!("✅ Configuration saved:\n{}", config)
        }
        ElicitationAction::Decline => "❌ User declined configuration".to_string(),
        ElicitationAction::Cancel => "🚫 User cancelled".to_string(),
    };

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: response_text,
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        _meta: None,
        #[cfg(feature = "mcp-tasks")]
        task_id: None,
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Define tool schemas
    let name_tool = Tool {
        name: "get_user_name".to_string(),
        title: Some("Get User Name".to_string()),
        description: Some("Request user's name via elicitation dialog".to_string()),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some(HashMap::new()),
            required: None,
            additional_properties: Some(false),
        },
        output_schema: None,
        annotations: None,
        meta: None,
        #[cfg(feature = "mcp-icons")]
        icons: None,
    };

    let config_tool = Tool {
        name: "configure_model".to_string(),
        title: Some("Configure Model".to_string()),
        description: Some("Configure LLM preferences with multiple field types".to_string()),
        input_schema: ToolInputSchema {
            schema_type: "object".to_string(),
            properties: Some(HashMap::new()),
            required: None,
            additional_properties: Some(false),
        },
        output_schema: None,
        annotations: None,
        meta: None,
        #[cfg(feature = "mcp-icons")]
        icons: None,
    };

    // Build and run server
    let server = ServerBuilder::new()
        .name("elicitation-demo")
        .version("1.0.0")
        .description("Simple elicitation demonstration")
        .tool(
            "get_user_name",
            FunctionToolHandler::new(name_tool, get_user_name),
        )?
        .tool(
            "configure_model",
            FunctionToolHandler::new(config_tool, configure_model),
        )?
        .build();

    server.run_stdio().await?;
    Ok(())
}
