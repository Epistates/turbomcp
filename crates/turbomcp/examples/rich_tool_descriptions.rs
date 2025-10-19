//! # Rich Tool Descriptions Example
//!
//! Demonstrates the new descriptive tool macro attributes for improved LLM understanding.
//!
//! The `#[tool]` macro now supports multiple metadata fields that are combined into
//! a single pipe-delimited description for MCP compliance:
//! - description: Primary tool description
//! - usage: When/why to use this tool
//! - performance: Expected performance characteristics
//! - related: Related/complementary tools
//! - examples: Common usage examples
//!
//! Run with: `cargo run --example rich_tool_descriptions`

use turbomcp::prelude::*;

#[derive(Clone)]
struct DataAnalysisServer;

#[turbomcp::server(
    name = "data-analysis",
    version = "1.0.0",
    description = "Server demonstrating rich tool descriptions"
)]
impl DataAnalysisServer {
    /// Simple tool with basic description (backward compatible)
    #[tool("Calculate the sum of two numbers")]
    async fn add(&self, a: f64, b: f64) -> McpResult<f64> {
        Ok(a + b)
    }

    /// Tool with rich metadata for better LLM understanding
    #[tool(
        description = "Search data records by pattern matching",
        usage = "Use this when you need to find specific records before bulk operations",
        performance = "Fast on small datasets (<1000 records), <50ms typical",
        related = ["batch_process", "export_results"],
        examples = ["name contains 'John'", "created_at > '2024-01-01'", "status = 'active'"]
    )]
    async fn search_records(&self, pattern: String) -> McpResult<Vec<String>> {
        // Simulate search logic
        let results = vec![
            format!("Record matching: {}", pattern),
            format!("Another match for: {}", pattern),
        ];
        Ok(results)
    }

    /// Another rich tool showing performance-critical operation
    #[tool(
        description = "Process multiple records in a single batch operation",
        usage = "Efficient for bulk operations after using search_records to identify targets",
        performance = "100-500ms for batches of 100 records, scales linearly",
        related = ["search_records", "export_results"],
        examples = ["process ids [1,2,3]", "apply transformation to batch"]
    )]
    async fn batch_process(&self, record_ids: Vec<u64>) -> McpResult<String> {
        Ok(format!("Processed {} records in batch", record_ids.len()))
    }

    /// Export tool with file format examples
    #[tool(
        description = "Export search results to various file formats",
        usage = "Use after search_records or batch_process to save results",
        performance = "Fast for small datasets, may take 1-2s for large exports (>10k records)",
        related = ["search_records", "batch_process"],
        examples = ["format: csv", "format: json", "format: xlsx"]
    )]
    async fn export_results(&self, format: String, data: Vec<String>) -> McpResult<String> {
        Ok(format!(
            "Exported {} records to {} format",
            data.len(),
            format
        ))
    }

    /// Analytics tool showing computation complexity
    #[tool(
        description = "Calculate statistical metrics on dataset",
        usage = "Run statistical analysis on search results or entire dataset",
        performance = "O(n) complexity, ~1ms per 1000 records",
        related = ["search_records"],
        examples = ["calculate mean and median", "show distribution", "find outliers"]
    )]
    async fn calculate_stats(&self, data: Vec<f64>) -> McpResult<String> {
        let count = data.len();
        let sum: f64 = data.iter().sum();
        let mean = if count > 0 { sum / count as f64 } else { 0.0 };

        Ok(format!(
            "Stats: count={}, sum={:.2}, mean={:.2}",
            count, sum, mean
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to see the generated descriptions
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("Starting Data Analysis Server with Rich Tool Descriptions");
    println!("=========================================================");
    println!();
    println!("This example demonstrates the new descriptive tool macro attributes.");
    println!("Each tool includes:");
    println!("  - Primary description");
    println!("  - Usage context (when to use)");
    println!("  - Performance characteristics");
    println!("  - Related tools");
    println!("  - Usage examples");
    println!();
    println!("These are combined into pipe-delimited descriptions for MCP compliance.");
    println!();
    println!("Connect via Claude Desktop or use turbomcp-cli to see the rich descriptions:");
    println!("  turbomcp-cli tools list --command ./target/debug/examples/rich_tool_descriptions");
    println!();

    DataAnalysisServer.run_stdio().await?;
    Ok(())
}
