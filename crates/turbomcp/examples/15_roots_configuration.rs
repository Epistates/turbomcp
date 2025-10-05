//! # 15: Roots Configuration - Filesystem Access Control
//!
//! **Learning Goals:**
//! - Configure filesystem roots via the #[server] macro
//! - Understand MCP 2025-06-18 roots specification
//! - See how clients discover available filesystem locations
//! - Implement root-based resource access
//!
//! **What this example demonstrates:**
//! - Multiple root definitions with names and URIs
//! - Root discovery by clients
//! - Resource access within configured roots
//! - Security boundaries via root restrictions
//!
//! **Run with:** `cargo run --example 15_roots_configuration`

use turbomcp::prelude::*;

/// File server with multiple configured roots
#[derive(Clone)]
struct FileServer;

#[server(
    name = "FileServer",
    version = "1.0.0",
    description = "Demonstrates filesystem roots configuration",
    // Configure multiple filesystem roots
    root = "file:///workspace:Workspace Files",
    root = "file:///tmp:Temporary Files",
    root = "file:///home:Home Directory"
)]
impl FileServer {
    #[tool("List available filesystem roots")]
    async fn list_roots(&self, ctx: Context) -> McpResult<String> {
        ctx.info("Listing configured filesystem roots").await?;

        Ok(r#"Available roots:
1. file:///workspace - Workspace Files
2. file:///tmp - Temporary Files
3. file:///home - Home Directory

Use these URIs to access resources within each root."#
            .to_string())
    }

    #[resource("file:///{root}/status")]
    async fn root_status(&self, uri: String) -> McpResult<String> {
        // Extract root from URI
        let root = uri
            .strip_prefix("file:///")
            .and_then(|s| s.strip_suffix("/status"))
            .unwrap_or("unknown");

        Ok(format!(
            "Root: {}\nStatus: Active\nPermissions: Read/Write",
            root
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // CRITICAL: For MCP STDIO protocol, do NOT initialize logging to stdout

    let server = FileServer;
    server.run_stdio().await?;

    Ok(())
}

/* 📝 **Key Concepts:**

**Roots in MCP:**
- Roots define filesystem locations the server can access
- Clients discover roots via the roots/list request
- Each root has a URI and optional human-readable name
- Roots provide security boundaries

**Macro Configuration:**
- `root = "URI:Name"` syntax in #[server] macro
- Multiple roots supported
- Automatic root registration during initialization

**Root URIs:**
- Format: `file:///path:Display Name`
- Path is the filesystem location
- Name helps users identify the purpose

**Security:**
- Servers should only access files within configured roots
- Roots act as security boundaries
- Clients can trust servers stay within declared boundaries

**Builder API Alternative:**
```rust,ignore
ServerBuilder::new()
    .root("file:///workspace", Some("Workspace".to_string()))
    .root("file:///tmp", Some("Temp".to_string()))
    .build()
```

**Next Example:** `16_robust_transport.rs` - Circuit breakers and resilience
*/
