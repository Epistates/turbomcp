//! CLI argument parsing and configuration types - Enhanced version

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Main CLI application structure
#[derive(Parser, Debug)]
#[command(
    name = "turbomcp-cli",
    version,
    about = "Comprehensive CLI for MCP servers - complete protocol support with rich UX",
    long_about = "TurboMCP CLI provides comprehensive access to MCP (Model Context Protocol) servers.\n\
                  Supports all MCP operations: tools, resources, prompts, completions, sampling, and more.\n\
                  Multiple transports: stdio, HTTP SSE, WebSocket, TCP, Unix sockets.\n\n\
                  SECURITY WARNINGS:\n\
                  - STDIO transport executes arbitrary commands on your system\n\
                  - Only use --command with trusted MCP servers from verified sources\n\
                  - Auth tokens passed via --auth or MCP_AUTH may be logged or exposed\n\
                  - Consider using environment variables or config files for sensitive credentials\n\
                  - Review server permissions before executing tools that modify data"
)]
pub struct Cli {
    /// Subcommand to run
    #[command(subcommand)]
    pub command: Commands,

    /// Output format
    #[arg(long, short = 'f', global = true, value_enum, default_value = "human")]
    pub format: OutputFormat,

    /// Enable verbose output
    #[arg(long, short = 'v', global = true)]
    pub verbose: bool,

    /// Connection config name (from ~/.turbomcp/config.yaml)
    #[arg(long, short = 'c', global = true)]
    pub connection: Option<String>,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,
}

/// Available CLI subcommands - Complete MCP coverage
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Tool operations
    #[command(subcommand)]
    Tools(ToolCommands),

    /// Resource operations
    #[command(subcommand)]
    Resources(ResourceCommands),

    /// Prompt operations
    #[command(subcommand)]
    Prompts(PromptCommands),

    /// Completion operations
    #[command(subcommand)]
    Complete(CompletionCommands),

    /// Server management
    #[command(subcommand)]
    Server(ServerCommands),

    /// Sampling operations (advanced)
    #[command(subcommand)]
    Sample(SamplingCommands),

    /// Interactive connection wizard
    Connect(Connection),

    /// Connection status
    Status(Connection),

    /// Development server with hot reload
    Dev(DevArgs),

    /// Install MCP server to Claude Desktop or Cursor
    Install(InstallArgs),

    /// Build an MCP server (supports WASM targets)
    Build(BuildArgs),

    /// Deploy an MCP server to cloud platforms
    Deploy(DeployArgs),

    /// Create a new MCP server project from a template
    New(NewArgs),
}

/// Tool-related commands
#[derive(Subcommand, Debug)]
pub enum ToolCommands {
    /// List available tools
    List {
        #[command(flatten)]
        conn: Connection,
    },

    /// Call a tool
    Call {
        #[command(flatten)]
        conn: Connection,

        /// Tool name
        name: String,

        /// Arguments as JSON object
        #[arg(long, short = 'a', default_value = "{}")]
        arguments: String,
    },

    /// Get tool schema
    Schema {
        #[command(flatten)]
        conn: Connection,

        /// Tool name (omit to get all schemas)
        name: Option<String>,
    },

    /// Export all tool schemas
    Export {
        #[command(flatten)]
        conn: Connection,

        /// Output directory
        #[arg(long, short = 'o')]
        output: PathBuf,
    },
}

/// Resource-related commands
#[derive(Subcommand, Debug)]
pub enum ResourceCommands {
    /// List resources
    List {
        #[command(flatten)]
        conn: Connection,
    },

    /// Read resource content
    Read {
        #[command(flatten)]
        conn: Connection,

        /// Resource URI
        uri: String,
    },

    /// List resource templates
    Templates {
        #[command(flatten)]
        conn: Connection,
    },

    /// Subscribe to resource updates
    Subscribe {
        #[command(flatten)]
        conn: Connection,

        /// Resource URI
        uri: String,
    },

    /// Unsubscribe from resource updates
    Unsubscribe {
        #[command(flatten)]
        conn: Connection,

        /// Resource URI
        uri: String,
    },
}

/// Prompt-related commands
#[derive(Subcommand, Debug)]
pub enum PromptCommands {
    /// List prompts
    List {
        #[command(flatten)]
        conn: Connection,
    },

    /// Get prompt with arguments
    Get {
        #[command(flatten)]
        conn: Connection,

        /// Prompt name
        name: String,

        /// Arguments as JSON object
        #[arg(long, short = 'a', default_value = "{}")]
        arguments: String,
    },

    /// Get prompt schema
    Schema {
        #[command(flatten)]
        conn: Connection,

        /// Prompt name
        name: String,
    },
}

/// Completion commands
#[derive(Subcommand, Debug)]
pub enum CompletionCommands {
    /// Get completions for a reference
    Get {
        #[command(flatten)]
        conn: Connection,

        /// Reference type (prompt, resource, etc.)
        #[arg(value_enum)]
        ref_type: RefType,

        /// Reference value
        ref_value: String,

        /// Argument name (for prompt arguments)
        #[arg(long)]
        argument: Option<String>,
    },
}

/// Server management commands
#[derive(Subcommand, Debug)]
pub enum ServerCommands {
    /// Get server info
    Info {
        #[command(flatten)]
        conn: Connection,
    },

    /// Ping server
    Ping {
        #[command(flatten)]
        conn: Connection,
    },

    /// Set server log level
    LogLevel {
        #[command(flatten)]
        conn: Connection,

        /// Log level
        #[arg(value_enum)]
        level: LogLevel,
    },

    /// List roots
    Roots {
        #[command(flatten)]
        conn: Connection,
    },
}

/// Sampling commands (advanced)
#[derive(Subcommand, Debug)]
pub enum SamplingCommands {
    /// Create a message sample
    Create {
        #[command(flatten)]
        conn: Connection,

        /// Messages as JSON array
        messages: String,

        /// Model preferences
        #[arg(long)]
        model_preferences: Option<String>,

        /// System prompt
        #[arg(long)]
        system_prompt: Option<String>,

        /// Max tokens
        #[arg(long)]
        max_tokens: Option<u32>,
    },
}

/// Connection configuration
#[derive(Args, Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    /// Transport protocol (auto-detected if not specified)
    #[arg(long, value_enum)]
    pub transport: Option<TransportKind>,

    /// Server URL or command
    #[arg(long, env = "MCP_URL", default_value = "http://localhost:8080/mcp")]
    pub url: String,

    /// Command for stdio transport (overrides --url)
    #[arg(long, env = "MCP_COMMAND")]
    pub command: Option<String>,

    /// Bearer token or API key
    #[arg(long, env = "MCP_AUTH")]
    pub auth: Option<String>,

    /// Connection timeout in seconds
    #[arg(long, default_value = "30")]
    pub timeout: u64,
}

/// Transport types - Extended
#[derive(Debug, Clone, ValueEnum, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    /// Standard input/output
    Stdio,
    /// HTTP with Server-Sent Events
    Http,
    /// WebSocket
    Ws,
    /// TCP socket
    Tcp,
    /// Unix domain socket
    Unix,
}

/// Output formats
#[derive(Debug, Clone, ValueEnum, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable with colors
    Human,
    /// JSON output
    Json,
    /// YAML output
    Yaml,
    /// Table format
    Table,
    /// Compact JSON (no pretty print)
    Compact,
}

/// Reference types for completions
#[derive(Debug, Clone, ValueEnum, PartialEq, Eq)]
pub enum RefType {
    /// Prompt reference
    Prompt,
    /// Resource reference
    Resource,
}

/// Log levels
#[derive(Debug, Clone, ValueEnum, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

impl From<LogLevel> for turbomcp_protocol::types::LogLevel {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Debug => turbomcp_protocol::types::LogLevel::Debug,
            LogLevel::Info => turbomcp_protocol::types::LogLevel::Info,
            LogLevel::Warning => turbomcp_protocol::types::LogLevel::Warning,
            LogLevel::Error => turbomcp_protocol::types::LogLevel::Error,
        }
    }
}

/// Development server arguments
#[derive(Args, Debug, Clone)]
pub struct DevArgs {
    /// Path to the server binary or cargo project
    pub path: PathBuf,

    /// Enable hot reload with cargo-watch
    #[arg(long, short = 'w')]
    pub watch: bool,

    /// Additional arguments to pass to the server
    #[arg(last = true)]
    pub server_args: Vec<String>,

    /// Build in release mode
    #[arg(long, short = 'r')]
    pub release: bool,

    /// Enable MCP Inspector integration
    #[arg(long)]
    pub inspector: bool,

    /// Port for the inspector (default: 5173)
    #[arg(long, default_value = "5173")]
    pub inspector_port: u16,
}

/// Install target applications
#[derive(Debug, Clone, ValueEnum, PartialEq, Eq)]
pub enum InstallTarget {
    /// Claude Desktop application
    ClaudeDesktop,
    /// Cursor IDE
    Cursor,
}

/// Install command arguments
#[derive(Args, Debug, Clone)]
pub struct InstallArgs {
    /// Target application to install to
    #[arg(value_enum)]
    pub target: InstallTarget,

    /// Path to the MCP server binary
    pub server_path: PathBuf,

    /// Name for the MCP server (defaults to binary name)
    #[arg(long, short = 'n')]
    pub name: Option<String>,

    /// Additional environment variables (KEY=VALUE)
    #[arg(long, short = 'e')]
    pub env: Vec<String>,

    /// Additional arguments to pass to the server
    #[arg(long, short = 'a')]
    pub args: Vec<String>,

    /// Force overwrite if server already exists
    #[arg(long, short = 'f')]
    pub force: bool,
}

// ============================================================================
// WASM Build, Deploy, and New Commands
// ============================================================================

/// Target platform for WASM builds
#[derive(Debug, Clone, ValueEnum, PartialEq, Eq)]
pub enum WasmPlatform {
    /// Cloudflare Workers
    CloudflareWorkers,
    /// Deno Deploy
    DenoWorkers,
    /// Generic WASM32
    Wasm32,
}

impl std::fmt::Display for WasmPlatform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CloudflareWorkers => write!(f, "cloudflare-workers"),
            Self::DenoWorkers => write!(f, "deno-workers"),
            Self::Wasm32 => write!(f, "wasm32"),
        }
    }
}

/// Project template for new MCP server
#[derive(Debug, Clone, ValueEnum, PartialEq, Eq)]
pub enum ProjectTemplate {
    /// Minimal MCP server (tools only)
    Minimal,
    /// Full-featured MCP server (tools, resources, prompts)
    Full,
    /// MCP server for Cloudflare Workers
    CloudflareWorkers,
    /// Cloudflare Workers with OAuth 2.1
    CloudflareWorkersOauth,
    /// Cloudflare Workers with Durable Objects
    CloudflareWorkersDurableObjects,
}

impl std::fmt::Display for ProjectTemplate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Minimal => write!(f, "minimal"),
            Self::Full => write!(f, "full"),
            Self::CloudflareWorkers => write!(f, "cloudflare-workers"),
            Self::CloudflareWorkersOauth => write!(f, "cloudflare-workers-oauth"),
            Self::CloudflareWorkersDurableObjects => {
                write!(f, "cloudflare-workers-durable-objects")
            }
        }
    }
}

/// Build command arguments
#[derive(Args, Debug, Clone)]
pub struct BuildArgs {
    /// Path to the Cargo project (default: current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Target platform for WASM builds
    #[arg(long, value_enum)]
    pub platform: Option<WasmPlatform>,

    /// Rust target triple (e.g., wasm32-unknown-unknown)
    #[arg(long)]
    pub target: Option<String>,

    /// Build in release mode
    #[arg(long, short = 'r')]
    pub release: bool,

    /// Optimize WASM binary with wasm-opt
    #[arg(long)]
    pub optimize: bool,

    /// Additional features to enable
    #[arg(long, short = 'F')]
    pub features: Vec<String>,

    /// Disable default features
    #[arg(long)]
    pub no_default_features: bool,

    /// Output directory for the built artifacts
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,
}

/// Deploy command arguments
#[derive(Args, Debug, Clone)]
pub struct DeployArgs {
    /// Path to the Cargo project (default: current directory)
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Target platform for deployment
    #[arg(long, value_enum, default_value = "cloudflare-workers")]
    pub platform: WasmPlatform,

    /// Deployment environment (e.g., staging, production)
    #[arg(long, short = 'e')]
    pub env: Option<String>,

    /// Path to wrangler.toml config (for Cloudflare Workers)
    #[arg(long)]
    pub wrangler_config: Option<PathBuf>,

    /// Build in release mode before deploying
    #[arg(long, short = 'r', default_value = "true")]
    pub release: bool,

    /// Optimize WASM binary with wasm-opt before deploying
    #[arg(long, default_value = "true")]
    pub optimize: bool,

    /// Skip the build step (deploy existing artifacts)
    #[arg(long)]
    pub skip_build: bool,

    /// Dry run (show what would be deployed without actually deploying)
    #[arg(long)]
    pub dry_run: bool,
}

/// New project command arguments
#[derive(Args, Debug, Clone)]
pub struct NewArgs {
    /// Name of the new project
    pub name: String,

    /// Project template to use
    #[arg(long, short = 't', value_enum, default_value = "minimal")]
    pub template: ProjectTemplate,

    /// Output directory (default: ./<name>)
    #[arg(long, short = 'o')]
    pub output: Option<PathBuf>,

    /// Initialize git repository
    #[arg(long, default_value = "true")]
    pub git: bool,

    /// MCP server description
    #[arg(long, short = 'd')]
    pub description: Option<String>,

    /// Package author
    #[arg(long)]
    pub author: Option<String>,
}
