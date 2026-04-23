# TurboMCP CLI

[![Crates.io](https://img.shields.io/crates/v/turbomcp-cli.svg)](https://crates.io/crates/turbomcp-cli)
[![Documentation](https://docs.rs/turbomcp-cli/badge.svg)](https://docs.rs/turbomcp-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**CLI for MCP servers with complete protocol support**

## Table of Contents

- [Overview](#overview)
- [Features](#features)
- [Installation](#installation)
- [Quick Start](#quick-start)
- [Usage](#usage)
- [Commands](#commands)
- [Transport Support](#transport-support)
- [Examples](#examples)
- [Related Tools](#related-tools)

## Overview

`turbomcp-cli` is a command-line interface for the Model Context Protocol, built on the `turbomcp-client` library. It provides complete MCP protocol coverage with rich, multi-format output and smart transport auto-detection.

## Features

- **🎯 Complete MCP Protocol** - All operations: tools, resources, prompts, completions, sampling, logging
- **🔧 Tool Management** - List, call, and export tool schemas
- **📦 Resource Access** - List, read, and subscribe to MCP resources
- **💬 Prompt Operations** - List and execute prompts with arguments
- **🌐 Multi-Transport** - STDIO (child process), TCP, Unix sockets, HTTP SSE, WebSocket
- **🚀 Smart Auto-Detection** - Automatically detects transport from URL format
- **🎨 Rich Output** - Human, JSON, YAML, and table formats with colored output
- **🛡️ Built on Core Libraries** - Uses `turbomcp-client` and `turbomcp-transport`
- **⚡ Error Handling** - Comprehensive error handling with actionable suggestions

## Installation

### From Crates.io

```bash
# Install latest stable version
cargo install turbomcp-cli

# Install specific version
cargo install turbomcp-cli --version 3.1.1
```

### From Source

```bash
git clone https://github.com/Epistates/turbomcp.git
cd turbomcp
cargo install --path crates/turbomcp-cli
```

## Quick Start

```bash
# List tools from a server
turbomcp-cli tools list --command "./my-mcp-server"

# Call a tool with arguments
turbomcp-cli tools call calculate --arguments '{"a": 5, "b": 3}'

# Get server information
turbomcp-cli server info

# List resources
turbomcp-cli resources list --url tcp://localhost:8080

# Work with prompts
turbomcp-cli prompts list --url unix:///tmp/mcp.sock
```

## Usage

```bash
turbomcp-cli <COMMAND>

Commands:
  tools       Tool operations (list, call, schema, export)
  resources   Resource operations (list, read, templates, subscribe, unsubscribe)
  prompts     Prompt operations (list, get, schema)
  complete    Completion operations (get)
  server      Server management (info, ping, log-level, roots)
  sample      Sampling operations (create)
  connect     Interactive connection wizard
  status      Connection status
  dev         Development server with hot reload
  install     Install MCP server to Claude Desktop or Cursor
  build       Build an MCP server (supports WASM targets)
  deploy      Deploy an MCP server to cloud platforms
  new         Create a new MCP server project from a template
  help        Print help information

Global Options:
  -f, --format <FORMAT>     Output format [default: human] [possible: human, json, yaml, table, compact]
  -v, --verbose             Enable verbose output
  -c, --connection <NAME>   Use saved connection from ~/.turbomcp/config.yaml
  --no-color                Disable colored output
  -h, --help                Print help
  -V, --version             Print version
```

### Connection Options

Commands that connect to a server accept these flags (via flattened `Connection`):

- `--transport <KIND>` - Force transport: `stdio`, `http`, `ws`, `tcp`, `unix` (auto-detected if omitted)
- `--url <URL>` - Server URL (env: `MCP_URL`, default: `http://localhost:8080/mcp`)
- `--command <COMMAND>` - Command to execute for STDIO transport, overrides `--url` (env: `MCP_COMMAND`)
- `--auth <AUTH>` - Bearer token or API key (env: `MCP_AUTH`)
- `--timeout <SECONDS>` - Connection timeout in seconds (default: `30`)

Use the global `-f, --format json` flag (not `--json`) to emit JSON output.

## Commands

### `tools list` - List Available Tools

List all tools available from an MCP server.

```bash
# List tools from HTTP server
turbomcp-cli tools list --url http://localhost:8080/mcp

# List tools from WebSocket server
turbomcp-cli tools list --url ws://localhost:8080/mcp

# List tools from STDIO server
turbomcp-cli tools list --command "./target/debug/my-server"
```

**Example Output:**
```
Available Tools:
- calculator_add: Add two numbers together
- file_read: Read contents of a file
- search_web: Search the web for information

Total: 3 tools
```

### `tools call` - Call a Tool

Execute a specific tool on the MCP server.

```bash
# Call a tool with JSON parameters (HTTP)
turbomcp-cli tools call calculator_add \
    --url http://localhost:8080/mcp \
    --arguments '{"a": 5, "b": 3}'

# Call a tool via WebSocket
turbomcp-cli tools call file_read \
    --url ws://localhost:8080/mcp \
    --arguments '{"path": "/etc/hosts"}'

# Call a tool via STDIO
turbomcp-cli tools call calculator_add \
    --command "./target/debug/my-server" \
    --arguments '{"a": 5, "b": 3}'
```

**Example Output:**
```json
{
  "result": 8,
  "success": true
}
```

### `tools schema` - Print Tool Schemas

Print the JSON input schema for one tool (by name) or all tools to stdout.

```bash
# Print schemas for all tools (HTTP)
turbomcp-cli tools schema --url http://localhost:8080/mcp

# Print schema for a single tool
turbomcp-cli tools schema calculator_add --url http://localhost:8080/mcp

# Schemas from a STDIO server
turbomcp-cli tools schema --command "./target/debug/my-server"
```

### `tools export` - Export Schemas to a Directory

Write each tool's input schema as a separate `<tool>.json` file inside an output
directory. The directory is created if needed; tool names are sanitized to
prevent path traversal, and output paths are validated so that symlink-based
escapes outside the directory are rejected at file creation time.

```bash
# Export every schema into ./schemas/
turbomcp-cli tools export \
    --url http://localhost:8080/mcp \
    --output ./schemas

# Export from a STDIO server
turbomcp-cli tools export \
    --command "./target/debug/my-server" \
    --output ./schemas
```

## Transport Support

The CLI supports five transports. Use `--transport` to force one, or rely on
URL-based auto-detection:

### HTTP / HTTPS (SSE)
```bash
turbomcp-cli tools list --url http://localhost:8080/mcp
turbomcp-cli tools list --url https://api.example.com/mcp
```

### WebSocket
```bash
turbomcp-cli tools list --url ws://localhost:8080/mcp
turbomcp-cli tools list --url wss://api.example.com/mcp
```

### TCP
```bash
turbomcp-cli tools list --url tcp://localhost:9000
```

### Unix Domain Socket
```bash
turbomcp-cli tools list --url unix:///tmp/mcp.sock
```

### STDIO (child process)
```bash
# Using --command option
turbomcp-cli tools list --command "./my-server"
turbomcp-cli tools list --command "python server.py"
```

**Transport Auto-Detection:**
- `http://`, `https://` → HTTP/SSE transport
- `ws://`, `wss://` → WebSocket transport
- `tcp://` → TCP transport
- `unix://` → Unix socket transport
- `--command` option → STDIO transport (spawns a child process)

## Examples

```bash
# List tools from HTTP server
turbomcp-cli tools list --url http://localhost:8080/mcp

# Call calculator tool via STDIO
turbomcp-cli tools call calculator_add \
  --command "./target/debug/calculator-server" \
  --arguments '{"a": 10, "b": 5}'

# Export all schemas to a directory via WebSocket
turbomcp-cli tools export \
  --url ws://localhost:8080/mcp \
  --output ./my-server-schemas

# Test STDIO server with authentication, emitting JSON
turbomcp-cli --format json tools list \
  --command "python my-server.py" \
  --auth "bearer-token-here"
```

## Roadmap

### Planned: Secure Credential Storage

**Status:** Not yet implemented

Modern CLI best practices mandate using OS-native credential stores for storing long-lived tokens:

| Platform | Credential Store |
|:---------|:-----------------|
| macOS | Keychain |
| Windows | DPAPI / Credential Manager |
| Linux | libsecret / Secret Service |

**Planned commands:**
```bash
# Future: Secure login flow
turbomcp-cli auth login --provider github
turbomcp-cli auth login --url https://mcp.example.com

# Future: Use stored credentials automatically
turbomcp-cli tools list --url https://mcp.example.com  # Uses keychain

# Future: Logout / clear credentials
turbomcp-cli auth logout --url https://mcp.example.com
```

**Current workaround:** Pass tokens via `--auth` flag or environment variables:
```bash
export MCP_AUTH_TOKEN="your-token"
turbomcp-cli tools list --url https://mcp.example.com --auth "$MCP_AUTH_TOKEN"
```

This feature will be implemented using the [`keyring`](https://crates.io/crates/keyring) crate for cross-platform credential storage.

## Related Tools

- **[turbomcp](../turbomcp/)** - Main TurboMCP framework
- **[turbomcp-server](../turbomcp-server/)** - Server implementation  
- **[turbomcp-client](../turbomcp-client/)** - Client implementation
- **[turbomcp-transport](../turbomcp-transport/)** - Transport protocols

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) Rust SDK for the Model Context Protocol.*