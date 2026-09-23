//! Build script for compiling Protocol Buffer definitions
//!
//! This script uses tonic-prost-build to generate Rust code from the MCP proto definitions.
//!
//! It needs the `protoc` compiler: on `PATH`, or named by the `PROTOC`
//! environment variable. When it is missing, prost-build fails with an error
//! that says so and how to install it for the host OS, so there is no wrapper
//! here.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Tell Cargo to rerun this build script if the proto file changes
    println!("cargo:rerun-if-changed=src/proto/mcp.proto");

    // Configure and compile the proto file
    tonic_prost_build::configure()
        // Generate server code
        .build_server(true)
        // Generate client code
        .build_client(true)
        // Compile the proto file
        .compile_protos(&["src/proto/mcp.proto"], &["src/proto"])?;

    Ok(())
}
