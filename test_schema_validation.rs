use serde_json;
use turbomcp_protocol::types::*;

fn main() {
    println!("=== TurboMCP Schema Validation Test ===\n");

    // Test 1: Server Request - CreateMessage
    println("1. Testing ServerRequest::CreateMessage serialization");
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
    println!("✓ Server CreateMessage Request JSON:");
    println!("{}\n", json);
    
    // Test deserialization back
    let _deserialized: ServerRequest = serde_json::from_str(&json).unwrap();
    println!("✓ Deserialization successful\n");

    // Test 2: Client Request - CallTool  
    println!("2. Testing ClientRequest::CallTool serialization");
    let mut arguments = std::collections::HashMap::new();
    arguments.insert("pattern".to_string(), serde_json::Value::String("*.py".to_string()));
    arguments.insert("directory".to_string(), serde_json::Value::String("/src".to_string()));
    
    let call_tool = CallToolRequest {
        name: "file_search".to_string(),
        arguments: Some(arguments),
    };
    
    let client_request = ClientRequest::CallTool(call_tool);
    let json = serde_json::to_string_pretty(&client_request).unwrap();
    println!("✓ Client CallTool Request JSON:");
    println!("{}\n", json);

    // Test 3: Annotations compatibility
    println!("3. Testing Annotations schema compatibility");
    let mut annotations = Annotations::default();
    annotations.audience = Some(vec!["user".to_string(), "assistant".to_string()]);
    annotations.priority = Some(0.8);
    
    let json = serde_json::to_string_pretty(&annotations).unwrap();
    println!("✓ Our Annotations JSON:");
    println!("{}", json);
    
    // Try schema-compliant annotation with lastModified
    let schema_annotation_json = r#"{
        "audience": ["user", "assistant"],
        "priority": 0.8,
        "lastModified": "2025-08-29T12:00:00Z"
    }"#;
    
    match serde_json::from_str::<Annotations>(schema_annotation_json) {
        Ok(parsed) => {
            println!("✓ Schema annotation parsed successfully:");
            println!("  - audience: {:?}", parsed.audience);
            println!("  - priority: {:?}", parsed.priority);
            println!("  - custom fields: {:?}", parsed.custom);
            
            if parsed.custom.contains_key("lastModified") {
                println!("✓ lastModified captured in custom fields");
            }
        }
        Err(e) => {
            println!("✗ Failed to parse schema annotation: {}", e);
        }
    }

    // Test 4: ElicitRequest serialization
    println!("\n4. Testing ElicitRequest schema compliance");
    let schema = ElicitationSchema::new()
        .add_string_property("username", true, Some("Database username".to_string()))
        .add_string_property("password", true, Some("Database password".to_string()));

    let elicit_params = ElicitRequestParams {
        message: "Please provide your database credentials".to_string(),
        requested_schema: schema,
    };
    
    let server_elicit = ServerRequest::ElicitationCreate(elicit_params);
    let json = serde_json::to_string_pretty(&server_elicit).unwrap();
    println!("✓ Server ElicitRequest JSON:");
    println!("{}\n", json);

    println!("=== Validation Complete ===");
}