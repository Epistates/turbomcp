//! # 03: Macros vs Builders - Two Approaches to Building MCP Servers
//!
//! **Learning Goals (15 minutes):**
//! - Understand the two main APIs: Macro-based vs Builder-based
//! - See when to use each approach
//! - Compare code clarity and maintainability
//! - Learn the trade-offs between ergonomics and control
//!
//! **What this example demonstrates:**
//! - Side-by-side implementation of the same server using both APIs
//! - Macro API: High-level, ergonomic, attribute-driven
//! - Builder API: Low-level, explicit, programmatic control
//! - Performance and maintenance considerations
//!
//! **Run with:** `cargo run --example 03_macros_vs_builders --features macros`
//! **Or run builder version:** `cargo run --example 03_macros_vs_builders`

use std::collections::HashMap;
use turbomcp_protocol::types::{
    CallToolRequest, CallToolResult, Content, TextContent, Tool, ToolInputSchema,
};
use turbomcp_server::{
    ServerBuilder,
    handlers::{FunctionPromptHandler, FunctionResourceHandler, FunctionToolHandler},
};

// Choose which implementation to use - defaulting to builder implementation
mod implementation {
    // Implementation module - imports handled per approach
}

// =============================================================================
// APPROACH 1: MACRO API - Clean and Declarative
// =============================================================================

#[allow(unused)]
mod macro_implementation {
    use super::*;
    use std::sync::Arc;
    use turbomcp::{McpResult, prompt, resource, server, tool};

    /// Text processing server using the ergonomic macro API
    ///
    /// **Advantages:**
    /// - Clean, declarative syntax
    /// - Automatic parameter parsing and validation
    /// - Built-in error handling
    /// - Type-safe parameter extraction
    /// - Automatic schema generation
    #[derive(Clone)]
    struct TextProcessorMacro {
        stats: Arc<std::sync::Mutex<HashMap<String, usize>>>,
    }

    #[server(
        name = "TextProcessorMacro",
        version = "1.0.0",
        description = "Text processing with macro API"
    )]
    impl TextProcessorMacro {
        fn new() -> Self {
            Self {
                stats: Arc::new(std::sync::Mutex::new(HashMap::new())),
            }
        }

        #[tool("Convert text to UPPERCASE")]
        async fn uppercase(&self, text: String) -> McpResult<String> {
            self.record_usage("uppercase");
            Ok(text.to_uppercase())
        }

        #[tool("Convert text to lowercase")]
        async fn lowercase(&self, text: String) -> McpResult<String> {
            self.record_usage("lowercase");
            Ok(text.to_lowercase())
        }

        #[tool("Reverse the order of characters")]
        async fn reverse(&self, text: String) -> McpResult<String> {
            self.record_usage("reverse");
            Ok(text.chars().rev().collect())
        }

        #[tool("Count words in text")]
        async fn word_count(&self, text: String) -> McpResult<usize> {
            self.record_usage("word_count");
            Ok(text.split_whitespace().count())
        }

        #[tool("Get character frequency")]
        async fn char_frequency(&self, text: String) -> McpResult<HashMap<char, usize>> {
            self.record_usage("char_frequency");
            let mut freq = HashMap::new();
            for ch in text.chars() {
                *freq.entry(ch).or_insert(0) += 1;
            }
            Ok(freq)
        }

        #[resource("stats://usage")]
        async fn usage_stats(&self, _uri: String) -> McpResult<String> {
            let stats = self.stats.lock().unwrap();
            let output: Vec<String> = stats
                .iter()
                .map(|(k, v)| format!("{k}: {v} calls"))
                .collect();
            Ok(output.join(
                "
",
            ))
        }

        #[prompt("Generate text analysis prompt")]
        async fn analysis_prompt(
            &self,
            _ctx: turbomcp::RequestContext,
            args: Option<serde_json::Value>,
        ) -> Result<String, String> {
            let text = args
                .as_ref()
                .and_then(|v| v.get("text"))
                .and_then(|v| v.as_str())
                .unwrap_or("sample text");

            Ok(format!(
                "Analyze the following text:

'{text}'

Provide:
1. Word count
2. Character count
3. Most frequent character
4. Sentiment analysis"
            ))
        }

        fn record_usage(&self, operation: &str) {
            let mut stats = self.stats.lock().unwrap();
            *stats.entry(operation.to_string()).or_insert(0) += 1;
        }
    }

    pub async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("🎯 Running with MACRO API - Clean and declarative");

        let server = TextProcessorMacro::new();
        server.run_stdio().await?;

        Ok(())
    }
}

// =============================================================================
// APPROACH 2: BUILDER API - Explicit and Flexible
// =============================================================================

mod builder_implementation {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex;

    /// Create the same text processing server using the Builder API
    ///
    /// **Builder Approach:**
    /// - Complete Tool structs with proper schemas
    /// - FunctionToolHandler::new() with full definitions
    /// - Manual parameter extraction with type safety
    /// - Full control over registration and configuration
    ///
    /// **Advantages:**
    /// - Full control over parameter parsing
    /// - Dynamic tool registration
    /// - Custom error handling logic
    /// - Runtime tool configuration
    /// - Integration with existing systems
    #[allow(dead_code)]
    pub async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
        tracing::info!("🔧 Running with BUILDER API - Explicit control");

        // Shared state for statistics
        let stats = Arc::new(Mutex::new(HashMap::<String, usize>::new()));

        // Build server with explicit tool registration
        let mut builder = ServerBuilder::new()
            .name("TextProcessorBuilder")
            .version("1.0.0")
            .description("Text processing with Builder API");

        // Create uppercase tool with complete schema
        {
            let stats = Arc::clone(&stats);
            let tool = Tool {
                name: "uppercase".to_string(),
                title: Some("Uppercase".to_string()),
                description: Some("Convert text to UPPERCASE".to_string()),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some({
                        let mut props = HashMap::new();
                        props.insert(
                            "text".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "The text to convert to uppercase"
                            }),
                        );
                        props
                    }),
                    required: Some(vec!["text".to_string()]),
                    additional_properties: Some(false),
                },
                output_schema: None,
                annotations: None,
                meta: None,
            };

            let handler = FunctionToolHandler::new(tool, move |req: CallToolRequest, _ctx| {
                let stats = Arc::clone(&stats);
                async move {
                    // Parameter extraction
                    let text = req
                        .arguments
                        .as_ref()
                        .and_then(|args| args.get("text"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            turbomcp_server::ServerError::handler(
                                "Missing required parameter: text",
                            )
                        })?;

                    // Record usage with proper error handling
                    {
                        let mut s = stats.lock().unwrap();
                        *s.entry("uppercase".to_string()).or_insert(0) += 1;
                    }

                    // Process and return result
                    let result = text.to_uppercase();
                    Ok(CallToolResult {
                        content: vec![Content::Text(TextContent {
                            text: result,
                            annotations: None,
                            meta: None,
                        })],
                        is_error: None,
                    })
                }
            });

            builder = builder.tool("uppercase", handler)?;
        }

        // Create lowercase tool with complete schema
        {
            let stats = Arc::clone(&stats);
            let tool = Tool {
                name: "lowercase".to_string(),
                title: Some("Lowercase".to_string()),
                description: Some("Convert text to lowercase".to_string()),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some({
                        let mut props = HashMap::new();
                        props.insert(
                            "text".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "The text to convert to lowercase"
                            }),
                        );
                        props
                    }),
                    required: Some(vec!["text".to_string()]),
                    additional_properties: Some(false),
                },
                output_schema: None,
                annotations: None,
                meta: None,
            };

            let handler = FunctionToolHandler::new(tool, move |req: CallToolRequest, _ctx| {
                let stats = Arc::clone(&stats);
                async move {
                    let text = req
                        .arguments
                        .as_ref()
                        .and_then(|args| args.get("text"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            turbomcp_server::ServerError::handler(
                                "Missing required parameter: text",
                            )
                        })?;

                    {
                        let mut s = stats.lock().unwrap();
                        *s.entry("lowercase".to_string()).or_insert(0) += 1;
                    }

                    let result = text.to_lowercase();
                    Ok(CallToolResult {
                        content: vec![Content::Text(TextContent {
                            text: result,
                            annotations: None,
                            meta: None,
                        })],
                        is_error: None,
                    })
                }
            });

            builder = builder.tool("lowercase", handler)?;
        }

        // Create reverse tool with complete schema
        {
            let stats = Arc::clone(&stats);
            let tool = Tool {
                name: "reverse".to_string(),
                title: Some("Reverse".to_string()),
                description: Some("Reverse the order of characters".to_string()),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some({
                        let mut props = HashMap::new();
                        props.insert(
                            "text".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "The text to reverse"
                            }),
                        );
                        props
                    }),
                    required: Some(vec!["text".to_string()]),
                    additional_properties: Some(false),
                },
                output_schema: None,
                annotations: None,
                meta: None,
            };

            let handler = FunctionToolHandler::new(tool, move |req: CallToolRequest, _ctx| {
                let stats = Arc::clone(&stats);
                async move {
                    let text = req
                        .arguments
                        .as_ref()
                        .and_then(|args| args.get("text"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            turbomcp_server::ServerError::handler(
                                "Missing required parameter: text",
                            )
                        })?;

                    {
                        let mut s = stats.lock().unwrap();
                        *s.entry("reverse".to_string()).or_insert(0) += 1;
                    }

                    let result: String = text.chars().rev().collect();
                    Ok(CallToolResult {
                        content: vec![Content::Text(TextContent {
                            text: result,
                            annotations: None,
                            meta: None,
                        })],
                        is_error: None,
                    })
                }
            });

            builder = builder.tool("reverse", handler)?;
        }

        // Create word_count tool with complete schema
        {
            let stats = Arc::clone(&stats);
            let tool = Tool {
                name: "word_count".to_string(),
                title: Some("Word Count".to_string()),
                description: Some("Count words in text".to_string()),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some({
                        let mut props = HashMap::new();
                        props.insert(
                            "text".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "The text to count words in"
                            }),
                        );
                        props
                    }),
                    required: Some(vec!["text".to_string()]),
                    additional_properties: Some(false),
                },
                output_schema: None,
                annotations: None,
                meta: None,
            };

            let handler = FunctionToolHandler::new(tool, move |req: CallToolRequest, _ctx| {
                let stats = Arc::clone(&stats);
                async move {
                    let text = req
                        .arguments
                        .as_ref()
                        .and_then(|args| args.get("text"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            turbomcp_server::ServerError::handler(
                                "Missing required parameter: text",
                            )
                        })?;

                    {
                        let mut s = stats.lock().unwrap();
                        *s.entry("word_count".to_string()).or_insert(0) += 1;
                    }

                    let word_count = text.split_whitespace().count();
                    Ok(CallToolResult {
                        content: vec![Content::Text(TextContent {
                            text: word_count.to_string(),
                            annotations: None,
                            meta: None,
                        })],
                        is_error: None,
                    })
                }
            });

            builder = builder.tool("word_count", handler)?;
        }

        // Create char_frequency tool with complete schema
        {
            let stats = Arc::clone(&stats);
            let tool = Tool {
                name: "char_frequency".to_string(),
                title: Some("Character Frequency".to_string()),
                description: Some("Get character frequency".to_string()),
                input_schema: ToolInputSchema {
                    schema_type: "object".to_string(),
                    properties: Some({
                        let mut props = HashMap::new();
                        props.insert(
                            "text".to_string(),
                            serde_json::json!({
                                "type": "string",
                                "description": "The text to analyze character frequency"
                            }),
                        );
                        props
                    }),
                    required: Some(vec!["text".to_string()]),
                    additional_properties: Some(false),
                },
                output_schema: None,
                annotations: None,
                meta: None,
            };

            let handler = FunctionToolHandler::new(tool, move |req: CallToolRequest, _ctx| {
                let stats = Arc::clone(&stats);
                async move {
                    let text = req
                        .arguments
                        .as_ref()
                        .and_then(|args| args.get("text"))
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            turbomcp_server::ServerError::handler(
                                "Missing required parameter: text",
                            )
                        })?;

                    {
                        let mut s = stats.lock().unwrap();
                        *s.entry("char_frequency".to_string()).or_insert(0) += 1;
                    }

                    let mut freq = HashMap::new();
                    for ch in text.chars() {
                        *freq.entry(ch).or_insert(0) += 1;
                    }
                    let result = serde_json::to_string(&freq).unwrap_or_else(|_| "{}".to_string());
                    Ok(CallToolResult {
                        content: vec![Content::Text(TextContent {
                            text: result,
                            annotations: None,
                            meta: None,
                        })],
                        is_error: None,
                    })
                }
            });

            builder = builder.tool("char_frequency", handler)?;
        }

        // Add resource handler for usage statistics
        {
            let stats = Arc::clone(&stats);
            use turbomcp_protocol::types::{ReadResourceRequest, ReadResourceResult, Resource};

            let resource = Resource {
                name: "Usage Stats".to_string(),
                title: Some("Usage Statistics".to_string()),
                uri: "stats://usage".to_string(),
                description: Some("Get usage statistics for all tools".to_string()),
                mime_type: Some("text/plain".to_string()),
                annotations: None,
                size: None,
                meta: None,
            };

            let handler =
                FunctionResourceHandler::new(resource, move |req: ReadResourceRequest, _ctx| {
                    let stats = Arc::clone(&stats);
                    async move {
                        if req.uri != "stats://usage" {
                            return Err(turbomcp_server::ServerError::handler(format!(
                                "Resource not found: {}",
                                req.uri
                            )));
                        }

                        let stats_guard = stats.lock().unwrap();
                        let output: Vec<String> = stats_guard
                            .iter()
                            .map(|(k, v)| format!("{k}: {v} calls"))
                            .collect();
                        let stats_text = output.join(
                            "
",
                        );

                        use turbomcp_protocol::types::{ResourceContent, TextResourceContents};
                        Ok(ReadResourceResult {
                            contents: vec![ResourceContent::Text(TextResourceContents {
                                uri: req.uri.clone(),
                                mime_type: Some("text/plain".to_string()),
                                text: stats_text,
                                meta: None,
                            })],
                        })
                    }
                });

            builder = builder.resource("stats://usage", handler)?;
        }

        // Add prompt handler for analysis prompt
        {
            use turbomcp_protocol::types::{GetPromptRequest, GetPromptResult, Prompt};

            let prompt = Prompt {
                name: "analysis_prompt".to_string(),
                title: Some("Generate Analysis Prompt".to_string()),
                description: Some("Generate text analysis prompt".to_string()),
                arguments: Some(vec![turbomcp_protocol::types::PromptArgument {
                    name: "text".to_string(),
                    title: Some("Text".to_string()),
                    description: Some("Text to analyze".to_string()),
                    required: Some(false),
                }]),
                meta: None,
            };

            let handler =
                FunctionPromptHandler::new(prompt, move |req: GetPromptRequest, _ctx| async move {
                    let text = req
                        .arguments
                        .as_ref()
                        .and_then(|args| args.get("text"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("sample text");

                    let prompt_text = format!(
                        "Analyze the following text:

'{text}'

Provide:
1. Word count
2. Character count
3. Most frequent character
4. Sentiment analysis"
                    );

                    Ok(GetPromptResult {
                        description: Some("Text analysis prompt".to_string()),
                        messages: vec![turbomcp_protocol::types::PromptMessage {
                            role: turbomcp_protocol::types::Role::User,
                            content: Content::Text(TextContent {
                                text: prompt_text,
                                annotations: None,
                                meta: None,
                            }),
                        }],
                    })
                });

            builder = builder.prompt("analysis_prompt", handler)?;
        }

        // Build and run the server
        let server = builder.build();
        server.run_stdio().await?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("info").init();

    tracing::info!("🚀 Starting Text Processor Server");
    tracing::info!("This example demonstrates both Macro and Builder APIs");

    // Check if we should use macros (could be from env var, feature flag, etc.)
    let _use_macros = std::env::var("USE_MACROS")
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    // Switch between implementations based on environment variable
    if _use_macros {
        tracing::info!("🎯 Running with MACRO API - Clean and declarative");
        macro_implementation::run_server().await?
    } else {
        tracing::info!("🔧 Running with BUILDER API - Explicit control");
        builder_implementation::run_server().await?
    }

    Ok(())
}

// 🎯 **Try it out:**
//
//    Run with Builder API (default):
//    cargo run --example 03_macros_vs_builders
//
//    Run with Macro API:
//    USE_MACROS=1 cargo run --example 03_macros_vs_builders
//
//    Compare the two implementations to see the trade-offs!

/* 📝 **Comparison Summary:**

**Macro API (Recommended for most cases):**
✅ Pros:
- Clean, declarative syntax
- Automatic parameter extraction
- Type-safe with compile-time checks
- Less boilerplate code
- Automatic schema generation
- Easier to maintain

❌ Cons:
- Less runtime flexibility
- Harder to debug macro expansions
- Fixed registration pattern

**Builder API (For advanced use cases):**
✅ Pros:
- Full control over registration
- Runtime configuration possible
- Custom error handling
- Dynamic tool addition
- Easier to integrate with existing code

❌ Cons:
- More verbose
- Manual parameter extraction
- More error-prone
- Requires deeper API knowledge

**When to use each:**
- Macros: 90% of use cases, standard servers
- Builder: Dynamic tools, runtime config, special requirements

**Next Example:** `04_comprehensive_server.rs` - A full-featured server implementation
*/
