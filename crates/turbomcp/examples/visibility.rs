//! # Progressive Disclosure Example
//!
//! Demonstrates using VisibilityLayer to control which tools/resources are
//! visible to clients based on tags and session state.
//!
//! This enables:
//! - Hiding admin tools from regular users
//! - Revealing features progressively as users advance
//! - Session-specific tool access
//!
//! Run with: `cargo run --example visibility`

use turbomcp::__macro_support::turbomcp_core::handler::McpHandler;
use turbomcp::prelude::*;
use turbomcp_server::{VisibilityLayer, VisibilitySessionGuard};
use turbomcp_types::component::ComponentFilter;

#[derive(Clone)]
struct FeatureServer;

#[turbomcp::server(name = "feature-server", version = "1.0.0")]
impl FeatureServer {
    /// Available to everyone
    #[tool(description = "Get current time", tags = ["public", "readonly"])]
    async fn get_time(&self) -> McpResult<String> {
        Ok(chrono::Utc::now().to_rfc3339())
    }

    /// Available to everyone
    #[tool(description = "Say hello", tags = ["public"])]
    async fn greet(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {}!", name))
    }

    /// Only for premium users
    #[tool(description = "Premium feature", tags = ["premium"])]
    async fn premium_feature(&self) -> McpResult<String> {
        Ok("Welcome to premium! You have access to advanced features.".into())
    }

    /// Only for admins
    #[tool(description = "Admin dashboard", tags = ["admin"])]
    async fn admin_dashboard(&self) -> McpResult<String> {
        Ok("Admin Dashboard: 42 users, 100 requests/min".into())
    }

    /// Dangerous - hidden by default
    #[tool(description = "Delete all data", tags = ["admin", "dangerous"])]
    async fn delete_all(&self) -> McpResult<String> {
        Ok("All data deleted (simulated)".into())
    }

    /// A resource with tags
    #[resource("config://settings", tags = ["premium", "config"])]
    async fn get_settings(&self, _uri: String, _ctx: &RequestContext) -> McpResult<String> {
        Ok(r#"{"theme": "dark", "notifications": true}"#.into())
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Progressive Disclosure Demo ===\n");

    let server = FeatureServer;

    // Show all tools without any filtering
    println!("All tools (no filtering):");
    println!("-------------------------");
    for tool in server.list_tools() {
        print_tool(&tool);
    }
    println!();

    // Create visibility layer that hides admin and dangerous tools
    let visibility = VisibilityLayer::new(server.clone())
        .with_disabled(ComponentFilter::with_tags(["admin", "dangerous"]));

    // Show tools after filtering
    println!("Tools visible to regular users (admin/dangerous hidden):");
    println!("--------------------------------------------------------");
    for tool in visibility.list_tools() {
        print_tool(&tool);
    }
    println!();

    // Show resources after filtering
    println!("Resources visible to regular users:");
    println!("-----------------------------------");
    for resource in visibility.list_resources() {
        println!("  {} ({})", resource.name, resource.uri);
    }
    println!();

    // Demonstrate session-specific overrides
    println!("=== Session-Specific Access ===\n");

    // Enable premium features for a specific session
    // Note: enable_for_session takes &[String], not ComponentFilter
    let session_id = "user-123";
    let premium_tags = vec!["premium".to_string()];
    visibility.enable_for_session(session_id, &premium_tags);

    println!("After enabling 'premium' tag for session '{}':", session_id);
    // Note: In a real server, you'd pass the session context to list_tools
    // For demo purposes, we show the concept
    println!("  Premium features would now be visible for this session");
    println!("  Other sessions still don't see premium tools");
    println!();

    // Clean up session state when done
    visibility.clear_session(session_id);
    println!("Session state cleared for '{}'", session_id);
    println!();

    // Show active session count
    println!(
        "Active sessions tracking: {}",
        visibility.active_sessions_count()
    );
    println!();

    // Best practice: Use session guard for automatic cleanup
    println!("=== Best Practice: Session Guard (RAII) ===\n");
    {
        let _guard: VisibilitySessionGuard = visibility.session_guard("user-456");
        let admin_tags = vec!["admin".to_string()];
        visibility.enable_for_session("user-456", &admin_tags);
        println!("  Session 'user-456' has admin access within this scope");
        println!("  Active sessions: {}", visibility.active_sessions_count());
        // Guard automatically clears session when dropped
    }
    println!("  After scope ends, session is automatically cleaned up");
    println!("  Active sessions: {}", visibility.active_sessions_count());

    Ok(())
}

fn print_tool(tool: &Tool) {
    print!("  {} ", tool.name);
    if let Some(meta) = &tool.meta
        && let Some(tags) = meta.get("tags")
    {
        print!("{}", tags);
    }
    println!();
}
