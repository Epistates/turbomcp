//! # 07: Transport Showcase - All Connection Methods (Simplified Demo)
//!
//! **Learning Goals (20 minutes):**
//! - Understand all TurboMCP transport methods
//! - Learn when to use each transport type
//! - See transport-specific configuration patterns
//!
//! **What this example demonstrates:**
//! - STDIO transport for CLI integration
//! - HTTP/SSE patterns for web services
//! - WebSocket patterns for bidirectional communication
//! - TCP patterns for network services
//!
//! **Run with:**
//! ```bash
//! cargo run --example 07_transport_showcase stdio
//! cargo run --example 07_transport_showcase http
//! cargo run --example 07_transport_showcase websocket
//! cargo run --example 07_transport_showcase tcp
//! ```
//!
//! Note: This demo uses STDIO for all transports to show the patterns.
//! In production, each would use its actual transport implementation.

use turbomcp::prelude::*;

/// Multi-transport demonstration server
#[derive(Clone)]
struct TransportServer {
    transport_type: String,
}

#[turbomcp::server(
    name = "transport-showcase",
    version = "1.0.0",
    description = "Demonstrates all TurboMCP transport methods"
)]
impl TransportServer {
    /// Get current transport information
    #[tool]
    async fn transport_info(&self) -> McpResult<String> {
        Ok(format!("Connected via: {}", self.transport_type))
    }

    /// Test round-trip communication
    #[tool]
    async fn ping(&self, message: String) -> McpResult<String> {
        Ok(format!("Pong: {} (via {})", message, self.transport_type))
    }
}

impl TransportServer {
    fn new(transport: &str) -> Self {
        Self {
            transport_type: transport.to_string(),
        }
    }

    /// Run with STDIO transport (for CLI tools like Claude Desktop)
    async fn run_stdio_transport(self) -> Result<(), Box<dyn std::error::Error>> {
        println!("📝 Starting STDIO transport...");
        println!("Perfect for: CLI tools, Claude Desktop, shell scripts");
        println!("📡 Server ready for JSON-RPC over stdin/stdout");
        println!(
            "Test with: echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}}' | cargo run --example 07_transport_showcase stdio"
        );
        self.run_stdio().await?;
        Ok(())
    }

    /// Demonstrate HTTP/SSE transport patterns
    async fn run_http_transport(self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🌐 HTTP/SSE Transport Configuration");
        println!("=====================================");
        println!("In production, you would configure:");
        println!("  • Bind address: 127.0.0.1:8080");
        println!("  • CORS origins: [\"*\"] or specific domains");
        println!("  • Max request size: 10MB");
        println!("  • SSE support: Enabled for streaming");
        println!("  • Security headers: CSP, HSTS, etc.");
        println!("\nTest command:");
        println!("curl -X POST http://localhost:8080/mcp \\");
        println!("  -H 'Content-Type: application/json' \\");
        println!("  -d '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}}'");
        println!("\nRunning STDIO for demo...");
        self.run_stdio().await?;
        Ok(())
    }

    /// Demonstrate WebSocket transport patterns
    async fn run_websocket_transport(self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🔌 WebSocket Transport Configuration");
        println!("=====================================");
        println!("In production, you would configure:");
        println!("  • Bind address: 127.0.0.1:9090");
        println!("  • Bidirectional messaging");
        println!("  • Auto-reconnect support");
        println!("  • Ping/pong heartbeat");
        println!("  • Message compression");
        println!("\nTest command:");
        println!("websocat ws://localhost:9090");
        println!("Then send: {{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}}");
        println!("\nRunning STDIO for demo...");
        self.run_stdio().await?;
        Ok(())
    }

    /// Demonstrate TCP transport patterns
    async fn run_tcp_transport(self) -> Result<(), Box<dyn std::error::Error>> {
        println!("🔗 TCP Transport Configuration");
        println!("================================");
        println!("In production, you would configure:");
        println!("  • Bind address: 127.0.0.1:7070");
        println!("  • Direct socket communication");
        println!("  • Low-level control");
        println!("  • Custom protocols possible");
        println!("  • Minimal overhead");
        println!("\nTest command:");
        println!(
            "echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}}' | nc localhost 7070"
        );
        println!("\nRunning STDIO for demo...");
        self.run_stdio().await?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let transport = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    match transport {
        "stdio" => {
            TransportServer::new("STDIO").run_stdio_transport().await?;
        }
        "http" => {
            TransportServer::new("HTTP/SSE")
                .run_http_transport()
                .await?;
        }
        "websocket" | "ws" => {
            TransportServer::new("WebSocket")
                .run_websocket_transport()
                .await?;
        }
        "tcp" => {
            TransportServer::new("TCP").run_tcp_transport().await?;
        }
        _ => {
            println!("\n╔════════════════════════════════════════╗");
            println!("║      TRANSPORT SHOWCASE - TURBOMCP     ║");
            println!("╚════════════════════════════════════════╝\n");

            println!("Available transports:\n");

            println!("📝 STDIO - Standard Input/Output");
            println!("   Best for: CLI tools, Claude Desktop, shell scripts");
            println!("   Usage: cargo run --example 07_transport_showcase stdio\n");

            println!("🌐 HTTP/SSE - HTTP with Server-Sent Events");
            println!("   Best for: Web services, REST APIs, browser clients");
            println!("   Usage: cargo run --example 07_transport_showcase http\n");

            println!("🔌 WebSocket - Bidirectional real-time");
            println!("   Best for: Real-time apps, live updates, chat systems");
            println!("   Usage: cargo run --example 07_transport_showcase websocket\n");

            println!("🔗 TCP - Direct network socket");
            println!("   Best for: Internal services, high performance, custom protocols");
            println!("   Usage: cargo run --example 07_transport_showcase tcp\n");

            println!("Choose the transport that best fits your use case!");
        }
    }

    Ok(())
}
