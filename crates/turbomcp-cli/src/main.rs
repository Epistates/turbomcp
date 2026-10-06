//! The `turbomcp` command: serve the servers in an mcpServers configuration
//! as one MCP server, and run protocol operations against any server.

use std::collections::BTreeMap;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

mod config;
mod ops;
mod proxy;
mod target;

use target::Target;

/// MCP from the command line: a gateway over an mcpServers configuration,
/// and protocol operations against any server (a URL, or a command to run
/// over stdio).
#[derive(Parser)]
#[command(name = "turbomcp", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Serve the servers in an mcpServers configuration as one MCP server.
    Proxy(proxy::ProxyArgs),
    /// List a server's tools.
    Tools {
        /// Print the protocol's JSON.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        target: Target,
    },
    /// List a server's resources and resource templates.
    Resources {
        /// Print the protocol's JSON.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        target: Target,
    },
    /// List a server's prompts.
    Prompts {
        /// Print the protocol's JSON.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        target: Target,
    },
    /// Call a tool.
    Call {
        /// The tool.
        tool: String,
        /// An argument, as `key=value` (a value that parses as JSON is JSON;
        /// repeatable).
        #[arg(short = 'a', long = "arg", value_name = "KEY=VALUE")]
        args: Vec<String>,
        /// All the arguments, as a JSON object (`--arg` adds to it).
        #[arg(long = "args", value_name = "JSON")]
        json_args: Option<String>,
        /// Print the protocol's JSON.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        target: Target,
    },
    /// Read a resource.
    Read {
        /// The resource's URI.
        uri: String,
        /// Print the protocol's JSON.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        target: Target,
    },
    /// Get a prompt.
    Prompt {
        /// The prompt.
        name: String,
        /// An argument, as `key=value` (repeatable).
        #[arg(short = 'a', long = "arg", value_name = "KEY=VALUE")]
        args: Vec<String>,
        /// Print the protocol's JSON.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        target: Target,
    },
    /// Show which revisions a server negotiates, who it says it is, and what
    /// it declares and offers.
    Probe {
        /// Print JSON.
        #[arg(long)]
        json: bool,
        #[command(flatten)]
        target: Target,
    },
}

fn prompt_arguments(pairs: &[String]) -> Result<BTreeMap<String, String>> {
    pairs
        .iter()
        .map(|pair| {
            let (key, value) = pair
                .split_once('=')
                .with_context(|| format!("argument `{pair}` is not `key=value`"))?;
            Ok((key.to_owned(), value.to_owned()))
        })
        .collect()
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Proxy(args) => proxy::run(args).await,
        Command::Tools { json, target } => ops::tools(&target, json).await,
        Command::Resources { json, target } => ops::resources(&target, json).await,
        Command::Prompts { json, target } => ops::prompts(&target, json).await,
        Command::Call {
            tool,
            args,
            json_args,
            json,
            target,
        } => {
            let args = ops::arguments(json_args.as_deref(), &args)?;
            ops::call(&target, &tool, args, json).await
        }
        Command::Read { uri, json, target } => ops::read(&target, &uri, json).await,
        Command::Prompt {
            name,
            args,
            json,
            target,
        } => ops::prompt(&target, &name, prompt_arguments(&args)?, json).await,
        Command::Probe { json, target } => ops::probe(&target, json).await,
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    // stdout is the protocol under `proxy` (and the output otherwise): logs
    // go to stderr.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}
