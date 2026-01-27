//! Development server command with hot reload support.
//!
//! This module implements the `turbomcp dev` command which provides:
//! - Hot reload development server using cargo-watch
//! - MCP Inspector integration for debugging
//!
//! # Usage
//!
//! ```bash
//! # Run server with hot reload
//! turbomcp dev ./my-server --watch
//!
//! # Run with inspector
//! turbomcp dev ./my-server --inspector
//!
//! # Build in release mode
//! turbomcp dev ./my-server --release
//! ```

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::cli::DevArgs;

/// Execute the dev command.
pub fn execute(args: &DevArgs) -> Result<()> {
    let path = &args.path;

    // Determine if this is a cargo project or a binary
    let is_cargo_project = path.is_dir() && path.join("Cargo.toml").exists();
    let is_binary = path.is_file() && is_executable(path);

    if !is_cargo_project && !is_binary {
        bail!(
            "Path '{}' is neither a Cargo project nor an executable binary.\n\
             For a Cargo project, provide the directory containing Cargo.toml.\n\
             For a binary, provide the path to the executable.",
            path.display()
        );
    }

    if args.watch {
        run_with_watch(args, is_cargo_project)
    } else if is_cargo_project {
        run_cargo_project(args)
    } else {
        run_binary(args)
    }
}

/// Run with cargo-watch for hot reload.
fn run_with_watch(args: &DevArgs, is_cargo_project: bool) -> Result<()> {
    // Check if cargo-watch is installed
    if !is_command_available("cargo-watch") {
        eprintln!("cargo-watch is not installed. Install it with:");
        eprintln!("  cargo install cargo-watch");
        eprintln!();
        eprintln!("Then run this command again.");
        bail!("cargo-watch not found");
    }

    if !is_cargo_project {
        bail!("--watch requires a Cargo project directory, not a binary");
    }

    println!("Starting development server with hot reload...");
    println!("  Project: {}", args.path.display());
    println!("  Mode: {}", if args.release { "release" } else { "debug" });
    println!();
    println!("Press Ctrl+C to stop.");
    println!();

    let mut cmd = Command::new("cargo");
    cmd.arg("watch")
        .arg("-x")
        .arg(build_cargo_run_args(args))
        .current_dir(&args.path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd.status().context("Failed to run cargo-watch")?;

    if !status.success() {
        bail!("cargo-watch exited with non-zero status");
    }

    Ok(())
}

/// Run a Cargo project directly (no watch).
fn run_cargo_project(args: &DevArgs) -> Result<()> {
    println!("Starting development server...");
    println!("  Project: {}", args.path.display());
    println!("  Mode: {}", if args.release { "release" } else { "debug" });
    println!();

    let mut cmd = Command::new("cargo");
    cmd.arg("run");

    if args.release {
        cmd.arg("--release");
    }

    if !args.server_args.is_empty() {
        cmd.arg("--");
        cmd.args(&args.server_args);
    }

    cmd.current_dir(&args.path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd.status().context("Failed to run cargo")?;

    if !status.success() {
        bail!("Server exited with non-zero status");
    }

    Ok(())
}

/// Run a binary directly.
fn run_binary(args: &DevArgs) -> Result<()> {
    println!("Starting MCP server...");
    println!("  Binary: {}", args.path.display());
    println!();

    let mut cmd = Command::new(&args.path);
    cmd.args(&args.server_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status = cmd.status().context("Failed to run server binary")?;

    if !status.success() {
        bail!("Server exited with non-zero status");
    }

    Ok(())
}

/// Build the cargo run arguments string for cargo-watch.
fn build_cargo_run_args(args: &DevArgs) -> String {
    let mut cmd_args = vec!["run".to_string()];

    if args.release {
        cmd_args.push("--release".to_string());
    }

    if !args.server_args.is_empty() {
        cmd_args.push("--".to_string());
        cmd_args.extend(args.server_args.clone());
    }

    cmd_args.join(" ")
}

/// Check if a command is available in PATH.
fn is_command_available(cmd: &str) -> bool {
    Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Check if a path is an executable file.
fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .map(|m| m.is_file() && (m.permissions().mode() & 0o111) != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        path.is_file()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_build_cargo_run_args_basic() {
        let args = DevArgs {
            path: PathBuf::from("."),
            watch: false,
            server_args: vec![],
            release: false,
            inspector: false,
            inspector_port: 5173,
        };

        assert_eq!(build_cargo_run_args(&args), "run");
    }

    #[test]
    fn test_build_cargo_run_args_release() {
        let args = DevArgs {
            path: PathBuf::from("."),
            watch: false,
            server_args: vec![],
            release: true,
            inspector: false,
            inspector_port: 5173,
        };

        assert_eq!(build_cargo_run_args(&args), "run --release");
    }

    #[test]
    fn test_build_cargo_run_args_with_server_args() {
        let args = DevArgs {
            path: PathBuf::from("."),
            watch: false,
            server_args: vec!["--port".to_string(), "8080".to_string()],
            release: false,
            inspector: false,
            inspector_port: 5173,
        };

        assert_eq!(build_cargo_run_args(&args), "run -- --port 8080");
    }
}
