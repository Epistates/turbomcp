//! Multi-Tenant SaaS Example
//!
//! Demonstrates how to build a multi-tenant SaaS application with TurboMCP.
//! This example shows:
//! - Tenant extraction from HTTP headers and subdomains
//! - Per-tenant configuration (rate limits, tool permissions)
//! - Tenant-scoped metrics tracking
//! - Accessing tenant context in tool handlers
//!
//! Run with:
//! ```bash
//! cargo run --example multi_tenant_saas --features http,multi-tenancy
//! ```
//!
//! Test with curl:
//! ```bash
//! # Using X-Tenant-ID header
//! curl -X POST http://localhost:3000 \
//!   -H "Content-Type: application/json" \
//!   -H "X-Tenant-ID: acme-corp" \
//!   -d '{"jsonrpc":"2.0","id":1,"method":"tools/list"}'
//!
//! # Using subdomain (configure DNS or hosts file)
//! curl -X POST http://acme-corp.localhost:3000 \
//!   -H "Content-Type: application/json" \
//!   -d '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"get_usage"}}'
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use turbomcp::{Context, McpResult, server, tool};
use turbomcp_server::TenantContextExt; // Extension trait for tenant methods
use turbomcp_server::config::multi_tenant::{
    StaticTenantConfigProvider, TenantConfig, TenantConfigProvider,
};
use turbomcp_server::metrics::multi_tenant::MultiTenantMetrics;
use turbomcp_server::middleware::tenancy::{
    CompositeTenantExtractor, HeaderTenantExtractor, SubdomainTenantExtractor,
    TenantExtractionLayer,
};

/// Example MCP server with multi-tenant support
#[derive(Clone)]
struct MultiTenantServer {
    /// Tenant configuration provider
    tenant_config: Arc<StaticTenantConfigProvider>,
    /// Multi-tenant metrics tracker
    metrics: Arc<MultiTenantMetrics>,
}

#[server(name = "multi-tenant-saas", version = "1.0.0")]
impl MultiTenantServer {
    /// Create a new multi-tenant server with configuration
    async fn new() -> McpResult<Self> {
        // Configure per-tenant settings
        let mut tenant_configs = HashMap::new();

        // Acme Corp: Higher limits, all tools enabled
        tenant_configs.insert(
            "acme-corp".to_string(),
            TenantConfig {
                rate_limit_per_second: Some(100),
                max_concurrent_requests: Some(50),
                tool_timeout_ms: Some(30000),
                enabled_tools: None, // All tools enabled
                disabled_tools: Some(HashSet::new()),
                max_request_body_size: Some(1024 * 1024), // 1MB
                is_active: Some(true),
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert(
                        "plan".to_string(),
                        serde_json::Value::String("enterprise".to_string()),
                    );
                    meta
                },
            },
        );

        // Startup Inc: Lower limits, restricted tools
        tenant_configs.insert(
            "startup-inc".to_string(),
            TenantConfig {
                rate_limit_per_second: Some(10),
                max_concurrent_requests: Some(5),
                tool_timeout_ms: Some(10000),
                enabled_tools: Some(vec!["get_usage".to_string()].into_iter().collect()),
                disabled_tools: Some(HashSet::new()),
                max_request_body_size: Some(256 * 1024), // 256KB
                is_active: Some(true),
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert(
                        "plan".to_string(),
                        serde_json::Value::String("starter".to_string()),
                    );
                    meta
                },
            },
        );

        // Demo tenant: Very low limits
        tenant_configs.insert(
            "demo".to_string(),
            TenantConfig {
                rate_limit_per_second: Some(5),
                max_concurrent_requests: Some(2),
                tool_timeout_ms: Some(5000),
                enabled_tools: Some(
                    vec!["hello".to_string(), "get_usage".to_string()]
                        .into_iter()
                        .collect(),
                ),
                disabled_tools: Some(HashSet::new()),
                max_request_body_size: Some(64 * 1024), // 64KB
                is_active: Some(true),
                metadata: {
                    let mut meta = HashMap::new();
                    meta.insert(
                        "plan".to_string(),
                        serde_json::Value::String("demo".to_string()),
                    );
                    meta.insert(
                        "expires".to_string(),
                        serde_json::Value::String("2025-12-31".to_string()),
                    );
                    meta
                },
            },
        );

        let tenant_config = Arc::new(StaticTenantConfigProvider::new(tenant_configs));

        // Initialize multi-tenant metrics with max 1000 tenants
        let metrics = Arc::new(MultiTenantMetrics::new(1000));

        Ok(Self {
            tenant_config,
            metrics,
        })
    }

    /// Simple hello tool that demonstrates tenant context access
    #[tool(
        description = "Greets the user with tenant-specific information",
        group = "greeting"
    )]
    async fn hello(&self, ctx: Context, name: String) -> McpResult<String> {
        // Access tenant ID from context
        let tenant_info = if let Some(tenant_id) = ctx.request.tenant() {
            // Get tenant configuration
            let config = self.tenant_config.get_config(tenant_id).await;

            // Record tenant metric
            self.metrics.record_request(tenant_id);

            let plan = config
                .as_ref()
                .and_then(|c| c.metadata.get("plan"))
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            format!(" (Tenant: {tenant_id}, Plan: {plan})")
        } else {
            " (No tenant)".to_string()
        };

        Ok(format!("Hello, {name}!{tenant_info}"))
    }

    /// Get tenant usage statistics
    #[tool(
        description = "Retrieves usage statistics for the current tenant",
        group = "monitoring"
    )]
    async fn get_usage(&self, ctx: Context) -> McpResult<serde_json::Value> {
        let tenant_id = ctx.request.require_tenant()?;

        // Record metric
        self.metrics.record_request(tenant_id);

        // Get tenant metrics
        let stats = self.metrics.get_tenant_metrics(tenant_id);

        // Get tenant configuration
        let config = self.tenant_config.get_config(tenant_id).await;

        let plan = config
            .as_ref()
            .and_then(|c| c.metadata.get("plan"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        let rate_limit = config.as_ref().and_then(|c| c.rate_limit_per_second);
        let max_concurrent = config.as_ref().and_then(|c| c.max_concurrent_requests);

        Ok(serde_json::json!({
            "tenant_id": tenant_id,
            "plan": plan,
            "requests": {
                "total": stats.as_ref().map(|s| s.requests_total()).unwrap_or(0),
                "successful": stats.as_ref().map(|s| s.requests_successful()).unwrap_or(0),
                "failed": stats.as_ref().map(|s| s.requests_failed()).unwrap_or(0),
            },
            "limits": {
                "rate_limit_per_second": rate_limit,
                "max_concurrent": max_concurrent,
            }
        }))
    }

    /// Expensive operation - only available to enterprise plans
    #[tool(
        description = "Performs expensive computation (enterprise plan only)",
        group = "advanced"
    )]
    async fn expensive_operation(&self, ctx: Context, data: String) -> McpResult<String> {
        let tenant_id = ctx.request.require_tenant()?;

        // Record metric
        self.metrics.record_request(tenant_id);

        // Check tenant plan
        let config = self.tenant_config.get_config(tenant_id).await;
        let plan = config
            .as_ref()
            .and_then(|c| c.metadata.get("plan"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string();

        if plan != "enterprise" {
            return Err(turbomcp::McpError::Tool(
                "This operation requires an enterprise plan".to_string(),
            ));
        }

        // Simulate expensive operation
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        Ok(format!(
            "Processed {} bytes of data (enterprise feature)",
            data.len()
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    tracing::info!("Starting multi-tenant MCP server...");

    // Create server instance
    let server = MultiTenantServer::new().await?;

    // Configure tenant extraction middleware
    // Try multiple strategies: header first, then subdomain
    let tenant_extractor = Arc::new(CompositeTenantExtractor::new(vec![
        Box::new(HeaderTenantExtractor::new("X-Tenant-ID")),
        Box::new(SubdomainTenantExtractor::new("localhost")),
    ]));

    // Build HTTP server with tenant extraction middleware
    tracing::info!("Configuring tenant extraction middleware...");
    tracing::info!("  - Header: X-Tenant-ID");
    tracing::info!("  - Subdomain: <tenant>.localhost:3000");

    // Build tenant extraction middleware layer
    let tenant_layer = TenantExtractionLayer::new(Arc::clone(&tenant_extractor));

    // Start HTTP server
    tracing::info!("Starting HTTP server on http://localhost:3000");
    tracing::info!("\nExample requests:");
    tracing::info!("  # Using header:");
    tracing::info!("  curl -X POST http://localhost:3000/mcp \\");
    tracing::info!("    -H 'Content-Type: application/json' \\");
    tracing::info!("    -H 'X-Tenant-ID: acme-corp' \\");
    tracing::info!(
        "    -d '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{{\"name\":\"hello\",\"arguments\":{{\"name\":\"World\"}}}}}}'"
    );
    tracing::info!("\n  # Using subdomain (configure /etc/hosts):");
    tracing::info!("  curl -X POST http://acme-corp.localhost:3000/mcp \\");
    tracing::info!("    -H 'Content-Type: application/json' \\");
    tracing::info!(
        "    -d '{{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{{\"name\":\"get_usage\"}}}}'"
    );
    tracing::info!("\nConfigured tenants:");
    tracing::info!("  - acme-corp (enterprise plan)");
    tracing::info!("  - startup-inc (starter plan)");
    tracing::info!("  - demo (demo plan - expires 2025-12-31)");
    tracing::info!(
        "\n✅ Multi-tenancy middleware is ACTIVE - X-Tenant-ID headers will be extracted!"
    );

    // Run HTTP server with tenant extraction middleware
    // This applies the middleware to the router, enabling automatic tenant extraction
    server
        .run_http_with_middleware(
            "localhost:3000",
            Box::new(move |router| router.layer(tenant_layer)),
        )
        .await?;

    Ok(())
}
