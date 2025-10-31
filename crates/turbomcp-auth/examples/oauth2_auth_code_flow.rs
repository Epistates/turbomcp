//! OAuth 2.1 Authorization Code Flow with PKCE
//!
//! This example demonstrates how to perform OAuth 2.1 authorization code flow
//! with PKCE (RFC 7636) for public or confidential clients.
//!
//! Flow:
//! 1. Client initiates authorization request with PKCE
//! 2. User authorizes the application
//! 3. Authorization server redirects with authorization code
//! 4. Client exchanges code for access token using code_verifier

use turbomcp_auth::{
    config::{OAuth2Config, OAuth2FlowType, ProviderType},
    oauth2::OAuth2Client,
};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: Create OAuth2 configuration
    let oauth_config = OAuth2Config {
        client_id: "my-client-id".to_string(),
        client_secret: "my-client-secret".to_string().into(), // Can be empty for public clients
        auth_url: "https://provider.example.com/oauth/authorize".to_string(),
        token_url: "https://provider.example.com/oauth/token".to_string(),
        revocation_url: Some("https://provider.example.com/oauth/revoke".to_string()), // RFC 7009
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec![
            "openid".to_string(),
            "profile".to_string(),
            "email".to_string(),
        ],
        flow_type: OAuth2FlowType::AuthorizationCode,
        additional_params: std::collections::HashMap::new(),
        security_level: Default::default(),
        #[cfg(feature = "dpop")]
        dpop_config: None,
        mcp_resource_uri: None,
        auto_resource_indicators: true,
    };

    // Step 2: Create OAuth2 client
    let oauth_client = OAuth2Client::new(&oauth_config, ProviderType::Generic)?;

    // Step 3: Generate authorization URL with PKCE
    println!("=== OAuth 2.1 Authorization Code Flow ===\n");

    let state = uuid::Uuid::new_v4().to_string(); // CSRF protection
    let (auth_url, code_verifier) =
        oauth_client.authorization_code_flow(oauth_config.scopes.clone(), state);

    println!("1. Authorization URL (open in browser):");
    println!("   {}\n", auth_url);
    println!("2. Code Verifier (save for token exchange):");
    println!("   {}\n", code_verifier);

    // Step 4: Simulate user authorizing and redirect with code
    // In a real application, the user would click the authorization URL,
    // authorize the application, and the authorization server would redirect
    // to the redirect_uri with an authorization code
    println!("3. After user authorizes, authorization server redirects to:");
    println!(
        "   {redirect_uri}?code=AUTH_CODE&state=STATE\n",
        redirect_uri = oauth_config.redirect_uri
    );

    // Step 5: Exchange authorization code for tokens
    // This would normally come from the redirect URL parameters
    // For demo purposes, we'll show the structure:
    println!("4. To exchange code for token, call:");
    println!(
        "   oauth_client.exchange_code_for_token(code, \"{}\").await?",
        code_verifier
    );
    println!("\nThis returns TokenInfo with:");
    println!("   - access_token: Bearer token for API requests");
    println!("   - refresh_token: Token to refresh access_token (if provided by provider)");
    println!("   - expires_in: Token expiration in seconds");
    println!("   - scope: Granted scopes");

    // Step 6: After getting access token, use it to access protected resources
    println!("\n5. Use access token in API requests:");
    println!("   Authorization: Bearer {{access_token}}");

    Ok(())
}
