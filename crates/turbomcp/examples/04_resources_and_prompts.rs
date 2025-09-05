//! # 04: Resources and Prompts - Serving Data and AI Integration
//!
//! **Learning Goals (15 minutes):**
//! - Create resource handlers to serve data
//! - Build prompt handlers for AI assistants
//! - Understand URI patterns for resources
//! - Learn prompt argument handling
//!
//! **Prerequisites:** 01_hello_world_macro.rs, 03_tools_and_parameters.rs
//!
//! **Run with:** `cargo run --example 04_resources_and_prompts`

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::{Context, McpResult, mcp_error, prompt, resource, server, tool};

/// Knowledge base server with resources and prompts
#[derive(Clone)]
struct KnowledgeServer {
    documents: Arc<RwLock<HashMap<String, String>>>,
    #[allow(dead_code)] // Used in resource handlers and templates
    templates: Arc<RwLock<HashMap<String, String>>>,
}

#[server(
    name = "KnowledgeBase",
    version = "1.0.0",
    description = "Tutorial 04: Resources and prompts for data serving",
    root = "file:///docs:Documentation",
    root = "file:///templates:Prompt Templates"
)]
impl KnowledgeServer {
    fn new() -> Self {
        let mut docs = HashMap::new();
        docs.insert(
            "readme".to_string(),
            "# TurboMCP\nHigh-performance MCP framework for building robust servers".to_string(),
        );
        docs.insert(
            "guide".to_string(),
            "## Getting Started\n1. Install TurboMCP\n2. Create server with macros\n3. Add tools, resources, and prompts\n4. Run with stdio transport!".to_string(),
        );
        docs.insert(
            "api".to_string(),
            "## API Reference\n### Macros\n- `#[server]` - Define MCP servers\n- `#[tool]` - Create tool handlers\n- `#[resource]` - Serve resources\n- `#[prompt]` - Handle prompts\n\n### Context API\n- `ctx.info()` - Log information\n- `ctx.set()` - Store data\n- `ctx.get()` - Retrieve data".to_string(),
        );

        let mut templates = HashMap::new();
        templates.insert(
            "code_review".to_string(),
            "You are a senior code reviewer. Review the following {language} code and provide constructive feedback:\n\n{code}\n\nFocus on:\n- Code quality and best practices\n- Performance implications\n- Security considerations\n- Maintainability".to_string(),
        );
        templates.insert(
            "documentation".to_string(),
            "Generate comprehensive documentation for the {project_type} project:\n\n{description}\n\nInclude:\n- Overview and purpose\n- Installation instructions\n- Usage examples\n- API reference\n- Contributing guidelines".to_string(),
        );

        Self {
            documents: Arc::new(RwLock::new(docs)),
            templates: Arc::new(RwLock::new(templates)),
        }
    }

    #[tool("Store a document in the knowledge base")]
    async fn store_document(
        &self,
        ctx: Context,
        name: String,
        content: String,
    ) -> McpResult<String> {
        ctx.info(&format!("Storing document: {}", name)).await?;

        if name.is_empty() {
            return Err(mcp_error!("Document name cannot be empty").into());
        }

        if content.len() > 10000 {
            ctx.warn("Large document being stored").await?;
        }

        let mut docs = self.documents.write().await;
        let was_update = docs.contains_key(&name);
        docs.insert(name.clone(), content.clone());

        // Store metadata in context
        ctx.set("document_name", &name).await?;
        ctx.set("content_length", content.len()).await?;
        ctx.set("was_update", was_update).await?;

        if was_update {
            Ok(format!(
                "Updated document: {} ({} chars)",
                name,
                content.len()
            ))
        } else {
            Ok(format!(
                "Created document: {} ({} chars)",
                name,
                content.len()
            ))
        }
    }

    #[tool("List all available documents")]
    async fn list_documents(&self, ctx: Context) -> McpResult<Vec<String>> {
        ctx.info("Listing all documents in knowledge base").await?;
        let docs = self.documents.read().await;
        let list: Vec<String> = docs.keys().cloned().collect();
        ctx.set("document_count", list.len()).await?;
        Ok(list)
    }

    #[resource("docs://list")]
    async fn resource_list_documents(&self, ctx: Context, _uri: String) -> McpResult<String> {
        ctx.info("Serving document list resource").await?;
        let docs = self.documents.read().await;
        let list: Vec<String> = docs.keys().cloned().collect();
        Ok(format!(
            "Available documents ({})::\n{}",
            list.len(),
            list.join("\n")
        ))
    }

    #[resource("docs://content/{name}")]
    async fn get_document(&self, ctx: Context, name: String) -> McpResult<String> {
        ctx.info(&format!("Serving document resource: {}", name))
            .await?;

        let docs = self.documents.read().await;
        match docs.get(&name) {
            Some(content) => {
                ctx.set("document_name", &name).await?;
                ctx.set("content_length", content.len()).await?;
                Ok(content.clone())
            }
            None => {
                ctx.warn(&format!("Document not found: {}", name)).await?;
                Err(mcp_error!("Document '{}' not found", name).into())
            }
        }
    }

    #[resource("docs://templates/{template}")]
    async fn get_template(&self, ctx: Context, template: String) -> McpResult<String> {
        ctx.info(&format!("Serving template resource: {}", template))
            .await?;

        let templates = self.templates.read().await;
        match templates.get(&template) {
            Some(content) => {
                ctx.set("template_name", &template).await?;
                Ok(content.clone())
            }
            None => {
                ctx.warn(&format!("Template not found: {}", template))
                    .await?;
                Err(mcp_error!("Template '{}' not found", template).into())
            }
        }
    }

    #[prompt("Generate documentation summary for {document}")]
    async fn summarize_docs(&self, ctx: Context, document: String) -> McpResult<String> {
        ctx.info(&format!(
            "Generating summary prompt for document: {}",
            document
        ))
        .await?;

        let docs = self.documents.read().await;
        let available_docs: Vec<String> = docs.keys().cloned().collect();

        ctx.set("target_document", &document).await?;
        ctx.set("available_docs", &available_docs).await?;

        if document == "all" {
            let doc_list = available_docs.join(", ");
            Ok(format!(
                "You are a technical documentation expert. Create a comprehensive summary of all documents in the knowledge base.\n\nAvailable documents: {}\n\nInstructions:\n- Review each document thoroughly\n- Extract key concepts and main points\n- Use clear bullet points for organization\n- Highlight important features and capabilities\n- Keep the summary concise but comprehensive\n- Group related information logically",
                doc_list
            ))
        } else if available_docs.contains(&document) {
            let content = docs.get(&document).unwrap();
            let preview = if content.len() > 200 {
                format!("{}...", &content[..200])
            } else {
                content.clone()
            };

            Ok(format!(
                "You are a technical documentation expert. Create a detailed summary of the '{}' document.\n\nDocument preview:\n{}\n\nInstructions:\n- Extract the main purpose and key points\n- Identify important concepts and features\n- Summarize in clear, organized bullet points\n- Include relevant examples if present\n- Focus on practical information",
                document, preview
            ))
        } else {
            Err(mcp_error!(
                "Document '{}' not found. Available: {}",
                document,
                available_docs.join(", ")
            )
            .into())
        }
    }

    #[prompt("Answer question about {topic} using documentation")]
    async fn answer_question(
        &self,
        ctx: Context,
        topic: String,
        question: String,
    ) -> McpResult<String> {
        ctx.info(&format!(
            "Generating Q&A prompt for topic '{}': {}",
            topic, question
        ))
        .await?;

        let docs = self.documents.read().await;
        let _templates = self.templates.read().await;

        ctx.set("topic", &topic).await?;
        ctx.set("question", &question).await?;

        // Try to find relevant documentation
        let mut relevant_content = Vec::new();

        for (doc_name, content) in docs.iter() {
            if doc_name.contains(&topic) || content.to_lowercase().contains(&topic.to_lowercase()) {
                relevant_content.push((doc_name, content));
            }
        }

        let context_info = if relevant_content.is_empty() {
            "No specific documentation found for this topic. Use general knowledge.".to_string()
        } else {
            let mut info = format!("Found {} relevant document(s):\n", relevant_content.len());
            for (doc_name, content) in &relevant_content {
                let preview = if content.len() > 300 {
                    format!("{}...", &content[..300])
                } else {
                    (*content).clone()
                };
                info.push_str(&format!("\n**{}:**\n{}\n", doc_name, preview));
            }
            info
        };

        Ok(format!(
            "You are a helpful technical assistant. Answer the following question about {}:\n\n**Question:** {}\n\n**Relevant Documentation:**\n{}\n\n**Instructions:**\n- Provide a clear, accurate answer based on the documentation\n- Include specific examples when helpful\n- If the documentation is insufficient, clearly state what information is missing\n- Structure your response with headings and bullet points for clarity\n- Be practical and actionable in your guidance",
            topic, question, context_info
        ))
    }

    #[prompt("Generate code review prompt using {language} template")]
    async fn code_review_prompt(
        &self,
        ctx: Context,
        language: String,
        code: String,
    ) -> McpResult<String> {
        ctx.info(&format!(
            "Generating code review prompt for {} code",
            language
        ))
        .await?;

        let templates = self.templates.read().await;
        let template = templates.get("code_review").unwrap();

        ctx.set("language", &language).await?;
        ctx.set("code_length", code.len()).await?;

        // Simple template substitution
        let prompt = template
            .replace("{language}", &language)
            .replace("{code}", &code);

        Ok(prompt)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt().with_env_filter("info").init();

    tracing::info!("📚 Starting Tutorial 04: Resources and Prompts");
    tracing::info!("This server demonstrates:");
    tracing::info!("  - Enhanced resource handlers with context logging");
    tracing::info!("  - Advanced prompt handlers with dynamic content");
    tracing::info!("  - URI pattern matching with parameter extraction");
    tracing::info!("  - Template-based prompt generation");
    tracing::info!("  - Filesystem roots configuration");
    tracing::info!("  - Context data storage and retrieval");

    let server = KnowledgeServer::new();

    // The run_stdio method is generated by the #[server] macro
    server.run_stdio().await?;

    Ok(())
}

// 🎯 **Try it out:**
//
//    Run the server:
//    cargo run --example 04_resources_and_prompts
//
//    Test Tools:
//    - store_document { "name": "tutorial", "content": "Complete guide to TurboMCP usage..." }
//    - list_documents {}
//
//    Test Resources:
//    - docs://list (via Resource access)
//    - docs://content/readme
//    - docs://content/guide
//    - docs://templates/code_review
//
//    Test Prompts:
//    - summarize_docs { "document": "guide" }
//    - summarize_docs { "document": "all" }
//    - answer_question { "topic": "turbomcp", "question": "How do I create tools?" }
//    - code_review_prompt { "language": "rust", "code": "fn main() { println!(\"Hello\"); }" }

/* 📝 **Key Concepts:**

**Enhanced Resources (1.0.3):**
- Serve data via URI patterns with context logging
- Support dynamic parameter extraction (e.g., {name})
- Context-aware error handling with ctx.warn() and ctx.error()
- Store metadata using ctx.set() for request correlation
- Return structured content with proper validation

**Advanced Prompts (1.0.3):**
- Template-based prompt generation with substitution
- Context-aware content retrieval and validation
- Dynamic prompt generation based on available data
- Parameterized prompts with {parameter} syntax
- Rich error messages with available options

**New Macro Features:**
- Context parameter injection in all handlers
- Enhanced error handling with mcp_error! macro
- Automatic parameter extraction from URIs
- Context data storage and retrieval
- Structured logging with different levels

**Filesystem Roots:**
- Configured directly in #[server] macro
- Multiple roots with descriptive names
- Foundation for file-system aware tools
- Enables secure file access patterns

**URI Patterns & Parameter Extraction:**
- Static: "docs://list"
- Dynamic: "docs://content/{name}" -> name parameter
- Multiple parameters: "docs://user/{user}/file/{file}"
- Automatic parameter extraction and validation

**Context Usage Patterns:**
- Request correlation: ctx.set("key", value)
- Logging: ctx.info(), ctx.warn(), ctx.error()
- Metadata tracking: Store operation details
- Error context: Include relevant state in errors

**Best Practices:**
- Use Context parameter in all new handlers
- Log operations for observability
- Store request metadata for correlation
- Validate parameters with helpful error messages
- Use mcp_error! for consistent error formatting
- Configure appropriate filesystem roots

**Production Benefits:**
- Better debugging with context correlation
- Enhanced monitoring and observability
- Structured error handling and recovery
- Request tracing across operations
- Performance insights via metadata

**Next Steps:**
- Continue to 05_error_handling.rs (now with enhanced Context usage)
- Learn about advanced error patterns with mcp_error!
- Explore state management in 06_stateful_server.rs
*/
