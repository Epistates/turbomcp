# Advanced Examples

Advanced TurboMCP patterns including LLM sampling, user elicitation, and complex multi-step workflows.

## LLM Sampling Integration

### Basic Sampling Request

Request LLM assistance from the client using the sampling protocol:

```rust
use turbomcp_protocol::RequestContext;
use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, Content, CreateMessageRequest,
    Role, SamplingMessage, TextContent,
};
use turbomcp_server::sampling::SamplingExt;
use turbomcp_server::{ServerBuilder, ServerError};

async fn ask_llm(
    req: CallToolRequest,
    ctx: RequestContext,
) -> Result<CallToolResult, ServerError> {
    // Extract question from tool arguments
    let question = req
        .arguments
        .as_ref()
        .and_then(|args| args.get("question"))
        .and_then(|v| v.as_str())
        .unwrap_or("What is 2+2?");

    // Create sampling request
    let sampling_request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: question.to_string(),
                annotations: None,
                meta: None,
            }),
            metadata: None,
        }],
        max_tokens: 500,
        model_preferences: None,
        system_prompt: Some("You are a helpful assistant. Be concise.".to_string()),
        include_context: Some(turbomcp_protocol::types::IncludeContext::None),
        temperature: Some(0.7),
        stop_sequences: None,
        #[cfg(feature = "mcp-sampling-tools")]
        tools: None,
        #[cfg(feature = "mcp-sampling-tools")]
        tool_choice: None,
        task: None,
        _meta: None,
    };

    // Send to client's LLM
    let result = ctx.create_message(sampling_request).await?;

    // Extract response
    let response = match &result.content {
        Content::Text(t) => t.text.clone(),
        _ => "(non-text response)".to_string(),
    };

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: format!("Question: {}\n\nLLM Response: {}", question, response),
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        _meta: None,
        task_id: None,
    })
}
```

**Key Features:**
- Server requests LLM capabilities from client
- Client handles LLM integration (OpenAI, Anthropic, etc.)
- Useful for AI-assisted tools and workflows
- Supports system prompts, temperature, max tokens

### Multi-Turn Conversation

Implement conversational sampling with context:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;

#[derive(Clone)]
struct ConversationalServer {
    conversations: Arc<RwLock<HashMap<String, Vec<SamplingMessage>>>>,
}

async fn continue_conversation(
    req: CallToolRequest,
    ctx: RequestContext,
    server: Arc<ConversationalServer>,
) -> Result<CallToolResult, ServerError> {
    let user_id = ctx.request_id().to_string();
    let message = req
        .arguments
        .as_ref()
        .and_then(|args| args.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Get or create conversation history
    let mut conversations = server.conversations.write().await;
    let history = conversations.entry(user_id.clone()).or_insert_with(Vec::new);

    // Add user message to history
    history.push(SamplingMessage {
        role: Role::User,
        content: Content::Text(TextContent {
            text: message.to_string(),
            annotations: None,
            meta: None,
        }),
        metadata: None,
    });

    // Create sampling request with full history
    let sampling_request = CreateMessageRequest {
        messages: history.clone(),
        max_tokens: 1000,
        system_prompt: Some("You are a helpful conversational assistant.".to_string()),
        temperature: Some(0.8),
        model_preferences: None,
        include_context: Some(turbomcp_protocol::types::IncludeContext::None),
        stop_sequences: None,
        #[cfg(feature = "mcp-sampling-tools")]
        tools: None,
        #[cfg(feature = "mcp-sampling-tools")]
        tool_choice: None,
        task: None,
        _meta: None,
    };

    let result = ctx.create_message(sampling_request).await?;

    // Add assistant response to history
    history.push(SamplingMessage {
        role: Role::Assistant,
        content: result.content.clone(),
        metadata: None,
    });

    // Extract response text
    let response = match &result.content {
        Content::Text(t) => t.text.clone(),
        _ => "(non-text response)".to_string(),
    };

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: response,
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        _meta: None,
        task_id: None,
    })
}
```

### Sampling with Model Preferences

Request specific models or capabilities:

```rust
async fn ask_with_model_preference(
    query: &str,
    ctx: &RequestContext,
) -> Result<String, ServerError> {
    let sampling_request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: query.to_string(),
                annotations: None,
                meta: None,
            }),
            metadata: None,
        }],
        max_tokens: 2000,
        model_preferences: Some(turbomcp_protocol::types::ModelPreferences {
            hints: Some(vec![
                turbomcp_protocol::types::ModelHint {
                    name: Some("claude-3-5-sonnet-20241022".to_string()),
                },
                turbomcp_protocol::types::ModelHint {
                    name: Some("gpt-4o".to_string()),
                },
            ]),
            cost_priority: Some(0.5), // Balance cost and quality
            speed_priority: Some(0.3),
            intelligence_priority: Some(0.9),
        }),
        system_prompt: Some("You are an expert technical analyst.".to_string()),
        temperature: Some(0.3), // Lower for more deterministic responses
        include_context: Some(turbomcp_protocol::types::IncludeContext::None),
        stop_sequences: Some(vec!["###END###".to_string()]),
        #[cfg(feature = "mcp-sampling-tools")]
        tools: None,
        #[cfg(feature = "mcp-sampling-tools")]
        tool_choice: None,
        task: None,
        _meta: None,
    };

    let result = ctx.create_message(sampling_request).await?;

    match &result.content {
        Content::Text(t) => Ok(t.text.clone()),
        _ => Ok("(non-text response)".to_string()),
    }
}
```

## User Elicitation Patterns

### Simple Form Input

Request structured input from users via client UI:

```rust
use turbomcp_protocol::types::{
    ElicitRequest, ElicitRequestParams, ElicitationAction,
    ElicitationSchema, PrimitiveSchemaDefinition,
};
use turbomcp_server::sampling::SamplingExt;

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
            Some(60000), // 60 second timeout
            Some(true),  // Allow cancellation
        ),
        _meta: None,
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
            format!("Hello, {}! Nice to meet you.", name)
        }
        ElicitationAction::Decline => "User declined to provide name".to_string(),
        ElicitationAction::Cancel => "User cancelled the request".to_string(),
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
        task_id: None,
    })
}
```

### Multi-Field Forms

Collect complex structured data:

```rust
async fn configure_model(
    _req: CallToolRequest,
    ctx: RequestContext,
) -> Result<CallToolResult, ServerError> {
    let mut schema = ElicitationSchema::new();

    // Model selection (enum)
    schema.properties.insert(
        "model".to_string(),
        PrimitiveSchemaDefinition::String {
            title: Some("Model".to_string()),
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
            title: Some("Temperature".to_string()),
            description: Some("Sampling temperature (0.0-1.0)".to_string()),
            minimum: Some(0.0),
            maximum: Some(1.0),
        },
    );

    // Max tokens (integer)
    schema.properties.insert(
        "maxTokens".to_string(),
        PrimitiveSchemaDefinition::Integer {
            title: Some("Max Tokens".to_string()),
            description: Some("Maximum response tokens".to_string()),
            minimum: Some(1),
            maximum: Some(4096),
        },
    );

    // Enable streaming (boolean)
    schema.properties.insert(
        "streaming".to_string(),
        PrimitiveSchemaDefinition::Boolean {
            title: Some("Enable Streaming".to_string()),
            description: Some("Stream responses as they're generated".to_string()),
        },
    );

    schema.required = Some(vec!["model".to_string(), "temperature".to_string()]);

    let request = ElicitRequest {
        params: ElicitRequestParams::form(
            "Configure your LLM preferences".to_string(),
            schema,
            Some(120000), // 2 minute timeout
            Some(true),
        ),
        _meta: None,
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
            format!("Configuration saved:\n{}", config)
        }
        ElicitationAction::Decline => "User declined configuration".to_string(),
        ElicitationAction::Cancel => "User cancelled".to_string(),
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
        task_id: None,
    })
}
```

### Conditional Elicitation

Request additional info based on previous responses:

```rust
async fn conditional_survey(
    _req: CallToolRequest,
    ctx: RequestContext,
) -> Result<CallToolResult, ServerError> {
    // First question: Are you a developer?
    let mut schema1 = ElicitationSchema::new();
    schema1.properties.insert(
        "is_developer".to_string(),
        PrimitiveSchemaDefinition::Boolean {
            title: Some("Developer".to_string()),
            description: Some("Are you a software developer?".to_string()),
        },
    );
    schema1.required = Some(vec!["is_developer".to_string()]);

    let request1 = ElicitRequest {
        params: ElicitRequestParams::form(
            "Quick survey".to_string(),
            schema1,
            Some(60000),
            Some(true),
        ),
        _meta: None,
        task: None,
    };

    let result1 = ctx.elicit(request1).await?;

    if result1.action != ElicitationAction::Accept {
        return Ok(CallToolResult {
            content: vec![Content::Text(TextContent {
                text: "Survey cancelled".to_string(),
                annotations: None,
                meta: None,
            })],
            is_error: None,
            structured_content: None,
            _meta: None,
            task_id: None,
        });
    }

    let is_developer = result1
        .content
        .as_ref()
        .and_then(|c| c.get("is_developer"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Conditional follow-up based on answer
    if is_developer {
        let mut schema2 = ElicitationSchema::new();
        schema2.properties.insert(
            "languages".to_string(),
            PrimitiveSchemaDefinition::String {
                title: Some("Programming Languages".to_string()),
                description: Some("What languages do you use?".to_string()),
                format: None,
                min_length: None,
                max_length: None,
                enum_values: Some(vec![
                    "Rust".to_string(),
                    "Python".to_string(),
                    "JavaScript".to_string(),
                    "Go".to_string(),
                    "Other".to_string(),
                ]),
                enum_names: None,
            },
        );

        let request2 = ElicitRequest {
            params: ElicitRequestParams::form(
                "Developer details".to_string(),
                schema2,
                Some(60000),
                Some(true),
            ),
            _meta: None,
            task: None,
        };

        let result2 = ctx.elicit(request2).await?;

        if result2.action == ElicitationAction::Accept {
            let lang = result2
                .content
                .as_ref()
                .and_then(|c| c.get("languages"))
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown");
            return Ok(CallToolResult {
                content: vec![Content::Text(TextContent {
                    text: format!("Great! You use {}. Thanks for the info!", lang),
                    annotations: None,
                    meta: None,
                })],
                is_error: None,
                structured_content: None,
                _meta: None,
                task_id: None,
            });
        }
    }

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: "Thanks for completing the survey!".to_string(),
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        _meta: None,
        task_id: None,
    })
}
```

### Confirmation Dialogs

Use elicitation for user confirmations:

```rust
async fn delete_with_confirmation(
    req: CallToolRequest,
    ctx: RequestContext,
) -> Result<CallToolResult, ServerError> {
    let resource_id = req
        .arguments
        .as_ref()
        .and_then(|args| args.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    // Create confirmation schema
    let mut schema = ElicitationSchema::new();
    schema.properties.insert(
        "confirm".to_string(),
        PrimitiveSchemaDefinition::Boolean {
            title: Some("Confirm Deletion".to_string()),
            description: Some(format!(
                "Are you sure you want to delete resource '{}'? This cannot be undone.",
                resource_id
            )),
        },
    );
    schema.required = Some(vec!["confirm".to_string()]);

    let request = ElicitRequest {
        params: ElicitRequestParams::form(
            "Confirm Deletion".to_string(),
            schema,
            Some(30000), // 30 second timeout
            Some(true),
        ),
        _meta: None,
        task: None,
    };

    let result = ctx.elicit(request).await?;

    match result.action {
        ElicitationAction::Accept => {
            let confirmed = result
                .content
                .as_ref()
                .and_then(|c| c.get("confirm"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            if confirmed {
                // Perform deletion
                // delete_resource(resource_id).await?;
                Ok(CallToolResult {
                    content: vec![Content::Text(TextContent {
                        text: format!("Resource '{}' deleted successfully", resource_id),
                        annotations: None,
                        meta: None,
                    })],
                    is_error: None,
                    structured_content: None,
                    _meta: None,
                    task_id: None,
                })
            } else {
                Ok(CallToolResult {
                    content: vec![Content::Text(TextContent {
                        text: "Deletion cancelled".to_string(),
                        annotations: None,
                        meta: None,
                    })],
                    is_error: None,
                    structured_content: None,
                    _meta: None,
                    task_id: None,
                })
            }
        }
        _ => Ok(CallToolResult {
            content: vec![Content::Text(TextContent {
                text: "Deletion cancelled".to_string(),
                annotations: None,
                meta: None,
            })],
            is_error: None,
            structured_content: None,
            _meta: None,
            task_id: None,
        }),
    }
}
```

## Complex Multi-Step Workflows

### AI-Assisted Code Review

Combine sampling and elicitation for interactive workflows:

```rust
async fn code_review_workflow(
    req: CallToolRequest,
    ctx: RequestContext,
) -> Result<CallToolResult, ServerError> {
    // Step 1: Get code from user
    let mut code_schema = ElicitationSchema::new();
    code_schema.properties.insert(
        "code".to_string(),
        PrimitiveSchemaDefinition::String {
            title: Some("Code".to_string()),
            description: Some("Paste the code to review".to_string()),
            format: None,
            min_length: Some(1),
            max_length: Some(10000),
            enum_values: None,
            enum_names: None,
        },
    );
    code_schema.required = Some(vec!["code".to_string()]);

    let code_request = ElicitRequest {
        params: ElicitRequestParams::form(
            "Code Review - Submit Code".to_string(),
            code_schema,
            Some(300000), // 5 minutes
            Some(true),
        ),
        _meta: None,
        task: None,
    };

    let code_result = ctx.elicit(code_request).await?;

    if code_result.action != ElicitationAction::Accept {
        return Ok(CallToolResult {
            content: vec![Content::Text(TextContent {
                text: "Code review cancelled".to_string(),
                annotations: None,
                meta: None,
            })],
            is_error: None,
            structured_content: None,
            _meta: None,
            task_id: None,
        });
    }

    let code = code_result
        .content
        .as_ref()
        .and_then(|c| c.get("code"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Step 2: Use LLM to review code
    let review_request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: format!(
                    "Please review this code for:\n\
                     1. Bugs and potential errors\n\
                     2. Security issues\n\
                     3. Performance concerns\n\
                     4. Best practices\n\n\
                     Code:\n```\n{}\n```",
                    code
                ),
                annotations: None,
                meta: None,
            }),
            metadata: None,
        }],
        max_tokens: 2000,
        system_prompt: Some("You are an expert code reviewer. Be thorough and constructive.".to_string()),
        temperature: Some(0.3),
        model_preferences: None,
        include_context: Some(turbomcp_protocol::types::IncludeContext::None),
        stop_sequences: None,
        #[cfg(feature = "mcp-sampling-tools")]
        tools: None,
        #[cfg(feature = "mcp-sampling-tools")]
        tool_choice: None,
        task: None,
        _meta: None,
    };

    let review_result = ctx.create_message(review_request).await?;

    let review_text = match &review_result.content {
        Content::Text(t) => t.text.clone(),
        _ => "Unable to generate review".to_string(),
    };

    // Step 3: Ask if user wants detailed explanations
    let mut followup_schema = ElicitationSchema::new();
    followup_schema.properties.insert(
        "want_details".to_string(),
        PrimitiveSchemaDefinition::Boolean {
            title: Some("Detailed Explanations".to_string()),
            description: Some("Would you like detailed explanations for each issue?".to_string()),
        },
    );

    let followup_request = ElicitRequest {
        params: ElicitRequestParams::form(
            "Code Review Complete".to_string(),
            followup_schema,
            Some(60000),
            Some(true),
        ),
        _meta: None,
        task: None,
    };

    let followup_result = ctx.elicit(followup_request).await?;

    let mut final_response = review_text.clone();

    if followup_result.action == ElicitationAction::Accept {
        let want_details = followup_result
            .content
            .as_ref()
            .and_then(|c| c.get("want_details"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if want_details {
            // Step 4: Get detailed explanations from LLM
            let details_request = CreateMessageRequest {
                messages: vec![
                    SamplingMessage {
                        role: Role::User,
                        content: Content::Text(TextContent {
                            text: format!("Code:\n```\n{}\n```", code),
                            annotations: None,
                            meta: None,
                        }),
                        metadata: None,
                    },
                    SamplingMessage {
                        role: Role::Assistant,
                        content: Content::Text(TextContent {
                            text: review_text,
                            annotations: None,
                            meta: None,
                        }),
                        metadata: None,
                    },
                    SamplingMessage {
                        role: Role::User,
                        content: Content::Text(TextContent {
                            text: "Please provide detailed explanations for each issue you identified, \
                                   including code examples of how to fix them.".to_string(),
                            annotations: None,
                            meta: None,
                        }),
                        metadata: None,
                    },
                ],
                max_tokens: 3000,
                system_prompt: Some("You are an expert code reviewer. Provide detailed, actionable advice.".to_string()),
                temperature: Some(0.4),
                model_preferences: None,
                include_context: Some(turbomcp_protocol::types::IncludeContext::None),
                stop_sequences: None,
                #[cfg(feature = "mcp-sampling-tools")]
                tools: None,
                #[cfg(feature = "mcp-sampling-tools")]
                tool_choice: None,
                task: None,
                _meta: None,
            };

            let details_result = ctx.create_message(details_request).await?;

            if let Content::Text(t) = &details_result.content {
                final_response = format!("{}\n\n## Detailed Explanations\n\n{}", final_response, t.text);
            }
        }
    }

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: final_response,
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        _meta: None,
        task_id: None,
    })
}
```

### Data Processing Pipeline

Chain multiple operations with progress tracking:

```rust
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
struct PipelineServer {
    progress: Arc<RwLock<HashMap<String, f32>>>,
}

async fn process_data_pipeline(
    req: CallToolRequest,
    ctx: RequestContext,
    server: Arc<PipelineServer>,
) -> Result<CallToolResult, ServerError> {
    let task_id = ctx.request_id().to_string();
    let input_data = req
        .arguments
        .as_ref()
        .and_then(|args| args.get("data"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Initialize progress
    server.progress.write().await.insert(task_id.clone(), 0.0);

    // Step 1: Validate input (20%)
    let validated = validate_input(input_data)?;
    server.progress.write().await.insert(task_id.clone(), 0.2);

    // Step 2: Transform data (40%)
    let transformed = transform_data(&validated).await?;
    server.progress.write().await.insert(task_id.clone(), 0.6);

    // Step 3: Analyze with LLM (80%)
    let analysis_request = CreateMessageRequest {
        messages: vec![SamplingMessage {
            role: Role::User,
            content: Content::Text(TextContent {
                text: format!("Analyze this data and provide insights:\n{}", transformed),
                annotations: None,
                meta: None,
            }),
            metadata: None,
        }],
        max_tokens: 1000,
        system_prompt: Some("You are a data analyst. Provide actionable insights.".to_string()),
        temperature: Some(0.5),
        model_preferences: None,
        include_context: Some(turbomcp_protocol::types::IncludeContext::None),
        stop_sequences: None,
        #[cfg(feature = "mcp-sampling-tools")]
        tools: None,
        #[cfg(feature = "mcp-sampling-tools")]
        tool_choice: None,
        task: None,
        _meta: None,
    };

    let analysis = ctx.create_message(analysis_request).await?;
    server.progress.write().await.insert(task_id.clone(), 0.8);

    // Step 4: Generate report (100%)
    let report = generate_report(&transformed, &analysis).await?;
    server.progress.write().await.insert(task_id.clone(), 1.0);

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: report,
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        _meta: None,
        task_id: None,
    })
}

fn validate_input(data: &str) -> Result<String, ServerError> {
    if data.is_empty() {
        return Err(ServerError::InvalidParams("Empty input".to_string()));
    }
    Ok(data.to_string())
}

async fn transform_data(data: &str) -> Result<String, ServerError> {
    // Simulate data transformation
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    Ok(format!("Transformed: {}", data))
}

async fn generate_report(
    data: &str,
    analysis: &turbomcp_protocol::types::CreateMessageResult,
) -> Result<String, ServerError> {
    let analysis_text = match &analysis.content {
        Content::Text(t) => &t.text,
        _ => "No analysis available",
    };

    Ok(format!(
        "# Data Processing Report\n\n## Processed Data\n{}\n\n## Analysis\n{}",
        data, analysis_text
    ))
}
```

### Interactive Wizard

Multi-step user interaction with branching logic:

```rust
async fn setup_wizard(
    _req: CallToolRequest,
    ctx: RequestContext,
) -> Result<CallToolResult, ServerError> {
    let mut config = serde_json::Map::new();

    // Step 1: Select deployment type
    let mut step1_schema = ElicitationSchema::new();
    step1_schema.properties.insert(
        "deployment".to_string(),
        PrimitiveSchemaDefinition::String {
            title: Some("Deployment Type".to_string()),
            description: Some("Where will you deploy?".to_string()),
            format: None,
            min_length: None,
            max_length: None,
            enum_values: Some(vec![
                "local".to_string(),
                "cloud".to_string(),
                "hybrid".to_string(),
            ]),
            enum_names: Some(vec![
                "Local (Development)".to_string(),
                "Cloud (AWS/GCP/Azure)".to_string(),
                "Hybrid (Multi-cloud)".to_string(),
            ]),
        },
    );

    let step1_request = ElicitRequest {
        params: ElicitRequestParams::form(
            "Setup Wizard - Step 1/3".to_string(),
            step1_schema,
            Some(120000),
            Some(true),
        ),
        _meta: None,
        task: None,
    };

    let step1_result = ctx.elicit(step1_request).await?;

    if step1_result.action != ElicitationAction::Accept {
        return Ok(CallToolResult {
            content: vec![Content::Text(TextContent {
                text: "Setup cancelled".to_string(),
                annotations: None,
                meta: None,
            })],
            is_error: None,
            structured_content: None,
            _meta: None,
            task_id: None,
        });
    }

    let deployment = step1_result
        .content
        .as_ref()
        .and_then(|c| c.get("deployment"))
        .and_then(|v| v.as_str())
        .unwrap_or("local");

    config.insert("deployment".to_string(), serde_json::Value::String(deployment.to_string()));

    // Step 2: Configure based on deployment type
    let mut step2_schema = ElicitationSchema::new();

    if deployment == "cloud" {
        step2_schema.properties.insert(
            "provider".to_string(),
            PrimitiveSchemaDefinition::String {
                title: Some("Cloud Provider".to_string()),
                description: Some("Select cloud provider".to_string()),
                format: None,
                min_length: None,
                max_length: None,
                enum_values: Some(vec!["AWS".to_string(), "GCP".to_string(), "Azure".to_string()]),
                enum_names: None,
            },
        );
        step2_schema.properties.insert(
            "region".to_string(),
            PrimitiveSchemaDefinition::String {
                title: Some("Region".to_string()),
                description: Some("Deployment region".to_string()),
                format: None,
                min_length: Some(1),
                max_length: Some(50),
                enum_values: None,
                enum_names: None,
            },
        );
    } else {
        step2_schema.properties.insert(
            "port".to_string(),
            PrimitiveSchemaDefinition::Integer {
                title: Some("Port".to_string()),
                description: Some("Server port".to_string()),
                minimum: Some(1024),
                maximum: Some(65535),
            },
        );
    }

    let step2_request = ElicitRequest {
        params: ElicitRequestParams::form(
            "Setup Wizard - Step 2/3".to_string(),
            step2_schema,
            Some(120000),
            Some(true),
        ),
        _meta: None,
        task: None,
    };

    let step2_result = ctx.elicit(step2_request).await?;

    if step2_result.action != ElicitationAction::Accept {
        return Ok(CallToolResult {
            content: vec![Content::Text(TextContent {
                text: "Setup cancelled".to_string(),
                annotations: None,
                meta: None,
            })],
            is_error: None,
            structured_content: None,
            _meta: None,
            task_id: None,
        });
    }

    if let Some(content) = step2_result.content {
        for (k, v) in content {
            config.insert(k, v);
        }
    }

    // Step 3: Final confirmation
    let config_json = serde_json::to_string_pretty(&config).unwrap_or_default();

    let mut step3_schema = ElicitationSchema::new();
    step3_schema.properties.insert(
        "confirm".to_string(),
        PrimitiveSchemaDefinition::Boolean {
            title: Some("Confirm".to_string()),
            description: Some(format!("Apply this configuration?\n\n{}", config_json)),
        },
    );

    let step3_request = ElicitRequest {
        params: ElicitRequestParams::form(
            "Setup Wizard - Step 3/3".to_string(),
            step3_schema,
            Some(120000),
            Some(true),
        ),
        _meta: None,
        task: None,
    };

    let step3_result = ctx.elicit(step3_request).await?;

    if step3_result.action == ElicitationAction::Accept {
        let confirmed = step3_result
            .content
            .as_ref()
            .and_then(|c| c.get("confirm"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if confirmed {
            return Ok(CallToolResult {
                content: vec![Content::Text(TextContent {
                    text: format!("Configuration applied successfully:\n{}", config_json),
                    annotations: None,
                    meta: None,
                })],
                is_error: None,
                structured_content: None,
                _meta: None,
                task_id: None,
            });
        }
    }

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: "Setup cancelled".to_string(),
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        _meta: None,
        task_id: None,
    })
}
```

## Best Practices

### Error Handling in Complex Workflows

Always handle errors gracefully in multi-step workflows:

```rust
async fn resilient_workflow(
    ctx: RequestContext,
) -> Result<CallToolResult, ServerError> {
    // Step 1: Try operation with fallback
    let result1 = match risky_operation_1().await {
        Ok(data) => data,
        Err(e) => {
            // Log error but continue
            eprintln!("Step 1 failed: {}", e);
            "default_value".to_string()
        }
    };

    // Step 2: Retry on failure
    let result2 = retry_with_backoff(|| risky_operation_2(), 3).await?;

    // Step 3: Validate before proceeding
    if !validate_intermediate_result(&result2) {
        return Err(ServerError::Internal("Validation failed".to_string()));
    }

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: format!("Results: {} / {}", result1, result2),
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        _meta: None,
        task_id: None,
    })
}

async fn retry_with_backoff<F, T, E>(
    mut operation: F,
    max_retries: u32,
) -> Result<T, ServerError>
where
    F: FnMut() -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<T, E>>>>,
{
    let mut delay = std::time::Duration::from_millis(100);
    for attempt in 0..max_retries {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(_) if attempt < max_retries - 1 => {
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(_) => return Err(ServerError::Internal("Max retries exceeded".to_string())),
        }
    }
    unreachable!()
}
```

### Progress Tracking

Keep users informed during long-running operations:

```rust
// Use task IDs and progress updates
async fn long_operation_with_progress(
    ctx: RequestContext,
) -> Result<CallToolResult, ServerError> {
    let task_id = uuid::Uuid::new_v4().to_string();

    // Return task ID immediately
    // In real implementation, spawn background task and use notifications
    tokio::spawn(async move {
        // Perform work...
        // Send progress notifications to client
    });

    Ok(CallToolResult {
        content: vec![Content::Text(TextContent {
            text: format!("Operation started. Task ID: {}", task_id),
            annotations: None,
            meta: None,
        })],
        is_error: None,
        structured_content: None,
        task_id: Some(task_id),
    })
}
```

## See Also

- [Real-World Patterns](./patterns.md) - State management, caching, validation
- [Sampling Server Example](https://github.com/turbomcp/turbomcp/blob/main/crates/turbomcp/examples/sampling_server.rs) - Full sampling implementation
- [Elicitation Server Example](https://github.com/turbomcp/turbomcp/blob/main/crates/turbomcp/examples/elicitation_server.rs) - Full elicitation implementation
- [MCP Specification](https://spec.modelcontextprotocol.io/) - Protocol details
