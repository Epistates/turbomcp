# TurboMCP CLI

[![Crates.io](https://img.shields.io/crates/v/turbomcp-cli.svg)](https://crates.io/crates/turbomcp-cli)
[![Documentation](https://docs.rs/turbomcp-cli/badge.svg)](https://docs.rs/turbomcp-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

**Comprehensive CLI for MCP servers - complete protocol support with enterprise-grade UX**

## Overview

`turbomcp-cli` is a comprehensive command-line interface for the Model Context Protocol, built on the production-ready `turbomcp-client` library. It provides complete MCP protocol coverage with rich, multi-format output and smart transport auto-detection.

## Features

- **🎯 Complete MCP Protocol** - All operations: tools, resources, prompts, completions, sampling, logging
- **🔧 Tool Management** - List, call, and export tool schemas
- **📦 Resource Access** - List, read, and subscribe to MCP resources
- **💬 Prompt Operations** - List and execute prompts with arguments
- **🌐 Multi-Transport** - STDIO (child process), TCP, Unix sockets (HTTP/WS coming soon)
- **🚀 Smart Auto-Detection** - Automatically detects transport from URL format
- **🎨 Rich Output** - Human, JSON, YAML, and table formats with colored output
- **🛡️ Production Ready** - Built on `turbomcp-client` and `turbomcp-transport`
- **⚡ Enterprise Grade** - Comprehensive error handling with actionable suggestions

## Installation

### From Crates.io

```bash
# Install latest stable version
cargo install turbomcp-cli

# Install specific version
cargo install turbomcp-cli --version 2.0.0
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

# Get server information (table format)
turbomcp-cli server info --format table

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
  resources   Resource operations (list, read, templates, subscribe)
  prompts     Prompt operations (list, get, schema)
  complete    Completion operations
  server      Server management (info, ping, log-level, roots)
  sample      Sampling operations (advanced)
  connect     Interactive connection wizard
  status      Connection status
  help        Print help information

Global Options:
  -f, --format <FORMAT>     Output format [default: human] [possible: human, json, yaml, table, compact]
  -v, --verbose             Enable verbose output
  -c, --connection <NAME>   Use saved connection from config
  --no-color                Disable colored output
  -h, --help                Print help
  -V, --version             Print version
```

### Connection Options

All commands support these connection options:

- `--url <URL>` - Server URL for HTTP/WebSocket or command path for STDIO (default: `http://localhost:8080/mcp`)
- `--command <COMMAND>` - Command to execute for STDIO transport (overrides `--url`)
- `--auth <AUTH>` - Bearer token or API key for authentication
- `--json` - Output results in JSON format

## Commands

### `tools-list` - List Available Tools

List all tools available from an MCP server.

```bash
# List tools from HTTP server
turbomcp-cli tools-list --url http://localhost:8080/mcp

# List tools from WebSocket server  
turbomcp-cli tools-list --url ws://localhost:8080/mcp

# List tools from STDIO server
turbomcp-cli tools-list --command "./target/debug/my-server"
```

**Example Output:**
```
Available Tools:
- calculator_add: Add two numbers together
- file_read: Read contents of a file
- search_web: Search the web for information

Total: 3 tools
```

### `tools-call` - Call a Tool

Execute a specific tool on the MCP server.

```bash
# Call a tool with JSON parameters (HTTP)
turbomcp-cli tools-call \
    --url http://localhost:8080/mcp \
    --name calculator_add \
    --arguments '{"a": 5, "b": 3}'

# Call a tool via WebSocket
turbomcp-cli tools-call \
    --url ws://localhost:8080/mcp \
    --name file_read \
    --arguments '{"path": "/etc/hosts"}'

# Call a tool via STDIO
turbomcp-cli tools-call \
    --command "./target/debug/my-server" \
    --name calculator_add \
    --arguments '{"a": 5, "b": 3}'
```

**Example Output:**
```json
{
  "result": 8,
  "success": true
}
```

### `schema-export` - Export Tool Schemas

Export JSON schemas for all tools from an MCP server.

```bash
# Export schemas to stdout (HTTP)
turbomcp-cli schema-export --url http://localhost:8080/mcp

# Export schemas to file (HTTP)
turbomcp-cli schema-export \
    --url http://localhost:8080/mcp \
    --output schemas.json

# Export schemas from STDIO server
turbomcp-cli schema-export \
    --command "./target/debug/my-server" \
    --output schemas.json
```

**Example Output:**
```json
{
  "tools": [
    {
      "name": "calculator_add",
      "description": "Add two numbers together",
      "inputSchema": {
        "type": "object",
        "properties": {
          "a": {"type": "number"},
          "b": {"type": "number"}
        },
        "required": ["a", "b"]
      }
    }
  ]
}
```

## Transport Support

The CLI supports three transport methods:

### HTTP/HTTPS
```bash
turbomcp-cli tools-list --url http://localhost:8080/mcp
turbomcp-cli tools-list --url https://api.example.com/mcp
```

### WebSocket
```bash
turbomcp-cli tools-list --url ws://localhost:8080/mcp
turbomcp-cli tools-list --url wss://api.example.com/mcp
```

### STDIO (Standard Input/Output)
```bash
# Using --command option
turbomcp-cli tools-list --command "./my-server"
turbomcp-cli tools-list --command "python server.py"

# Or specify path in --url (auto-detected)
turbomcp-cli tools-list --url "./my-server"
```

**Transport Auto-Detection:**
- URLs starting with `http://`, `https://` → HTTP transport
- URLs starting with `ws://`, `wss://` → WebSocket transport  
- `--command` option or executable paths → STDIO transport

## Examples

```bash
# List tools from HTTP server
turbomcp-cli tools-list --url http://localhost:8080/mcp

# Call calculator tool via STDIO
turbomcp-cli tools-call \
  --command "./target/debug/calculator-server" \
  --name calculator_add \
  --arguments '{"a": 10, "b": 5}'

# Export all schemas to file via WebSocket
turbomcp-cli schema-export \
  --url ws://localhost:8080/mcp \
  --output my-server-schemas.json

# Test STDIO server with authentication
turbomcp-cli tools-list \
  --command "python my-server.py" \
  --auth "bearer-token-here" \
  --json
```

## Related Tools

- **[turbomcp](../turbomcp/)** - Main TurboMCP framework
- **[turbomcp-server](../turbomcp-server/)** - Server implementation  
- **[turbomcp-client](../turbomcp-client/)** - Client implementation
- **[turbomcp-transport](../turbomcp-transport/)** - Transport protocols

## License

Licensed under the [MIT License](../../LICENSE).

---

*Part of the [TurboMCP](../../) high-performance Rust SDK for the Model Context Protocol.*