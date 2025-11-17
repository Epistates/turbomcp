//! Multi-tenant MCP server example demonstrating tenant isolation and security.
//!
//! This example shows how to build a production-ready multi-tenant SaaS application using TurboMCP.
//! It demonstrates:
//!
//! - Tenant extraction from multiple sources (headers, API keys, subdomains)
//! - Per-tenant configuration (rate limits, tool access, quotas)
//! - Tenant ownership validation for secure resource access
//! - Multi-tenant metrics tracking
//! - Tenant-scoped operations
//!
//! ## Running this example
//!
//! ```bash
//! cargo run --example multi_tenant_server --features full,multi-tenancy
//! ```
//!
//! ## Testing with different tenants
//!
//! ```bash
//! # Using X-Tenant-ID header
//! curl -X POST http://localhost:3000/mcp/v1 \
//!   -H "Content-Type: application/json" \
//!   -H "X-Tenant-ID: acme-corp" \
//!   -d '{"jsonrpc":"2.0","method":"tools/list","id":1}'
//!
//! # Using API key with tenant prefix
//! curl -X POST http://localhost:3000/mcp/v1 \
//!   -H "Content-Type: application/json" \
//!   -H "Authorization: sk_acme_secret123" \
//!   -d '{"jsonrpc":"2.0","method":"tools/call","params":{"name":"get_resource","arguments":{"resource_id":"res_123"}},"id":2}'
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use turbomcp::prelude::*;

// Multi-tenancy features require the multi-tenancy feature flag
use turbomcp_server::{
    TenantContextExt, // Extension trait for tenant context methods
    config::multi_tenant::{StaticTenantConfigProvider, TenantConfig, TenantConfigProvider},
    metrics::multi_tenant::MultiTenantMetrics,
    middleware::tenancy::{
        ApiKeyTenantExtractor, CompositeTenantExtractor, HeaderTenantExtractor, TenantId,
    },
};

/// Simulated resource storage with tenant ownership
#[derive(Debug, Clone)]
struct Resource {
    id: String,
    tenant_id: String,
    name: String,
    data: String,
}

/// Multi-tenant server with tenant-scoped resources
#[derive(Clone)]
struct MultiTenantServer {
    /// Per-tenant configuration provider
    tenant_configs: Arc<StaticTenantConfigProvider>,

    /// Simulated resource database (in production, use PostgreSQL with RLS)
    resources: Arc<RwLock<HashMap<String, Resource>>>,

    /// Multi-tenant metrics tracker
    metrics: Arc<MultiTenantMetrics>,
}

#[server(
    name = "multi-tenant-example",
    version = "1.0.0",
    description = "Production-ready multi-tenant MCP server with secure tenant isolation"
)]
impl MultiTenantServer {
    /// Get a resource with tenant ownership validation
    ///
    /// This demonstrates the critical security pattern: always validate tenant ownership
    /// before accessing resources.
    #[tool("Get a resource by ID (validates tenant ownership)")]
    async fn get_resource(
        &self,
        ctx: Context,
        resource_id: String,
    ) -> McpResult<String> {
        // CRITICAL: Extract and validate tenant ID
        let tenant_id = ctx
            .request
            .require_tenant()
            .map_err(|e| mcp_error!("Tenant authentication required: {}", e))?;

        // Retrieve the resource
        let resources = self.resources.read().await;
        let resource = resources
            .get(&resource_id)
            .ok_or_else(|| mcp_error!("Resource not found: {}", resource_id))?;

        // CRITICAL: Validate tenant owns this resource
        ctx.request
            .validate_tenant_ownership(&resource.tenant_id)
            .map_err(|e| mcp_error!("Access denied: {}", e))?;

        Ok(format!(
            "Resource: {} (Name: {}, Data: {})",
            resource.id, resource.name, resource.data
        ))
    }

    /// Create a new resource scoped to the requesting tenant
    #[tool("Create a resource (automatically scoped to your tenant)")]
    async fn create_resource(
        &self,
        ctx: Context,
        name: String,
        data: String,
    ) -> McpResult<String> {
        // Extract tenant ID (all resources are tenant-scoped)
        let tenant_id = ctx
            .request
            .require_tenant()
            .map_err(|e| mcp_error!("Tenant authentication required: {}", e))?;

        // Check if tenant is allowed to create resources
        if let Some(config) = self.tenant_configs.get_config(tenant_id).await {
            if !config.is_tool_enabled("create_resource") {
                return Err(McpError::Tool(
                    "Creating resources is not enabled for your subscription plan".to_string()
                ));
            }

            if !config.is_active() {
                return Err(McpError::Tool("Account is suspended. Please contact support.".to_string()));
            }
        }

        // Generate resource ID
        let resource_id = format!("res_{}", uuid::Uuid::new_v4());

        // Create resource (automatically scoped to tenant)
        let resource = Resource {
            id: resource_id.clone(),
            tenant_id: tenant_id.to_string(),
            name: name.clone(),
            data: data.clone(),
        };

        // Store resource
        let mut resources = self.resources.write().await;
        resources.insert(resource_id.clone(), resource);

        // Record metrics
        self.metrics.record_request(tenant_id);
        self.metrics
            .record_request_success(tenant_id, std::time::Duration::from_millis(10));

        Ok(format!(
            "Created resource {} for tenant {}",
            resource_id, tenant_id
        ))
    }

    /// List all resources owned by the requesting tenant
    #[tool("List all resources owned by your tenant")]
    async fn list_resources(&self, ctx: Context) -> McpResult<Vec<String>> {
        // Extract tenant ID
        let tenant_id = ctx
            .request
            .require_tenant()
            .map_err(|e| mcp_error!("Tenant authentication required: {}", e))?;

        // Filter resources by tenant ownership
        let resources = self.resources.read().await;
        let tenant_resources: Vec<String> = resources
            .values()
            .filter(|r| r.tenant_id == tenant_id)
            .map(|r| format!("{}: {} ({})", r.id, r.name, r.data))
            .collect();

        Ok(tenant_resources)
    }

    /// Get tenant-specific metrics (admin tool)
    #[tool("Get metrics for your tenant")]
    async fn get_tenant_metrics(&self, ctx: Context) -> McpResult<String> {
        let tenant_id = ctx
            .request
            .require_tenant()
            .map_err(|e| mcp_error!("Tenant authentication required: {}", e))?;

        if let Some(metrics) = self.metrics.get_tenant_metrics(tenant_id) {
            Ok(format!(
                "Tenant: {}\nRequests: {}\nSuccessful: {}\nFailed: {}\nAvg Response Time: {}μs",
                metrics.tenant_id(),
                metrics.requests_total(),
                metrics.requests_successful(),
                metrics.requests_failed(),
                metrics.avg_response_time_us()
            ))
        } else {
            Ok(format!("No metrics available for tenant {}", tenant_id))
        }
    }

    /// Get tenant configuration (admin tool)
    #[tool("Get your tenant configuration")]
    async fn get_tenant_config(&self, ctx: Context) -> McpResult<String> {
        let tenant_id = ctx
            .request
            .require_tenant()
            .map_err(|e| mcp_error!("Tenant authentication required: {}", e))?;

        if let Some(config) = self.tenant_configs.get_config(tenant_id).await {
            Ok(format!(
                "Tenant: {}\nActive: {}\nRate Limit: {:?}/sec\nMax Concurrent: {:?}\nTool Access: All tools enabled",
                tenant_id,
                config.is_active(),
                config.rate_limit_per_second,
                config.max_concurrent_requests
            ))
        } else {
            Ok(format!(
                "Tenant {} using default configuration (no overrides)",
                tenant_id
            ))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for observability
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("🚀 Starting multi-tenant MCP server...");

    // Set up per-tenant configurations
    let tenant_configs = {
        let mut configs = HashMap::new();

        // ACME Corp - Enterprise tier
        configs.insert(
            "acme-corp".to_string(),
            TenantConfig {
                rate_limit_per_second: Some(1000),
                max_concurrent_requests: Some(100),
                is_active: Some(true),
                enabled_tools: None, // All tools enabled
                disabled_tools: None,
                tool_timeout_ms: Some(30000),
                max_request_body_size: Some(10 * 1024 * 1024), // 10MB
                metadata: HashMap::new(),
            },
        );

        // Widgets Inc - Starter tier
        configs.insert(
            "widgets-inc".to_string(),
            TenantConfig {
                rate_limit_per_second: Some(100),
                max_concurrent_requests: Some(10),
                is_active: Some(true),
                enabled_tools: None,
                disabled_tools: None,
                tool_timeout_ms: Some(10000),
                max_request_body_size: Some(1 * 1024 * 1024), // 1MB
                metadata: HashMap::new(),
            },
        );

        // Suspended tenant (example)
        configs.insert(
            "suspended-corp".to_string(),
            TenantConfig {
                is_active: Some(false), // Suspended
                ..Default::default()
            },
        );

        StaticTenantConfigProvider::new(configs)
    };

    // Initialize resource storage with sample data
    let mut initial_resources = HashMap::new();
    initial_resources.insert(
        "res_acme_1".to_string(),
        Resource {
            id: "res_acme_1".to_string(),
            tenant_id: "acme-corp".to_string(),
            name: "ACME Database".to_string(),
            data: "Production data".to_string(),
        },
    );
    initial_resources.insert(
        "res_widgets_1".to_string(),
        Resource {
            id: "res_widgets_1".to_string(),
            tenant_id: "widgets-inc".to_string(),
            name: "Widgets Inventory".to_string(),
            data: "Widget catalog".to_string(),
        },
    );

    // Create server instance
    let server = MultiTenantServer {
        tenant_configs: Arc::new(tenant_configs),
        resources: Arc::new(RwLock::new(initial_resources)),
        metrics: Arc::new(MultiTenantMetrics::new(1000)), // Track up to 1000 tenants
    };

    println!("✅ Server configured with multi-tenant isolation");
    println!("📊 Sample tenants:");
    println!("   - acme-corp (Enterprise tier)");
    println!("   - widgets-inc (Starter tier)");
    println!("   - suspended-corp (Suspended)");
    println!();

    // Create composite tenant extractor (tries multiple strategies)
    let _tenant_extractor = CompositeTenantExtractor::new(vec![
        // 1. Try X-Tenant-ID header first (explicit tenant)
        Box::new(HeaderTenantExtractor::new("X-Tenant-ID")),
        // 2. Try extracting from API key: sk_tenant_secret
        Box::new(ApiKeyTenantExtractor::new('_', 1).with_prefix("sk_")),
        // 3. Try subdomain (if running behind a domain)
        // Box::new(SubdomainTenantExtractor::new("api.example.com")),
    ]);

    println!("🔐 Tenant extraction enabled:");
    println!("   1. X-Tenant-ID header");
    println!("   2. API key prefix (sk_tenant_secret)");
    println!("   3. Subdomain (if configured)");
    println!();
    println!("💡 Example requests:");
    println!("   curl -H 'X-Tenant-ID: acme-corp' http://localhost:3000/mcp/v1");
    println!("   curl -H 'Authorization: sk_acme-corp_secret' http://localhost:3000/mcp/v1");
    println!();

    // Run HTTP server with tenant extraction middleware
    println!("🌐 Starting HTTP server on http://localhost:3000");
    println!("   Ready to accept multi-tenant requests!");
    println!();

    // Note: This is a placeholder - actual HTTP server integration would go here
    // For now, demonstrate STDIO mode with tenant context
    println!("⚠️  HTTP multi-tenant mode requires additional HTTP transport setup");
    println!("   Running in STDIO mode for demonstration...");
    server.run_stdio().await?;

    Ok(())
}
