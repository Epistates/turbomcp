//! # 18: Completion Protocol - Autocompletion for Prompts & Resources
//!
//! **Learning Goals:**
//! - Implement autocompletion for prompt arguments
//! - Provide resource URI completions
//! - Understand completion contexts and dependencies
//! - Build IDE-like user experiences
//!
//! **What this example demonstrates:**
//! - Prompt argument completion with contextual suggestions
//! - Resource URI completion for templates
//! - Completion with previously resolved arguments
//! - Rich, IDE-like completion experiences
//!
//! **Run with:** `cargo run --example 18_completion_protocol`

use std::collections::HashMap;
use turbomcp_client::ClientBuilder;
use turbomcp_protocol::types::CompletionContext;
use turbomcp_transport::stdio::StdioTransport;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging to stderr for MCP STDIO compatibility
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_writer(std::io::stderr)
        .init();

    tracing::info!("⌨️  Completion Protocol Demo - Autocompletion");

    // Create client with completion support
    let client = ClientBuilder::new()
        .with_tools(true)
        .with_prompts(true)
        .with_resources(true)
        .build(StdioTransport::new())
        .await?;

    tracing::info!("✅ Client initialized");

    // ============================================================================
    // SIMPLE COMPLETION - No Context
    // ============================================================================
    tracing::info!("\n📝 1. Simple Completion (no context)");

    // Complete a file path
    match client.complete("complete_path", "/usr/b").await {
        Ok(result) => {
            tracing::info!("  Completions for '/usr/b':");
            for value in &result.completion.values {
                tracing::info!("    - {}", value);
            }
        }
        Err(e) => tracing::warn!("  Server doesn't support path completion: {}", e),
    }

    // ============================================================================
    // PROMPT ARGUMENT COMPLETION - With Context
    // ============================================================================
    tracing::info!("\n📝 2. Prompt Argument Completion (with context)");

    // Example: Code review prompt with language and framework arguments
    // User has already selected language="rust", now completing framework
    let mut context_args = HashMap::new();
    context_args.insert("language".to_string(), "rust".to_string());
    let context = CompletionContext {
        arguments: Some(context_args),
    };

    match client
        .complete_prompt("code_review", "framework", "tok", Some(context))
        .await
    {
        Ok(result) => {
            tracing::info!("  Completions for framework starting with 'tok' (language=rust):");
            for value in &result.completion.values {
                tracing::info!("    - {}", value);
            }
            // Expected: tokio, tokio-rs, etc.
        }
        Err(e) => tracing::warn!("  Prompt completion not available: {}", e),
    }

    // ============================================================================
    // RESOURCE TEMPLATE COMPLETION
    // ============================================================================
    tracing::info!("\n📝 3. Resource Template Completion");

    // Complete a resource template variable
    // Template: "/files/{path}" → complete the {path} variable
    match client
        .complete_resource("/files/{path}", "path", "/home/user/doc", None)
        .await
    {
        Ok(result) => {
            tracing::info!("  Completions for path starting with '/home/user/doc':");
            for value in &result.completion.values {
                tracing::info!("    - {}", value);
            }
            // Expected: /home/user/documents, /home/user/downloads, etc.
        }
        Err(e) => tracing::warn!("  Resource completion not available: {}", e),
    }

    // ============================================================================
    // CASCADING COMPLETIONS
    // ============================================================================
    tracing::info!("\n📝 4. Cascading Completions (dependent fields)");

    // Scenario: First complete country, then complete city based on selected country
    let mut step1_context = HashMap::new();
    match client
        .complete_prompt("travel_planner", "country", "uni", None)
        .await
    {
        Ok(result) => {
            tracing::info!("  Countries starting with 'uni':");
            for value in &result.completion.values {
                tracing::info!("    - {}", value);
            }

            // User selects "United States"
            step1_context.insert("country".to_string(), "United States".to_string());
        }
        Err(e) => tracing::warn!("  Country completion failed: {}", e),
    }

    // Now complete cities, knowing the country
    if !step1_context.is_empty() {
        match client
            .complete_prompt(
                "travel_planner",
                "city",
                "new",
                Some(CompletionContext {
                    arguments: Some(step1_context),
                }),
            )
            .await
        {
            Ok(result) => {
                tracing::info!("\n  Cities in United States starting with 'new':");
                for value in &result.completion.values {
                    tracing::info!("    - {}", value);
                }
                // Expected: New York, New Orleans, Newark, etc.
            }
            Err(e) => tracing::warn!("  City completion failed: {}", e),
        }
    }

    // ============================================================================
    // COMPLETION USE CASES
    // ============================================================================
    tracing::info!("\n🎯 Completion Protocol Use Cases:");
    tracing::info!("  ✓ File path autocompletion");
    tracing::info!("  ✓ Framework/library suggestions based on language");
    tracing::info!("  ✓ City suggestions based on selected country");
    tracing::info!("  ✓ Configuration value completion");
    tracing::info!("  ✓ Dynamic enum value suggestions");
    tracing::info!("  ✓ Contextual help text");

    Ok(())
}

/* 📝 **Key Concepts:**

**Completion Types:**

1. **Prompt Argument Completion**
   - Suggest values for prompt arguments
   - Context-aware based on already-filled fields
   - Example: Suggest cities after country selected

2. **Resource URI Completion**
   - Autocomplete resource template variables
   - Example: File paths, database IDs, resource names

3. **Generic Handler Completion**
   - Simple key-value completion
   - No MCP-specific context needed

**Completion Context:**
```rust,ignore
CompletionContext {
    arguments: Some({
        "language": "rust",      // Previously selected value
        "category": "web"        // Influences completion results
    })
}
```

**Server-Side Implementation:**
```rust,ignore
#[completion("code_review")]
async fn complete_framework(
    ctx: Context,
    prompt_name: String,
    argument_name: String,
    argument_value: String,
    context: Option<CompletionContext>
) -> McpResult<CompletionResponse> {
    // Extract previously selected language
    let language = context
        .and_then(|c| c.arguments)
        .and_then(|args| args.get("language"))
        .unwrap_or(&"generic".to_string());

    // Return language-specific framework suggestions
    let suggestions = match (language.as_str(), argument_value.as_str()) {
        ("rust", "tok") => vec!["tokio", "tokio-tungstenite"],
        ("python", "dja") => vec!["django", "django-rest-framework"],
        _ => vec![]
    };

    Ok(CompletionResponse {
        completion: Completion {
            values: suggestions.into_iter().map(String::from).collect(),
            total: None,
            has_more: false,
        }
    })
}
```

**Cascading Completions Pattern:**
```text
User Input Flow:
1. Select Country: "uni" → ["United States", "United Kingdom", "United Arab Emirates"]
2. User picks: "United States"
3. Select City: "new" → ["New York", "New Orleans", "Newark"]
                          ↑ Filtered by country!
```

**Completion Response Format:**
```json
{
  "completion": {
    "values": ["tokio", "tokio-rs", "tokio-tungstenite"],
    "total": 3,
    "hasMore": false
  }
}
```

**Best Practices:**

1. **Limit Results** - Return top 10-20 suggestions, not thousands
2. **Fuzzy Matching** - Match "tok" to "tokio" even if not exact prefix
3. **Contextual** - Use previously selected values to filter
4. **Fast** - Completion should be <100ms for good UX
5. **Descriptive** - Include descriptions when helpful
6. **Sorted** - Most relevant results first

**IDE Integration:**

Completion protocol enables:
- VSCode-like autocomplete in CLI tools
- Web form dropdown suggestions
- Configuration wizards
- Rich REPL experiences

**Next Steps:**
- Implement server-side completion handlers
- Add fuzzy matching for better UX
- Cache completion results for performance
- Use completion in interactive CLI tools
*/
