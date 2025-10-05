//! Integration tests to ensure TurboMCP macros work correctly as external dependencies
//!
//! These tests prevent the catastrophic 1.1.0 issue where macros worked internally
//! but completely failed when used as external dependencies.

use std::fs;
use std::process::Command;
use tempfile::TempDir;

/// Test that the #[server] macro compiles correctly as an external dependency
///
/// This test creates a temporary project that depends on TurboMCP and verifies
/// that the macro system generates code that compiles successfully.
#[test]
fn test_server_macro_external_dependency_compilation() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let project_path = temp_dir.path();

    // Create a test Cargo.toml that uses TurboMCP as external dependency
    let cargo_toml = format!(
        r#"[package]
name = "external-test"
version = "0.1.0"
edition = "2021"

[dependencies]
turbomcp = {{ path = "{}", features = ["http", "tcp", "unix"] }}
tokio = {{ version = "1.0", features = ["rt", "rt-multi-thread", "macros"] }}
serde_json = "1.0"
"#,
        std::env::current_dir()
            .unwrap()
            .join("crates/turbomcp")
            .display()
    );

    fs::write(project_path.join("Cargo.toml"), cargo_toml)
        .expect("Failed to write Cargo.toml");

    // Create src directory
    fs::create_dir_all(project_path.join("src"))
        .expect("Failed to create src directory");

    // Create a comprehensive test main.rs that exercises the macro system
    let main_rs = r#"
use turbomcp::prelude::*;
use std::sync::Arc;

#[derive(Clone)]
struct TestServer {
    counter: Arc<std::sync::atomic::AtomicU64>,
}

#[server(name = "external-test", version = "1.0.0")]
impl TestServer {
    #[tool("Add two numbers")]
    async fn add(&self, a: i32, b: i32) -> McpResult<i32> {
        Ok(a + b)
    }

    #[tool("Get system info")]
    async fn system_info(&self, ctx: Context) -> McpResult<String> {
        ctx.info("Getting system info").await?;
        Ok("System running".to_string())
    }

    #[prompt("Generate greeting")]
    async fn greeting(&self, name: String) -> McpResult<String> {
        Ok(format!("Hello, {}!", name))
    }

    #[resource(uri = "data://{id}", name = "test-resource")]
    async fn get_resource(&self, id: String) -> McpResult<String> {
        Ok(format!("Resource data for {}", id))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = TestServer {
        counter: Arc::new(std::sync::atomic::AtomicU64::new(0)),
    };

    // CRITICAL: Test all generated methods exist and work

    // Test Arc conversion and MCP-compliant router generation (these methods MUST exist)
    let server_arc = Arc::new(server.clone());
    let _router = server_arc.clone().into_mcp_router();
    let _router_with_path = server_arc.into_mcp_router_with_path("/test");

    // Test metadata access (must be public)
    let tools_metadata = TestServer::get_tools_metadata();
    assert!(!tools_metadata.is_empty(), "Tools metadata should not be empty");

    let prompts_metadata = TestServer::get_prompts_metadata();
    assert!(!prompts_metadata.is_empty(), "Prompts metadata should not be empty");

    let resources_metadata = TestServer::get_resources_metadata();
    assert!(!resources_metadata.is_empty(), "Resources metadata should not be empty");

    // Test direct tool calling
    let result = server.test_tool_call("add", serde_json::json!({"a": 5, "b": 3})).await?;
    assert!(!result.content.is_empty(), "Tool result should have content");

    // Test server info access
    let (name, version, _) = TestServer::server_info();
    assert_eq!(name, "external-test");
    assert_eq!(version, "1.0.0");

    // Test transport methods exist (but don't actually run them)
    // These should compile even if we don't call them
    let _ = server.clone().into_server_with_shutdown();

    println!("External dependency test passed!");
    Ok(())
}
"#;

    fs::write(project_path.join("src/main.rs"), main_rs)
        .expect("Failed to write main.rs");

    // Run cargo check to verify compilation
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(project_path)
        .output()
        .expect("Failed to run cargo check");

    if !output.status.success() {
        panic!(
            "External dependency test failed to compile!\nSTDOUT:\n{}\nSTDERR:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Also run the compiled binary to ensure runtime works
    let build_output = Command::new("cargo")
        .arg("build")
        .current_dir(project_path)
        .output()
        .expect("Failed to build external test");

    if !build_output.status.success() {
        panic!(
            "External dependency test failed to build!\nSTDOUT:\n{}\nSTDERR:\n{}",
            String::from_utf8_lossy(&build_output.stdout),
            String::from_utf8_lossy(&build_output.stderr)
        );
    }

    let run_output = Command::new("./target/debug/external-test")
        .current_dir(project_path)
        .output()
        .expect("Failed to run external test binary");

    if !run_output.status.success() {
        panic!(
            "External dependency test failed at runtime!\nSTDOUT:\n{}\nSTDERR:\n{}",
            String::from_utf8_lossy(&run_output.stdout),
            String::from_utf8_lossy(&run_output.stderr)
        );
    }

    let stdout = String::from_utf8_lossy(&run_output.stdout);
    assert!(
        stdout.contains("External dependency test passed!"),
        "External test did not complete successfully: {}",
        stdout
    );
}

/// Test that macro-generated code doesn't use direct imports that break externally
#[test]
fn test_no_direct_imports_in_generated_code() {
    use std::fs;
    use std::path::Path;

    let macro_src_dir = Path::new("crates/turbomcp-macros/src");

    // Check all Rust files in the macros crate for problematic patterns
    let rust_files = fs::read_dir(macro_src_dir)
        .expect("Failed to read macro src directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.extension()? == "rs" {
                Some(path)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

    for file_path in rust_files {
        let content = fs::read_to_string(&file_path)
            .expect(&format!("Failed to read {:?}", file_path));

        // Check for direct imports that would break external dependencies
        let problematic_patterns = vec![
            "use axum::",  // Should be ::turbomcp::axum::
            "use tokio::", // Should be ::turbomcp::tokio::
            "use turbomcp_protocol::", // Should be ::turbomcp::turbomcp_protocol::
            "use turbomcp_core::", // Should be ::turbomcp::turbomcp_core::
            "axum::", // Should be ::turbomcp::axum::
            "tokio::", // Should be ::turbomcp::tokio::
            // Allow ::turbomcp:: prefixed versions
        ];

        for pattern in problematic_patterns {
            if content.contains(pattern) && !content.contains(&format!("::{}", pattern)) {
                panic!(
                    "File {:?} contains problematic direct import pattern: '{}'
This pattern breaks external dependencies. Use ::turbomcp:: prefixed version instead.",
                    file_path, pattern
                );
            }
        }

        // Check for feature gates in generated code
        if content.contains("#[cfg(feature =") {
            // This is allowed only in specific non-generated sections
            // For now, we've removed them all, but this test catches if they get re-added
            let lines: Vec<&str> = content.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                if line.contains("#[cfg(feature =") {
                    // Allow in specific contexts like test modules
                    if i > 0 && lines[i - 1].contains("#[cfg(test)]") {
                        continue;
                    }
                    if line.contains("test") || line.contains("Test") {
                        continue;
                    }

                    panic!(
                        "File {:?} line {} contains feature gate in generated code: '{}'
Feature gates in macro-generated code break external dependencies.",
                        file_path, i + 1, line.trim()
                    );
                }
            }
        }
    }
}

/// Test that re-exports exist for all dependencies used in generated code
#[test]
fn test_required_reexports_exist() {
    use std::fs;

    let turbomcp_lib = fs::read_to_string("crates/turbomcp/src/lib.rs")
        .expect("Failed to read turbomcp lib.rs");

    // Verify critical re-exports exist
    let required_reexports = vec![
        "pub use axum;",
        "pub use tokio;",
        "pub use turbomcp_core;",
        "pub use turbomcp_protocol;",
        "pub use tracing;",
    ];

    for reexport in required_reexports {
        assert!(
            turbomcp_lib.contains(reexport),
            "Missing critical re-export: {} - this will break macro-generated code",
            reexport
        );
    }
}

/// Test multiple feature combinations work externally
#[test]
fn test_feature_combinations_external() {
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let project_path = temp_dir.path();

    // Test different feature combinations
    let feature_combinations = vec![
        vec!["http"],
        vec!["tcp"],
        vec!["unix"],
        vec!["http", "tcp"],
        vec!["http", "unix"],
        vec!["tcp", "unix"],
        vec!["http", "tcp", "unix"],
    ];

    for features in feature_combinations {
        let features_str = features.join("\", \"");

        let cargo_toml = format!(
            r#"[package]
name = "feature-test"
version = "0.1.0"
edition = "2021"

[dependencies]
turbomcp = {{ path = "{}", features = ["{}"] }}
tokio = {{ version = "1.0", features = ["rt", "macros"] }}
"#,
            std::env::current_dir()
                .unwrap()
                .join("crates/turbomcp")
                .display(),
            features_str
        );

        fs::write(project_path.join("Cargo.toml"), cargo_toml)
            .expect("Failed to write Cargo.toml");

        fs::create_dir_all(project_path.join("src"))
            .expect("Failed to create src directory");

        let main_rs = r#"
use turbomcp::prelude::*;

#[derive(Clone)]
struct TestServer;

#[server(name = "feature-test", version = "1.0.0")]
impl TestServer {
    #[tool("test tool")]
    async fn test(&self) -> McpResult<String> {
        Ok("test".to_string())
    }
}

fn main() {
    let server = TestServer;
    let _metadata = TestServer::get_tools_metadata();
    println!("Feature combination test passed!");
}
"#;

        fs::write(project_path.join("src/main.rs"), main_rs)
            .expect("Failed to write main.rs");

        let output = Command::new("cargo")
            .arg("check")
            .current_dir(project_path)
            .output()
            .expect("Failed to run cargo check");

        if !output.status.success() {
            panic!(
                "Feature combination {:?} failed to compile!\nSTDOUT:\n{}\nSTDERR:\n{}",
                features,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
}