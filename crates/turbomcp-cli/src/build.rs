//! Build command implementation for MCP servers.
//!
//! Supports building for native and WASM targets, with platform-specific
//! optimizations for Cloudflare Workers and other edge runtimes.

use crate::cli::{BuildArgs, WasmPlatform};
use crate::error::{CliError, CliResult};
use std::path::Path;
use std::process::Command;

/// Execute the build command.
pub fn execute(args: &BuildArgs) -> CliResult<()> {
    let project_path = args.path.canonicalize().map_err(|e| {
        CliError::Other(format!(
            "Failed to resolve project path '{}': {}",
            args.path.display(),
            e
        ))
    })?;

    // Check if Cargo.toml exists
    let cargo_toml = project_path.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(CliError::Other(format!(
            "No Cargo.toml found at '{}'",
            project_path.display()
        )));
    }

    // Determine target based on platform or explicit target
    let target = determine_target(args)?;

    println!("Building MCP server...");
    if let Some(ref t) = target {
        println!("  Target: {}", t);
    }
    if args.release {
        println!("  Mode: release");
    } else {
        println!("  Mode: debug");
    }

    // Build the cargo command
    let mut cmd = Command::new("cargo");
    cmd.arg("build");
    cmd.current_dir(&project_path);

    // Add target if specified
    if let Some(ref t) = target {
        cmd.arg("--target").arg(t);
    }

    // Release mode
    if args.release {
        cmd.arg("--release");
    }

    // Features
    if args.no_default_features {
        cmd.arg("--no-default-features");
    }

    for feature in &args.features {
        cmd.arg("--features").arg(feature);
    }

    // Execute cargo build
    let status = cmd
        .status()
        .map_err(|e| CliError::Other(format!("Failed to execute cargo build: {}", e)))?;

    if !status.success() {
        return Err(CliError::Other("Cargo build failed".to_string()));
    }

    println!("Build successful!");

    // Determine output path
    let profile = if args.release { "release" } else { "debug" };
    let target_dir = project_path.join("target");

    let output_dir = if let Some(ref t) = target {
        target_dir.join(t).join(profile)
    } else {
        target_dir.join(profile)
    };

    // For WASM targets, run wasm-opt if requested
    if args.optimize && target.as_ref().is_some_and(|t| t.contains("wasm")) {
        optimize_wasm(&output_dir, args)?;
    }

    // Copy to output directory if specified
    if let Some(ref output) = args.output {
        copy_artifacts(&output_dir, output, &target)?;
    }

    // Print output location
    if let Some(ref output) = args.output {
        println!("Artifacts copied to: {}", output.display());
    } else {
        println!("Artifacts at: {}", output_dir.display());
    }

    Ok(())
}

/// Determine the Rust target based on platform or explicit target argument.
fn determine_target(args: &BuildArgs) -> CliResult<Option<String>> {
    // Explicit target takes precedence
    if let Some(ref target) = args.target {
        return Ok(Some(target.clone()));
    }

    // Platform-specific targets
    if let Some(ref platform) = args.platform {
        let target = match platform {
            WasmPlatform::CloudflareWorkers | WasmPlatform::DenoWorkers | WasmPlatform::Wasm32 => {
                "wasm32-unknown-unknown"
            }
        };
        return Ok(Some(target.to_string()));
    }

    // No target specified - build for native
    Ok(None)
}

/// Optimize WASM binary using wasm-opt.
fn optimize_wasm(output_dir: &Path, args: &BuildArgs) -> CliResult<()> {
    // Check if wasm-opt is available
    let wasm_opt_check = Command::new("wasm-opt").arg("--version").output();

    if wasm_opt_check.is_err() {
        println!("Warning: wasm-opt not found, skipping optimization");
        println!("  Install with: cargo install wasm-opt");
        return Ok(());
    }

    println!("Optimizing WASM binary...");

    // Find all .wasm files in the output directory
    let wasm_files: Vec<_> = std::fs::read_dir(output_dir)
        .map_err(|e| CliError::Other(format!("Failed to read output directory: {}", e)))?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "wasm"))
        .collect();

    for entry in wasm_files {
        let wasm_path = entry.path();
        let optimized_path = wasm_path.with_extension("optimized.wasm");

        let opt_level = if args.release { "-O3" } else { "-O1" };

        let status = Command::new("wasm-opt")
            .arg(opt_level)
            .arg("-o")
            .arg(&optimized_path)
            .arg(&wasm_path)
            .status()
            .map_err(|e| CliError::Other(format!("Failed to run wasm-opt: {}", e)))?;

        if status.success() {
            // Replace original with optimized
            std::fs::rename(&optimized_path, &wasm_path)
                .map_err(|e| CliError::Other(format!("Failed to replace WASM file: {}", e)))?;

            // Get file size for reporting
            let metadata = std::fs::metadata(&wasm_path)
                .map_err(|e| CliError::Other(format!("Failed to get file metadata: {}", e)))?;
            let size_kb = metadata.len() / 1024;

            println!("  Optimized: {} ({}KB)", wasm_path.display(), size_kb);
        } else {
            println!("Warning: wasm-opt failed for {}", wasm_path.display());
        }
    }

    Ok(())
}

/// Copy build artifacts to the specified output directory.
fn copy_artifacts(source_dir: &Path, output_dir: &Path, target: &Option<String>) -> CliResult<()> {
    // Create output directory
    std::fs::create_dir_all(output_dir)
        .map_err(|e| CliError::Other(format!("Failed to create output directory: {}", e)))?;

    // Determine which files to copy based on target
    let is_wasm = target.as_ref().is_some_and(|t| t.contains("wasm"));

    if is_wasm {
        // Copy .wasm files
        for entry in std::fs::read_dir(source_dir)
            .map_err(|e| CliError::Other(format!("Failed to read source directory: {}", e)))?
        {
            let entry =
                entry.map_err(|e| CliError::Other(format!("Failed to read entry: {}", e)))?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "wasm") {
                let dest = output_dir.join(path.file_name().unwrap());
                std::fs::copy(&path, &dest)
                    .map_err(|e| CliError::Other(format!("Failed to copy file: {}", e)))?;
            }
        }
    } else {
        // Copy binary files (no extension on Unix, .exe on Windows)
        for entry in std::fs::read_dir(source_dir)
            .map_err(|e| CliError::Other(format!("Failed to read source directory: {}", e)))?
        {
            let entry =
                entry.map_err(|e| CliError::Other(format!("Failed to read entry: {}", e)))?;
            let path = entry.path();

            if path.is_file() {
                let is_binary = if cfg!(windows) {
                    path.extension().is_some_and(|ext| ext == "exe")
                } else {
                    path.extension().is_none()
                        && std::fs::metadata(&path)
                            .map(|m| m.permissions().mode() & 0o111 != 0)
                            .unwrap_or(false)
                };

                if is_binary {
                    let dest = output_dir.join(path.file_name().unwrap());
                    std::fs::copy(&path, &dest)
                        .map_err(|e| CliError::Other(format!("Failed to copy file: {}", e)))?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

#[cfg(not(unix))]
trait PermissionsExt {
    fn mode(&self) -> u32 {
        0
    }
}

#[cfg(not(unix))]
impl PermissionsExt for std::fs::Permissions {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_determine_target_explicit() {
        let args = BuildArgs {
            path: ".".into(),
            platform: None,
            target: Some("x86_64-unknown-linux-gnu".to_string()),
            release: false,
            optimize: false,
            features: vec![],
            no_default_features: false,
            output: None,
        };

        let target = determine_target(&args).unwrap();
        assert_eq!(target, Some("x86_64-unknown-linux-gnu".to_string()));
    }

    #[test]
    fn test_determine_target_platform() {
        let args = BuildArgs {
            path: ".".into(),
            platform: Some(WasmPlatform::CloudflareWorkers),
            target: None,
            release: false,
            optimize: false,
            features: vec![],
            no_default_features: false,
            output: None,
        };

        let target = determine_target(&args).unwrap();
        assert_eq!(target, Some("wasm32-unknown-unknown".to_string()));
    }

    #[test]
    fn test_determine_target_none() {
        let args = BuildArgs {
            path: ".".into(),
            platform: None,
            target: None,
            release: false,
            optimize: false,
            features: vec![],
            no_default_features: false,
            output: None,
        };

        let target = determine_target(&args).unwrap();
        assert_eq!(target, None);
    }
}
