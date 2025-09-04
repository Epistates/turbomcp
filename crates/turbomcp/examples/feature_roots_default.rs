//! Default Roots Example - Shows OS-specific default roots
//!
//! This example demonstrates the default roots behavior when no
//! custom roots are configured. The server provides OS-specific
//! defaults for Linux, macOS, and Windows.
//!
//! Run with:
//! ```bash
//! cargo run --example feature_roots_default
//! ```
//!
//! Test roots listing:
//! ```bash
//! echo '{"jsonrpc":"2.0","id":1,"method":"roots/list"}' | cargo run --example feature_roots_default 2>/dev/null | jq
//! ```

use turbomcp_server::ServerBuilder;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing to stderr
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    eprintln!("🌳 Server with Default OS-Specific Roots");
    eprintln!("========================================\n");

    // Build server WITHOUT custom roots - will use OS defaults
    let server = ServerBuilder::new()
        .name("default-roots-server")
        .version("1.0.0")
        .description("Server demonstrating default OS-specific roots")
        // Note: No .roots() call - uses OS defaults
        .build();

    eprintln!("📁 Default Roots by OS:");
    eprintln!();
    eprintln!("  Linux:");
    eprintln!("    • / (root)");
    eprintln!();
    eprintln!("  macOS:");
    eprintln!("    • / (root)");
    eprintln!("    • /Volumes");
    eprintln!();
    eprintln!("  Windows:");
    eprintln!("    • C:\\ through H:\\ (common drive letters)");
    eprintln!();

    let os = if cfg!(target_os = "linux") {
        "Linux"
    } else if cfg!(target_os = "macos") {
        "macOS"
    } else if cfg!(target_os = "windows") {
        "Windows"
    } else {
        "Unknown"
    };

    eprintln!("🖥️  Current OS: {}", os);
    eprintln!();

    eprintln!("📋 Test Command:");
    eprintln!("  echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"roots/list\"}}' | \\");
    eprintln!("  cargo run --example feature_roots_default 2>/dev/null | jq");
    eprintln!();

    eprintln!("✅ Server starting with stdio transport...\n");

    // Run server with stdio transport
    server.run_stdio().await?;

    Ok(())
}
