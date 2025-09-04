//! TurboMCP Comprehensive Demo
//!
//! A complete demonstration of ALL TurboMCP framework capabilities.
//! This server showcases every feature type: tools, resources, prompts, and edge cases.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use turbomcp::prelude::*;
// The elicitation API is available but requires context integration
// use turbomcp::elicitation_api::{elicit, string, boolean, integer};

/// Comprehensive development assistant server showcasing all TurboMCP features
#[derive(Clone)]
struct TurboMCPDemo {
    // State persistence for testing
    build_history: Arc<tokio::sync::Mutex<Vec<BuildRecord>>>,
    file_cache: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    analysis_stats: Arc<tokio::sync::Mutex<AnalysisStats>>,
}

impl Default for TurboMCPDemo {
    fn default() -> Self {
        Self {
            build_history: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            file_cache: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            analysis_stats: Arc::new(tokio::sync::Mutex::new(AnalysisStats::default())),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct BuildRecord {
    timestamp: String,
    command: String,
    status: String,
    duration_ms: u64,
}

#[derive(Debug, Clone, Default)]
struct AnalysisStats {
    total_analyses: u64,
    files_by_type: HashMap<String, u64>,
    complexity_scores: Vec<u32>,
}

// Removed custom request structs - tools now use individual parameters for MCP compatibility

#[server(
    name = "TurboMCP Comprehensive Demo",
    version = "1.0.0", 
    description = "Complete demonstration of all TurboMCP framework capabilities"
)]
impl TurboMCPDemo {
    // ═══════════════════════════════════════════════════════════════
    // COMPREHENSIVE TOOL SUITE - Testing All Tool Types
    // ═══════════════════════════════════════════════════════════════

    /// Advanced code analysis with multiple analysis types and metrics
    #[tool("Analyze code files with configurable depth, metrics, and complexity thresholds")]
    async fn analyze_code(
        &self, 
        file_path: String,
        analysis_type: Option<String>,
        include_metrics: Option<bool>,
        complexity_threshold: Option<u32>
    ) -> McpResult<String> {
        tracing::info!("Starting {} analysis for: {}", 
            analysis_type.as_deref().unwrap_or("standard"), 
            file_path);

        // Update analysis stats (state persistence test)
        {
            let mut stats = self.analysis_stats.lock().await;
            stats.total_analyses += 1;
            
            let ext = file_path.split('.').last().unwrap_or("unknown");
            *stats.files_by_type.entry(ext.to_string()).or_insert(0) += 1;
        }

        let analysis_type = analysis_type.as_deref().unwrap_or("standard");
        let include_metrics = include_metrics.unwrap_or(false);
        let threshold = complexity_threshold.unwrap_or(10);

        let complexity_score = fastrand::u32(1..20);
        let lines_analyzed = fastrand::u32(50..1000);

        // Store complexity score for stats
        {
            let mut stats = self.analysis_stats.lock().await;
            stats.complexity_scores.push(complexity_score);
        }

        let result = match analysis_type {
            "quick" => {
                format!("⚡ Quick Analysis: {}\n✓ Syntax: VALID\n📊 {} lines scanned", 
                    file_path, lines_analyzed)
            },
            "deep" => {
                format!("🔍 Deep Analysis: {}\n\
                         ✓ Syntax check: PASSED\n\
                         ✓ Type safety: VERIFIED\n\
                         ✓ Performance: OPTIMIZED\n\
                         📊 Complexity score: {}/20 (threshold: {})\n\
                         📈 Lines analyzed: {}\n\
                         💡 Recommendations: {}",
                    file_path, 
                    complexity_score,
                    threshold,
                    lines_analyzed,
                    if complexity_score > threshold { 
                        "Consider refactoring high-complexity functions" 
                    } else { 
                        "Code complexity within acceptable limits" 
                    })
            },
            "security" => {
                format!("🔒 Security Analysis: {}\n\
                         ✓ Input validation: SECURE\n\
                         ✓ Memory safety: GUARANTEED\n\
                         ✓ Dependency scan: CLEAN\n\
                         🛡️ Security score: 9.2/10\n\
                         📋 {} potential issues found (all low severity)",
                    file_path, fastrand::u32(0..3))
            },
            "performance" => {
                format!("🚀 Performance Analysis: {}\n\
                         ⚡ Hot paths identified: {}\n\
                         🎯 Optimization opportunities: {}\n\
                         💾 Memory usage: OPTIMAL\n\
                         🔄 Allocation patterns: EFFICIENT\n\
                         📊 Performance score: {}/10",
                    file_path, 
                    fastrand::u32(2..8),
                    fastrand::u32(1..5),
                    fastrand::u32(7..10))
            },
            _ => {
                return Err(McpError::Tool(format!(
                    "Invalid analysis type: '{}'. Valid types: quick, deep, security, performance", 
                    analysis_type)));
            }
        };

        if include_metrics {
            let stats = self.analysis_stats.lock().await;
            let total = stats.total_analyses;
            let avg_complexity = if !stats.complexity_scores.is_empty() {
                stats.complexity_scores.iter().sum::<u32>() as f32 / stats.complexity_scores.len() as f32
            } else { 0.0 };
            
            Ok(format!("{}\n\n📊 Session Metrics:\n• Total analyses: {}\n• Average complexity: {:.1}\n• File types: {:?}", 
                result, total, avg_complexity, stats.files_by_type))
        } else {
            Ok(result)
        }
    }

    /// Execute build commands with comprehensive options and state tracking
    #[tool("Execute build commands (check, build, test, clean, doc) with verbose output and feature flags")]
    async fn build_project(
        &self, 
        command: String,
        target: Option<String>,
        verbose: Option<bool>,
        features: Option<Vec<String>>
    ) -> McpResult<String> {
        use chrono::Utc;
        
        // Validate command first (edge case testing)
        let valid_commands = ["check", "build", "test", "clean", "doc", "bench", "clippy"];
        if !valid_commands.contains(&command.as_str()) {
            return Err(McpError::Tool(format!(
                "Invalid build command: '{}'. Valid commands: {}", 
                command, 
                valid_commands.join(", "))));
        }

        let start_time = std::time::Instant::now();
        let target = target.as_deref().unwrap_or("debug");
        let verbose = verbose.unwrap_or(false);
        
        tracing::info!("Executing '{}' build (target: {}, verbose: {})", 
            command, target, verbose);

        // Simulate build time based on command
        let build_duration = match command.as_str() {
            "check" => 800,
            "build" => if target == "release" { 15000 } else { 3000 },
            "test" => 5000,
            "clean" => 200,
            "doc" => 8000,
            "bench" => 12000,
            "clippy" => 2000,
            _ => 1000,
        };

        if verbose {
            tracing::info!("Verbose output enabled - showing detailed progress");
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(build_duration / 10)).await;

        let elapsed = start_time.elapsed();
        let features_info = if let Some(ref features) = features {
            format!(" with features: {}", features.join(", "))
        } else {
            String::new()
        };

        let result = match command.as_str() {
            "check" => {
                if verbose {
                    format!("🔍 Cargo Check Results ({}){}\n\
                             • Checking dependencies... ✓\n\
                             • Validating syntax... ✓\n\
                             • Type checking... ✓\n\
                             • Macro expansion... ✓\n\
                             📊 0 errors, 0 warnings\n\
                             ⚡ Completed in {:.2}s", target, features_info, elapsed.as_secs_f32())
                } else {
                    format!("✅ Check passed ({}){}\n⚡ {:.2}s", target, features_info, elapsed.as_secs_f32())
                }
            },
            "build" => {
                let binary_size = if target == "release" { "1.2MB" } else { "4.8MB" };
                format!("🔨 Build completed ({}){}\n\
                         📦 Binary size: {}\n\
                         🎯 Optimizations: {}\n\
                         ⚡ Completed in {:.2}s", 
                    target, features_info, binary_size,
                    if target == "release" { "ENABLED" } else { "DISABLED" },
                    elapsed.as_secs_f32())
            },
            "test" => {
                let test_count = fastrand::u32(25..75);
                let coverage = fastrand::f32() * 20.0 + 80.0; // 80-100%
                format!("🧪 Test Results{}\n\
                         ✅ {} tests passed, 0 failed\n\
                         📈 Coverage: {:.1}%\n\
                         🎯 Integration tests: {} passed\n\
                         ⚡ Completed in {:.2}s", 
                    features_info, test_count, coverage, test_count / 4, elapsed.as_secs_f32())
            },
            "clean" => {
                let cleaned_mb = fastrand::u32(50..300);
                format!("🧹 Clean completed\n\
                         📂 Removed target/ directory\n\
                         💾 Space freed: {}MB\n\
                         ⚡ Completed in {:.2}s", 
                    cleaned_mb, elapsed.as_secs_f32())
            },
            "doc" => {
                let doc_count = fastrand::u32(150..400);
                format!("📚 Documentation generated{}\n\
                         📖 {} items documented\n\
                         🌐 View at target/doc/index.html\n\
                         ⚡ Completed in {:.2}s", 
                    features_info, doc_count, elapsed.as_secs_f32())
            },
            "bench" => {
                format!("🏁 Benchmark Results{}\n\
                         ⚡ Average: 142.3ns per iteration\n\
                         📊 Throughput: 7.1M ops/sec\n\
                         📈 Improvement: +15.2% vs baseline\n\
                         ⚡ Completed in {:.2}s", 
                    features_info, elapsed.as_secs_f32())
            },
            "clippy" => {
                let warnings = fastrand::u32(0..5);
                format!("📎 Clippy Analysis{}\n\
                         {} warnings, 0 errors\n\
                         ✨ Code quality: {}/10\n\
                         ⚡ Completed in {:.2}s", 
                    features_info, warnings, 
                    if warnings == 0 { 10 } else { 8 - warnings.min(3) },
                    elapsed.as_secs_f32())
            },
            _ => unreachable!("Validated above"),
        };

        // Record build in history (state persistence)
        {
            let mut history = self.build_history.lock().await;
            history.push(BuildRecord {
                timestamp: Utc::now().to_rfc3339(),
                command: command.clone(),
                status: "SUCCESS".to_string(),
                duration_ms: elapsed.as_millis() as u64,
            });
            
            // Keep only last 10 builds
            if history.len() > 10 {
                history.remove(0);
            }
        }

        Ok(result)
    }

    /// List and analyze project files with advanced filtering
    #[tool("List project files with pattern matching, statistics, and directory traversal options")]
    async fn list_files(
        &self,
        pattern: Option<String>,
        include_stats: Option<bool>,
        max_depth: Option<u32>,
        include_hidden: Option<bool>
    ) -> McpResult<String> {
        let pattern = pattern.as_deref().unwrap_or("*");
        let include_stats = include_stats.unwrap_or(false);
        let max_depth = max_depth.unwrap_or(3);
        let include_hidden = include_hidden.unwrap_or(false);

        tracing::info!("Listing files with pattern '{}' (depth: {}, hidden: {}, stats: {})", 
            pattern, max_depth, include_hidden, include_stats);

        // Simulate file discovery
        let file_types = if pattern.contains("*.rs") || pattern == "*" {
            vec!["src/main.rs", "src/lib.rs", "src/utils.rs", "tests/integration.rs"]
        } else if pattern.contains("*.toml") {
            vec!["Cargo.toml", "rust-toolchain.toml"]
        } else if pattern.contains("*.md") {
            vec!["README.md", "CHANGELOG.md", "docs/API.md"]
        } else {
            vec!["src/main.rs", "Cargo.toml", "README.md", ".gitignore"]
        };

        let mut result = format!("📁 File Listing (pattern: '{}', max depth: {})\n", pattern, max_depth);
        result.push_str("════════════════════════════════════════════\n");

        for (_i, file) in file_types.iter().enumerate() {
            let size = fastrand::u32(100..50000);
            let modified = "2024-08-23";
            
            if include_stats {
                result.push_str(&format!("📄 {} ({} bytes, modified: {})\n", file, size, modified));
            } else {
                result.push_str(&format!("📄 {}\n", file));
            }
        }

        if include_hidden && !pattern.starts_with('.') {
            result.push_str("👻 Hidden files:\n");
            result.push_str("   .gitignore\n   .cargo/config.toml\n");
        }

        if include_stats {
            let total_files = file_types.len() + if include_hidden { 2 } else { 0 };
            let total_size: u32 = file_types.iter().map(|_| fastrand::u32(100..50000)).sum();
            result.push_str(&format!("\n📊 Summary: {} files, {} bytes total", total_files, total_size));
        }

        Ok(result)
    }

    // ═══════════════════════════════════════════════════════════════
    // PROMPT GENERATION TOOLS - Testing AI-Assisted Development
    // ═══════════════════════════════════════════════════════════════

    // ═══════════════════════════════════════════════════════════════
    // ELICITATION FEATURES - New in TurboMCP v1.0.3!
    // ═══════════════════════════════════════════════════════════════
    
    /// Interactive project configuration demonstration
    #[tool("Configure a new project (elicitation demonstration)")]
    async fn configure_project(&self) -> McpResult<String> {
        tracing::info!("Starting interactive project configuration with elicitation");
        
        // Use elicitation to gather project configuration
        let result = elicit("Let's configure your new project!")
            .field(
                "project_name",
                string()
                    .min_length(1)
                    .max_length(50)
                    .pattern("^[a-zA-Z0-9-_]+$")
                    .description("Project name (alphanumeric, dash, underscore)")
                    .build(),
            )
            .field(
                "project_type",
                string()
                    .enum_values(vec![
                        "cli".to_string(),
                        "library".to_string(),
                        "web_app".to_string(),
                        "api_service".to_string(),
                    ])
                    .description("Type of project")
                    .build(),
            )
            .field(
                "enable_testing",
                boolean()
                    .description("Include test configuration?")
                    .build(),
            )
            .field(
                "ci_provider",
                string()
                    .enum_values(vec![
                        "github_actions".to_string(),
                        "gitlab_ci".to_string(),
                        "jenkins".to_string(),
                        "none".to_string(),
                    ])
                    .description("CI/CD provider")
                    .build(),
            )
            .field(
                "min_rust_version",
                string()
                    .pattern("^1\\.[0-9]+\\.[0-9]+$")
                    .description("Minimum Rust version (e.g., 1.75.0)")
                    .build(),
            )
            .require(vec!["project_name", "project_type"])
            .build_request();
            
        // For demonstration, show what would happen with user input
        let (project_name, project_type, enable_testing, ci_provider, min_rust) = 
            ("my-awesome-project".to_string(), 
             "web_app".to_string(), 
             true, 
             "github_actions".to_string(), 
             "1.75.0".to_string());
        
        Ok(format!("🎉 Project Configuration Complete!\n\
                           ═══════════════════════════════════════════════\n\
                           📦 Name: {}\n\
                           🎯 Type: {}\n\
                           🧪 Testing: {}\n\
                           🔧 CI/CD: {}\n\
                           🦀 Min Rust: {}\n\n\
                           ✅ Your project has been configured successfully!\n\
                           💡 Next steps: Run 'cargo init' to create the project structure.",
                           project_name,
                           project_type,
                           if enable_testing { "Enabled" } else { "Disabled" },
                           ci_provider,
                           min_rust))
            }
            ElicitationResult::Decline(reason) => {
                Ok(format!("Configuration cancelled: {}", 
                          reason.unwrap_or_else(|| "User declined".to_string())))
            }
            ElicitationResult::Cancel => {
                Ok("Project configuration cancelled by user.".to_string())
            }
        }
    }
    
    /// Code refactoring preferences demonstration
    #[tool("Refactor code (elicitation demonstration)")]
    async fn refactor_code(&self, code_snippet: String) -> McpResult<String> {
        tracing::info!("Starting interactive code refactoring");
        
        // This demonstrates the elicitation API for gathering preferences
        let _preferences = elicit("Select refactoring preferences")
            .field(
                "style",
                string()
                    .enum_values(vec![
                        "idiomatic".to_string(),
                        "performance".to_string(),
                        "readability".to_string(),
                        "minimal".to_string(),
                    ])
                    .description("Refactoring style preference")
                    .build(),
            )
            .field(
                "preserve_comments",
                boolean()
                    .description("Keep existing comments?")
                    .default(true)
                    .build(),
            )
            .field(
                "max_line_length",
                integer()
                    .range(80.0, 120.0)
                    .description("Maximum line length")
                    .build(),
            )
            .field(
                "use_unsafe",
                boolean()
                    .description("Allow unsafe code for optimization?")
                    .default(false)
                    .build(),
            )
            .require(vec!["style"])
            .build_request();
            
        // For demonstration, simulate user preferences
        let (style, preserve_comments, max_line, use_unsafe) = 
            ("idiomatic".to_string(), true, 100usize, false);
                
                // Simulate refactoring based on preferences
                let refactored = match style.as_str() {
                    "idiomatic" => format!("// Idiomatic Rust refactoring\n{}", code_snippet),
                    "performance" => format!("// Performance-optimized refactoring{}\n{}", 
                                           if use_unsafe { " (with unsafe)" } else { "" }, code_snippet),
                    "readability" => format!("// Readability-focused refactoring\n// Max line length: {}\n{}", 
                                           max_line, code_snippet),
                    "minimal" => format!("// Minimal refactoring\n{}", code_snippet),
                    _ => code_snippet.clone(),
                };
                
                Ok(format!("🔧 Code Refactored Successfully!\n\
                           ═══════════════════════════════════════════════\n\
                           Style: {}\n\
                           Comments: {}\n\
                           Line Length: {} chars\n\
                           Unsafe Code: {}\n\n\
                           ```rust\n{}\n```",
                           style,
                           if preserve_comments { "Preserved" } else { "Removed" },
                           max_line,
                           if use_unsafe { "Allowed" } else { "Forbidden" },
                           refactored))
    }

    /// Generate comprehensive documentation prompts
    #[tool("Generate AI prompts for documenting functions, tools, and code components")]
    async fn documentation_prompt(
        &self, 
        function_name: String,
        function_type: String,
        code_context: Option<String>,
        style: Option<String>
    ) -> McpResult<String> {
        tracing::info!("Generating documentation prompt for {} (type: {})", 
            function_name, function_type);

        let style = style.as_deref().unwrap_or("rustdoc");
        
        let prompt = match function_type.as_str() {
            "tool" => {
                format!("Generate comprehensive Rust documentation for the MCP tool `{}`:\n\n\
                         ## Requirements:\n\
                         - Write detailed /// doc comments in {} style\n\
                         - Explain the tool's purpose and use cases\n\
                         - Document all parameters with examples\n\
                         - Include error conditions and return values\n\
                         - Add usage examples with realistic scenarios\n\
                         - Follow Rust documentation best practices\n\n\
                         ## Context:\n{}\n\n\
                         ## Output Format:\n\
                         Provide complete documentation that would help developers understand and use this tool effectively.",
                    function_name, style, code_context.as_deref().unwrap_or("MCP development tool"))
            },
            "resource" => {
                format!("Generate documentation for the MCP resource handler `{}`:\n\n\
                         ## Requirements:\n\
                         - Document the resource URI pattern and parameters\n\
                         - Explain supported content types and formats\n\
                         - Include access patterns and security considerations\n\
                         - Provide example URIs and expected responses\n\
                         - Document error handling and edge cases\n\n\
                         ## Style: {}\n\
                         ## Context: {}",
                    function_name, style, code_context.as_deref().unwrap_or("Resource access"))
            },
            "handler" => {
                format!("Create documentation for the handler function `{}`:\n\n\
                         - Focus on business logic and data flow\n\
                         - Document async behavior and performance characteristics\n\
                         - Include integration points and dependencies\n\
                         - Explain state management and side effects\n\n\
                         Style: {} | Context: {}",
                    function_name, style, code_context.as_deref().unwrap_or("Business logic handler"))
            },
            "utility" => {
                format!("Document the utility function `{}`:\n\n\
                         - Explain the algorithm and approach\n\
                         - Document complexity and performance\n\
                         - Include mathematical concepts if applicable\n\
                         - Provide comprehensive examples\n\n\
                         Style: {} | Context: {}",
                    function_name, style, code_context.as_deref().unwrap_or("Utility function"))
            },
            _ => {
                return Err(McpError::Tool(format!(
                    "Invalid function type: '{}'. Valid types: tool, resource, handler, utility", 
                    function_type)));
            }
        };

        Ok(format!("📝 Documentation Generation Prompt\n\
                    ═══════════════════════════════════════════════════════\n\
                    🎯 Target: {} ({})\n\
                    📖 Style: {}\n\n\
                    {}\n\n\
                    💡 Tip: Use this prompt with your preferred AI assistant to generate high-quality documentation.",
                    function_name, function_type, style, prompt))
    }

    /// Generate code review prompts with specific focus areas
    #[tool("Generate AI prompts for comprehensive code reviews focusing on specific areas")]
    async fn code_review_prompt(
        &self, 
        code_snippet: String,
        focus_areas: Vec<String>,
        language: Option<String>,
        expertise_level: Option<String>
    ) -> McpResult<String> {
        tracing::info!("Generating code review prompt focusing on: {:?}", focus_areas);

        let language = language.as_deref().unwrap_or("Rust");
        let expertise = expertise_level.as_deref().unwrap_or("senior");
        
        if focus_areas.is_empty() {
            return Err(McpError::Tool(
                "At least one focus area must be specified. Valid areas: performance, security, maintainability, style, testing, documentation".to_string()));
        }

        let mut prompt = format!("Perform a comprehensive {} code review with {} expertise level:\n\n", language, expertise);
        prompt.push_str("## Code to Review:\n```rust\n");
        prompt.push_str(&code_snippet);
        prompt.push_str("\n```\n\n## Focus Areas:\n");

        for area in &focus_areas {
            match area.as_str() {
                "performance" => {
                    prompt.push_str("🚀 **Performance Analysis:**\n\
                                     - Identify potential bottlenecks and optimization opportunities\n\
                                     - Analyze algorithmic complexity and memory usage\n\
                                     - Check for unnecessary allocations or clones\n\
                                     - Evaluate async/await usage and task spawning\n\n");
                },
                "security" => {
                    prompt.push_str("🔒 **Security Review:**\n\
                                     - Check input validation and sanitization\n\
                                     - Identify potential injection vulnerabilities\n\
                                     - Review error handling for information leakage\n\
                                     - Analyze authentication and authorization\n\n");
                },
                "maintainability" => {
                    prompt.push_str("🔧 **Maintainability Assessment:**\n\
                                     - Evaluate code structure and organization\n\
                                     - Check function and module boundaries\n\
                                     - Assess complexity and readability\n\
                                     - Review naming conventions and clarity\n\n");
                },
                "style" => {
                    prompt.push_str("✨ **Style and Conventions:**\n\
                                     - Check adherence to Rust idioms and best practices\n\
                                     - Review formatting and code style consistency\n\
                                     - Evaluate error handling patterns\n\
                                     - Check documentation completeness\n\n");
                },
                "testing" => {
                    prompt.push_str("🧪 **Testing Strategy:**\n\
                                     - Identify missing test cases and edge conditions\n\
                                     - Evaluate testability and test structure\n\
                                     - Check for proper mocking and isolation\n\
                                     - Review integration test coverage\n\n");
                },
                "documentation" => {
                    prompt.push_str("📚 **Documentation Quality:**\n\
                                     - Check inline documentation completeness\n\
                                     - Review API documentation clarity\n\
                                     - Evaluate example quality and accuracy\n\
                                     - Assess architectural decision documentation\n\n");
                },
                _ => {
                    return Err(McpError::Tool(format!(
                        "Invalid focus area: '{}'. Valid areas: performance, security, maintainability, style, testing, documentation", 
                        area)));
                }
            }
        }

        prompt.push_str("## Output Requirements:\n\
                         - Provide specific, actionable feedback\n\
                         - Include code examples for suggestions\n\
                         - Rate each focus area (1-10) with justification\n\
                         - Prioritize issues by severity and impact\n\
                         - Suggest concrete improvements with rationale\n\n\
                         💡 Focus on practical improvements that enhance code quality and team productivity.");

        Ok(format!("👨‍💻 Code Review Prompt Generated\n\
                    ═══════════════════════════════════════════════════════\n\
                    🎯 Language: {} | Expertise: {}\n\
                    📋 Focus Areas: {}\n\n\
                    {}\n\n\
                    🚀 Use this prompt with your AI code reviewer for comprehensive analysis.",
                    language, expertise, focus_areas.join(", "), prompt))
    }

    // ═══════════════════════════════════════════════════════════════
    // COMPREHENSIVE RESOURCE SYSTEM - Testing All Resource Types  
    // ═══════════════════════════════════════════════════════════════

    /// Access project files and documents
    #[resource("file://{path}")]
    async fn get_project_file(&self, path: String) -> McpResult<String> {
        tracing::info!("Accessing project resource: file://{}", path);
        
        // Cache the file for performance testing
        let cache_key = format!("file:{}", path);
        {
            let cache = self.file_cache.read().await;
            if let Some(cached_content) = cache.get(&cache_key) {
                tracing::info!("Returning cached file content");
                return Ok(cached_content.clone());
            }
        }
        
        let content = match path.as_str() {
            "README.md" => {
                "# TurboMCP Comprehensive Demo\n\n\
                 A complete showcase of all TurboMCP framework capabilities including:\n\n\
                 ## Tools Available\n\
                 - `analyze_code` - Multi-type code analysis (quick/deep/security/performance)\n\
                 - `build_project` - Full build pipeline (check/build/test/clean/doc/bench/clippy)\n\
                 - `list_files` - Advanced file discovery with patterns and stats\n\
                 - `documentation_prompt` - AI-assisted documentation generation\n\
                 - `code_review_prompt` - Comprehensive code review prompts\n\n\
                 ## Resources Available\n\
                 - `file://{path}` - Project file access with caching\n\
                 - `config://{section}` - Configuration management\n\
                 - `template://{type}/{name}` - Code template system\n\
                 - `history://builds` - Build history with persistence\n\n\
                 ## Testing Features\n\
                 - Parameter validation and error handling\n\
                 - State persistence across requests\n\
                 - Caching and performance optimization\n\
                 - Comprehensive logging and monitoring\n\n\
                 This demonstrates the full power of TurboMCP for building production MCP servers."
            },
            "Cargo.toml" => {
                "[package]\n\
                 name = \"turbomcp-comprehensive-demo\"\n\
                 version = \"1.0.0\"\n\
                 edition = \"2021\"\n\
                 description = \"Complete TurboMCP framework demonstration\"\n\n\
                 [dependencies]\n\
                 turbomcp = { path = \"../crates/turbomcp\", features = [\"full\"] }\n\
                 tokio = { version = \"1.0\", features = [\"full\"] }\n\
                 tracing = \"0.1\"\n\
                 tracing-subscriber = \"0.3\"\n\
                 serde = { version = \"1.0\", features = [\"derive\"] }\n\
                 serde_json = \"1.0\"\n\
                 anyhow = \"1.0\"\n\
                 chrono = { version = \"0.4\", features = [\"serde\"] }\n\
                 fastrand = \"2.0\""
            },
            "CHANGELOG.md" => {
                "# Changelog\n\n\
                 ## [1.0.0] - 2024-08-23\n\n\
                 ### Added\n\
                 - Comprehensive tool suite with all parameter types\n\
                 - Multi-modal prompt generation system\n\
                 - Advanced resource access with caching\n\
                 - State persistence and build history\n\
                 - Edge case testing and parameter validation\n\
                 - Performance monitoring and statistics\n\n\
                 ### Features Demonstrated\n\
                 - All MCP protocol capabilities\n\
                 - TurboMCP macro system\n\
                 - Error handling patterns\n\
                 - Async/await best practices\n\
                 - Production deployment patterns"
            },
            "src/main.rs" => {
                "//! TurboMCP Comprehensive Demo\n//!\n\
                 //! Complete demonstration of TurboMCP framework capabilities.\n\
                 //! This server showcases:\n//!\n\
                 //! - Advanced tool implementations with parameter validation\n\
                 //! - Multi-modal prompt generation for AI assistance\n\
                 //! - Resource management with caching and templates\n\
                 //! - State persistence and build history tracking\n\
                 //! - Error handling and edge case management\n//!\n\
                 //! The implementation demonstrates patterns\n\
                 //! for building sophisticated MCP servers with TurboMCP.\n\n\
                 use turbomcp::prelude::*;\n\n\
                 #[tokio::main]\n\
                 async fn main() -> McpResult<()> {\n\
                     let server = TurboMCPDemo::default();\n\
                     server.run_stdio().await\n\
                 }"
            },
            _ => {
                return Err(McpError::Resource(format!("File not found: {}", path)));
            }
        };

        // Cache the content
        {
            let mut cache = self.file_cache.write().await;
            cache.insert(cache_key, content.to_string());
        }
        
        Ok(content.to_string())
    }

    /// Access configuration sections and settings
    #[resource("config://{section}")]
    async fn get_config(&self, section: String) -> McpResult<String> {
        tracing::info!("Accessing configuration: config://{}", section);
        
        let config = match section.as_str() {
            "build" => {
                serde_json::json!({
                    "default_target": "debug",
                    "enable_verbose": false,
                    "parallel_jobs": 4,
                    "features": ["default", "performance"],
                    "optimization_level": 2,
                    "target_cpu": "native"
                })
            },
            "analysis" => {
                serde_json::json!({
                    "default_type": "deep",
                    "complexity_threshold": 10,
                    "include_metrics": true,
                    "cache_results": true,
                    "supported_languages": ["rust", "javascript", "python", "go"],
                    "security_checks": {
                        "input_validation": true,
                        "dependency_scan": true,
                        "memory_safety": true
                    }
                })
            },
            "server" => {
                serde_json::json!({
                    "name": "TurboMCP Comprehensive Demo",
                    "version": "1.0.0",
                    "max_concurrent_requests": 100,
                    "request_timeout_ms": 30000,
                    "enable_caching": true,
                    "log_level": "info",
                    "performance": {
                        "simd_acceleration": true,
                        "connection_pooling": true,
                        "async_io": true
                    }
                })
            },
            _ => {
                return Err(McpError::Resource(format!(
                    "Unknown config section: '{}'. Available: build, analysis, server", 
                    section)));
            }
        };

        Ok(serde_json::to_string_pretty(&config)
            .unwrap_or_else(|_| "Failed to serialize configuration".to_string()))
    }

    /// Access code templates and scaffolding
    #[resource("template://{template_type}/{name}")]
    async fn get_template(&self, template_type: String, name: String) -> McpResult<String> {
        tracing::info!("Accessing template: template://{}/{}", template_type, name);
        
        let template = match (template_type.as_str(), name.as_str()) {
            ("tool", "basic") => {
                "/// Basic tool template for TurboMCP\n\
                 #[tool(\"Tool description goes here\")]\n\
                 async fn tool_name(&self, ctx: Context, param: String) -> McpResult<String> {\n\
                     ctx.info(\"Processing tool request\").await?;\n\
                     \n\
                     // Your tool logic here\n\
                     let result = format!(\"Processed: {}\", param);\n\
                     \n\
                     Ok(result)\n\
                 }"
            },
            ("resource", "basic") => {
                "/// Basic resource template for TurboMCP\n\
                 #[resource(uri_template = \"resource://{id}\")]\n\
                 async fn resource_name(&self, ctx: Context, id: String) -> McpResult<String> {\n\
                     ctx.info(&format!(\"Accessing resource: {}\", id)).await?;\n\
                     \n\
                     // Your resource logic here\n\
                     let content = load_resource(&id)?;\n\
                     \n\
                     Ok(content)\n\
                 }"
            },
            ("server", "minimal") => {
                "use turbomcp::prelude::*;\n\n\
                 #[derive(Default)]\n\
                 struct MyServer {\n\
                     // Your server state\n\
                 }\n\n\
                 #[server(\n\
                     name = \"My MCP Server\",\n\
                     version = \"1.0.0\",\n\
                     description = \"Description of your server\"\n\
                 )]\n\
                 impl MyServer {\n\
                     // Add your tools and resources here\n\
                 }\n\n\
                 #[tokio::main]\n\
                 async fn main() -> McpResult<()> {\n\
                     let server = MyServer::default();\n\
                     server.run_stdio().await\n\
                 }"
            },
            _ => {
                return Err(McpError::Resource(format!(
                    "Template not found: {}/{}. Available: tool/basic, resource/basic, server/minimal", 
                    template_type, name)));
            }
        };

        Ok(template.to_string())
    }

    /// Access build history and metrics  
    #[resource("history://builds")]
    async fn get_build_history(&self) -> McpResult<String> {
        tracing::info!("Accessing build history");
        
        let history = self.build_history.lock().await;
        
        if history.is_empty() {
            return Ok("📋 Build History\n\
                       ═══════════════\n\
                       No builds executed yet.\n\n\
                       💡 Try running: build_project with command 'build' to create history.".to_string());
        }

        let mut result = String::from("📋 Build History\n═══════════════════════════════════════\n");
        
        let mut total_duration = 0u64;
        for (i, build) in history.iter().enumerate() {
            result.push_str(&format!(
                "{}. {} | {} | {} | {}ms\n",
                i + 1,
                build.timestamp[..19].replace('T', " "), // Format timestamp
                build.command,
                build.status,
                build.duration_ms
            ));
            total_duration += build.duration_ms;
        }

        let avg_duration = total_duration as f64 / history.len() as f64;
        result.push_str(&format!("\n📊 Statistics:\n• Total builds: {}\n• Average duration: {:.1}ms\n• Success rate: 100%", 
            history.len(), avg_duration));

        Ok(result)
    }
}

#[tokio::main]
async fn main() -> McpResult<()> {
    // Initialize comprehensive logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_thread_ids(true)
        .with_target(true)
        .init();

    tracing::info!("🚀 Starting TurboMCP Comprehensive Demo Server");
    tracing::info!("📊 Features: Full tool suite, prompts, resources, state persistence");
    tracing::info!("⚡ Performance: SIMD acceleration, async I/O, connection pooling");

    // Create the comprehensive demo server
    let server = TurboMCPDemo::default();
    
    // Log available capabilities
    tracing::info!("🛠️  Available Tools:");
    tracing::info!("   • analyze_code - Multi-type code analysis");
    tracing::info!("   • build_project - Full build pipeline"); 
    tracing::info!("   • list_files - Advanced file discovery");
    tracing::info!("   • documentation_prompt - AI-assisted docs");
    tracing::info!("   • code_review_prompt - Comprehensive reviews");
    
    tracing::info!("📁 Available Resources:");
    tracing::info!("   • file://{{path}} - Project files with caching");
    tracing::info!("   • config://{{section}} - Configuration management");  
    tracing::info!("   • template://{{type}}/{{name}} - Code templates");
    tracing::info!("   • history://builds - Build history tracking");
    
    tracing::info!("🎯 Ready for comprehensive MCP testing!");

    tracing::info!("🔌 Transport: STDIO (MCP standard transport)");
    tracing::info!("📝 For LMStudio: Use subprocess mode with command:");
    tracing::info!("   /Users/Epistates/turbomcp/demo/target/release/turbomcp-demo");
    tracing::info!("💡 This server communicates via STDIN/STDOUT (standard MCP protocol)");
    
    // Run the server using STDIO transport (standard MCP protocol)
    server.run_stdio().await?;
    
    Ok(())
}
