//! # Resource Links Example
//!
//! Demonstrates proper usage of ResourceLink content blocks with complete metadata.
//! Shows how to provide rich context to LLMs about resources through descriptions,
//! MIME types, sizes, and annotations.
//!
//! **Key Point:** The MCP specification explicitly states that `description` is
//! "used by clients to improve the LLM's understanding of available resources"
//! and should be treated as "a hint to the model".
//!
//! Run with: `cargo run --example resource_links`

use turbomcp::prelude::*;
use turbomcp_protocol::types::{
    Annotations, CallToolResult, ContentBlock, ResourceLink, TextContent,
};

#[derive(Clone)]
struct DocumentServer;

#[turbomcp::server(
    name = "resource-links-demo",
    version = "1.0.0",
    description = "Demonstrates proper ResourceLink usage with rich metadata",
    transports = ["stdio"]
)]
impl DocumentServer {
    /// Search for documents and return resource links with complete metadata
    ///
    /// This demonstrates the MCP best practice: always populate description,
    /// mimeType, and size fields to help LLMs understand resource context.
    #[tool("Search for documents by keyword")]
    async fn search_documents(&self, query: String) -> McpResult<CallToolResult> {
        // Simulate document search results
        #[allow(clippy::useless_vec)] // Vec is clearer for extensible data
        let documents = vec![
            ResourceLink {
                name: "api-documentation.md".to_string(),
                title: Some("API Documentation".to_string()),
                uri: "file:///docs/api/README.md".to_string(),
                // IMPORTANT: Description helps LLMs understand when to use this resource
                description: Some(format!(
                    "Complete API reference for the REST endpoints. \
                    Matches query '{}'. Contains authentication details, \
                    endpoint specifications, and example requests.",
                    query
                )),
                // MIME type helps LLMs understand file format
                mime_type: Some("text/markdown".to_string()),
                // Size helps LLMs estimate context window usage
                size: Some(45_312), // bytes
                // Optional: lastModified is useful for cache invalidation
                annotations: Some(Annotations {
                    last_modified: Some("2025-11-05T14:30:00Z".to_string()),
                    ..Default::default()
                }),
                meta: None,
            },
            ResourceLink {
                name: "user-guide.pdf".to_string(),
                title: Some("User Guide".to_string()),
                uri: "file:///docs/guides/user-guide.pdf".to_string(),
                description: Some(format!(
                    "End-user documentation and tutorials. \
                    Contains information about '{}'. \
                    Suitable for beginners and includes screenshots.",
                    query
                )),
                mime_type: Some("application/pdf".to_string()),
                size: Some(2_450_000), // 2.45 MB
                annotations: Some(Annotations {
                    last_modified: Some("2025-11-01T09:00:00Z".to_string()),
                    ..Default::default()
                }),
                meta: None,
            },
            ResourceLink {
                name: "config-example.json".to_string(),
                title: Some("Configuration Example".to_string()),
                uri: "file:///examples/config/default.json".to_string(),
                description: Some(
                    "Example configuration file showing all available options. \
                    Use this as a template for setting up your environment."
                        .to_string(),
                ),
                mime_type: Some("application/json".to_string()),
                size: Some(1_024), // 1 KB
                annotations: None,
                meta: None,
            },
        ];

        // Create summary text for backward compatibility
        let summary = format!(
            "Found {} documents matching '{}':\n{}",
            documents.len(),
            query,
            documents
                .iter()
                .map(|doc| {
                    let size_kb = doc.size.map(|s| s / 1024).unwrap_or(0);
                    format!(
                        "- {} ({} KB) - {}",
                        doc.title.as_ref().unwrap_or(&doc.name),
                        size_kb,
                        doc.description.as_deref().unwrap_or("No description")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        );

        Ok(CallToolResult {
            content: vec![
                // Text summary for backward compatibility
                ContentBlock::Text(TextContent {
                    text: summary,
                    annotations: None,
                    meta: None,
                }),
                // Resource links with complete metadata
                ContentBlock::ResourceLink(documents[0].clone()),
                ContentBlock::ResourceLink(documents[1].clone()),
                ContentBlock::ResourceLink(documents[2].clone()),
            ],
            is_error: Some(false),
            structured_content: None,
            _meta: Some(serde_json::json!({
                "query": query,
                "total_results": 3,
                "search_time_ms": 15
            })),
        })
    }

    /// Get detailed information about a specific resource
    #[tool("Get metadata for a specific document")]
    async fn get_document_info(&self, uri: String) -> McpResult<CallToolResult> {
        // Simulate document metadata lookup
        let resource = ResourceLink {
            name: "database-schema.sql".to_string(),
            title: Some("Database Schema Definition".to_string()),
            uri: uri.clone(),
            // Detailed description helps LLMs understand the resource's purpose
            description: Some(
                "PostgreSQL schema definition for the production database. \
                Includes table definitions, indexes, constraints, and migrations. \
                Use this to understand data relationships and query structure."
                    .to_string(),
            ),
            mime_type: Some("application/sql".to_string()),
            size: Some(128_000), // 128 KB
            annotations: Some(Annotations {
                last_modified: Some("2025-11-06T08:15:00Z".to_string()),
                ..Default::default()
            }),
            meta: {
                let mut map = std::collections::HashMap::new();
                map.insert(
                    "checksum".to_string(),
                    serde_json::json!("sha256:abc123..."),
                );
                map.insert("encoding".to_string(), serde_json::json!("utf-8"));
                Some(map)
            },
        };

        // Format detailed text description
        let text = format!(
            "Document Information:\n\
            Name: {}\n\
            Title: {}\n\
            URI: {}\n\
            Type: {}\n\
            Size: {} KB\n\
            Last Modified: {}\n\n\
            Description: {}",
            resource.name,
            resource.title.as_deref().unwrap_or("N/A"),
            resource.uri,
            resource.mime_type.as_deref().unwrap_or("unknown"),
            resource.size.map(|s| s / 1024).unwrap_or(0),
            resource
                .annotations
                .as_ref()
                .and_then(|a| a.last_modified.as_deref())
                .unwrap_or("unknown"),
            resource.description.as_deref().unwrap_or("No description")
        );

        Ok(CallToolResult {
            content: vec![
                ContentBlock::Text(TextContent {
                    text,
                    annotations: None,
                    meta: None,
                }),
                ContentBlock::ResourceLink(resource),
            ],
            is_error: Some(false),
            structured_content: None,
            _meta: None,
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("Resource Links Example Server");
    println!("==============================\n");
    println!("This server demonstrates MCP ResourceLink best practices:\n");
    println!("1. Always populate 'description' - it helps LLMs understand resources");
    println!("2. Include 'mimeType' to indicate file format and content type");
    println!("3. Provide 'size' to help LLMs estimate context window usage");
    println!("4. Optionally include 'lastModified' for cache invalidation\n");
    println!("⚠️  Common Mistake to Avoid:");
    println!("   DO NOT format as: [Resource: name (uri)]");
    println!("   This loses critical context that LLMs need!\n");
    println!("✅  Correct Approach:");
    println!("   Provide complete ResourceLink objects with all metadata fields.");
    println!("   The description field is explicitly designed for LLM understanding.\n");
    println!("Available tools:");
    println!("  - search_documents: Find documents with rich metadata");
    println!("  - get_document_info: Get detailed resource information\n");
    println!("Client Usage Example:");
    println!("---------------------");
    println!("```rust");
    println!("let result = client.call_tool(\"search_documents\", args).await?;");
    println!();
    println!("for content in &result.content {{");
    println!("    if let ContentBlock::ResourceLink(link) = content {{");
    println!("        // All fields available!");
    println!("        println!(\"Resource: {{}}\", link.name);");
    println!(
        "        println!(\"Description: {{}}\", link.description.as_deref().unwrap_or(\"\"));"
    );
    println!("        println!(\"Type: {{}}\", link.mime_type.as_deref().unwrap_or(\"unknown\"));");
    println!("        println!(\"Size: {{}} bytes\", link.size.unwrap_or(0));");
    println!("    }}");
    println!("}}");
    println!("```\n");

    DocumentServer.run_stdio().await?;
    Ok(())
}
