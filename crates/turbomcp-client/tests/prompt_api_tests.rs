// Tests for enhanced prompt API functionality with full MCP compliance
//
// This test suite verifies that TurboMCP's prompt API correctly implements the
// MCP 2025-06-18 specification, providing full schema information and argument support.

use async_trait::async_trait;
use bytes::Bytes;
use std::collections::HashMap;
use turbomcp_client::Client;
use turbomcp_core::MessageId;
// Types are accessed through the client, no direct import needed
use turbomcp_transport::{
    Transport, TransportCapabilities, TransportMessage, TransportMetrics, TransportResult,
    TransportState, TransportType,
};

// Mock transport for testing
#[derive(Debug)]
struct MockTransport {
    responses: Vec<serde_json::Value>,
    request_count: std::sync::Arc<std::sync::Mutex<usize>>,
    capabilities: TransportCapabilities,
    state: TransportState,
    metrics: TransportMetrics,
}

impl MockTransport {
    fn new(responses: Vec<serde_json::Value>) -> Self {
        Self {
            responses,
            request_count: std::sync::Arc::new(std::sync::Mutex::new(0)),
            capabilities: TransportCapabilities::default(),
            state: TransportState::Disconnected,
            metrics: TransportMetrics::default(),
        }
    }
}

#[async_trait]
impl Transport for MockTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Stdio
    }

    fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    async fn state(&self) -> TransportState {
        self.state.clone()
    }

    async fn connect(&mut self) -> TransportResult<()> {
        self.state = TransportState::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> TransportResult<()> {
        self.state = TransportState::Disconnected;
        Ok(())
    }

    async fn send(&mut self, _message: TransportMessage) -> TransportResult<()> {
        Ok(())
    }

    async fn receive(&mut self) -> TransportResult<Option<TransportMessage>> {
        let mut count = self.request_count.lock().unwrap();
        let response = self.responses.get(*count).cloned().unwrap_or_else(|| {
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": *count,
                "error": {
                    "code": -1,
                    "message": "Unexpected request"
                }
            })
        });
        *count += 1;
        let message = TransportMessage::new(
            MessageId::String(count.to_string()),
            Bytes::from(response.to_string()),
        );
        Ok(Some(message))
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.clone()
    }

    fn endpoint(&self) -> Option<String> {
        Some("mock://transport".to_string())
    }
}

#[tokio::test]
async fn test_list_prompts_returns_full_schema() {
    // Setup mock responses
    let initialize_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "prompts": {}
            },
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }
    });

    let prompts_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "prompts": [
                {
                    "name": "greeting",
                    "title": "Greeting Generator",
                    "description": "Generates personalized greeting messages",
                    "arguments": [
                        {
                            "name": "name",
                            "title": "User Name",
                            "description": "The name of the person to greet",
                            "required": true
                        },
                        {
                            "name": "greeting",
                            "title": "Greeting Type",
                            "description": "The type of greeting (hello, hi, hey)",
                            "required": false
                        }
                    ]
                },
                {
                    "name": "simple",
                    "title": "Simple Prompt",
                    "description": "A prompt without arguments"
                }
            ]
        }
    });

    let transport = MockTransport::new(vec![initialize_response, prompts_response]);
    let mut client = Client::new(transport);

    // Initialize client
    client.initialize().await.unwrap();

    // Test list_prompts (now returns full Prompt objects)
    let prompts = client.list_prompts().await.unwrap();

    assert_eq!(prompts.len(), 2);

    // Verify first prompt (with arguments)
    let greeting_prompt = &prompts[0];
    assert_eq!(greeting_prompt.name, "greeting");
    assert_eq!(
        greeting_prompt.title,
        Some("Greeting Generator".to_string())
    );
    assert_eq!(
        greeting_prompt.description,
        Some("Generates personalized greeting messages".to_string())
    );

    let args = greeting_prompt.arguments.as_ref().unwrap();
    assert_eq!(args.len(), 2);

    // Verify arguments
    assert_eq!(args[0].name, "name");
    assert_eq!(args[0].title, Some("User Name".to_string()));
    assert_eq!(
        args[0].description,
        Some("The name of the person to greet".to_string())
    );
    assert_eq!(args[0].required, Some(true));

    assert_eq!(args[1].name, "greeting");
    assert_eq!(args[1].title, Some("Greeting Type".to_string()));
    assert_eq!(
        args[1].description,
        Some("The type of greeting (hello, hi, hey)".to_string())
    );
    assert_eq!(args[1].required, Some(false));

    // Verify second prompt (without arguments)
    let simple_prompt = &prompts[1];
    assert_eq!(simple_prompt.name, "simple");
    assert_eq!(simple_prompt.title, Some("Simple Prompt".to_string()));
    assert_eq!(
        simple_prompt.description,
        Some("A prompt without arguments".to_string())
    );
    assert!(simple_prompt.arguments.is_none());
}

#[tokio::test]
async fn test_list_prompts_deprecated_still_works() {
    // Setup mock responses
    let initialize_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "prompts": {}
            },
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }
    });

    let prompts_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "prompts": [
                {
                    "name": "greeting",
                    "title": "Greeting Generator",
                    "description": "Generates personalized greeting messages",
                    "arguments": [
                        {
                            "name": "name",
                            "title": "User Name",
                            "description": "The name of the person to greet",
                            "required": true
                        }
                    ]
                },
                {
                    "name": "simple",
                    "title": "Simple Prompt",
                    "description": "A prompt without arguments"
                }
            ]
        }
    });

    let transport = MockTransport::new(vec![initialize_response, prompts_response]);
    let mut client = Client::new(transport);

    // Initialize client
    client.initialize().await.unwrap();

    // Test list_prompts returns full objects now
    let prompts = client.list_prompts().await.unwrap();

    assert_eq!(prompts.len(), 2);
    assert_eq!(prompts[0].name, "greeting");
    assert_eq!(prompts[1].name, "simple");
}

#[tokio::test]
async fn test_get_prompt_supports_parameters() {
    // Setup mock responses
    let initialize_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "prompts": {}
            },
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }
    });

    let prompt_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "description": "A greeting prompt",
            "messages": [
                {
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": "Hello Alice! How are you today?"
                    }
                }
            ]
        }
    });

    let transport = MockTransport::new(vec![initialize_response, prompt_response]);
    let mut client = Client::new(transport);

    // Initialize client
    client.initialize().await.unwrap();

    // Test get_prompt with parameters
    let mut arguments = HashMap::new();
    arguments.insert(
        "name".to_string(),
        serde_json::Value::String("Alice".to_string()),
    );
    arguments.insert(
        "greeting".to_string(),
        serde_json::Value::String("Hello".to_string()),
    );

    let result = client
        .get_prompt("greeting", Some(arguments))
        .await
        .unwrap();

    assert_eq!(result.description, Some("A greeting prompt".to_string()));
    assert_eq!(result.messages.len(), 1);
}

#[tokio::test]
async fn test_get_prompt_with_args_without_parameters() {
    // Setup mock responses
    let initialize_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "prompts": {}
            },
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }
    });

    let prompt_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "description": "A template greeting prompt",
            "messages": [
                {
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": "{greeting} {name}! How are you today?"
                    }
                }
            ]
        }
    });

    let transport = MockTransport::new(vec![initialize_response, prompt_response]);
    let mut client = Client::new(transport);

    // Initialize client
    client.initialize().await.unwrap();

    // Test get_prompt_with_args without parameters (template form)
    let result = client.get_prompt("greeting", None).await.unwrap();

    assert_eq!(
        result.description,
        Some("A template greeting prompt".to_string())
    );
    assert_eq!(result.messages.len(), 1);
    // Should contain template variables
    // Note: This would show the template form with {greeting} {name} placeholders
}

#[tokio::test]
async fn test_prompt_arguments_accessible_from_list() {
    // Setup mock responses
    let initialize_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "prompts": {}
            },
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }
    });

    let prompts_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "prompts": [
                {
                    "name": "greeting",
                    "title": "Greeting Generator",
                    "description": "Generates personalized greeting messages",
                    "arguments": [
                        {
                            "name": "name",
                            "title": "User Name",
                            "description": "The name of the person to greet",
                            "required": true
                        },
                        {
                            "name": "greeting",
                            "title": "Greeting Type",
                            "description": "The type of greeting (hello, hi, hey)",
                            "required": false
                        }
                    ]
                },
                {
                    "name": "simple",
                    "title": "Simple Prompt",
                    "description": "A prompt without arguments"
                }
            ]
        }
    });

    let transport = MockTransport::new(vec![initialize_response, prompts_response]);
    let mut client = Client::new(transport);

    // Initialize client
    client.initialize().await.unwrap();

    // Test that argument schemas are accessible from list_prompts
    let prompts = client.list_prompts().await.unwrap();
    assert_eq!(prompts.len(), 2);

    // Find prompt with arguments
    let greeting_prompt = prompts.iter().find(|p| p.name == "greeting").unwrap();
    let schema = greeting_prompt.arguments.as_ref().unwrap();
    assert_eq!(schema.len(), 2);

    // Verify first argument
    assert_eq!(schema[0].name, "name");
    assert_eq!(schema[0].title, Some("User Name".to_string()));
    assert_eq!(
        schema[0].description,
        Some("The name of the person to greet".to_string())
    );
    assert_eq!(schema[0].required, Some(true));

    // Find prompt without arguments
    let simple_prompt = prompts.iter().find(|p| p.name == "simple").unwrap();
    assert!(simple_prompt.arguments.is_none());
}

#[tokio::test]
async fn test_get_prompt_without_arguments() {
    // Setup mock responses
    let initialize_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "prompts": {}
            },
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }
    });

    let prompt_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "description": "A greeting prompt",
            "messages": [
                {
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": "Hello! How are you today?"
                    }
                }
            ]
        }
    });

    let transport = MockTransport::new(vec![initialize_response, prompt_response]);
    let mut client = Client::new(transport);

    // Initialize client
    client.initialize().await.unwrap();

    // Test get_prompt without arguments
    let result = client.get_prompt("greeting", None).await.unwrap();

    assert_eq!(result.description, Some("A greeting prompt".to_string()));
    assert_eq!(result.messages.len(), 1);
}

#[tokio::test]
async fn test_error_handling_uninitialized_client() {
    let transport = MockTransport::new(vec![]);
    let mut client = Client::new(transport);

    // Test all methods fail with uninitialized client
    let result = client.list_prompts().await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Client not initialized")
    );

    let result = client.get_prompt("test", None).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Client not initialized")
    );
}

#[tokio::test]
async fn test_error_handling_empty_prompt_name() {
    // Setup mock responses
    let initialize_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "prompts": {}
            },
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }
    });

    let transport = MockTransport::new(vec![initialize_response]);
    let mut client = Client::new(transport);

    // Initialize client
    client.initialize().await.unwrap();

    // Test get_prompt with empty name
    let result = client.get_prompt("", None).await;
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Prompt name cannot be empty")
    );
}

// Integration test demonstrating the complete workflow for MCP Studio
#[tokio::test]
async fn test_mcp_studio_integration_workflow() {
    // Setup comprehensive mock responses
    let initialize_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 0,
        "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "prompts": {}
            },
            "serverInfo": {
                "name": "test-server",
                "version": "1.0.0"
            }
        }
    });

    let prompts_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "prompts": [
                {
                    "name": "code_review",
                    "title": "Code Review Assistant",
                    "description": "Reviews code for best practices and potential issues",
                    "arguments": [
                        {
                            "name": "code",
                            "title": "Code to Review",
                            "description": "The source code to be reviewed",
                            "required": true
                        },
                        {
                            "name": "language",
                            "title": "Programming Language",
                            "description": "The programming language of the code",
                            "required": true
                        },
                        {
                            "name": "focus",
                            "title": "Review Focus",
                            "description": "Specific areas to focus on (security, performance, style)",
                            "required": false
                        }
                    ]
                }
            ]
        }
    });

    let prompt_execution_response = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "result": {
            "description": "Code review with parameters substituted",
            "messages": [
                {
                    "role": "user",
                    "content": {
                        "type": "text",
                        "text": "Please review this Rust code for security issues: fn main() { println!(\"Hello, world!\"); }"
                    }
                }
            ]
        }
    });

    let transport = MockTransport::new(vec![
        initialize_response,
        prompts_response,
        prompt_execution_response,
    ]);
    let mut client = Client::new(transport);

    // Initialize client
    client.initialize().await.unwrap();

    // Step 1: MCP Studio lists all available prompts with schemas
    let prompts = client.list_prompts().await.unwrap();
    assert_eq!(prompts.len(), 1);

    let code_review_prompt = &prompts[0];
    assert_eq!(code_review_prompt.name, "code_review");
    assert_eq!(
        code_review_prompt.title,
        Some("Code Review Assistant".to_string())
    );

    // Step 2: MCP Studio gets the schema for dynamic form generation from prompt object
    let schema = code_review_prompt.arguments.as_ref().unwrap();
    assert_eq!(schema.len(), 3);

    // Verify schema details for UI form generation
    let code_arg = &schema[0];
    assert_eq!(code_arg.name, "code");
    assert_eq!(code_arg.required, Some(true));

    let language_arg = &schema[1];
    assert_eq!(language_arg.name, "language");
    assert_eq!(language_arg.required, Some(true));

    let focus_arg = &schema[2];
    assert_eq!(focus_arg.name, "focus");
    assert_eq!(focus_arg.required, Some(false));

    // Step 3: MCP Studio executes the prompt with user-provided arguments
    let mut arguments = HashMap::new();
    arguments.insert(
        "code".to_string(),
        serde_json::Value::String("fn main() { println!(\"Hello, world!\"); }".to_string()),
    );
    arguments.insert(
        "language".to_string(),
        serde_json::Value::String("rust".to_string()),
    );
    arguments.insert(
        "focus".to_string(),
        serde_json::Value::String("security".to_string()),
    );

    let result = client
        .get_prompt("code_review", Some(arguments))
        .await
        .unwrap();

    assert_eq!(
        result.description,
        Some("Code review with parameters substituted".to_string())
    );
    assert_eq!(result.messages.len(), 1);

    // The test successfully demonstrates the complete MCP Studio workflow:
    // 1. List prompts with full schema information
    // 2. Extract argument schemas for dynamic form generation
    // 3. Execute prompts with user-provided arguments
}
