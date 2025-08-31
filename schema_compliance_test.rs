// Schema Compliance Validation Test
// This test validates TurboMCP types against the official MCP 2025-06-18 schema

use serde_json;
use turbomcp_protocol::types::*;

#[cfg(test)]
mod schema_compliance_tests {
    use super::*;

    #[test]
    fn test_server_request_serialization() {
        // Test CreateMessage (server-initiated sampling)
        let create_message = CreateMessageRequest {
            messages: vec![SamplingMessage {
                role: Role::User,
                content: ContentBlock::Text(TextContent {
                    text: "Hello, please help me".to_string(),
                    annotations: None,
                    meta: None,
                }),
            }],
            model_preferences: None,
            system_prompt: Some("You are a helpful assistant.".to_string()),
            include_context: Some(IncludeContext::ThisServer),
            temperature: Some(0.7),
            max_tokens: Some(1000),
            stop_sequences: None,
            metadata: None,
        };
        
        let server_request = ServerRequest::CreateMessage(create_message);
        let json = serde_json::to_string_pretty(&server_request).unwrap();
        println!("CreateMessage Request: {}", json);
        
        // Verify it can be deserialized back
        let _deserialized: ServerRequest = serde_json::from_str(&json).unwrap();
    }
    
    #[test]
    fn test_client_request_serialization() {
        // Test CallTool request
        let mut arguments = std::collections::HashMap::new();
        arguments.insert("pattern".to_string(), serde_json::Value::String("*.py".to_string()));
        arguments.insert("directory".to_string(), serde_json::Value::String("/src".to_string()));
        
        let call_tool = CallToolRequest {
            name: "file_search".to_string(),
            arguments: Some(arguments),
        };
        
        let client_request = ClientRequest::CallTool(call_tool);
        let json = serde_json::to_string_pretty(&client_request).unwrap();
        println!("CallTool Request: {}", json);
        
        // Verify it can be deserialized back
        let _deserialized: ClientRequest = serde_json::from_str(&json).unwrap();
    }
    
    #[test]
    fn test_annotations_compliance() {
        // Test our current Annotations implementation
        let mut annotations = Annotations::default();
        annotations.audience = Some(vec!["user".to_string(), "assistant".to_string()]);
        annotations.priority = Some(0.8);
        
        let json = serde_json::to_string_pretty(&annotations).unwrap();
        println!("Annotations: {}", json);
        
        // Try to deserialize an annotation with lastModified (from official schema)
        let schema_annotation_json = r#"{
            "audience": ["user", "assistant"],
            "priority": 0.8,
            "lastModified": "2025-08-29T12:00:00Z"
        }"#;
        
        // This should work if our implementation is compatible
        let result: Result<Annotations, _> = serde_json::from_str(schema_annotation_json);
        match result {
            Ok(parsed) => {
                println!("Successfully parsed schema annotation: {:?}", parsed);
                assert_eq!(parsed.audience, Some(vec!["user".to_string(), "assistant".to_string()]));
                assert_eq!(parsed.priority, Some(0.8));
                // lastModified should be in custom field due to flatten
                assert!(parsed.custom.contains_key("lastModified"));
            }
            Err(e) => {
                eprintln!("Failed to parse schema annotation: {}", e);
                panic!("Annotation parsing failed");
            }
        }
    }
}

fn main() {
    println!("Running schema compliance validation...");
    
    // This would normally be run with `cargo test`
    println!("Run with: cargo test schema_compliance_tests");
}