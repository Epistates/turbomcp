//! Human-readable colored output formatter
//!
//! Provides beautiful, colored output for terminal display with:
//! - Color-coded sections
//! - Tree-style hierarchies
//! - Table formatting for tools/resources
//! - TTY detection for automatic color disabling

use colored::*;
use std::io::Write;

use super::OutputFormatter;
use crate::error::ProxyResult;
use crate::introspection::ServerSpec;

/// Human-readable formatter with colored output
pub struct HumanFormatter {
    use_color: bool,
}

impl HumanFormatter {
    /// Create a new human formatter
    pub fn new() -> Self {
        Self {
            use_color: atty::is(atty::Stream::Stdout) && !std::env::var("NO_COLOR").is_ok(),
        }
    }

    /// Format a section header
    fn section_header(&self, title: &str) -> String {
        if self.use_color {
            format!("\n{}\n{}", title.bold().cyan(), "─".repeat(title.len()))
        } else {
            format!("\n{}\n{}", title, "─".repeat(title.len()))
        }
    }

    /// Format a key-value pair
    fn kv(&self, key: &str, value: &str) -> String {
        if self.use_color {
            format!("  {}: {}", key.bold(), value)
        } else {
            format!("  {}: {}", key, value)
        }
    }

    /// Format a list item
    fn list_item(&self, text: &str, level: usize) -> String {
        let indent = "  ".repeat(level);
        if self.use_color {
            format!("{}• {}", indent, text.bright_white())
        } else {
            format!("{}• {}", indent, text)
        }
    }
}

impl Default for HumanFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl OutputFormatter for HumanFormatter {
    fn write_spec(&self, spec: &ServerSpec, writer: &mut dyn Write) -> ProxyResult<()> {
        // Header
        writeln!(writer)?;
        if self.use_color {
            writeln!(
                writer,
                "{}",
                "═══════════════════════════════════════════════════════".cyan()
            )?;
            writeln!(
                writer,
                "  {}",
                "MCP Server Introspection Report".bold().bright_cyan()
            )?;
            writeln!(
                writer,
                "{}",
                "═══════════════════════════════════════════════════════".cyan()
            )?;
        } else {
            writeln!(
                writer,
                "═══════════════════════════════════════════════════════"
            )?;
            writeln!(writer, "  MCP Server Introspection Report")?;
            writeln!(
                writer,
                "═══════════════════════════════════════════════════════"
            )?;
        }

        // Server Info
        writeln!(writer, "{}", self.section_header("Server Information"))?;
        writeln!(writer, "{}", self.kv("Name", &spec.server_info.name))?;
        writeln!(writer, "{}", self.kv("Version", &spec.server_info.version))?;
        writeln!(writer, "{}", self.kv("Protocol", &spec.protocol_version))?;

        if let Some(ref instructions) = spec.instructions {
            writeln!(writer, "{}", self.kv("Instructions", instructions))?;
        }

        // Capabilities Summary
        writeln!(writer, "{}", self.section_header("Capabilities"))?;
        writeln!(
            writer,
            "{}",
            self.kv("Tools", &format!("{} available", spec.tools.len()))
        )?;
        writeln!(
            writer,
            "{}",
            self.kv("Resources", &format!("{} available", spec.resources.len()))
        )?;
        writeln!(
            writer,
            "{}",
            self.kv("Prompts", &format!("{} available", spec.prompts.len()))
        )?;

        if spec.capabilities.logging.is_some() {
            writeln!(
                writer,
                "  {}",
                if self.use_color {
                    "✓ Logging".green()
                } else {
                    "✓ Logging".normal()
                }
            )?;
        }
        if spec.supports_list_changed("tools") {
            writeln!(
                writer,
                "  {}",
                if self.use_color {
                    "✓ Tools list_changed notifications".green()
                } else {
                    "✓ Tools list_changed notifications".normal()
                }
            )?;
        }
        if spec.supports_list_changed("resources") {
            writeln!(
                writer,
                "  {}",
                if self.use_color {
                    "✓ Resources list_changed notifications".green()
                } else {
                    "✓ Resources list_changed notifications".normal()
                }
            )?;
        }
        if spec.supports_resource_subscriptions() {
            writeln!(
                writer,
                "  {}",
                if self.use_color {
                    "✓ Resource subscriptions".green()
                } else {
                    "✓ Resource subscriptions".normal()
                }
            )?;
        }

        // Tools
        if !spec.tools.is_empty() {
            writeln!(
                writer,
                "{}",
                self.section_header(&format!("Tools ({})", spec.tools.len()))
            )?;
            for tool in &spec.tools {
                writeln!(
                    writer,
                    "{}",
                    self.list_item(
                        &format!(
                            "{}",
                            if self.use_color {
                                tool.name.as_str().bold()
                            } else {
                                tool.name.as_str().normal()
                            }
                        ),
                        0
                    )
                )?;
                if let Some(ref desc) = tool.description {
                    writeln!(writer, "    {}", desc.dimmed())?;
                }

                // Show input schema summary
                if let Some(ref props) = tool.input_schema.properties {
                    if !props.is_empty() {
                        writeln!(
                            writer,
                            "    {}: {}",
                            "Parameters".bold(),
                            props.keys().cloned().collect::<Vec<_>>().join(", ")
                        )?;
                    }
                }

                // Show annotations
                if let Some(ref ann) = tool.annotations {
                    let mut hints = Vec::new();
                    if ann.read_only_hint == Some(true) {
                        hints.push("read-only");
                    }
                    if ann.destructive_hint == Some(true) {
                        hints.push("destructive");
                    }
                    if ann.idempotent_hint == Some(true) {
                        hints.push("idempotent");
                    }
                    if !hints.is_empty() {
                        writeln!(writer, "    {}: {}", "Hints".dimmed(), hints.join(", "))?;
                    }
                }
            }
        }

        // Resources
        if !spec.resources.is_empty() {
            writeln!(
                writer,
                "{}",
                self.section_header(&format!("Resources ({})", spec.resources.len()))
            )?;
            for resource in &spec.resources {
                writeln!(
                    writer,
                    "{}",
                    self.list_item(
                        &format!(
                            "{}",
                            if self.use_color {
                                resource.name.as_str().bold()
                            } else {
                                resource.name.as_str().normal()
                            }
                        ),
                        0
                    )
                )?;
                writeln!(writer, "    {}: {}", "URI".dimmed(), resource.uri)?;
                if let Some(ref desc) = resource.description {
                    writeln!(writer, "    {}", desc.dimmed())?;
                }
                if let Some(ref mime) = resource.mime_type {
                    writeln!(writer, "    {}: {}", "Type".dimmed(), mime)?;
                }
            }
        }

        // Prompts
        if !spec.prompts.is_empty() {
            writeln!(
                writer,
                "{}",
                self.section_header(&format!("Prompts ({})", spec.prompts.len()))
            )?;
            for prompt in &spec.prompts {
                writeln!(
                    writer,
                    "{}",
                    self.list_item(
                        &format!(
                            "{}",
                            if self.use_color {
                                prompt.name.as_str().bold()
                            } else {
                                prompt.name.as_str().normal()
                            }
                        ),
                        0
                    )
                )?;
                if let Some(ref desc) = prompt.description {
                    writeln!(writer, "    {}", desc.dimmed())?;
                }
                if !prompt.arguments.is_empty() {
                    let args: Vec<String> = prompt
                        .arguments
                        .iter()
                        .map(|a| {
                            if a.required == Some(true) {
                                format!("{} (required)", a.name)
                            } else {
                                a.name.clone()
                            }
                        })
                        .collect();
                    writeln!(writer, "    {}: {}", "Arguments".dimmed(), args.join(", "))?;
                }
            }
        }

        // Footer
        writeln!(writer)?;
        if self.use_color {
            writeln!(
                writer,
                "{}",
                "═══════════════════════════════════════════════════════".cyan()
            )?;
            writeln!(writer, "  {} {}", "Summary:".bold(), spec.summary())?;
            writeln!(
                writer,
                "{}",
                "═══════════════════════════════════════════════════════".cyan()
            )?;
        } else {
            writeln!(
                writer,
                "═══════════════════════════════════════════════════════"
            )?;
            writeln!(writer, "  Summary: {}", spec.summary())?;
            writeln!(
                writer,
                "═══════════════════════════════════════════════════════"
            )?;
        }

        Ok(())
    }

    fn write_error(&self, error: &str, writer: &mut dyn Write) -> ProxyResult<()> {
        if self.use_color {
            writeln!(writer, "{}: {}", "Error".bold().red(), error)?;
        } else {
            writeln!(writer, "Error: {}", error)?;
        }
        Ok(())
    }

    fn write_success(&self, message: &str, writer: &mut dyn Write) -> ProxyResult<()> {
        if self.use_color {
            writeln!(writer, "{} {}", "✓".green(), message)?;
        } else {
            writeln!(writer, "✓ {}", message)?;
        }
        Ok(())
    }
}
