//! Example 15: Elicitation Comparison - Server
//!
//! This example demonstrates TWO ways to implement elicitation:
//! 1. Using the `elicit!()` macro (simple, concise)
//! 2. Using the builder API (flexible, powerful)
//!
//! Both approaches work with the same client (see 15_elicitation_comparison_client.rs)
//!
//! Run the server:
//! ```bash
//! cargo run --example 15_elicitation_comparison_server
//! ```
//!
//! In another terminal, run the client:
//! ```bash
//! cargo run --example 15_elicitation_comparison_client
//! ```

use turbomcp::elicitation_api::{
    ElicitationResult, checkbox, choices, elicit, integer_field, text,
};
use turbomcp::{Context, McpResult, server, tool};

#[derive(Clone)]
struct DeploymentServer;

#[server(
    name = "elicitation-comparison",
    version = "1.0.5",
    description = "Demonstrates both macro and builder elicitation approaches"
)]
impl DeploymentServer {
    /// Deploy using the elicit!() macro approach - simple and concise
    #[tool("Deploy with macro")]
    async fn deploy_macro(&self, ctx: Context, project: String) -> McpResult<String> {
        // Log using context (no ctx_info macro exists, just use the context directly)
        // In a real app, you might use tracing or log crates
        println!("Starting deployment for project: {}", project);

        // Simple confirmation using elicitation builder
        let confirm_builder = elicit(format!("Ready to deploy {} to production?", project))
            .field("confirmed", checkbox("Confirm deployment").default(false));

        let confirm = confirm_builder.send(&ctx.request).await?;

        match confirm {
            ElicitationResult::Accept(data) => {
                // For simple confirmations, check if user approved
                if data.get_boolean("confirmed").unwrap_or(false) {
                    // Now get deployment configuration using macro with schema
                    let config_schema = elicit("Configure deployment")
                        .field("environment", choices(&["staging", "production"]))
                        .field(
                            "replicas",
                            integer_field("Number of replicas").range(1.0, 10.0),
                        )
                        .field("auto_scale", checkbox("Enable auto-scaling"))
                        .field("monitoring", checkbox("Enable monitoring"));

                    let config = config_schema.send(&ctx.request).await?;

                    match config {
                        ElicitationResult::Accept(config_data) => {
                            let env = config_data
                                .get_string("environment")
                                .unwrap_or_else(|_| "staging".to_string());
                            let replicas = config_data.get_integer("replicas").unwrap_or(1);
                            let auto_scale = config_data.get_boolean("auto_scale").unwrap_or(false);
                            let monitoring = config_data.get_boolean("monitoring").unwrap_or(true);

                            Ok(format!(
                                "✅ Deployed {} to {} with {} replicas (auto-scale: {}, monitoring: {})",
                                project, env, replicas, auto_scale, monitoring
                            ))
                        }
                        ElicitationResult::Decline(reason) => Ok(format!(
                            "❌ Deployment cancelled: {}",
                            reason.unwrap_or_else(|| "User declined".to_string())
                        )),
                        ElicitationResult::Cancel => Ok("❌ Deployment cancelled".to_string()),
                    }
                } else {
                    Ok("❌ Deployment not confirmed".to_string())
                }
            }
            ElicitationResult::Decline(reason) => Ok(format!(
                "❌ Deployment declined: {}",
                reason.unwrap_or_else(|| "User declined".to_string())
            )),
            ElicitationResult::Cancel => Ok("❌ Deployment cancelled".to_string()),
        }
    }

    /// Deploy using the builder API - more control and flexibility
    #[tool("Deploy with builder")]
    async fn deploy_builder(&self, ctx: Context, project: String) -> McpResult<String> {
        println!("Starting deployment for project: {}", project);

        // First, ask for confirmation using builder approach
        let confirm_builder = elicit(format!("Ready to deploy {} to production?", project))
            .field(
                "confirmed",
                checkbox(format!("Confirm deployment of {}", project)),
            )
            .field(
                "review_changes",
                checkbox("Review changes before deployment"),
            )
            .require(vec!["confirmed"]);

        let confirm_result = confirm_builder.send(&ctx.request).await?;

        match confirm_result {
            ElicitationResult::Accept(data) => {
                if !data.get_boolean("confirmed").unwrap_or(false) {
                    return Ok("❌ Deployment not confirmed".to_string());
                }

                let review = data.get_boolean("review_changes").unwrap_or(false);
                if review {
                    println!("User requested change review");
                }

                // Now get detailed configuration using builder
                let config_builder = elicit("Configure deployment settings")
                    .field(
                        "environment",
                        text("Target Environment").options(&[
                            "development",
                            "staging",
                            "production",
                        ]),
                    )
                    .field(
                        "replicas",
                        integer_field("Number of replicas").range(1.0, 20.0),
                    )
                    .field("auto_scale", checkbox("Enable auto-scaling").default(true))
                    .field("monitoring", checkbox("Enable monitoring").default(true))
                    .field("health_check", text("Health check endpoint"))
                    .field(
                        "rollback_on_failure",
                        checkbox("Auto-rollback on failure").default(true),
                    );

                let config_result = config_builder.send(&ctx.request).await?;

                match config_result {
                    ElicitationResult::Accept(config) => {
                        let env = config
                            .get_string("environment")
                            .unwrap_or_else(|_| "staging".to_string());
                        let replicas = config.get_integer("replicas").unwrap_or(3);
                        let auto_scale = config.get_boolean("auto_scale").unwrap_or(true);
                        let monitoring = config.get_boolean("monitoring").unwrap_or(true);
                        let health_check = config
                            .get_string("health_check")
                            .unwrap_or_else(|_| "/health".to_string());
                        let rollback = config.get_boolean("rollback_on_failure").unwrap_or(true);

                        Ok(format!(
                            "✅ Deployed {} to {}\n\
                             📊 Configuration:\n\
                             - Replicas: {}\n\
                             - Auto-scaling: {}\n\
                             - Monitoring: {}\n\
                             - Health check: {}\n\
                             - Auto-rollback: {}",
                            project, env, replicas, auto_scale, monitoring, health_check, rollback
                        ))
                    }
                    ElicitationResult::Decline(reason) => Ok(format!(
                        "❌ Configuration cancelled: {}",
                        reason.unwrap_or_else(|| "User declined".to_string())
                    )),
                    ElicitationResult::Cancel => Ok("❌ Configuration cancelled".to_string()),
                }
            }
            ElicitationResult::Decline(reason) => Ok(format!(
                "❌ Deployment declined: {}",
                reason.unwrap_or_else(|| "User declined".to_string())
            )),
            ElicitationResult::Cancel => Ok("❌ Deployment cancelled".to_string()),
        }
    }

    /// List available deployment methods
    #[tool("List deployment methods")]
    async fn list_methods(&self, _ctx: Context) -> McpResult<String> {
        Ok("Available deployment methods:\n\
            1. deploy_macro - Simple deployment using elicit!() macro\n\
            2. deploy_builder - Advanced deployment using builder API\n\
            \n\
            Both methods support the same elicitation protocol and work with any MCP client."
            .to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging if needed
    // env_logger::init();

    println!("🚀 Elicitation Comparison Server");
    println!("================================");
    println!("This server demonstrates two approaches to elicitation:");
    println!("1. elicit!() macro - Simple and concise");
    println!("2. Builder API - Flexible and powerful");
    println!();
    println!("Available tools:");
    println!("- deploy_macro: Deploy using the macro approach");
    println!("- deploy_builder: Deploy using the builder approach");
    println!("- list_methods: List available deployment methods");
    println!();
    println!("Run the client example to interact with this server:");
    println!("cargo run --example 15_elicitation_comparison_client");
    println!();
    println!("Server running on stdio...");

    let server = DeploymentServer;
    server.run_stdio().await?;

    Ok(())
}
