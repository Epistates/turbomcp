//! Deploy command implementation for MCP servers.
//!
//! Supports deploying to Cloudflare Workers and other edge platforms.

use crate::cli::{BuildArgs, DeployArgs, WasmPlatform};
use crate::error::{CliError, CliResult};
use std::path::Path;
use std::process::Command;

/// Execute the deploy command.
pub fn execute(args: &DeployArgs) -> CliResult<()> {
    let project_path = args.path.canonicalize().map_err(|e| {
        CliError::Other(format!(
            "Failed to resolve project path '{}': {}",
            args.path.display(),
            e
        ))
    })?;

    println!("Deploying MCP server to {}...", args.platform);

    // Build first unless skipped
    if !args.skip_build {
        build_for_deploy(args, &project_path)?;
    }

    // Deploy based on platform
    match args.platform {
        WasmPlatform::CloudflareWorkers => deploy_cloudflare(args, &project_path),
        WasmPlatform::DenoWorkers => deploy_deno(args, &project_path),
        WasmPlatform::Wasm32 => Err(CliError::NotSupported(
            "Generic WASM32 deployment is not supported. Please specify a platform.".to_string(),
        )),
    }
}

/// Build the project for deployment.
fn build_for_deploy(args: &DeployArgs, project_path: &Path) -> CliResult<()> {
    let build_args = BuildArgs {
        path: project_path.to_path_buf(),
        platform: Some(args.platform.clone()),
        target: None,
        release: args.release,
        optimize: args.optimize,
        features: vec![],
        no_default_features: false,
        output: None,
    };

    crate::build::execute(&build_args)
}

/// Deploy to Cloudflare Workers using wrangler.
fn deploy_cloudflare(args: &DeployArgs, project_path: &Path) -> CliResult<()> {
    // Check if wrangler is available
    let wrangler_check = Command::new("wrangler").arg("--version").output();

    if wrangler_check.is_err() {
        return Err(CliError::Other(
            "wrangler CLI not found. Install with: npm install -g wrangler".to_string(),
        ));
    }

    // Determine wrangler config path
    let config_path = args
        .wrangler_config
        .clone()
        .unwrap_or_else(|| project_path.join("wrangler.toml"));

    if !config_path.exists() {
        return Err(CliError::Other(format!(
            "wrangler.toml not found at '{}'. Create one or specify --wrangler-config",
            config_path.display()
        )));
    }

    // Build wrangler command
    let mut cmd = Command::new("wrangler");
    cmd.arg("deploy");
    cmd.current_dir(project_path);

    // Add config path if not default
    if args.wrangler_config.is_some() {
        cmd.arg("--config").arg(&config_path);
    }

    // Add environment if specified
    if let Some(ref env) = args.env {
        cmd.arg("--env").arg(env);
    }

    // Dry run
    if args.dry_run {
        cmd.arg("--dry-run");
    }

    println!(
        "Running: wrangler deploy{}",
        if args.dry_run { " --dry-run" } else { "" }
    );

    // Execute wrangler deploy
    let status = cmd
        .status()
        .map_err(|e| CliError::Other(format!("Failed to execute wrangler: {}", e)))?;

    if !status.success() {
        return Err(CliError::Other("Wrangler deploy failed".to_string()));
    }

    if args.dry_run {
        println!("Dry run complete. No changes were made.");
    } else {
        println!("Deployment successful!");
    }

    Ok(())
}

/// Deploy to Deno Deploy.
fn deploy_deno(args: &DeployArgs, project_path: &Path) -> CliResult<()> {
    // Check if deployctl is available
    let deployctl_check = Command::new("deployctl").arg("--version").output();

    if deployctl_check.is_err() {
        return Err(CliError::Other(
            "deployctl not found. Install with: deno install --allow-all --name=deployctl https://deno.land/x/deploy/deployctl.ts".to_string(),
        ));
    }

    // Look for deno.json or main entry point
    let config_path = project_path.join("deno.json");
    let main_path = if config_path.exists() {
        // Read config to find entry point
        None // Will use default from config
    } else {
        // Look for common entry points
        let candidates = ["main.ts", "mod.ts", "index.ts", "src/main.ts"];
        candidates
            .iter()
            .map(|c| project_path.join(c))
            .find(|p| p.exists())
    };

    // Build deployctl command
    let mut cmd = Command::new("deployctl");
    cmd.arg("deploy");
    cmd.current_dir(project_path);

    // Add entry point if found
    if let Some(ref main) = main_path {
        cmd.arg("--entrypoint").arg(main);
    }

    // Add environment (project name)
    if let Some(ref env) = args.env {
        cmd.arg("--project").arg(env);
    }

    // Dry run
    if args.dry_run {
        cmd.arg("--dry-run");
    }

    println!(
        "Running: deployctl deploy{}",
        if args.dry_run { " --dry-run" } else { "" }
    );

    // Execute deployctl
    let status = cmd
        .status()
        .map_err(|e| CliError::Other(format!("Failed to execute deployctl: {}", e)))?;

    if !status.success() {
        return Err(CliError::Other("Deno Deploy failed".to_string()));
    }

    if args.dry_run {
        println!("Dry run complete. No changes were made.");
    } else {
        println!("Deployment successful!");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_display() {
        assert_eq!(
            WasmPlatform::CloudflareWorkers.to_string(),
            "cloudflare-workers"
        );
        assert_eq!(WasmPlatform::DenoWorkers.to_string(), "deno-workers");
        assert_eq!(WasmPlatform::Wasm32.to_string(), "wasm32");
    }
}
