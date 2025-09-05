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
async fn create_test_oauth_provider(
    provider_type: ProviderType,
) -> Result<OAuth2Provider, McpError> {
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

    OAuth2Provider::new(provider_name, config, provider_type, token_storage).await
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
        let provider = create_test_oauth_provider(provider_type.clone()).await;
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
    let provider = create_test_oauth_provider(ProviderType::Google)
        .await
        .unwrap();

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
    let provider = create_test_oauth_provider(ProviderType::Google)
        .await
        .unwrap();

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
    let provider = create_test_oauth_provider(ProviderType::Google)
        .await
        .unwrap();

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
    let provider = create_test_oauth_provider(ProviderType::GitHub)
        .await
        .unwrap();

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
    let provider = create_test_oauth_provider(ProviderType::Microsoft)
        .await
        .unwrap();

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
    let google_provider = create_test_oauth_provider(ProviderType::Google)
        .await
        .unwrap();
    let github_provider = create_test_oauth_provider(ProviderType::GitHub)
        .await
        .unwrap();
    let microsoft_provider = create_test_oauth_provider(ProviderType::Microsoft)
        .await
        .unwrap();

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
    let provider = create_test_oauth_provider(ProviderType::Custom("device_test".to_string()))
        .await
        .unwrap();

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
    let provider = create_test_oauth_provider(ProviderType::Custom("client_test".to_string()))
        .await
        .unwrap();

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
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());

    let provider = OAuth2Provider::new(
        "test_provider".to_string(),
        config,
        ProviderType::Custom("config_test".to_string()),
        token_storage,
    )
    .await;

    assert!(
        provider.is_ok(),
        "Provider creation with valid config should succeed"
    );
}

#[tokio::test]
async fn test_comprehensive_oauth_workflow() {
    // This test validates the complete OAuth workflow integration
    let provider = create_test_oauth_provider(ProviderType::Google)
        .await
        .unwrap();
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
        #[cfg(feature = "dpop")]
        dpop_config: None,
    };

    let token_storage: Arc<dyn TokenStorage> = Arc::new(TestTokenStorage::new());
    let provider = OAuth2Provider::new(
        "error_test".to_string(),
        invalid_config,
        ProviderType::Custom("error_test".to_string()),
        token_storage,
    )
    .await;

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
        let provider = create_test_oauth_provider(provider_type).await.unwrap();
        assert_eq!(provider.name(), expected_name);
    }
}

#[tokio::test]
async fn test_concurrent_oauth_operations() {
    use tokio::task;

    let provider = Arc::new(
        create_test_oauth_provider(ProviderType::GitHub)
            .await
            .unwrap(),
    );
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
