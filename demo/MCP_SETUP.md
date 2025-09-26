# TurboMCP Demo - MCP Server Setup

## Local Configuration

1. **Ex: Add to LM Studio Settings**: Copy this configuration to your LM Studio MCP servers section:

```json
{
  "mcpServers": {
    "turbomcp-dev-assistant": {
      "command": "cargo",
      "args": ["run", "--manifest-path", "/Users/Epistates/turbomcp/demo/Cargo.toml"],
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

2. **Alternative: Use compiled binary** (recommended for production):
   
   First compile the demo:
   ```bash
   cd /Users/Epistates/turbomcp/demo
   cargo build --release
   ```
   
   Then use this config:
   ```json
   {
     "mcpServers": {
       "turbomcp-dev-assistant": {
         "command": "/Users/Epistates/turbomcp/target/release/turbomcp-demo"
       }
     }
   }
   ```

## Claude Desktop Configuration

Add to your `~/Library/Application Support/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "turbomcp-dev-assistant": {
      "command": "cargo",
      "args": ["run", "--manifest-path", "/Users/Epistates/turbomcp/demo/Cargo.toml"],
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

## Testing the Server

Once connected, you can test these features:

### 🔧 Tools Available:
- `analyze_code` - Analyze code complexity and style
- `build_project` - Run build commands (check, build, test)
- `list_files` - List and filter project files

### 📚 Resources Available:
- `config://project/build` - Project build configuration
- `config://project/dev` - Development environment config
- `config://project/deployment` - Deployment settings
- `history://builds` - Build history and statistics
- `templates://rust/struct` - Rust struct template
- `templates://rust/error` - Rust error template  
- `templates://rust/test` - Rust test template

### 🤖 Prompts Available:
- `documentation_prompt` - Generate documentation for code
- `code_review_prompt` - Generate code review comments

### Example Usage:

1. **Analyze a file**: 
   ```json
   {
     "name": "analyze_code",
     "arguments": {
       "file_path": "src/main.rs",
       "analysis_type": "deep",
       "include_metrics": true,
       "complexity_threshold": 15
     }
   }
   ```

2. **Run a build**:
   ```json
   {
     "name": "build_project", 
     "arguments": {
       "command": "check",
       "verbose": true,
       "target": "debug",
       "features": ["full"]
     }
   }
   ```

3. **List files**:
   ```json
   {
     "name": "list_files",
     "arguments": {
       "pattern": "*.rs",
       "include_stats": true,
       "max_depth": 3,
       "include_hidden": false
     }
   }
   ```

4. **Generate documentation prompt**:
   ```json
   {
     "name": "documentation_prompt",
     "arguments": {
       "function_name": "analyze_code",
       "function_type": "tool",
       "code_context": "MCP server tool for code analysis",
       "style": "rustdoc"
     }
   }
   ```

5. **Generate code review prompt**:
   ```json
   {
     "name": "code_review_prompt",
     "arguments": {
       "code_snippet": "fn example() { let x = 42; }",
       "focus_areas": ["performance", "style"],
       "language": "Rust",
       "expertise_level": "senior"
     }
   }
   ```

3. **Get build history resource**:
   ```
   history://builds
   ```

4. **Generate documentation prompt**:
   ```json
   {
     "function": "calculate_fibonacci",
     "type": "function"
   }
   ```

## Troubleshooting

1. **Server won't start**: Make sure you're in the right directory and Rust is installed
2. **Connection issues**: Check the logs in LM Studio/Claude Desktop for error messages
3. **Tools not working**: Verify the server started without compile errors

The server will show startup logs like:
```
🚀 Starting TurboMCP Development Assistant Demo
🔧 Server configured with:
  • Code analysis tools (analyze_code, build_project, list_files)
  • AI writing prompts (documentation_prompt, code_review_prompt)  
  • Project resources (config://, history://, templates://)
📡 Starting MCP server on STDIO...
```

Enjoy testing the TurboMCP implementation! 🎉