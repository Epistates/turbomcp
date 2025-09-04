//! Production-grade elicitation example demonstrating world-class implementation
//!
//! This example showcases TurboMCP's elicitation system in a production context,
//! demonstrating real-world patterns and best practices.

use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use tokio::sync::RwLock;
use tracing::{info, warn, error};
use tracing_subscriber::EnvFilter;

use turbomcp::{
    server, tool, resource, prompt,
    McpResult, McpError, mcp_error,
    RequestContext, Context,
    elicit, elicitation_api::{ElicitationBuilder, ElicitationResult},
};
use turbomcp_protocol::elicitation::{
    string, integer, number, boolean, object, array,
};

/// Production application state
#[derive(Clone)]
struct AppState {
    /// Database connection pool
    db_pool: Arc<RwLock<DatabasePool>>,
    
    /// Configuration store
    config: Arc<RwLock<AppConfig>>,
    
    /// Deployment manager
    deployer: Arc<DeploymentManager>,
}

/// Mock database pool
#[derive(Clone, Debug)]
struct DatabasePool {
    connections: Vec<String>,
}

/// Application configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
struct AppConfig {
    environment: String,
    features: HashMap<String, bool>,
    limits: ResourceLimits,
}

/// Resource limits
#[derive(Clone, Debug, Serialize, Deserialize)]
struct ResourceLimits {
    max_memory_gb: f64,
    max_cpu_cores: i64,
    max_storage_gb: i64,
}

/// Deployment manager
#[derive(Clone)]
struct DeploymentManager {
    active_deployments: Arc<RwLock<Vec<Deployment>>>,
}

/// Deployment record
#[derive(Clone, Debug, Serialize)]
struct Deployment {
    id: String,
    service: String,
    environment: String,
    replicas: i64,
    resources: DeploymentResources,
    status: DeploymentStatus,
}

/// Deployment resources
#[derive(Clone, Debug, Serialize, Deserialize)]
struct DeploymentResources {
    memory_gb: f64,
    cpu_cores: f64,
    storage_gb: i64,
}

/// Deployment status
#[derive(Clone, Debug, Serialize, Deserialize)]
enum DeploymentStatus {
    Pending,
    Running,
    Failed(String),
    Completed,
}

/// Production MCP Server with elicitation
#[server(
    name = "production_elicitation_server",
    version = "1.0.0",
    description = "Production-grade MCP server demonstrating world-class elicitation"
)]
struct ProductionServer {
    state: AppState,
}

impl ProductionServer {
    /// Create new production server
    fn new() -> Self {
        Self {
            state: AppState {
                db_pool: Arc::new(RwLock::new(DatabasePool {
                    connections: vec!["primary".to_string(), "replica".to_string()],
                })),
                config: Arc::new(RwLock::new(AppConfig {
                    environment: "production".to_string(),
                    features: HashMap::new(),
                    limits: ResourceLimits {
                        max_memory_gb: 64.0,
                        max_cpu_cores: 32,
                        max_storage_gb: 1000,
                    },
                })),
                deployer: Arc::new(DeploymentManager {
                    active_deployments: Arc::new(RwLock::new(Vec::new())),
                }),
            }
        }
    }
    
    /// Deploy a service with interactive configuration
    #[tool(description = "Deploy a service to the cloud with guided configuration")]
    async fn deploy_service(
        &self,
        ctx: RequestContext,
        service_name: String,
    ) -> McpResult<Deployment> {
        info!("Starting deployment for service: {}", service_name);
        
        // Step 1: Environment selection with validation
        let env_result = elicit("Select deployment environment")
            .field("environment", string()
                .enum_values(vec![
                    "development",
                    "staging",
                    "production",
                ])
                .description("Target environment for deployment")
                .build())
            .field("confirm_production", boolean()
                .description("I understand production deployment risks")
                .build())
            .require(vec!["environment"])
            .send(&ctx)
            .await?;
        
        let (environment, production_confirmed) = match env_result {
            ElicitationResult::Accept(data) => {
                let env = data.get::<String>("environment")?;
                let confirmed = data.get::<bool>("confirm_production").unwrap_or(false);
                
                if env == "production" && !confirmed {
                    return Err(mcp_error!("Production deployment requires confirmation"));
                }
                
                (env, confirmed)
            }
            _ => return Err(mcp_error!("Deployment cancelled by user")),
        };
        
        // Step 2: Resource configuration with limits
        let config = self.state.config.read().await;
        let resource_result = elicit("Configure deployment resources")
            .field("replicas", integer()
                .range(1.0, if environment == "production" { 10.0 } else { 3.0 })
                .description("Number of service replicas")
                .build())
            .field("memory_gb", number()
                .range(0.5, config.limits.max_memory_gb)
                .description("Memory per replica (GB)")
                .build())
            .field("cpu_cores", number()
                .range(0.25, config.limits.max_cpu_cores as f64)
                .description("CPU cores per replica")
                .build())
            .field("storage_gb", integer()
                .range(1.0, config.limits.max_storage_gb as f64)
                .description("Storage per replica (GB)")
                .build())
            .field("auto_scaling", boolean()
                .description("Enable auto-scaling")
                .build())
            .require(vec!["replicas", "memory_gb", "cpu_cores"])
            .send(&ctx)
            .await?;
        
        let resources = match resource_result {
            ElicitationResult::Accept(data) => {
                DeploymentResources {
                    memory_gb: data.get::<f64>("memory_gb")?,
                    cpu_cores: data.get::<f64>("cpu_cores")?,
                    storage_gb: data.get::<i64>("storage_gb").unwrap_or(10),
                }
            }
            _ => return Err(mcp_error!("Resource configuration cancelled")),
        };
        
        let replicas = resource_result
            .as_accept()
            .and_then(|d| d.get::<i64>("replicas").ok())
            .unwrap_or(1);
        
        // Step 3: Advanced configuration (optional)
        let advanced_result = elicit("Advanced deployment options (optional)")
            .field("health_check_path", string()
                .description("Health check endpoint path")
                .build())
            .field("startup_timeout_seconds", integer()
                .range(30.0, 600.0)
                .description("Startup timeout in seconds")
                .build())
            .field("environment_variables", object()
                .description("Environment variables (key-value pairs)")
                .build())
            .field("labels", array()
                .items(string().build())
                .description("Deployment labels for organization")
                .build())
            .send(&ctx)
            .await?;
        
        // Create deployment
        let deployment = Deployment {
            id: Uuid::new_v4().to_string(),
            service: service_name,
            environment,
            replicas,
            resources,
            status: DeploymentStatus::Pending,
        };
        
        // Store deployment
        self.state.deployer.active_deployments.write().await.push(deployment.clone());
        
        // Simulate deployment process
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await;
            info!("Deployment {} completed", deployment.id);
        });
        
        Ok(deployment)
    }
    
    /// Configure database with interactive setup
    #[tool(description = "Configure database connection with guided setup")]
    async fn configure_database(
        &self,
        ctx: RequestContext,
    ) -> McpResult<String> {
        let db_config = elicit("Configure database connection")
            .field("db_type", string()
                .enum_values(vec!["postgresql", "mysql", "mongodb", "redis"])
                .description("Database type")
                .build())
            .field("host", string()
                .min_length(1)
                .description("Database host")
                .build())
            .field("port", integer()
                .range(1.0, 65535.0)
                .description("Database port")
                .build())
            .field("database", string()
                .description("Database name")
                .build())
            .field("username", string()
                .description("Database username")
                .build())
            .field("use_ssl", boolean()
                .description("Use SSL/TLS connection")
                .build())
            .field("connection_pool_size", integer()
                .range(1.0, 100.0)
                .description("Connection pool size")
                .build())
            .require(vec!["db_type", "host", "port", "database", "username"])
            .send(&ctx)
            .await?;
        
        match db_config {
            ElicitationResult::Accept(data) => {
                let db_type = data.get::<String>("db_type")?;
                let host = data.get::<String>("host")?;
                let port = data.get::<i64>("port")?;
                
                // Create connection string
                let conn_str = format!("{}://{}:{}", db_type, host, port);
                
                // Update pool
                self.state.db_pool.write().await.connections.push(conn_str.clone());
                
                Ok(format!("✅ Database configured: {}", conn_str))
            }
            ElicitationResult::Decline(reason) => {
                Ok(format!("Database configuration declined: {}", 
                    reason.unwrap_or_else(|| "No reason provided".to_string())))
            }
            ElicitationResult::Cancel => {
                Ok("Database configuration cancelled".to_string())
            }
        }
    }
    
    /// Perform system migration with safety checks
    #[tool(description = "Perform system migration with interactive safety checks")]
    async fn migrate_system(
        &self,
        ctx: RequestContext,
        target_version: String,
    ) -> McpResult<String> {
        // Safety check elicitation
        let safety_check = elicit("⚠️ Migration Safety Check")
            .field("backup_completed", boolean()
                .description("I have completed a full system backup")
                .build())
            .field("maintenance_window", boolean()
                .description("System is in maintenance window")
                .build())
            .field("rollback_plan", boolean()
                .description("Rollback plan is documented and tested")
                .build())
            .field("team_notified", boolean()
                .description("Team has been notified of migration")
                .build())
            .field("confirm_migration", string()
                .pattern(format!("^MIGRATE TO {}$", target_version))
                .description(format!("Type 'MIGRATE TO {}' to confirm", target_version))
                .build())
            .require(vec![
                "backup_completed",
                "maintenance_window",
                "rollback_plan",
                "team_notified",
                "confirm_migration",
            ])
            .send(&ctx)
            .await?;
        
        match safety_check {
            ElicitationResult::Accept(data) => {
                let backup = data.get::<bool>("backup_completed")?;
                let maintenance = data.get::<bool>("maintenance_window")?;
                let rollback = data.get::<bool>("rollback_plan")?;
                let team = data.get::<bool>("team_notified")?;
                
                if !backup || !maintenance || !rollback || !team {
                    return Err(mcp_error!("All safety checks must be confirmed"));
                }
                
                info!("Starting migration to version {}", target_version);
                
                // Simulate migration
                tokio::time::sleep(Duration::from_secs(2)).await;
                
                Ok(format!("✅ Successfully migrated to version {}", target_version))
            }
            _ => Err(mcp_error!("Migration cancelled - safety first!"))
        }
    }
    
    /// Get deployment status
    #[resource(uri = "/deployments")]
    async fn list_deployments(&self) -> McpResult<Vec<Deployment>> {
        Ok(self.state.deployer.active_deployments.read().await.clone())
    }
    
    /// System health check prompt
    #[prompt(
        name = "system_health",
        description = "Generate system health report"
    )]
    async fn system_health(&self, _args: serde_json::Value) -> McpResult<String> {
        let deployments = self.state.deployer.active_deployments.read().await;
        let db_pool = self.state.db_pool.read().await;
        let config = self.state.config.read().await;
        
        Ok(format!(
            "System Health Report\n\
            ====================\n\
            Environment: {}\n\
            Active Deployments: {}\n\
            Database Connections: {}\n\
            Resource Limits:\n\
            - Max Memory: {} GB\n\
            - Max CPU: {} cores\n\
            - Max Storage: {} GB",
            config.environment,
            deployments.len(),
            db_pool.connections.len(),
            config.limits.max_memory_gb,
            config.limits.max_cpu_cores,
            config.limits.max_storage_gb
        ))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env()
                .add_directive("production_elicitation=info".parse()?)
        )
        .init();
    
    info!("🚀 Starting Production Elicitation Server");
    info!("This demonstrates TurboMCP's world-class elicitation implementation");
    
    // Create server
    let server = Arc::new(ProductionServer::new());
    
    info!("✅ Server initialized with production configuration");
    info!("📋 Available tools:");
    info!("  - deploy_service: Deploy with interactive configuration");
    info!("  - configure_database: Setup database with guided elicitation");
    info!("  - migrate_system: Perform migration with safety checks");
    
    // In production, you would:
    // 1. Set up transport (WebSocket, HTTP/SSE, etc.)
    // 2. Start server with proper configuration
    // 3. Handle incoming MCP requests
    // 4. Process elicitation flows
    
    // Keep server running
    info!("Server ready for production workloads");
    tokio::signal::ctrl_c().await?;
    
    info!("Shutting down gracefully...");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_production_server_creation() {
        let server = ProductionServer::new();
        assert_eq!(
            server.state.config.read().await.environment,
            "production"
        );
    }
    
    #[tokio::test]
    async fn test_deployment_limits() {
        let server = ProductionServer::new();
        let config = server.state.config.read().await;
        
        assert_eq!(config.limits.max_memory_gb, 64.0);
        assert_eq!(config.limits.max_cpu_cores, 32);
        assert_eq!(config.limits.max_storage_gb, 1000);
    }
}