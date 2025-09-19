//! Comprehensive OAuth 2.0 Integration Tests
//!
//! Tests the complete OAuth implementation including:
//! - All OAuth flows (Authorization Code, Client Credentials, Device Code)
//! - Token storage and retrieval
//! - Provider configuration
//! - Token expiration and refresh logic
//! - Session management and cleanup
//! - Multi-provider support

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use turbomcp::McpError;
use turbomcp::auth::{
    AccessToken, AuthProvider, OAuth2Config, OAuth2FlowType, OAuth2Provider, ProviderType,
    SecurityLevel, TokenStorage,
};

/// Test implementation of TokenStorage for comprehensive testing
#[derive(Debug, Default)]
struct TestTokenStorage {
    access_tokens: Arc<RwLock<HashMap<String, AccessToken>>>,
    refresh_tokens: Arc<RwLock<HashMap<String, oauth2::RefreshToken>>>,
}

#[async_trait::async_trait]
impl TokenStorage for TestTokenStorage {
    async fn store_access_token(&self, user_id: &str, token: &AccessToken) -> Result<(), McpError> {
        self.access_tokens
            .write()
            .await
            .insert(user_id.to_string(), token.clone());
        Ok(())
    }

    async fn get_access_token(&self, user_id: &str) -> Result<Option<AccessToken>, McpError> {
        Ok(self.access_tokens.read().await.get(user_id).cloned())
    }

    async fn store_refresh_token(
        &self,
        user_id: &str,
        token: &oauth2::RefreshToken,
    ) -> Result<(), McpError> {
        self.refresh_tokens
            .write()
            .await
            .insert(user_id.to_string(), token.clone());
        Ok(())
    }

    async fn get_refresh_token(
        &self,
        user_id: &str,
    ) -> Result<Option<oauth2::RefreshToken>, McpError> {
        Ok(self.refresh_tokens.read().await.get(user_id).cloned())
    }

    async fn revoke_tokens(&self, user_id: &str) -> Result<(), McpError> {
        self.access_tokens.write().await.remove(user_id);
        self.refresh_tokens.write().await.remove(user_id);
        Ok(())
    }

    async fn list_users(&self) -> Result<Vec<String>, McpError> {
        Ok(self.access_tokens.read().await.keys().cloned().collect())
    }
}

impl TestTokenStorage {
    fn new() -> Self {
        Self {
            access_tokens: Arc::new(RwLock::new(HashMap::new())),
            refresh_tokens: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn token_count(&self) -> usize {
        self.access_tokens.read().await.len()
    }
}

/// Create a test OAuth provider for comprehensive testing
fn create_test_oauth_provider(provider_type: ProviderType) -> Result<OAuth2Provider, McpError> {
    let config = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: "test_client_secret".to_string(),
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["read".to_string(), "profile".to_string()],
        additional_params: {
            let mut params = HashMap::new();
            params.insert("access_type".to_string(), "offline".to_string());
            params
        },
        flow_type: OAuth2FlowType::AuthorizationCode,
        security_level: SecurityLevel::Standard,
        mcp_resource_uri: None,
        auto_resource_indicators: false,
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());

    let provider_name = match &provider_type {
        ProviderType::Google => "google_provider".to_string(),
        ProviderType::GitHub => "github_provider".to_string(),
        ProviderType::Microsoft => "microsoft_provider".to_string(),
        ProviderType::Custom(name) => format!("{}_provider", name),
        ProviderType::GitLab => "gitlab_provider".to_string(),
        ProviderType::Generic => "generic_provider".to_string(),
    };

    OAuth2Provider::new(provider_name, config, provider_type, token_storage)
}

#[tokio::test]
async fn test_oauth_provider_creation_all_types() {
    // Test creating providers for all supported types
    let provider_types = [
        ProviderType::Google,
        ProviderType::GitHub,
        ProviderType::Microsoft,
        ProviderType::Custom("test_custom".to_string()),
    ];

    for provider_type in provider_types {
        let provider = create_test_oauth_provider(provider_type.clone());
        assert!(
            provider.is_ok(),
            "Failed to create {:?} provider",
            provider_type
        );

        let provider = provider.unwrap();
        assert_eq!(provider.get_provider_type(), provider_type);
    }
}

#[tokio::test]
async fn test_authorization_flow_url_generation() {
    let provider = create_test_oauth_provider(ProviderType::Google).unwrap();

    // Test authorization URL generation
    let auth_result = provider.start_authorization().await;
    assert!(auth_result.is_ok(), "Authorization start should succeed");

    let auth_result = auth_result.unwrap();
    assert!(
        auth_result
            .auth_url
            .contains("https://example.com/oauth/authorize")
    );
    assert!(auth_result.auth_url.contains("client_id=test_client_id"));
    assert!(auth_result.auth_url.contains("redirect_uri="));
    assert!(auth_result.auth_url.contains("code_challenge="));
    assert!(auth_result.auth_url.contains("code_challenge_method=S256"));
    assert!(!auth_result.state.is_empty());
}

#[tokio::test]
async fn test_token_storage_integration() {
    let token_storage = TestTokenStorage::new();
    let user_id = "test_user_123";

    // Test storing and retrieving access token
    let access_token = AccessToken::new(
        "access_token_value".to_string(),
        Some(SystemTime::now() + Duration::from_secs(3600)),
        vec!["read".to_string(), "write".to_string()],
        {
            let mut meta = HashMap::new();
            meta.insert(
                "provider".to_string(),
                serde_json::Value::String("google".to_string()),
            );
            meta
        },
    );

    // Store token
    let result = token_storage
        .store_access_token(user_id, &access_token)
        .await;
    assert!(result.is_ok(), "Token storage should succeed");

    // Retrieve token
    let retrieved = token_storage.get_access_token(user_id).await;
    assert!(retrieved.is_ok(), "Token retrieval should succeed");

    let retrieved = retrieved.unwrap();
    assert!(retrieved.is_some(), "Token should be found");

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.token(), access_token.token());
    assert_eq!(retrieved.scopes(), access_token.scopes());
    assert_eq!(retrieved.metadata(), access_token.metadata());

    // Test token count
    assert_eq!(token_storage.token_count().await, 1);

    // Test listing users
    let users = token_storage.list_users().await.unwrap();
    assert_eq!(users.len(), 1);
    assert_eq!(users[0], user_id);

    // Test token revocation
    let result = token_storage.revoke_tokens(user_id).await;
    assert!(result.is_ok(), "Token revocation should succeed");
    assert_eq!(token_storage.token_count().await, 0);
}

#[tokio::test]
async fn test_token_expiration_logic() {
    let provider = create_test_oauth_provider(ProviderType::Google).unwrap();

    // Test expired token
    let expired_token = AccessToken::new(
        "expired_token".to_string(),
        Some(SystemTime::now() - Duration::from_secs(3600)), // 1 hour ago
        vec![],
        HashMap::new(),
    );

    assert!(
        provider.is_token_expired(&expired_token),
        "Token should be expired"
    );

    // Test non-expired token
    let valid_token = AccessToken::new(
        "valid_token".to_string(),
        Some(SystemTime::now() + Duration::from_secs(3600)), // 1 hour from now
        vec![],
        HashMap::new(),
    );

    assert!(
        !provider.is_token_expired(&valid_token),
        "Token should not be expired"
    );

    // Test token without expiration
    let no_expiry_token =
        AccessToken::new("no_expiry_token".to_string(), None, vec![], HashMap::new());

    assert!(
        !provider.is_token_expired(&no_expiry_token),
        "Token without expiration should not be expired"
    );
}

#[tokio::test]
async fn test_refresh_behavior_logic() {
    let provider = create_test_oauth_provider(ProviderType::Google).unwrap();

    // Test proactive refresh behavior (should refresh before expiry)
    let soon_to_expire_token = AccessToken::new(
        "soon_to_expire".to_string(),
        Some(SystemTime::now() + Duration::from_secs(60)), // Expires in 1 minute
        vec![],
        HashMap::new(),
    );

    let should_refresh = provider.should_refresh_token(&soon_to_expire_token);
    // Should refresh proactively (within 5 minutes of expiry)
    assert!(
        should_refresh,
        "Should proactively refresh token expiring soon"
    );

    // Test token that's not yet ready for proactive refresh
    let long_valid_token = AccessToken::new(
        "long_valid".to_string(),
        Some(SystemTime::now() + Duration::from_secs(3600)), // Expires in 1 hour
        vec![],
        HashMap::new(),
    );

    let should_refresh = provider.should_refresh_token(&long_valid_token);
    assert!(
        !should_refresh,
        "Should not refresh token with long validity"
    );
}

#[tokio::test]
async fn test_token_metadata_management() {
    let provider = create_test_oauth_provider(ProviderType::GitHub).unwrap();

    let mut token = AccessToken::new("test_token".to_string(), None, vec![], HashMap::new());

    // Add metadata
    provider.add_token_metadata(
        &mut token,
        "user_id",
        serde_json::Value::String("user123".to_string()),
    );
    provider.add_token_metadata(
        &mut token,
        "login_time",
        serde_json::Value::Number(serde_json::Number::from(1234567890)),
    );

    // Verify metadata was added
    assert_eq!(token.metadata().len(), 2);
    assert_eq!(
        token.metadata()["user_id"],
        serde_json::Value::String("user123".to_string())
    );
    assert_eq!(
        token.metadata()["login_time"],
        serde_json::Value::Number(serde_json::Number::from(1234567890))
    );
}

#[tokio::test]
async fn test_session_cleanup() {
    let provider = create_test_oauth_provider(ProviderType::Microsoft).unwrap();

    // Start multiple auth sessions
    let _auth1 = provider.start_authorization().await.unwrap();
    let _auth2 = provider.start_authorization().await.unwrap();
    let _auth3 = provider.start_authorization().await.unwrap();

    // Clean up expired sessions (this tests the method exists and doesn't panic)
    provider.cleanup_expired_sessions().await;
    // Note: We can't easily test the actual cleanup without manipulating internal state
    // but we verify the method works without errors
}

#[tokio::test]
async fn test_multi_provider_configuration() {
    // Test different provider configurations
    let google_provider = create_test_oauth_provider(ProviderType::Google).unwrap();
    let github_provider = create_test_oauth_provider(ProviderType::GitHub).unwrap();
    let microsoft_provider = create_test_oauth_provider(ProviderType::Microsoft).unwrap();

    // Each provider should have distinct configurations
    assert_eq!(google_provider.get_provider_type(), ProviderType::Google);
    assert_eq!(github_provider.get_provider_type(), ProviderType::GitHub);
    assert_eq!(
        microsoft_provider.get_provider_type(),
        ProviderType::Microsoft
    );

    // Each should generate different auth URLs
    let google_auth = google_provider.start_authorization().await.unwrap();
    let github_auth = github_provider.start_authorization().await.unwrap();
    let microsoft_auth = microsoft_provider.start_authorization().await.unwrap();

    assert_ne!(google_auth.state, github_auth.state);
    assert_ne!(github_auth.state, microsoft_auth.state);
    assert_ne!(google_auth.auth_url, github_auth.auth_url);
}

#[tokio::test]
async fn test_device_authorization_flow() {
    let provider =
        create_test_oauth_provider(ProviderType::Custom("device_test".to_string())).unwrap();

    // Test device authorization flow (this will fail without actual OAuth server, but tests the API)
    let result = provider.device_code_flow().await;

    // We expect this to fail since we don't have a real OAuth server
    // but we test that the error is the expected type (not a compilation error)
    assert!(
        result.is_err(),
        "Device flow should fail without real OAuth server"
    );

    // The error should be related to the request, not the code structure
    let error = result.unwrap_err();
    match error {
        McpError::InvalidInput(_) => {} // Expected - invalid configuration or request failure
        _ => panic!("Unexpected error type: {:?}", error),
    }
}

#[tokio::test]
async fn test_client_credentials_flow() {
    let provider =
        create_test_oauth_provider(ProviderType::Custom("client_test".to_string())).unwrap();

    // Test client credentials flow
    let result = provider.client_credentials_flow().await;

    // We expect this to fail since our test provider doesn't have client credentials configured
    assert!(
        result.is_err(),
        "Client credentials flow should fail without proper configuration"
    );

    let error = result.unwrap_err();
    match error {
        McpError::InvalidInput(msg) => {
            assert!(msg.contains("Client credentials flow not supported by this provider"));
        }
        McpError::Unauthorized(msg) => {
            // Test provider has client credentials configured but no real OAuth server
            assert!(msg.contains("Client credentials exchange failed"));
        }
        _ => panic!("Unexpected error type: {:?}", error),
    }
}

#[tokio::test]
async fn test_oauth_config_validation() {
    // Test OAuth config with all required fields
    let config = OAuth2Config {
        client_id: "test_id".to_string(),
        client_secret: "test_secret".to_string(),
        auth_url: "https://auth.example.com".to_string(),
        token_url: "https://token.example.com".to_string(),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["read".to_string()],
        additional_params: HashMap::new(),
        flow_type: OAuth2FlowType::AuthorizationCode,
        security_level: SecurityLevel::Standard,
        mcp_resource_uri: None,
        auto_resource_indicators: false,
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());

    let provider = OAuth2Provider::new(
        "test_provider".to_string(),
        config,
        ProviderType::Custom("config_test".to_string()),
        token_storage,
    );

    assert!(
        provider.is_ok(),
        "Provider creation with valid config should succeed"
    );
}

#[tokio::test]
async fn test_comprehensive_oauth_workflow() {
    // This test validates the complete OAuth workflow integration
    let provider = create_test_oauth_provider(ProviderType::Google).unwrap();
    let token_storage = TestTokenStorage::new();

    // Step 1: Start authorization
    let auth_result = provider.start_authorization().await;
    assert!(
        auth_result.is_ok(),
        "Authorization should start successfully"
    );

    let auth_result = auth_result.unwrap();
    assert!(!auth_result.auth_url.is_empty());
    assert!(!auth_result.state.is_empty());

    // Step 2: Simulate token storage
    let test_token = AccessToken::new(
        "comprehensive_test_token".to_string(),
        Some(SystemTime::now() + Duration::from_secs(3600)),
        vec!["read".to_string(), "profile".to_string()],
        HashMap::new(),
    );

    let user_id = "comprehensive_test_user";
    let result = token_storage.store_access_token(user_id, &test_token).await;
    assert!(result.is_ok(), "Token storage should succeed");

    // Step 3: Token retrieval and validation
    let retrieved = token_storage.get_access_token(user_id).await;
    assert!(retrieved.is_ok() && retrieved.as_ref().unwrap().is_some());

    let retrieved_token = retrieved.unwrap().unwrap();
    assert_eq!(retrieved_token.token(), test_token.token());

    // Step 4: Token expiration check
    assert!(!provider.is_token_expired(&retrieved_token));

    // Step 5: Refresh behavior check
    let should_refresh = provider.should_refresh_token(&retrieved_token);
    assert!(!should_refresh, "Fresh token should not need refresh");

    // Step 6: Provider type validation
    assert_eq!(provider.get_provider_type(), ProviderType::Google);

    // Step 7: Cleanup
    let cleanup_result = token_storage.revoke_tokens(user_id).await;
    assert!(cleanup_result.is_ok(), "Token cleanup should succeed");
    assert_eq!(token_storage.token_count().await, 0);
}

#[tokio::test]
async fn test_oauth_error_handling() {
    // Test various error scenarios

    // Invalid provider creation (this tests that errors are properly handled)
    let invalid_config = OAuth2Config {
        client_id: "".to_string(), // Invalid empty client ID
        client_secret: "secret".to_string(),
        auth_url: "not_a_url".to_string(),       // Invalid URL
        token_url: "also_not_a_url".to_string(), // Invalid URL
        redirect_uri: "invalid_redirect".to_string(),
        scopes: vec![],
        additional_params: HashMap::new(),
        flow_type: OAuth2FlowType::AuthorizationCode,
        security_level: SecurityLevel::Standard,
        mcp_resource_uri: None,
        auto_resource_indicators: false,
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());
    let provider = OAuth2Provider::new(
        "error_test".to_string(),
        invalid_config,
        ProviderType::Custom("error_test".to_string()),
        token_storage,
    );

    // Our robust implementation validates URLs during creation
    // Invalid URLs should cause provider creation to fail
    assert!(
        provider.is_err(),
        "Provider creation should fail with invalid URLs"
    );

    let error = provider.unwrap_err();
    match error {
        McpError::InvalidInput(msg) => {
            assert!(msg.contains("Invalid") && (msg.contains("URL") || msg.contains("URI")));
        }
        _ => panic!("Unexpected error type for invalid URLs: {:?}", error),
    }
}

#[tokio::test]
async fn test_oauth_provider_names() {
    let providers = [
        (ProviderType::Google, "google_provider"),
        (ProviderType::GitHub, "github_provider"),
        (ProviderType::Microsoft, "microsoft_provider"),
        (
            ProviderType::Custom("test_names".to_string()),
            "test_names_provider",
        ),
    ];

    for (provider_type, expected_name) in providers {
        let provider = create_test_oauth_provider(provider_type).unwrap();
        assert_eq!(provider.name(), expected_name);
    }
}

#[tokio::test]
async fn test_concurrent_oauth_operations() {
    use tokio::task;

    let provider = Arc::new(create_test_oauth_provider(ProviderType::GitHub).unwrap());
    let mut handles = vec![];

    // Spawn multiple concurrent authorization requests
    for i in 0..10 {
        let provider_clone = Arc::clone(&provider);
        let handle = task::spawn(async move {
            let result = provider_clone.start_authorization().await;
            (i, result)
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    let mut states = vec![];
    for handle in handles {
        let (i, result) = handle.await.unwrap();
        assert!(result.is_ok(), "Authorization {} should succeed", i);
        states.push(result.unwrap().state);
    }

    // Verify all states are unique (important for security)
    states.sort();
    states.dedup();
    assert_eq!(
        states.len(),
        10,
        "All authorization states should be unique"
    );
}

#[tokio::test]
async fn test_resource_indicators_rfc_8707_compliance() {
    // Test RFC 8707 Resource Indicators implementation
    let config = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: "test_client_secret".to_string(),
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["read".to_string(), "profile".to_string()],
        additional_params: HashMap::new(),
        flow_type: OAuth2FlowType::AuthorizationCode,
        security_level: SecurityLevel::Standard,
        mcp_resource_uri: Some("https://mcp.example.com".to_string()),
        auto_resource_indicators: true,
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());
    let provider = OAuth2Provider::new("test_provider".to_string(), config.clone(), ProviderType::Custom("test".to_string()), token_storage).unwrap();

    // Test authorization with Resource Indicators
    let auth_result = provider.start_authorization().await.unwrap();

    // Verify resource parameter is included in authorization URL
    assert!(auth_result.auth_url.contains("resource=https%3A%2F%2Fmcp.example.com"));
    assert!(auth_result.auth_url.contains("code_challenge_method=S256")); // OAuth 2.1 PKCE
    assert!(auth_result.auth_url.contains("code_challenge="));

    // Test explicit resource URI authorization
    let auth_result_explicit = provider
        .start_authorization_with_resource("https://mcp.other.com")
        .await
        .unwrap();

    assert!(auth_result_explicit.auth_url.contains("resource=https%3A%2F%2Fmcp.other.com"));
}

#[tokio::test]
async fn test_resource_uri_validation() {
    let config = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: "test_client_secret".to_string(),
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["read".to_string()],
        additional_params: HashMap::new(),
        flow_type: OAuth2FlowType::AuthorizationCode,
        security_level: SecurityLevel::Standard,
        mcp_resource_uri: None,
        auto_resource_indicators: false,
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());
    let provider = OAuth2Provider::new("test_provider".to_string(), config, ProviderType::Custom("test".to_string()), token_storage).unwrap();

    // Test valid canonical URIs
    let valid_uris = [
        "https://mcp.example.com",
        "https://mcp.example.com:8443",
        "https://mcp.example.com/mcp",
        "http://localhost:8080/mcp",
    ];

    for uri in valid_uris {
        let result = provider.start_authorization_with_resource(uri).await;
        assert!(result.is_ok(), "Valid URI should be accepted: {}", uri);
    }

    // Test invalid URIs
    let invalid_uris = [
        "mcp.example.com", // Missing scheme
        "https://mcp.example.com#fragment", // Contains fragment
        "ftp://mcp.example.com", // Invalid scheme
        "https://", // Missing host
        "https://MCP.EXAMPLE.COM", // Non-canonical case
    ];

    for uri in invalid_uris {
        let result = provider.start_authorization_with_resource(uri).await;
        assert!(result.is_err(), "Invalid URI should be rejected: {}", uri);
    }
}

#[tokio::test]
async fn test_auto_resource_indicators_configuration() {
    // Test auto_resource_indicators enabled with no URI configured (should error)
    let config_auto_no_uri = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: "test_client_secret".to_string(),
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["read".to_string()],
        additional_params: HashMap::new(),
        flow_type: OAuth2FlowType::AuthorizationCode,
        security_level: SecurityLevel::Standard,
        mcp_resource_uri: None,
        auto_resource_indicators: true, // Enabled but no URI
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());
    let provider = OAuth2Provider::new("test_provider".to_string(), config_auto_no_uri, ProviderType::Custom("test".to_string()), token_storage).unwrap();

    // Should error when trying to start authorization
    let result = provider.start_authorization().await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("MCP Resource Indicators enabled but no resource URI configured"));

    // Test auto_resource_indicators disabled (should work without URI)
    let config_auto_disabled = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: "test_client_secret".to_string(),
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["read".to_string()],
        additional_params: HashMap::new(),
        flow_type: OAuth2FlowType::AuthorizationCode,
        security_level: SecurityLevel::Standard,
        mcp_resource_uri: None,
        auto_resource_indicators: false, // Disabled
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage2: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());
    let provider2 = OAuth2Provider::new("test_provider2".to_string(), config_auto_disabled, ProviderType::Custom("test".to_string()), token_storage2).unwrap();

    // Should work when auto_resource_indicators is disabled
    let result = provider2.start_authorization().await;
    assert!(result.is_ok());

    // Verify no resource parameter in legacy flow
    let auth_result = result.unwrap();
    assert!(!auth_result.auth_url.contains("resource="));
}

#[tokio::test]
async fn test_mcp_oauth_2_1_compliance() {
    // Test complete MCP OAuth 2.1 compliance
    let config = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: "test_client_secret".to_string(),
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["read".to_string(), "write".to_string()],
        additional_params: HashMap::new(),
        flow_type: OAuth2FlowType::AuthorizationCode,
        security_level: SecurityLevel::Standard,
        mcp_resource_uri: Some("https://mcp.example.com".to_string()),
        auto_resource_indicators: true,
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());
    let provider = OAuth2Provider::new("mcp_compliant".to_string(), config, ProviderType::Custom("mcp".to_string()), token_storage).unwrap();

    // Test MCP-compliant authorization
    let auth_result = provider.start_authorization().await.unwrap();

    // Verify OAuth 2.1 compliance
    assert!(auth_result.auth_url.contains("code_challenge_method=S256")); // PKCE mandatory
    assert!(auth_result.auth_url.contains("code_challenge="));
    assert!(!auth_result.auth_url.contains("response_type=token")); // No implicit flow

    // Verify Resource Indicators compliance (RFC 8707)
    assert!(auth_result.auth_url.contains("resource=https%3A%2F%2Fmcp.example.com"));

    // Verify state parameter for CSRF protection
    assert!(!auth_result.state.is_empty());
    assert!(auth_result.auth_url.contains(&format!("state={}", auth_result.state)));
}

#[tokio::test]
async fn test_protected_resource_metadata_rfc_9728() {
    use turbomcp::auth::{McpResourceRegistry, ProtectedResourceMetadata, BearerTokenMethod};

    // Create MCP resource registry
    let registry = Arc::new(McpResourceRegistry::new(
        "https://mcp.example.com".to_string(),
        "https://auth.example.com".to_string(),
    ));

    // Register MCP resources
    registry.register_resource(
        "tools",
        vec!["mcp:tools:read".to_string(), "mcp:tools:execute".to_string()],
        Some("MCP Tool execution and discovery".to_string()),
    ).await.unwrap();

    registry.register_resource(
        "resources",
        vec!["mcp:resources:read".to_string()],
        None,
    ).await.unwrap();

    // Test resource metadata generation
    let metadata = registry.generate_well_known_metadata().await;

    assert_eq!(metadata.len(), 2);

    // Verify tools resource metadata
    let tools_uri = "https://mcp.example.com/tools";
    assert!(metadata.contains_key(tools_uri));

    let tools_metadata = &metadata[tools_uri];
    assert_eq!(tools_metadata.resource, tools_uri);
    assert_eq!(tools_metadata.authorization_server, "https://auth.example.com");
    assert_eq!(tools_metadata.scopes_supported, Some(vec![
        "mcp:tools:read".to_string(),
        "mcp:tools:execute".to_string()
    ]));
    assert_eq!(tools_metadata.bearer_methods_supported, Some(vec![
        BearerTokenMethod::Header,
        BearerTokenMethod::Body
    ]));
    assert_eq!(tools_metadata.resource_documentation, Some("MCP Tool execution and discovery".to_string()));

    // Test scope validation
    let valid_scopes = vec!["mcp:tools:read".to_string(), "other:scope".to_string()];
    let invalid_scopes = vec!["other:scope".to_string()];

    assert!(registry.validate_scope_for_resource(tools_uri, &valid_scopes).await.unwrap());
    assert!(!registry.validate_scope_for_resource(tools_uri, &invalid_scopes).await.unwrap());
}

#[tokio::test]
async fn test_oauth_provider_with_resource_registry_rfc_9728() {
    use turbomcp::auth::{McpResourceRegistry, OAuth2Provider, ProviderType, SecurityLevel};

    let config = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: "test_client_secret".to_string(),
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["read".to_string()],
        additional_params: HashMap::new(),
        flow_type: OAuth2FlowType::AuthorizationCode,
        security_level: SecurityLevel::Standard,
        mcp_resource_uri: Some("https://mcp.example.com".to_string()),
        auto_resource_indicators: true,
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());

    // Create resource registry
    let registry = OAuth2Provider::create_default_mcp_registry(
        "https://mcp.example.com",
        "https://auth.example.com"
    );

    // Create provider with resource registry
    let provider = OAuth2Provider::new("mcp_server".to_string(), config, ProviderType::Custom("mcp".to_string()), token_storage)
        .unwrap()
        .with_resource_registry(registry);

    // Register standard MCP resources
    provider.register_standard_mcp_resources().await.unwrap();

    // Test resource metadata generation
    let metadata = provider.generate_resource_metadata().await.unwrap();

    assert_eq!(metadata.len(), 3); // tools, resources, prompts
    assert!(metadata.contains_key("https://mcp.example.com/tools"));
    assert!(metadata.contains_key("https://mcp.example.com/resources"));
    assert!(metadata.contains_key("https://mcp.example.com/prompts"));

    // Test resource access validation
    let tool_scopes = vec!["mcp:tools:read".to_string()];
    let invalid_scopes = vec!["invalid:scope".to_string()];

    assert!(provider.validate_resource_access("https://mcp.example.com/tools", &tool_scopes).await.unwrap());
    assert!(!provider.validate_resource_access("https://mcp.example.com/tools", &invalid_scopes).await.unwrap());
}

#[tokio::test]
async fn test_rfc_9728_json_serialization() {
    use turbomcp::auth::{ProtectedResourceMetadata, BearerTokenMethod};

    let metadata = ProtectedResourceMetadata {
        resource: "https://mcp.example.com/tools".to_string(),
        authorization_server: "https://auth.example.com".to_string(),
        scopes_supported: Some(vec!["mcp:tools:read".to_string(), "mcp:tools:execute".to_string()]),
        bearer_methods_supported: Some(vec![BearerTokenMethod::Header, BearerTokenMethod::Body]),
        resource_documentation: Some("Tool execution endpoint".to_string()),
        additional_metadata: {
            let mut meta = HashMap::new();
            meta.insert("version".to_string(), serde_json::Value::String("1.0".to_string()));
            meta
        },
    };

    // Test JSON serialization
    let json = serde_json::to_string_pretty(&metadata).unwrap();

    // Verify required fields are present
    assert!(json.contains("\"resource\"") && json.contains("https://mcp.example.com/tools"));
    assert!(json.contains("\"authorization_server\"") && json.contains("https://auth.example.com"));
    assert!(json.contains("\"scopes_supported\""));
    assert!(json.contains("\"mcp:tools:read\""));
    assert!(json.contains("\"bearer_methods_supported\""));
    assert!(json.contains("\"header\""));
    assert!(json.contains("\"body\""));
    assert!(json.contains("\"resource_documentation\""));
    assert!(json.contains("\"version\": \"1.0\""));

    // Test JSON deserialization
    let deserialized: ProtectedResourceMetadata = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.resource, metadata.resource);
    assert_eq!(deserialized.authorization_server, metadata.authorization_server);
    assert_eq!(deserialized.scopes_supported, metadata.scopes_supported);
}

#[tokio::test]
async fn test_rfc_9728_well_known_endpoint_format() {
    use turbomcp::auth::{McpResourceRegistry};

    let registry = Arc::new(McpResourceRegistry::new(
        "https://mcp.example.com".to_string(),
        "https://auth.example.com".to_string(),
    ));

    // Register multiple resources
    registry.register_resource("tools", vec!["mcp:tools:read".to_string()], None).await.unwrap();
    registry.register_resource("resources", vec!["mcp:resources:read".to_string()], None).await.unwrap();
    registry.register_resource("prompts", vec!["mcp:prompts:read".to_string()], None).await.unwrap();

    // Generate well-known metadata
    let well_known_metadata = registry.generate_well_known_metadata().await;

    // Verify well-known endpoint format (can be served at /.well-known/resource-metadata)
    let json = serde_json::to_string_pretty(&well_known_metadata).unwrap();

    // Should be a JSON object with resource URIs as keys
    assert!(json.starts_with('{'));
    assert!(json.contains("https://mcp.example.com/tools"));
    assert!(json.contains("https://mcp.example.com/resources"));
    assert!(json.contains("https://mcp.example.com/prompts"));

    // Each resource should have proper metadata structure
    assert!(json.contains("\"authorization_server\""));
    assert!(json.contains("\"scopes_supported\""));

    println!("RFC 9728 Well-Known Metadata:\n{}", json);
}

#[tokio::test]
async fn test_dynamic_client_registration_rfc_7591() {
    use turbomcp::auth::{DynamicClientRegistration, ClientRegistrationRequest, ApplicationType};

    // Mock registration endpoint (in real test, this would be a test server)
    let registration_endpoint = "https://auth.example.com/oauth/register".to_string();

    // Create dynamic registration manager
    let registration = DynamicClientRegistration::new(registration_endpoint);

    // Create MCP client registration request
    let request = DynamicClientRegistration::create_mcp_client_request(
        "Test MCP Client",
        vec!["http://localhost:8080/callback".to_string()],
        "https://mcp.example.com",
    );

    // Verify request structure
    assert_eq!(request.client_name, Some("MCP Client: Test MCP Client".to_string()));
    assert_eq!(request.application_type, Some(ApplicationType::Web));
    assert_eq!(request.grant_types, Some(vec!["authorization_code".to_string()]));
    assert_eq!(request.response_types, Some(vec!["code".to_string()]));
    assert_eq!(request.scope, Some("mcp:tools:read mcp:tools:execute mcp:resources:read mcp:prompts:read".to_string()));
    assert_eq!(request.software_id, Some("turbomcp".to_string()));
    assert!(request.software_version.is_some());
    assert_eq!(request.client_uri, Some("https://mcp.example.com".to_string()));

    // Note: Actual registration test would require a mock HTTP server
    // This test validates the request creation logic
}

#[tokio::test]
async fn test_rfc_7591_json_serialization() {
    use turbomcp::auth::{ClientRegistrationRequest, ClientRegistrationResponse, ApplicationType, ClientRegistrationError, ClientRegistrationErrorCode};

    // Test ClientRegistrationRequest serialization
    let request = ClientRegistrationRequest {
        redirect_uris: Some(vec!["https://client.example.com/callback".to_string()]),
        response_types: Some(vec!["code".to_string()]),
        grant_types: Some(vec!["authorization_code".to_string()]),
        application_type: Some(ApplicationType::Web),
        client_name: Some("Test Client".to_string()),
        client_uri: Some("https://client.example.com".to_string()),
        scope: Some("read write".to_string()),
        software_id: Some("test-client".to_string()),
        software_version: Some("1.0.0".to_string()),
        logo_uri: None,
        contacts: None,
        tos_uri: None,
        policy_uri: None,
    };

    let request_json = serde_json::to_string_pretty(&request).unwrap();
    assert!(request_json.contains("\"redirect_uris\""));
    assert!(request_json.contains("\"application_type\"") && request_json.contains("\"web\""));
    assert!(request_json.contains("\"grant_types\""));
    assert!(request_json.contains("\"authorization_code\""));

    // Test ClientRegistrationResponse serialization
    let response = ClientRegistrationResponse {
        client_id: "client123".to_string(),
        client_secret: Some("secret456".to_string()),
        registration_access_token: Some("token789".to_string()),
        registration_client_uri: Some("https://auth.example.com/clients/client123".to_string()),
        client_id_issued_at: Some(1640995200),
        client_secret_expires_at: Some(1672531200),
        redirect_uris: Some(vec!["https://client.example.com/callback".to_string()]),
        response_types: Some(vec!["code".to_string()]),
        grant_types: Some(vec!["authorization_code".to_string()]),
        application_type: Some(ApplicationType::Web),
        client_name: Some("Test Client".to_string()),
        scope: Some("read write".to_string()),
    };

    let response_json = serde_json::to_string_pretty(&response).unwrap();
    assert!(response_json.contains("\"client_id\"") && response_json.contains("\"client123\""));
    assert!(response_json.contains("\"client_secret\"") && response_json.contains("\"secret456\""));
    assert!(response_json.contains("\"registration_access_token\""));

    // Test error response serialization
    let error = ClientRegistrationError {
        error: ClientRegistrationErrorCode::InvalidRedirectUri,
        error_description: Some("Invalid redirect URI format".to_string()),
    };

    let error_json = serde_json::to_string_pretty(&error).unwrap();
    assert!(error_json.contains("\"error\"") && error_json.contains("\"invalid_redirect_uri\""));
    assert!(error_json.contains("\"error_description\""));
}

#[tokio::test]
async fn test_oauth_provider_dynamic_registration_integration() {
    use turbomcp::auth::{DynamicClientRegistration, OAuth2Provider, SecurityLevel};

    let config = OAuth2Config {
        client_id: "test_client_id".to_string(),
        client_secret: "test_client_secret".to_string(),
        auth_url: "https://example.com/oauth/authorize".to_string(),
        token_url: "https://example.com/oauth/token".to_string(),
        redirect_uri: "http://localhost:8080/callback".to_string(),
        scopes: vec!["read".to_string()],
        additional_params: HashMap::new(),
        flow_type: OAuth2FlowType::AuthorizationCode,
        security_level: SecurityLevel::Standard,
        mcp_resource_uri: Some("https://mcp.example.com".to_string()),
        auto_resource_indicators: true,
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());

    // Create registration manager
    let registration = Arc::new(DynamicClientRegistration::new(
        "https://auth.example.com/oauth/register".to_string()
    ));

    // Create provider with dynamic registration
    let provider = OAuth2Provider::new("test_provider".to_string(), config, ProviderType::Generic, token_storage)
        .unwrap()
        .with_dynamic_registration(registration.clone());

    // Verify dynamic registration is configured
    assert!(provider.dynamic_registration().is_some());

    // Verify the registration manager is the same instance
    let stored_registration = provider.dynamic_registration().unwrap();
    assert!(Arc::ptr_eq(stored_registration, &registration));
}

#[tokio::test]
async fn test_mcp_compliance_full_integration() {
    use turbomcp::auth::{OAuth2Provider, McpResourceRegistry, DynamicClientRegistration, SecurityLevel};

    // This test demonstrates the complete MCP OAuth 2.1 compliance implementation
    let config = OAuth2Config {
        client_id: "mcp_client".to_string(),
        client_secret: "mcp_secret".to_string(),
        auth_url: "https://auth.example.com/oauth/authorize".to_string(),
        token_url: "https://auth.example.com/oauth/token".to_string(),
        redirect_uri: "https://mcp.example.com/oauth/callback".to_string(),
        scopes: vec!["mcp:tools:read".to_string(), "mcp:tools:execute".to_string()],
        additional_params: HashMap::new(),
        flow_type: OAuth2FlowType::AuthorizationCode,
        security_level: SecurityLevel::Standard,
        mcp_resource_uri: Some("https://mcp.example.com".to_string()),
        auto_resource_indicators: true,
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());

    // Create resource registry (RFC 9728)
    let resource_registry = Arc::new(McpResourceRegistry::new(
        "https://mcp.example.com".to_string(),
        "https://auth.example.com".to_string(),
    ));

    // Create dynamic registration (RFC 7591)
    let dynamic_registration = Arc::new(DynamicClientRegistration::new(
        "https://auth.example.com/oauth/register".to_string()
    ));

    // Create fully compliant MCP OAuth provider
    let provider = OAuth2Provider::new("mcp_server".to_string(), config, ProviderType::Generic, token_storage)
        .unwrap()
        .with_resource_registry(resource_registry)
        .with_dynamic_registration(dynamic_registration);

    // Register standard MCP resources
    provider.register_standard_mcp_resources().await.unwrap();

    // Test RFC 8707 - Resource Indicators
    let auth_result = provider.start_authorization().await.unwrap();
    assert!(auth_result.auth_url.contains("resource=https%3A%2F%2Fmcp.example.com"));

    // Test RFC 9728 - Protected Resource Metadata
    let metadata = provider.generate_resource_metadata().await.unwrap();
    assert_eq!(metadata.len(), 3); // tools, resources, prompts

    // Test RFC 7591 - Dynamic Client Registration is available
    assert!(provider.dynamic_registration().is_some());

    // Verify full MCP compliance
    assert!(provider.resource_registry().is_some());
    assert!(provider.dynamic_registration().is_some());
    assert_eq!(provider.config().security_level, SecurityLevel::Standard);
    assert!(provider.config().auto_resource_indicators);
    assert!(provider.config().mcp_resource_uri.is_some());
}
