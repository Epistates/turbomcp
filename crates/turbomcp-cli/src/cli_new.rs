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
                  Multiple transports: stdio, HTTP SSE, WebSocket, TCP, Unix sockets."
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
