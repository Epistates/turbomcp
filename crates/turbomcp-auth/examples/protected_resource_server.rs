//! MCP Server with Protected Resources (RFC 9728)
//!
//! This example shows how to set up an MCP server that:
//! 1. Advertises protected resources via /.well-known/protected-resource
//! 2. Validates bearer tokens on incoming requests
//! 3. Returns appropriate 401 responses with WWW-Authenticate headers

use turbomcp_auth::server::{
    ProtectedResourceMetadataBuilder, WwwAuthenticateBuilder, BearerTokenValidator,
    unauthorized_response_body,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Protected Resource Server Example ===\n");

    // Step 1: Build Protected Resource Metadata (RFC 9728)
    println!("1. Protected Resource Metadata (/.well-known/protected-resource):");
    let metadata = ProtectedResourceMetadataBuilder::new(
        "https://mcp.example.com".to_string(),
        "https://auth.example.com/.well-known/oauth-authorization-server".to_string(),
    )
    .with_scopes(vec![
        "mcp:read".to_string(),
        "mcp:write".to_string(),
        "openid".to_string(),
    ])
    .with_documentation("https://mcp.example.com/docs".to_string())
    .build();

    println!("{}\n", serde_json::to_string_pretty(&metadata)?);

    // Step 2: Handle 401 Unauthorized with WWW-Authenticate header
    println!("2. Handling 401 Unauthorized Response:");
    println!("   Status: 401 Unauthorized");

    let www_authenticate = WwwAuthenticateBuilder::new(
        "https://mcp.example.com/.well-known/protected-resource".to_string(),
    )
    .with_scope("mcp:read mcp:write".to_string())
    .build();

    println!("   Header: WWW-Authenticate: {}\n", www_authenticate);

    let error_body = unauthorized_response_body(
        "https://mcp.example.com/.well-known/protected-resource",
        Some("mcp:read"),
    );
    println!("   Body: {}\n", serde_json::to_string_pretty(&error_body)?);

    // Step 3: Extract and validate bearer tokens
    println!("3. Bearer Token Validation:");

    let examples = vec![
        ("Bearer valid_token_123", true),
        ("bearer valid_token_456", true), // Case insensitive
        ("invalid_format", false),
        ("Bearer ", false),
    ];

    for (header, should_succeed) in examples {
        let result = BearerTokenValidator::extract_from_header(header);
        let status = if result.is_ok() == should_succeed {
            "✓"
        } else {
            "✗"
        };
        println!(
            "   {} {} => {:?}",
            status,
            header,
            result.map(|t| format!("token: {}", t))
                .unwrap_or_else(|e| format!("error: {}", e))
        );
    }

    println!("\n4. Token Format Validation:");
    let tokens = vec![
        ("", false, "empty"),
        ("123", false, "too short"),
        ("valid_token_12345", true, "valid"),
    ];

    for (token, should_succeed, desc) in tokens {
        let result = BearerTokenValidator::validate_format(token);
        let status = if result.is_ok() == should_succeed {
            "✓"
        } else {
            "✗"
        };
        println!("   {} {} => {:?}", status, desc, result);
    }

    // Step 5: OAuth2 Flow for Client Authorization
    println!("\n5. OAuth2 Authorization Flow:");
    println!("   Client: Initiates OAuth2 Authorization Code flow");
    println!("   User: Grants permissions");
    println!("   Server: Receives access token");
    println!("   Server: Validates token and returns protected resource");

    // Step 6: Resource Access Pattern
    println!("\n6. Typical Request/Response Pattern:");
    println!("\n   Request 1 (no token):");
    println!("     GET /api/resource");
    println!("     -->  401 Unauthorized");
    println!("     -->  WWW-Authenticate: Bearer resource_metadata=\"...\"");
    println!("\n   Request 2 (after getting token):");
    println!("     GET /api/resource");
    println!("     Authorization: Bearer access_token_xyz");
    println!("     -->  200 OK");
    println!("     -->  {{\"data\": \"protected_resource_data\"}}");

    // Step 7: Recommended Server Implementation
    println!("\n7. Recommended Server Implementation:");
    println!("   a. Implement /.well-known/protected-resource endpoint");
    println!("      Returns: ProtectedResourceMetadataBuilder::new(...).build()");
    println!();
    println!("   b. Implement Bearer token extraction middleware");
    println!("      Use: BearerTokenValidator::extract_from_header()");
    println!();
    println!("   c. Implement token validation");
    println!("      - Call OAuth2 provider's introspection endpoint");
    println!("      - Check token scopes against required scopes");
    println!("      - Verify token not expired");
    println!();
    println!("   d. Return 401 with WWW-Authenticate on invalid token");
    println!("      Use: WwwAuthenticateBuilder::new(metadata_uri).build()");

    Ok(())
}
