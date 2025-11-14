# TurboMCP CLI - Comprehensive Architecture

## Design Principles

1. **Leverage Core Libraries**: Use `turbomcp-client` and `turbomcp-transport` - never reimplement
2. **Complete MCP Coverage**: Support ALL MCP protocol operations
3. **Rich User Experience**: Multiple output formats, colored output, progress indicators
4. **Enterprise Ready**: Robust error handling, logging, configuration management
5. **Extensible**: Plugin system for custom commands and formatters

## Architecture Layers

```
┌─────────────────────────────────────────────────┐
│          CLI Interface (clap)                   │
│  ┌──────────┬──────────┬───────────┬─────────┐ │
│  │ Tools    │ Prompts  │ Resources │ Server  │ │
│  └──────────┴──────────┴───────────┴─────────┘ │
└─────────────────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────┐
│        Command Executor & Context               │
│  • Client builder configuration                 │
│  • Output formatting (JSON, Table, YAML)        │
│  • Progress tracking & error handling           │
└─────────────────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────┐
│         turbomcp-client (Production)            │
│  • All MCP operations                           │
│  • Connection management                        │
│  • Bidirectional communication                  │
│  • Plugin system                                │
└─────────────────────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────┐
│       turbomcp-transport (All Protocols)        │
│  • Stdio, HTTP SSE, WebSocket, TCP, Unix        │
│  • Compression, metrics, health checks          │
│  • Robust transport with retries                │
└─────────────────────────────────────────────────┘
```

## Command Categories

### 1. Tools
- `tools list` - List available tools
- `tools call <name>` - Execute a tool
- `tools schema <name>` - Get tool schema
- `tools export` - Export all schemas

### 2. Resources
- `resources list` - List resources
- `resources read <uri>` - Read resource content
- `resources templates` - List resource templates
- `resources subscribe <uri>` - Subscribe to updates

### 3. Prompts
- `prompts list` - List prompts
- `prompts get <name>` - Get prompt with args
- `prompts schema <name>` - Get prompt schema

### 4. Completions
- `complete <type> <ref>` - Get completions
- `complete prompt <name>` - Complete prompt args
- `complete resource <uri>` - Complete resource URIs

### 5. Server Management
- `server info` - Get server info (from initialize)
- `server ping` - Ping server
- `server log-level <level>` - Set logging level
- `server roots` - List roots

### 6. Sampling (Advanced)
- `sample create <messages>` - Create LLM sample

### 7. Connection
- `connect` - Interactive connection wizard
- `status` - Connection status

## Transport Auto-Detection

```rust
// URL-based detection
http://... → HttpSseTransport
https://... → HttpSseTransport
ws://... → WebSocketTransport
wss://... → WebSocketTransport
tcp://... → TcpTransport
unix://... → UnixTransport

// Command-based
./binary → StdioTransport (ChildProcessTransport)
python script.py → StdioTransport
node server.js → StdioTransport
```

## Output Formats

1. **Human** (default): Colored, formatted for terminal
   - Table view for lists
   - Tree view for nested data
   - Progress indicators for long operations

2. **JSON**: Machine-readable
   - Pretty-printed by default
   - Compact with `--compact`

3. **YAML**: Configuration-friendly

4. **Table**: Tabular format with customizable columns

## Configuration

Support config files for connection presets:

```yaml
# ~/.turbomcp/config.yaml
connections:
  dev:
    transport: stdio
    command: ./target/debug/my-server

  prod:
    transport: https
    url: https://api.example.com/mcp
    auth: ${PROD_TOKEN}

  local-ws:
    transport: ws
    url: ws://localhost:8080/mcp

default: dev
```

Usage: `turbomcp-cli -c prod tools list`

## Error Handling

```rust
// Rich error types
pub enum CliError {
    Transport(TransportError),
    Protocol(ProtocolError),
    InvalidArguments(String),
    ServerError { code: i32, message: String },
    Timeout { operation: String, elapsed: Duration },
    NotInitialized,
}

// User-friendly messages
Error: Failed to connect to server
  Caused by: Connection refused (tcp://localhost:8080)

  Suggestions:
    • Check if the server is running
    • Verify the connection URL
    • Use --transport to specify transport explicitly
```

## Examples

```bash
# List tools with table output
turbomcp-cli tools list --format table

# Call tool with progress indicator
turbomcp-cli tools call calculate --args '{"a":5,"b":3}' --verbose

# Read resource with custom output
turbomcp-cli resources read file:///etc/hosts --format yaml

# Interactive connection
turbomcp-cli connect

# Use saved connection
turbomcp-cli -c prod prompts get user-preferences

# Export all schemas
turbomcp-cli tools export --output schemas/ --format json
```

## Implementation Status

### Phase 1: Core Refactor ✅ COMPLETE
- [x] Remove custom transport implementations
- [x] Integrate turbomcp-client properly
- [x] Implement transport factory with auto-detection
- [x] Add basic error handling framework

### Phase 2: Complete Commands ✅ COMPLETE
- [x] Add all resource commands (list, read, templates, subscribe, unsubscribe)
- [x] Add all prompt commands (list, get, schema)
- [x] Add completion commands (get with prompt/resource refs)
- [x] Add server management commands (info, ping, log-level, roots)

### Phase 3: UX Enhancement ✅ COMPLETE
- [x] Multiple output formats (human, json, yaml, table, compact)
- [x] Colored output with `owo-colors`
- [x] Table formatting with `comfy-table`
- [x] Progress indicators with `indicatif`

### Phase 4: Configuration 🚧 PARTIAL
- [x] Config file support (Connection struct uses config crate)
- [x] Connection presets (--connection flag implemented)
- [x] Environment variable expansion (MCP_URL, MCP_COMMAND, MCP_AUTH supported)

### Phase 5: Advanced Features ⏳ TODO
- [ ] Interactive mode
- [ ] Batch operations
- [ ] Scripting support
- [ ] Shell completions (bash, zsh, fish)
