//! Rich output formatting for CLI

use crate::cli::OutputFormat;
use crate::error::{CliError, CliResult};
use comfy_table::{Table, modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL};
use owo_colors::OwoColorize;
use serde::Serialize;
use turbomcp_protocol::types::*;

/// Format and display output based on format preference
pub struct Formatter {
    format: OutputFormat,
    colored: bool,
}

impl Formatter {
    #[must_use]
    pub fn new(format: OutputFormat, colored: bool) -> Self {
        Self { format, colored }
    }

    /// Display any serializable value
    pub fn display<T: Serialize + ?Sized>(&self, value: &T) -> CliResult<()> {
        match self.format {
            OutputFormat::Human => self.display_human(value),
            OutputFormat::Json => self.display_json(value, true),
            OutputFormat::Compact => self.display_json(value, false),
            OutputFormat::Yaml => self.display_yaml(value),
            OutputFormat::Table => self.display_human(value), // fallback to human
        }
    }

    /// Display tools list
    pub fn display_tools(&self, tools: &[Tool]) -> CliResult<()> {
        match self.format {
            OutputFormat::Human => {
                if tools.is_empty() {
                    self.print_info("No tools available");
                    return Ok(());
                }

                self.print_header("Available Tools");
                for tool in tools {
                    self.print_tool(tool);
                }
                self.print_footer(&format!("Total: {} tools", tools.len()));
                Ok(())
            }
            OutputFormat::Table => {
                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL)
                    .apply_modifier(UTF8_ROUND_CORNERS)
                    .set_header(vec!["Name", "Description", "Input Schema"]);

                for tool in tools {
                    let schema_summary = format_schema_summary(&tool.input_schema);

                    table.add_row(vec![
                        &tool.name,
                        tool.description.as_deref().unwrap_or("-"),
                        &schema_summary,
                    ]);
                }

                println!("{table}");
                Ok(())
            }
            _ => self.display(tools),
        }
    }

    /// Display resources list
    pub fn display_resources(&self, resources: &[Resource]) -> CliResult<()> {
        match self.format {
            OutputFormat::Human => {
                if resources.is_empty() {
                    self.print_info("No resources available");
                    return Ok(());
                }

                self.print_header("Available Resources");
                for resource in resources {
                    self.print_resource(resource);
                }
                self.print_footer(&format!("Total: {} resources", resources.len()));
                Ok(())
            }
            OutputFormat::Table => {
                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL)
                    .apply_modifier(UTF8_ROUND_CORNERS)
                    .set_header(vec!["URI", "Name", "Description", "MIME Type"]);

                for resource in resources {
                    let mime_str = resource
                        .mime_type
                        .as_ref()
                        .map(|m| m.as_str())
                        .unwrap_or("-");

                    table.add_row(vec![
                        resource.uri.as_str(),
                        &resource.name,
                        resource.description.as_deref().unwrap_or("-"),
                        mime_str,
                    ]);
                }

                println!("{table}");
                Ok(())
            }
            _ => self.display(resources),
        }
    }

    /// Display prompts list
    pub fn display_prompts(&self, prompts: &[Prompt]) -> CliResult<()> {
        match self.format {
            OutputFormat::Human => {
                if prompts.is_empty() {
                    self.print_info("No prompts available");
                    return Ok(());
                }

                self.print_header("Available Prompts");
                for prompt in prompts {
                    self.print_prompt(prompt);
                }
                self.print_footer(&format!("Total: {} prompts", prompts.len()));
                Ok(())
            }
            OutputFormat::Table => {
                let mut table = Table::new();
                table
                    .load_preset(UTF8_FULL)
                    .apply_modifier(UTF8_ROUND_CORNERS)
                    .set_header(vec!["Name", "Description", "Arguments"]);

                for prompt in prompts {
                    let args = prompt
                        .arguments
                        .as_ref()
                        .map(|a| {
                            a.iter()
                                .map(|arg| arg.name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .unwrap_or_else(|| "None".to_string());

                    table.add_row(vec![
                        &prompt.name,
                        prompt.description.as_deref().unwrap_or("-"),
                        &args,
                    ]);
                }

                println!("{table}");
                Ok(())
            }
            _ => self.display(prompts),
        }
    }

    /// Display server info
    pub fn display_server_info(&self, info: &Implementation) -> CliResult<()> {
        match self.format {
            OutputFormat::Human => {
                self.print_header("Server Information");
                self.print_kv("Name", &info.name);
                self.print_kv("Version", &info.version);
                Ok(())
            }
            _ => self.display(info),
        }
    }

    /// Display error with suggestions
    pub fn display_error(&self, error: &CliError) {
        if self.colored {
            eprintln!("{}: {}", "Error".bright_red().bold(), error);

            let suggestions = error.suggestions();
            if !suggestions.is_empty() {
                eprintln!("\n{}", "Suggestions:".bright_yellow().bold());
                for suggestion in suggestions {
                    eprintln!("  {} {}", "•".bright_blue(), suggestion);
                }
            }
        } else {
            eprintln!("Error: {error}");

            let suggestions = error.suggestions();
            if !suggestions.is_empty() {
                eprintln!("\nSuggestions:");
                for suggestion in suggestions {
                    eprintln!("  • {suggestion}");
                }
            }
        }
    }

    // Internal formatting helpers

    fn display_json<T: Serialize + ?Sized>(&self, value: &T, pretty: bool) -> CliResult<()> {
        let json = if pretty {
            serde_json::to_string_pretty(value)?
        } else {
            serde_json::to_string(value)?
        };
        println!("{json}");
        Ok(())
    }

    fn display_yaml<T: Serialize + ?Sized>(&self, value: &T) -> CliResult<()> {
        let yaml = serde_yaml::to_string(value)?;
        println!("{yaml}");
        Ok(())
    }

    fn display_human<T: Serialize + ?Sized>(&self, value: &T) -> CliResult<()> {
        // Fallback to pretty JSON for generic types
        self.display_json(value, true)
    }

    fn print_header(&self, text: &str) {
        if self.colored {
            println!("\n{}", text.bright_cyan().bold());
            println!("{}", "=".repeat(text.len()).bright_cyan());
        } else {
            println!("\n{text}");
            println!("{}", "=".repeat(text.len()));
        }
    }

    fn print_footer(&self, text: &str) {
        if self.colored {
            println!("\n{}", text.bright_black());
        } else {
            println!("\n{text}");
        }
    }

    fn print_info(&self, text: &str) {
        if self.colored {
            println!("{}", text.bright_blue());
        } else {
            println!("{text}");
        }
    }

    fn print_kv(&self, key: &str, value: &str) {
        if self.colored {
            println!("  {}: {}", key.bright_green().bold(), value);
        } else {
            println!("  {key}: {value}");
        }
    }

    fn print_tool(&self, tool: &Tool) {
        if self.colored {
            println!(
                "  {} {}",
                "•".bright_blue(),
                tool.name.bright_green().bold()
            );
            if let Some(desc) = &tool.description {
                println!("    {desc}");
            }
        } else {
            println!("  • {}", tool.name);
            if let Some(desc) = &tool.description {
                println!("    {desc}");
            }
        }
    }

    fn print_resource(&self, resource: &Resource) {
        if self.colored {
            println!(
                "  {} {}",
                "•".bright_blue(),
                resource.uri.as_str().bright_green().bold()
            );
            println!("    Name: {}", resource.name);
            if let Some(desc) = &resource.description {
                println!("    {desc}");
            }
        } else {
            println!("  • {}", resource.uri.as_str());
            println!("    Name: {}", resource.name);
            if let Some(desc) = &resource.description {
                println!("    {desc}");
            }
        }
    }

    fn print_prompt(&self, prompt: &Prompt) {
        if self.colored {
            println!(
                "  {} {}",
                "•".bright_blue(),
                prompt.name.bright_green().bold()
            );
            if let Some(desc) = &prompt.description {
                println!("    {desc}");
            }
            if let Some(args) = &prompt.arguments {
                if !args.is_empty() {
                    let arg_names: Vec<_> = args.iter().map(|a| a.name.as_str()).collect();
                    println!("    Arguments: {}", arg_names.join(", ").bright_yellow());
                }
            }
        } else {
            println!("  • {}", prompt.name);
            if let Some(desc) = &prompt.description {
                println!("    {desc}");
            }
            if let Some(args) = &prompt.arguments {
                if !args.is_empty() {
                    let arg_names: Vec<_> = args.iter().map(|a| a.name.as_str()).collect();
                    println!("    Arguments: {}", arg_names.join(", "));
                }
            }
        }
    }
}

/// Format schema summary for table display
fn format_schema_summary(schema: &ToolInputSchema) -> String {
    if let Some(props) = &schema.properties {
        if !props.is_empty() {
            let prop_names: Vec<_> = props.keys().map(|k| k.as_str()).collect();
            return prop_names.join(", ");
        }
    }
    "No properties".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_formatter_creation() {
        let formatter = Formatter::new(OutputFormat::Human, true);
        assert!(formatter.colored);
    }
}
