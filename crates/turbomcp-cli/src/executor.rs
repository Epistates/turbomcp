//! Command execution using turbomcp-client

use crate::cli_new::*;
use crate::error::{CliError, CliResult};
use crate::formatter::Formatter;
use crate::transport::{ClientType, create_client};
use std::collections::HashMap;

/// Execute CLI commands
pub struct CommandExecutor {
    pub formatter: Formatter,
    verbose: bool,
}

impl CommandExecutor {
    pub fn new(format: OutputFormat, colored: bool, verbose: bool) -> Self {
        Self {
            formatter: Formatter::new(format, colored),
            verbose,
        }
    }

    /// Display an error with rich formatting
    pub fn display_error(&self, error: &CliError) {
        self.formatter.display_error(error);
    }

    /// Execute a command
    pub async fn execute(&self, command: Commands) -> CliResult<()> {
        match command {
            Commands::Tools(cmd) => self.execute_tool_command(cmd).await,
            Commands::Resources(cmd) => self.execute_resource_command(cmd).await,
            Commands::Prompts(cmd) => self.execute_prompt_command(cmd).await,
            Commands::Complete(cmd) => self.execute_completion_command(cmd).await,
            Commands::Server(cmd) => self.execute_server_command(cmd).await,
            Commands::Sample(cmd) => self.execute_sampling_command(cmd).await,
            Commands::Connect(conn) => self.execute_connect(conn).await,
            Commands::Status(conn) => self.execute_status(conn).await,
        }
    }

    // Tool commands

    async fn execute_tool_command(&self, command: ToolCommands) -> CliResult<()> {
        match command {
            ToolCommands::List { conn } => {
                let client_type = create_client(&conn).await?;
                let tools = match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.list_tools().await?
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.list_tools().await?
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.list_tools().await?
                    }
                };
                self.formatter.display_tools(&tools)
            }

            ToolCommands::Call {
                conn,
                name,
                arguments,
            } => {
                let args: HashMap<String, serde_json::Value> =
                    if arguments.trim().is_empty() || arguments == "{}" {
                        HashMap::new()
                    } else {
                        serde_json::from_str(&arguments).map_err(|e| {
                            CliError::InvalidArguments(format!("Invalid JSON arguments: {}", e))
                        })?
                    };

                let client_type = create_client(&conn).await?;
                let result = match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.call_tool(&name, Some(args)).await?
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.call_tool(&name, Some(args)).await?
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.call_tool(&name, Some(args)).await?
                    }
                };
                self.formatter.display(&result)
            }

            ToolCommands::Schema { conn, name } => {
                let client_type = create_client(&conn).await?;
                let tools = match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.list_tools().await?
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.list_tools().await?
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.list_tools().await?
                    }
                };

                if let Some(tool_name) = name {
                    let tool = tools.iter().find(|t| t.name == tool_name).ok_or_else(|| {
                        CliError::Other(format!("Tool '{}' not found", tool_name))
                    })?;

                    self.formatter.display(&tool.input_schema)
                } else {
                    let schemas: Vec<_> = tools
                        .iter()
                        .map(|t| {
                            serde_json::json!({
                                "name": t.name,
                                "schema": t.input_schema
                            })
                        })
                        .collect();

                    self.formatter.display(&schemas)
                }
            }

            ToolCommands::Export { conn, output } => {
                let client_type = create_client(&conn).await?;
                let tools = match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.list_tools().await?
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.list_tools().await?
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.list_tools().await?
                    }
                };

                std::fs::create_dir_all(&output)?;

                for tool in tools {
                    let filename = format!("{}.json", tool.name);
                    let filepath = output.join(filename);
                    let schema = serde_json::to_string_pretty(&tool.input_schema)?;
                    std::fs::write(&filepath, schema)?;

                    if self.verbose {
                        println!("Exported: {}", filepath.display());
                    }
                }

                println!("✓ Exported schemas to: {}", output.display());
                Ok(())
            }
        }
    }

    // Resource commands

    async fn execute_resource_command(&self, command: ResourceCommands) -> CliResult<()> {
        match command {
            ResourceCommands::List { conn } => {
                let client_type = create_client(&conn).await?;
                let resource_uris = match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.list_resources().await?
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.list_resources().await?
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.list_resources().await?
                    }
                };

                // Note: Client returns URIs only. For full Resource objects with metadata,
                // we would need to extend the client API.
                self.formatter.display(&resource_uris)
            }

            ResourceCommands::Read { conn, uri } => {
                let client_type = create_client(&conn).await?;
                let result = match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.read_resource(&uri).await?
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.read_resource(&uri).await?
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.read_resource(&uri).await?
                    }
                };
                self.formatter.display(&result)
            }

            ResourceCommands::Templates { conn } => {
                let client_type = create_client(&conn).await?;
                let templates = match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.list_resource_templates().await?
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.list_resource_templates().await?
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.list_resource_templates().await?
                    }
                };
                self.formatter.display(&templates)
            }

            ResourceCommands::Subscribe { conn, uri } => {
                let client_type = create_client(&conn).await?;
                match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.subscribe(&uri).await?;
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.subscribe(&uri).await?;
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.subscribe(&uri).await?;
                    }
                }
                println!("✓ Subscribed to: {uri}");
                Ok(())
            }

            ResourceCommands::Unsubscribe { conn, uri } => {
                let client_type = create_client(&conn).await?;
                match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.unsubscribe(&uri).await?;
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.unsubscribe(&uri).await?;
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.unsubscribe(&uri).await?;
                    }
                }
                println!("✓ Unsubscribed from: {uri}");
                Ok(())
            }
        }
    }

    // Prompt commands

    async fn execute_prompt_command(&self, command: PromptCommands) -> CliResult<()> {
        match command {
            PromptCommands::List { conn } => {
                let client_type = create_client(&conn).await?;
                let prompts = match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.list_prompts().await?
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.list_prompts().await?
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.list_prompts().await?
                    }
                };
                self.formatter.display_prompts(&prompts)
            }

            PromptCommands::Get {
                conn,
                name,
                arguments,
            } => {
                // Parse arguments as HashMap<String, Value>
                let args: HashMap<String, serde_json::Value> =
                    if arguments.trim().is_empty() || arguments == "{}" {
                        HashMap::new()
                    } else {
                        serde_json::from_str(&arguments).map_err(|e| {
                            CliError::InvalidArguments(format!("Invalid JSON arguments: {}", e))
                        })?
                    };

                let args_option = if args.is_empty() { None } else { Some(args) };

                let client_type = create_client(&conn).await?;
                let result = match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.get_prompt(&name, args_option).await?
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.get_prompt(&name, args_option.clone()).await?
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.get_prompt(&name, args_option.clone()).await?
                    }
                };
                self.formatter.display(&result)
            }

            PromptCommands::Schema { conn, name } => {
                let client_type = create_client(&conn).await?;
                let prompts = match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.list_prompts().await?
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.list_prompts().await?
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.list_prompts().await?
                    }
                };

                let prompt = prompts
                    .iter()
                    .find(|p| p.name == name)
                    .ok_or_else(|| CliError::Other(format!("Prompt '{}' not found", name)))?;

                self.formatter.display(&prompt.arguments)
            }
        }
    }

    // Completion commands

    async fn execute_completion_command(&self, command: CompletionCommands) -> CliResult<()> {
        match command {
            CompletionCommands::Get {
                conn,
                ref_type,
                ref_value,
                argument,
            } => {
                let client_type = create_client(&conn).await?;

                // Use the appropriate completion method based on reference type
                let result = match ref_type {
                    RefType::Prompt => {
                        let arg_name = argument.as_deref().unwrap_or("value");
                        match client_type {
                            ClientType::Stdio(mut client) => {
                                client.initialize().await?;
                                client
                                    .complete_prompt(&ref_value, arg_name, "", None)
                                    .await?
                            }
                            ClientType::Tcp(mut client) => {
                                client.initialize().await?;
                                client
                                    .complete_prompt(&ref_value, arg_name, "", None)
                                    .await?
                            }
                            ClientType::Unix(mut client) => {
                                client.initialize().await?;
                                client
                                    .complete_prompt(&ref_value, arg_name, "", None)
                                    .await?
                            }
                        }
                    }
                    RefType::Resource => {
                        let arg_name = argument.as_deref().unwrap_or("uri");
                        match client_type {
                            ClientType::Stdio(mut client) => {
                                client.initialize().await?;
                                client
                                    .complete_resource(&ref_value, arg_name, "", None)
                                    .await?
                            }
                            ClientType::Tcp(mut client) => {
                                client.initialize().await?;
                                client
                                    .complete_resource(&ref_value, arg_name, "", None)
                                    .await?
                            }
                            ClientType::Unix(mut client) => {
                                client.initialize().await?;
                                client
                                    .complete_resource(&ref_value, arg_name, "", None)
                                    .await?
                            }
                        }
                    }
                };

                self.formatter.display(&result)
            }
        }
    }

    // Server commands

    async fn execute_server_command(&self, command: ServerCommands) -> CliResult<()> {
        match command {
            ServerCommands::Info { conn } => {
                let client_type = create_client(&conn).await?;
                let info = match client_type {
                    ClientType::Stdio(mut client) => {
                        let result = client.initialize().await?;
                        result.server_info
                    }
                    ClientType::Tcp(mut client) => {
                        let result = client.initialize().await?;
                        result.server_info
                    }
                    ClientType::Unix(mut client) => {
                        let result = client.initialize().await?;
                        result.server_info
                    }
                };
                self.formatter.display_server_info(&info)
            }

            ServerCommands::Ping { conn } => {
                let client_type = create_client(&conn).await?;
                let start = std::time::Instant::now();

                match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.ping().await?;
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.ping().await?;
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.ping().await?;
                    }
                }

                let elapsed = start.elapsed();
                println!("✓ Pong! ({:.2}ms)", elapsed.as_secs_f64() * 1000.0);
                Ok(())
            }

            ServerCommands::LogLevel { conn, level } => {
                // Convert level once before using
                let protocol_level: turbomcp_protocol::types::LogLevel = level.clone().into();

                let client_type = create_client(&conn).await?;
                match client_type {
                    ClientType::Stdio(mut client) => {
                        client.initialize().await?;
                        client.set_log_level(protocol_level).await?;
                    }
                    ClientType::Tcp(mut client) => {
                        client.initialize().await?;
                        client.set_log_level(protocol_level).await?;
                    }
                    ClientType::Unix(mut client) => {
                        client.initialize().await?;
                        client.set_log_level(protocol_level).await?;
                    }
                }
                println!("✓ Log level set to: {:?}", level);
                Ok(())
            }

            ServerCommands::Roots { conn } => {
                // Roots are part of server capabilities returned during initialization
                let client_type = create_client(&conn).await?;
                let result = match client_type {
                    ClientType::Stdio(mut client) => client.initialize().await?,
                    ClientType::Tcp(mut client) => client.initialize().await?,
                    ClientType::Unix(mut client) => client.initialize().await?,
                };

                // Display server capabilities which includes roots info
                self.formatter.display(&result.server_capabilities)
            }
        }
    }

    // Sampling commands

    async fn execute_sampling_command(&self, _command: SamplingCommands) -> CliResult<()> {
        Err(CliError::NotSupported(
            "Sampling commands require LLM handler implementation".to_string(),
        ))
    }

    // Connection commands

    async fn execute_connect(&self, conn: Connection) -> CliResult<()> {
        println!("Connecting to server...");
        let client_type = create_client(&conn).await?;

        let info = match client_type {
            ClientType::Stdio(mut client) => {
                let result = client.initialize().await?;
                result.server_info
            }
            ClientType::Tcp(mut client) => {
                let result = client.initialize().await?;
                result.server_info
            }
            ClientType::Unix(mut client) => {
                let result = client.initialize().await?;
                result.server_info
            }
        };

        println!("✓ Connected successfully!");
        self.formatter.display_server_info(&info)
    }

    async fn execute_status(&self, conn: Connection) -> CliResult<()> {
        let client_type = create_client(&conn).await?;

        let info = match client_type {
            ClientType::Stdio(mut client) => {
                let result = client.initialize().await?;
                result.server_info
            }
            ClientType::Tcp(mut client) => {
                let result = client.initialize().await?;
                result.server_info
            }
            ClientType::Unix(mut client) => {
                let result = client.initialize().await?;
                result.server_info
            }
        };

        println!("Status: Connected");
        self.formatter.display_server_info(&info)
    }
}
