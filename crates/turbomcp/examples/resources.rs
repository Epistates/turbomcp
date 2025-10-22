//! # Resources - Serving Data via URIs
//!
//! Demonstrates how to create resource handlers for serving data.
//!
//! Run with: `cargo run --example resources`

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

#[derive(Clone)]
struct DocsServer {
    documents: Arc<RwLock<HashMap<String, String>>>,
}

#[turbomcp::server(name = "docs", version = "1.0.0", root = "file:///docs:Documentation", transports = ["stdio"])]
impl DocsServer {
    fn new() -> Self {
        let mut docs = HashMap::new();
        docs.insert(
            "readme".to_string(),
            "# TurboMCP\nFast MCP framework".to_string(),
        );
        docs.insert(
            "guide".to_string(),
            "## Getting Started\n1. Install\n2. Code\n3. Run!".to_string(),
        );

        Self {
            documents: Arc::new(RwLock::new(docs)),
        }
    }

    /// List all available documents
    #[resource("docs://list")]
    async fn list_docs(&self) -> McpResult<String> {
        let docs = self.documents.read().await;
        let names: Vec<String> = docs.keys().cloned().collect();
        Ok(format!("Available docs:\n{}", names.join("\n")))
    }

    /// Get a specific document by name
    #[resource("docs://{name}")]
    async fn get_doc(&self, name: String) -> McpResult<String> {
        let docs = self.documents.read().await;
        docs.get(&name)
            .cloned()
            .ok_or_else(|| McpError::protocol(format!("Document '{}' not found", name)))
    }

    /// Tool to add new documents
    #[tool("Add a new document")]
    async fn add_doc(&self, name: String, content: String) -> McpResult<String> {
        let mut docs = self.documents.write().await;
        docs.insert(name.clone(), content);
        Ok(format!("Added document: {}", name))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    DocsServer::new().run_stdio().await?;
    Ok(())
}
