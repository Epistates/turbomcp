//! Comprehensive integration tests for authentication flows
//! Tests OAuth2, API keys, JWT tokens, session management, and RBAC

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::time::timeout;

use turbomcp::auth::*;
use turbomcp::{McpError, McpResult};

// Mock HTTP server for OAuth2 testing
struct MockOAuthServer {
    responses: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    delay: Option<Duration>,
    should_fail: bool,
}

impl MockOAuthServer {
    fn new() -> Self {
        Self {
            responses: Arc::new(RwLock::new(HashMap::new())),
            delay: None,
            should_fail: false,
        }
    }

    fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = Some(delay);
        self
    }

    fn with_failure(mut self) -> Self {
        self.should_fail = true;
        self
    }

    async fn set_token_response(&self, response: serde_json::Value) {
        self.responses.write().await.insert("token".to_string(), response);
    }
}

#[tokio::test]
async fn test_oauth2_authorization_code_flow_success() {
    let config = OAuth2Config {
        client_id: "test_client".to_string(),
        client_secret: "test_secret".to_string(),
        auth_url: "https://auth.example.com/oauth/authorize".to_string(),
        token_url: "https://auth.example.com/oauth/token".to_string(),
        redirect_uri: "https://app.example.com/callback".to_string(),
        scopes: vec!["read".to_string(), "write".to_string()],
        flow_type: OAuth2FlowType::AuthorizationCode,
        additional_params: HashMap::new(),
    };

    let provider = OAuth2Provider::new("test_oauth".to_string(), config);
    
    // Test authorization start
    let auth_result = provider.start_authorization().await.unwrap();
    
    assert!(auth_result.auth_url.contains("client_id=test_client"));
    assert!(auth_result.auth_url.contains("response_type=code"));
    assert!(auth_result.auth_url.contains("scope=read%20write"));
    assert!(auth_result.code_verifier.is_some());
    assert!(!auth_result.state.is_empty());
}

#[tokio::test]
async fn test_oauth2_pkce_code_generation() {
    let verifier = generate_code_verifier();
    let challenge = generate_code_challenge(&verifier);
    
    assert_eq!(verifier.len(), 128); // RFC 7636 recommendation
    assert!(verifier.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.' || c == '_' || c == '~'));
    
    assert_eq!(challenge.len(), 43); // Base64url encoded SHA256 without padding
    assert!(challenge.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
}

#[tokio::test]
async fn test_oauth2_state_validation() {
    let config = OAuth2Config {
        client_id: "test".to_string(),
        client_secret: "secret".to_string(),
        auth_url: "https://auth.example.com".to_string(),
        token_url: "https://token.example.com".to_string(),
        redirect_uri: "https://app.example.com".to_string(),
        scopes: vec![],
        flow_type: OAuth2FlowType::AuthorizationCode,
        additional_params: HashMap::new(),
    };

    let provider = OAuth2Provider::new("test".to_string(), config);
    
    // Test invalid state parameter
    let result = provider.exchange_code("test_code", "invalid_state").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), McpError::Tool(msg) if msg.contains("Invalid state parameter")));
}

#[tokio::test]
#[ignore = "TODO: OAuth2Provider does not expose pending authorization expiry via public API. \
    Requires either: (a) a time-injection hook on OAuth2Provider for deterministic testing, \
    or (b) a get_pending_authorization_count() method to verify expired entries are purged. \
    Until then this test cannot make meaningful assertions about expiration behavior."]
async fn test_oauth2_expired_authorization() {
    // This test is blocked on OAuth2Provider exposing testable expiry hooks.
    // The intended scenario:
    //   1. Start an authorization flow (generates state + PKCE verifier)
    //   2. Advance time past the authorization expiry window
    //   3. Assert that exchange_code() returns an error indicating the
    //      authorization has expired (not "invalid state")
    //
    // To implement: add a clock abstraction or `with_expiry_duration(Duration)`
    // to OAuth2Provider so tests can set a very short expiry (e.g. 1ms) and
    // verify behavior without sleeping for the real timeout.
    unimplemented!("See ignore reason above");
}

#[tokio::test]
async fn test_api_key_provider_basic_auth() {
    let provider = ApiKeyProvider::new("api_key_test".to_string());
    
    let credentials = AuthCredentials::ApiKey {
        key: "valid_api_key_12345".to_string(),
    };
    
    let result = provider.authenticate(credentials).await;
    assert!(result.is_ok());
    
    let auth_context = result.unwrap();
    assert_eq!(auth_context.provider, "api_key_test");
    assert!(!auth_context.user_id.is_empty());
}

#[tokio::test]
async fn test_api_key_validation_edge_cases() {
    let provider = ApiKeyProvider::new("api_key_test".to_string());
    
    // Test empty key
    let empty_key = AuthCredentials::ApiKey {
        key: "".to_string(),
    };
    let result = provider.authenticate(empty_key).await;
    assert!(result.is_err());
    
    // Test very long key
    let long_key = AuthCredentials::ApiKey {
        key: "a".repeat(10000),
    };
    let result = provider.authenticate(long_key).await;
    assert!(result.is_err());
    
    // Test key with invalid characters
    let invalid_key = AuthCredentials::ApiKey {
        key: "key_with_\0_null_byte".to_string(),
    };
    let result = provider.authenticate(invalid_key).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_concurrent_api_key_operations() {
    let provider = Arc::new(ApiKeyProvider::new("concurrent_test".to_string()));
    let mut handles = vec![];
    
    // Start 100 concurrent authentication attempts
    for i in 0..100 {
        let provider_clone = Arc::clone(&provider);
        let handle = tokio::spawn(async move {
            let credentials = AuthCredentials::ApiKey {
                key: format!("api_key_{}", i),
            };
            provider_clone.authenticate(credentials).await
        });
        handles.push(handle);
    }
    
    // Wait for all to complete
    let results: Vec<_> = futures::future::join_all(handles).await;
    
    // All should succeed
    for result in results {
        assert!(result.is_ok());
        assert!(result.unwrap().is_ok());
    }
}

#[tokio::test]
async fn test_auth_manager_provider_priority() {
    let mut manager = AuthManager::new();
    
    // Add providers with different priorities
    let high_priority = AuthProviderConfig {
        name: "high_priority".to_string(),
        provider_type: AuthProviderType::ApiKey,
        settings: HashMap::new(),
        enabled: true,
        priority: 1,
    };
    
    let low_priority = AuthProviderConfig {
        name: "low_priority".to_string(),
        provider_type: AuthProviderType::ApiKey,
        settings: HashMap::new(),
        enabled: true,
        priority: 10,
    };
    
    manager.add_provider(Box::new(ApiKeyProvider::new("high".to_string())), high_priority);
    manager.add_provider(Box::new(ApiKeyProvider::new("low".to_string())), low_priority);
    
    // Test that high priority provider is tried first
    let credentials = AuthCredentials::ApiKey {
        key: "test_key".to_string(),
    };
    
    let result = manager.authenticate(credentials).await;
    assert!(result.is_ok());
    
    let auth_context = result.unwrap();
    assert_eq!(auth_context.provider, "high");
}

#[tokio::test]
async fn test_auth_manager_provider_failover() {
    let mut manager = AuthManager::new();
    
    // Add a provider that always fails
    let failing_provider = FailingAuthProvider::new();
    let failing_config = AuthProviderConfig {
        name: "failing".to_string(),
        provider_type: AuthProviderType::Custom,
        settings: HashMap::new(),
        enabled: true,
        priority: 1,
    };
    
    // Add a working provider with lower priority
    let working_config = AuthProviderConfig {
        name: "working".to_string(),
        provider_type: AuthProviderType::ApiKey,
        settings: HashMap::new(),
        enabled: true,
        priority: 2,
    };
    
    manager.add_provider(Box::new(failing_provider), failing_config);
    manager.add_provider(Box::new(ApiKeyProvider::new("working".to_string())), working_config);
    
    let credentials = AuthCredentials::ApiKey {
        key: "test_key".to_string(),
    };
    
    let result = manager.authenticate(credentials).await;
    assert!(result.is_ok());
    
    let auth_context = result.unwrap();
    assert_eq!(auth_context.provider, "working");
}

#[tokio::test]
async fn test_session_management_lifecycle() {
    let manager = AuthManager::new();
    
    // Create a session
    let user_info = UserInfo {
        id: "user123".to_string(),
        username: "testuser".to_string(),
        email: Some("test@example.com".to_string()),
        display_name: Some("Test User".to_string()),
        avatar_url: None,
        metadata: HashMap::new(),
    };
    
    let session_id = manager.create_session(&user_info, vec!["user".to_string()]).await;
    assert!(!session_id.is_empty());
    
    // Validate session
    let session = manager.get_session(&session_id).await;
    assert!(session.is_some());
    
    let session = session.unwrap();
    assert_eq!(session.user.id, "user123");
    assert_eq!(session.roles, vec!["user"]);
    
    // Test session expiry
    manager.expire_session(&session_id).await;
    let expired_session = manager.get_session(&session_id).await;
    assert!(expired_session.is_none());
}

#[tokio::test]
async fn test_session_cleanup_race_conditions() {
    let manager = Arc::new(AuthManager::new());
    let mut handles = vec![];
    
    // Create multiple sessions concurrently
    for i in 0..50 {
        let manager_clone = Arc::clone(&manager);
        let handle = tokio::spawn(async move {
            let user_info = UserInfo {
                id: format!("user{}", i),
                username: format!("user{}", i),
                email: None,
                display_name: None,
                avatar_url: None,
                metadata: HashMap::new(),
            };
            
            let session_id = manager_clone.create_session(&user_info, vec!["user".to_string()]).await;
            
            // Immediately try to clean up
            tokio::time::sleep(Duration::from_millis(10)).await;
            manager_clone.cleanup_expired_sessions().await;
            
            session_id
        });
        handles.push(handle);
    }
    
    let session_ids: Vec<String> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();
    
    // Verify sessions were created and cleanup didn't interfere
    assert_eq!(session_ids.len(), 50);
    assert!(session_ids.iter().all(|id| !id.is_empty()));
}

#[tokio::test]
async fn test_rbac_permission_checking() {
    let context = AuthContext {
        user_id: "user123".to_string(),
        user: UserInfo {
            id: "user123".to_string(),
            username: "testuser".to_string(),
            email: None,
            display_name: None,
            avatar_url: None,
            metadata: HashMap::new(),
        },
        roles: vec!["editor".to_string(), "viewer".to_string()],
        permissions: vec!["read".to_string(), "write".to_string()],
        session_id: "session123".to_string(),
        token: None,
        provider: "test".to_string(),
        authenticated_at: SystemTime::now(),
        expires_at: None,
        metadata: HashMap::new(),
    };
    
    // Test direct permission check
    assert!(check_permission(&context, "read"));
    assert!(check_permission(&context, "write"));
    assert!(!check_permission(&context, "admin"));
    
    // Test role-based permission check
    assert!(check_role(&context, "editor"));
    assert!(check_role(&context, "viewer"));
    assert!(!check_role(&context, "admin"));
}

#[tokio::test]
async fn test_rbac_permission_inheritance() {
    let mut inheritance_rules = HashMap::new();
    inheritance_rules.insert("admin".to_string(), vec!["editor".to_string(), "viewer".to_string()]);
    inheritance_rules.insert("editor".to_string(), vec!["viewer".to_string()]);
    
    let config = AuthorizationConfig {
        rbac_enabled: true,
        default_roles: vec!["viewer".to_string()],
        inheritance_rules,
        resource_permissions: HashMap::new(),
    };
    
    let context = AuthContext {
        user_id: "user123".to_string(),
        user: UserInfo {
            id: "user123".to_string(),
            username: "testuser".to_string(),
            email: None,
            display_name: None,
            avatar_url: None,
            metadata: HashMap::new(),
        },
        roles: vec!["admin".to_string()],
        permissions: vec![],
        session_id: "session123".to_string(),
        token: None,
        provider: "test".to_string(),
        authenticated_at: SystemTime::now(),
        expires_at: None,
        metadata: HashMap::new(),
    };
    
    // Admin should have editor and viewer permissions through inheritance
    assert!(check_inherited_permission(&context, "admin", &config));
    assert!(check_inherited_permission(&context, "editor", &config));
    assert!(check_inherited_permission(&context, "viewer", &config));
}

#[tokio::test]
async fn test_authentication_timeout_scenarios() {
    let provider = SlowAuthProvider::new(Duration::from_secs(5));
    
    let credentials = AuthCredentials::ApiKey {
        key: "test_key".to_string(),
    };
    
    // Test timeout during authentication
    let result = timeout(Duration::from_millis(100), provider.authenticate(credentials)).await;
    assert!(result.is_err()); // Should timeout
}

#[tokio::test]
async fn test_token_refresh_scenarios() {
    let provider = OAuth2Provider::new("test".to_string(), OAuth2Config {
        client_id: "test".to_string(),
        client_secret: "secret".to_string(),
        auth_url: "https://auth.example.com".to_string(),
        token_url: "https://token.example.com".to_string(),
        redirect_uri: "https://app.example.com".to_string(),
        scopes: vec![],
        flow_type: OAuth2FlowType::AuthorizationCode,
        additional_params: HashMap::new(),
    });
    
    // Test refresh with invalid token
    let result = provider.refresh_token("invalid_refresh_token").await;
    assert!(result.is_err());
    
    // Test refresh with expired token
    let result = provider.refresh_token("expired_refresh_token").await;
    assert!(result.is_err());
}

// Helper implementations for testing

#[derive(Debug)]
struct FailingAuthProvider {
    name: String,
}

impl FailingAuthProvider {
    fn new() -> Self {
        Self {
            name: "failing_provider".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl AuthProvider for FailingAuthProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> AuthProviderType {
        AuthProviderType::Custom
    }

    async fn authenticate(&self, _credentials: AuthCredentials) -> McpResult<AuthContext> {
        Err(McpError::Tool("Provider always fails".to_string()))
    }

    async fn validate_token(&self, _token: &str) -> McpResult<AuthContext> {
        Err(McpError::Tool("Provider always fails".to_string()))
    }

    async fn refresh_token(&self, _refresh_token: &str) -> McpResult<TokenInfo> {
        Err(McpError::Tool("Provider always fails".to_string()))
    }

    async fn revoke_token(&self, _token: &str) -> McpResult<()> {
        Err(McpError::Tool("Provider always fails".to_string()))
    }

    async fn get_user_info(&self, _token: &str) -> McpResult<UserInfo> {
        Err(McpError::Tool("Provider always fails".to_string()))
    }
}

#[derive(Debug)]
struct SlowAuthProvider {
    name: String,
    delay: Duration,
}

impl SlowAuthProvider {
    fn new(delay: Duration) -> Self {
        Self {
            name: "slow_provider".to_string(),
            delay,
        }
    }
}

#[async_trait::async_trait]
impl AuthProvider for SlowAuthProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> AuthProviderType {
        AuthProviderType::Custom
    }

    async fn authenticate(&self, _credentials: AuthCredentials) -> McpResult<AuthContext> {
        tokio::time::sleep(self.delay).await;
        
        Ok(AuthContext {
            user_id: "slow_user".to_string(),
            user: UserInfo {
                id: "slow_user".to_string(),
                username: "slow_user".to_string(),
                email: None,
                display_name: None,
                avatar_url: None,
                metadata: HashMap::new(),
            },
            roles: vec!["user".to_string()],
            permissions: vec!["read".to_string()],
            session_id: "slow_session".to_string(),
            token: None,
            provider: self.name.clone(),
            authenticated_at: SystemTime::now(),
            expires_at: None,
            metadata: HashMap::new(),
        })
    }

    async fn validate_token(&self, _token: &str) -> McpResult<AuthContext> {
        tokio::time::sleep(self.delay).await;
        Err(McpError::Tool("Slow validation".to_string()))
    }

    async fn refresh_token(&self, _refresh_token: &str) -> McpResult<TokenInfo> {
        tokio::time::sleep(self.delay).await;
        Err(McpError::Tool("Slow refresh".to_string()))
    }

    async fn revoke_token(&self, _token: &str) -> McpResult<()> {
        tokio::time::sleep(self.delay).await;
        Ok(())
    }

    async fn get_user_info(&self, _token: &str) -> McpResult<UserInfo> {
        tokio::time::sleep(self.delay).await;
        Err(McpError::Tool("Slow user info".to_string()))
    }
}

// Helper functions that would need to be implemented in the auth module
fn check_permission(context: &AuthContext, permission: &str) -> bool {
    context.permissions.contains(&permission.to_string())
}

fn check_role(context: &AuthContext, role: &str) -> bool {
    context.roles.contains(&role.to_string())
}

fn check_inherited_permission(context: &AuthContext, role: &str, config: &AuthorizationConfig) -> bool {
    if context.roles.contains(&role.to_string()) {
        return true;
    }
    
    // Check inheritance
    for user_role in &context.roles {
        if let Some(inherited) = config.inheritance_rules.get(user_role) {
            if inherited.contains(&role.to_string()) {
                return true;
            }
        }
    }
    
    false
}

fn generate_code_verifier() -> String {
    use rand::Rng;
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    let mut rng = rand::thread_rng();
    (0..128)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

fn generate_code_challenge(verifier: &str) -> String {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    base64::encode_config(hash, base64::URL_SAFE_NO_PAD)
}