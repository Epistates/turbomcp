//! # Stateful Server - Shared State Management
//!
//! Demonstrates managing shared state across requests with Arc<RwLock<T>>.
//!
//! Run with: `cargo run --example stateful`

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

#[derive(Clone)]
struct CounterServer {
    /// Shared state: counters keyed by name
    counters: Arc<RwLock<HashMap<String, i64>>>,
}

#[turbomcp::server(name = "counter", version = "1.0.0")]
impl CounterServer {
    fn new() -> Self {
        Self {
            counters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[tool("Increment a counter by name")]
    async fn increment(&self, name: String) -> McpResult<i64> {
        let mut counters = self.counters.write().await;
        let counter = counters.entry(name).or_insert(0);
        *counter += 1;
        Ok(*counter)
    }

    #[tool("Get current counter value")]
    async fn get(&self, name: String) -> McpResult<i64> {
        let counters = self.counters.read().await;
        Ok(*counters.get(&name).unwrap_or(&0))
    }

    #[tool("Reset a counter")]
    async fn reset(&self, name: String) -> McpResult<String> {
        let mut counters = self.counters.write().await;
        counters.remove(&name);
        Ok(format!("Counter '{}' reset", name))
    }

    #[tool("List all counters")]
    async fn list(&self) -> McpResult<String> {
        let counters = self.counters.read().await;
        let list: Vec<String> = counters
            .iter()
            .map(|(k, v)| format!("{}: {}", k, v))
            .collect();
        Ok(list.join(", "))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing to stderr (MUST NOT write to stdout as it pollutes the MCP protocol)
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("Starting Stateful Counter Server...");

    CounterServer::new().run_stdio().await?;
    Ok(())
}
