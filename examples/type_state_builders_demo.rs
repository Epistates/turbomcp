//! # Type-State Capability Builders Demo
//!
//! This example demonstrates TurboMCP's const-generic type-state builders
//! that provide compile-time validation of capability configurations with
//! zero-cost abstractions and advanced safety features.

use turbomcp_protocol::capabilities::builders::{
    ServerCapabilitiesBuilder, ClientCapabilitiesBuilder
};

fn main() {
    println!("🚀 TurboMCP Type-State Capability Builders Demo");
    println!("===============================================\n");

    // Example 1: Server capabilities with compile-time validation
    println!("1. Server Capabilities with Type-State Validation");
    println!("   -----------------------------------------------");

    let server_caps = ServerCapabilitiesBuilder::new()
        .enable_experimental()  // Enables experimental capability state
        .enable_tools()         // Enables tools capability state
        .enable_prompts()       // Enables prompts capability state
        .enable_resources()     // Enables resources capability state
        // These methods are only available because we enabled the parent capabilities!
        .enable_tool_list_changed()     // ✅ Only available when tools enabled
        .enable_prompts_list_changed()  // ✅ Only available when prompts enabled
        .enable_resources_list_changed() // ✅ Only available when resources enabled
        .enable_resources_subscribe()   // ✅ Only available when resources enabled
        // TurboMCP exclusive features!
        .with_simd_optimization("avx2")       // 🚀 TurboMCP exclusive
        .with_enterprise_security(true)       // 🚀 TurboMCP exclusive
        .build();

    println!("   ✅ Server capabilities configured with compile-time validation");
    println!("   📊 Tools enabled: {}", server_caps.tools.is_some());
    println!("   📝 Prompts enabled: {}", server_caps.prompts.is_some());
    println!("   📚 Resources enabled: {}", server_caps.resources.is_some());
    println!("   🧪 Experimental features: {}", server_caps.experimental.as_ref().map_or(0, |e| e.len()));

    // Example 2: Client capabilities with type safety
    println!("\n2. Client Capabilities with Type-State Validation");
    println!("   ----------------------------------------------");

    let client_caps = ClientCapabilitiesBuilder::new()
        .enable_experimental()  // Enables experimental capability state
        .enable_roots()         // Enables roots capability state
        .enable_sampling()      // Enables sampling capability state
        .enable_elicitation()   // Enables elicitation capability state
        // Sub-capability only available when roots are enabled!
        .enable_roots_list_changed()  // ✅ Only available when roots enabled
        // TurboMCP exclusive features!
        .with_llm_provider("openai", "gpt-4")                  // 🚀 TurboMCP exclusive
        .with_ui_capabilities(vec!["form", "dialog", "toast"]) // 🚀 TurboMCP exclusive
        .build();

    println!("   ✅ Client capabilities configured with compile-time validation");
    println!("   🗂️  Roots enabled: {}", client_caps.roots.is_some());
    println!("   🎯 Sampling enabled: {}", client_caps.sampling.is_some());
    println!("   🤝 Elicitation enabled: {}", client_caps.elicitation.is_some());

    // Example 3: Convenience builders for common patterns
    println!("\n3. Convenience Builders for Common Patterns");
    println!("   ----------------------------------------");

    // Full-featured server (all capabilities enabled)
    let full_server = ServerCapabilitiesBuilder::full_featured().build();
    println!("   🚀 Full-featured server: {} capabilities enabled",
        count_server_capabilities(&full_server));

    // Minimal server (only tools)
    let minimal_server = ServerCapabilitiesBuilder::minimal().build();
    println!("   ⚡ Minimal server: {} capabilities enabled",
        count_server_capabilities(&minimal_server));

    // Sampling-focused client
    let sampling_client = ClientCapabilitiesBuilder::sampling_focused().build();
    println!("   🎯 Sampling-focused client: {} capabilities enabled",
        count_client_capabilities(&sampling_client));

    // Example 4: Demonstrate compile-time safety
    println!("\n4. Compile-Time Safety Demonstration");
    println!("   ---------------------------------");
    println!("   ✅ The following code would NOT compile:");
    println!("
   ServerCapabilitiesBuilder::new()
       // .enable_tools()  // ← This line commented out
       .enable_tool_list_changed()  // ← This would cause compile error!
       .build();
   ");
    println!("   🛡️  Compile-time validation prevents impossible configurations!");

    println!("\n5. TurboMCP Exclusive Features");
    println!("   ----------------------------");

    // Show TurboMCP-specific experimental features
    if let Some(ref experimental) = server_caps.experimental {
        println!("   🚀 TurboMCP Server Extensions:");
        for (key, value) in experimental {
            if key.starts_with("turbomcp_") {
                println!("      - {}: {}", key.replace("turbomcp_", ""), value);
            }
        }
    }

    if let Some(ref experimental) = client_caps.experimental {
        println!("   🚀 TurboMCP Client Extensions:");
        for (key, value) in experimental {
            if key.starts_with("turbomcp_") {
                println!("      - {}: {}", key.replace("turbomcp_", ""), value);
            }
        }
    }

    println!("\n🎉 Demo Complete! TurboMCP type-state builders provide:");
    println!("   ✅ Compile-time capability validation");
    println!("   ✅ Advanced MCP capability support");
    println!("   ✅ Performance and security optimizations");
    println!("   ✅ Full backwards compatibility");
    println!("   ✅ Zero-cost abstractions");
    println!("\n🏆 TurboMCP delivers enterprise-grade MCP capability management!");
}

/// Count enabled server capabilities
fn count_server_capabilities(caps: &turbomcp_protocol::types::ServerCapabilities) -> usize {
    let mut count = 0;
    if caps.experimental.is_some() { count += 1; }
    if caps.logging.is_some() { count += 1; }
    if caps.completions.is_some() { count += 1; }
    if caps.prompts.is_some() { count += 1; }
    if caps.resources.is_some() { count += 1; }
    if caps.tools.is_some() { count += 1; }
    count
}

/// Count enabled client capabilities
fn count_client_capabilities(caps: &turbomcp_protocol::types::ClientCapabilities) -> usize {
    let mut count = 0;
    if caps.experimental.is_some() { count += 1; }
    if caps.roots.is_some() { count += 1; }
    if caps.sampling.is_some() { count += 1; }
    if caps.elicitation.is_some() { count += 1; }
    count
}