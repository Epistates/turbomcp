# TurboMCP CLI - Architecture Migration

## Overview

The CLI has been redesigned to leverage `turbomcp-client` and provide complete MCP protocol coverage. This document outlines the changes and migration path.

## What Changed

### 🎯 Architecture Improvements

The CLI now:
- Uses `turbomcp-client` for all operations
- Supports complete MCP protocol (15+ operations)
- Provides rich error handling with actionable suggestions
- Supports multiple output formats (human, JSON, YAML, table)
- Includes transport abstraction with auto-detection
- Offers config files and connection presets

### 📦 New File Structure

```
crates/turbomcp-cli/src/
├── lib_new.rs           # New main library entry point
├── cli_new.rs           # Enhanced CLI definitions (all MCP commands)
├── error.rs             # Rich error types with suggestions
├── transport.rs         # Transport factory with auto-detection
├── executor.rs          # Command execution using turbomcp-client
├── formatter.rs         # Rich output formatting
│
├── lib.rs               # Legacy (kept for backward compat)
├── cli.rs               # Legacy
├── commands.rs          # Legacy
└── transports/          # Legacy (will be removed)
```

### 🚀 New Commands

#### Tools (Enhanced)
```bash
# Old
turbomcp-cli tools-list --url URL
turbomcp-cli tools-call --url URL --name NAME --arguments ARGS
turbomcp-cli schema-export --url URL --output FILE

# New
turbomcp-cli tools list --url URL [--format table]
turbomcp-cli tools call NAME --arguments ARGS
turbomcp-cli tools schema [NAME]
turbomcp-cli tools export --output DIR
```

#### Resources (NEW)
```bash
turbomcp-cli resources list
turbomcp-cli resources read URI
turbomcp-cli resources templates
turbomcp-cli resources subscribe URI
turbomcp-cli resources unsubscribe URI
```

#### Prompts (NEW)
```bash
turbomcp-cli prompts list
turbomcp-cli prompts get NAME --arguments ARGS
turbomcp-cli prompts schema NAME
```

#### Completions (NEW)
```bash
turbomcp-cli complete get prompt NAME
turbomcp-cli complete get resource URI --argument ARG
```

#### Server Management (NEW)
```bash
turbomcp-cli server info
turbomcp-cli server ping
turbomcp-cli server log-level debug|info|warning|error
turbomcp-cli server roots
```

#### Connection (NEW)
```bash
turbomcp-cli connect --url URL
turbomcp-cli status
```

### 🎨 Output Formats

```bash
# Human-readable with colors (default)
turbomcp-cli tools list --format human

# JSON (pretty-printed)
turbomcp-cli tools list --format json

# Compact JSON
turbomcp-cli tools list --format compact

# YAML
turbomcp-cli tools list --format yaml

# Table view
turbomcp-cli tools list --format table
```

### 🔌 Transport Support

#### Auto-Detection
```bash
# STDIO (auto-detected from executable path)
turbomcp-cli tools list --url ./my-server
turbomcp-cli tools list --command "python server.py"

# TCP (auto-detected from tcp:// prefix)
turbomcp-cli tools list --url tcp://localhost:8080

# Unix socket (auto-detected from path or unix:// prefix)
turbomcp-cli tools list --url /tmp/mcp.sock
turbomcp-cli tools list --url unix:///tmp/mcp.sock

# HTTP SSE (not yet implemented, will use http:// prefix)
# WebSocket (not yet implemented, will use ws:// prefix)
```

#### Explicit Transport
```bash
turbomcp-cli tools list --url localhost:8080 --transport tcp
```

### 💡 Error Handling

**Before:**
```
error: Connection refused
```

**After:**
```
Error: Connection failed: Connection refused (tcp://localhost:8080)

Suggestions:
  • Check if the server is running
  • Verify the connection URL
  • Use --transport to specify transport explicitly
```

## Migration Steps

### Step 1: Update Entry Point

**Current `src/lib.rs`:**
```rust
pub fn run_cli() {
    let cli = cli::Cli::parse();
    // ... old implementation
}
```

**New `src/lib.rs`:**
```rust
// Import new modules
mod cli_new;
mod error;
mod executor;
mod formatter;
mod transport;

pub async fn run_cli_new() -> CliResult<()> {
    let cli = cli_new::Cli::parse();
    let executor = CommandExecutor::new(cli.format.clone(), !cli.no_color, cli.verbose);
    executor.execute(cli.command).await
}

// Keep old run_cli for backward compatibility
pub fn run_cli() {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(async { run_cli_new().await.unwrap() })
}
```

### Step 2: Update `src/main.rs`

**Replace:**
```rust
fn main() {
    turbomcp_cli::run_cli();
}
```

**With:**
```rust
#[tokio::main]
async fn main() {
    if let Err(e) = turbomcp_cli::run_cli_new().await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}
```

### Step 3: Update Dependencies (Already Done)

The `Cargo.toml` now includes:
- `turbomcp-client` - For all MCP operations
- `turbomcp-transport` - For transport implementations
- `comfy-table` - Table formatting
- `owo-colors` - Colored output
- `indicatif` - Progress bars
- `serde_yaml` - YAML support
- `anyhow` / `thiserror` - Better errors

### Step 4: Remove Legacy Code (Future)

After migration is complete and tested:
1. Remove `src/transports/` directory
2. Remove old `src/cli.rs`, `src/commands.rs`
3. Rename `cli_new.rs` → `cli.rs`
4. Rename `lib_new.rs` → `lib.rs`

## Testing Migration

### 1. Build New Version
```bash
cargo build --release
```

### 2. Test Basic Operations
```bash
# Start a test server
./target/debug/my-test-server &

# Test new CLI
./target/release/turbomcp-cli tools list --command "./target/debug/my-test-server"
./target/release/turbomcp-cli server info --command "./target/debug/my-test-server"
./target/release/turbomcp-cli tools list --format table
```

### 3. Compare Outputs
```bash
# Old version
turbomcp-cli-old tools-list --url URL --json > old.json

# New version
turbomcp-cli tools list --url URL --format json > new.json

# Compare
diff old.json new.json
```

## Configuration Files (Future)

The new architecture supports config files:

```yaml
# ~/.turbomcp/config.yaml
connections:
  dev:
    transport: stdio
    command: ./target/debug/my-server
    timeout: 30

  prod:
    transport: tcp
    url: tcp://prod.example.com:8080
    auth: ${PROD_TOKEN}
    timeout: 60

default: dev
```

Usage:
```bash
turbomcp-cli -c prod tools list
```

## Rollback Plan

If issues arise, rollback is simple:

1. The old implementation is preserved in `lib.rs`, `cli.rs`, `commands.rs`
2. Change `main.rs` back to use `run_cli()` instead of `run_cli_new()`
3. Remove new files: `lib_new.rs`, `cli_new.rs`, etc.

## Benefits of Migration

### For Users
- ✅ Complete MCP protocol support
- ✅ Better error messages with suggestions
- ✅ Multiple output formats
- ✅ Colored, beautiful output
- ✅ More transport options

### For Developers
- ✅ Uses `turbomcp-client`
- ✅ No duplicate transport logic
- ✅ Easier to add new commands
- ✅ Better error handling
- ✅ More maintainable code
- ✅ Better test coverage potential

### For the Project
- ✅ Cohesive with rest of TurboMCP
- ✅ Showcases the power of `turbomcp-client`
- ✅ Enterprise-ready CLI
- ✅ Competitive with other MCP CLIs

## Next Steps

1. **Immediate**: Test new implementation with existing servers
2. **Short-term**: Add HTTP SSE and WebSocket transport support
3. **Medium-term**: Add config file support
4. **Long-term**: Add interactive mode, batch operations, shell completions

## Questions?

See `ARCHITECTURE.md` for detailed design documentation.
