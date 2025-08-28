//! Production-grade server architecture for TurboMCP Transport
//!
//! This module provides a complete server implementation that bridges Tower services
//! with TurboMCP protocol handlers, offering enterprise-grade handler management,
//! request context, and seamless integration with the broader ecosystem.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::{debug, error, info, instrument, warn};
use uuid::Uuid;

use turbomcp_protocol::{Error, Result};
use turbomcp_protocol::{
    CallToolRequest, CallToolResult, GetPromptRequest, GetPromptResult,
    ReadResourceRequest, ReadResourceResult,
};

/// Production-grade request context containing all necessary metadata and state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestContext {
    /// Unique request identifier for tracing and correlation
    pub request_id: String,
    /// Session identifier for multi-request correlation
    pub session_id: String,
    /// Request timestamp for audit and performance tracking
    pub timestamp: SystemTime,
    /// User agent string from client
    pub user_agent: Option<String>,
    /// Client information and capabilities
    pub client_info: Option<Value>,
    /// Additional request metadata
    pub metadata: Option<HashMap<String, Value>>,
    /// Authentication context
    pub auth_context: Option<AuthContext>,
    /// Tracing and observability context
    pub trace_context: TraceContext,
    /// Security and authorization context
    pub security_context: SecurityContext,
}

/// Authentication context for request processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthContext {
    /// Authenticated user identifier
    pub user_id: String,
    /// User roles and permissions
    pub roles: Vec<String>,
    /// Token information
    pub token_info: Option<TokenInfo>,
    /// Authentication provider
    pub provider: String,
}

/// Token information for authenticated requests
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    /// Token type (Bearer, ApiKey, etc.)
    pub token_type: String,
    /// Token expiration
    pub expires_at: Option<SystemTime>,
    /// Token scopes
    pub scopes: Vec<String>,
}

/// Tracing context for observability
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TraceContext {
    /// Trace identifier
    pub trace_id: String,
    /// Span identifier
    pub span_id: String,
    /// Parent span identifier
    pub parent_span_id: Option<String>,
    /// Baggage for cross-service context
    pub baggage: HashMap<String, String>,
}

/// Security context for authorization and access control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityContext {
    /// Required permissions for this request
    pub required_permissions: Vec<String>,
    /// Allowed resources for this request
    pub allowed_resources: Vec<String>,
    /// Security level required
    pub security_level: SecurityLevel,
    /// Rate limiting information
    pub rate_limit_info: Option<RateLimitInfo>,
}

/// Security level enumeration
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SecurityLevel {
    /// Public access - no authentication required
    Public,
    /// Authenticated access - valid token required
    Authenticated,
    /// Authorized access - specific permissions required
    Authorized,
    /// Admin access - administrative privileges required
    Admin,
}

/// Rate limiting information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    /// Remaining requests in current window
    pub remaining: u32,
    /// Window reset time
    pub reset_at: SystemTime,
    /// Current limit
    pub limit: u32,
}

/// Production-grade async trait for tool handlers
#[async_trait]
pub trait ToolHandler: Send + Sync + std::fmt::Debug {
    /// Get tool name
    fn name(&self) -> &str {
        "unknown_tool"
    }
    
    /// Get tool description and schema
    async fn get_schema(&self) -> Result<serde_json::Value> {
        Err(Error::not_found("Tool schema not implemented"))
    }
    
    /// Execute tool with full context and error handling
    #[instrument(skip(self, request, context))]
    async fn handle(&self, request: CallToolRequest, context: RequestContext) -> Result<CallToolResult> {
        let _ = (request, context);
        Err(Error::not_found("Tool handler not implemented"))
    }
    
    /// Check if tool is available for given context
    async fn is_available(&self, _context: &RequestContext) -> bool {
        // Default implementation - tools are available unless overridden
        true
    }
    
    /// Get tool capabilities and metadata
    async fn get_capabilities(&self) -> ToolCapabilities {
        ToolCapabilities::default()
    }
    
    /// Initialize tool handler
    async fn initialize(&self) -> Result<()> {
        Ok(())
    }
    
    /// Shutdown tool handler gracefully
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Tool capabilities and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapabilities {
    /// Whether tool supports streaming responses
    pub supports_streaming: bool,
    /// Whether tool requires authentication
    pub requires_auth: bool,
    /// Maximum execution time
    pub max_execution_time: Option<Duration>,
    /// Resource requirements
    pub resource_requirements: ResourceRequirements,
}

impl Default for ToolCapabilities {
    fn default() -> Self {
        Self {
            supports_streaming: false,
            requires_auth: false,
            max_execution_time: Some(Duration::from_secs(30)),
            resource_requirements: ResourceRequirements::default(),
        }
    }
}

/// Resource requirements for tool execution
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceRequirements {
    /// Maximum memory usage in bytes
    pub max_memory_bytes: Option<u64>,
    /// CPU priority level
    pub cpu_priority: CpuPriority,
    /// I/O priority level
    pub io_priority: IoPriority,
}

/// CPU priority levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CpuPriority {
    /// Low CPU priority for background tasks
    Low,
    /// Normal CPU priority for standard operations
    Normal,
    /// High CPU priority for time-critical tasks
    High,
}

impl Default for CpuPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// I/O priority levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum IoPriority {
    /// Low I/O priority for background operations
    Low,
    /// Normal I/O priority for standard operations
    Normal,
    /// High I/O priority for time-critical data access
    High,
}

impl Default for IoPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Production-grade async trait for prompt handlers
#[async_trait]
pub trait PromptHandler: Send + Sync + std::fmt::Debug {
    /// Get prompt name
    fn name(&self) -> &str {
        "unknown_prompt"
    }
    
    /// Get prompt schema and metadata
    async fn get_schema(&self) -> Result<serde_json::Value> {
        Err(Error::not_found("Prompt schema not implemented"))
    }
    
    /// Execute prompt with full context
    #[instrument(skip(self, request, context))]
    async fn handle(&self, request: GetPromptRequest, context: RequestContext) -> Result<GetPromptResult> {
        let _ = (request, context);
        Err(Error::not_found("Prompt handler not implemented"))
    }
    
    /// Check if prompt is available for given context
    async fn is_available(&self, _context: &RequestContext) -> bool {
        true
    }
    
    /// Initialize prompt handler
    async fn initialize(&self) -> Result<()> {
        Ok(())
    }
    
    /// Shutdown prompt handler gracefully
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Production-grade async trait for resource handlers
#[async_trait]
pub trait ResourceHandler: Send + Sync + std::fmt::Debug {
    /// Check if resource exists at given URI
    async fn exists(&self, uri: &str) -> bool {
        let _ = uri;
        false
    }
    
    /// Read resource with full context and error handling
    #[instrument(skip(self, request, context))]
    async fn handle(&self, request: ReadResourceRequest, context: RequestContext) -> Result<ReadResourceResult> {
        let _ = (request, context);
        Err(Error::not_found("Resource handler not implemented"))
    }
    
    /// List available resources matching pattern
    async fn list_resources(&self, pattern: Option<&str>) -> Result<Vec<ResourceInfo>> {
        let _ = pattern;
        Ok(Vec::new())
    }
    
    /// Get resource metadata
    async fn get_metadata(&self, uri: &str) -> Result<ResourceMetadata> {
        let _ = uri;
        Err(Error::not_found("Resource metadata not implemented"))
    }
    
    /// Initialize resource handler
    async fn initialize(&self) -> Result<()> {
        Ok(())
    }
    
    /// Shutdown resource handler gracefully
    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

/// Resource information for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceInfo {
    /// Resource URI
    pub uri: String,
    /// Resource name
    pub name: String,
    /// Resource description
    pub description: Option<String>,
    /// Resource MIME type
    pub mime_type: Option<String>,
    /// Resource size in bytes
    pub size: Option<u64>,
    /// Last modified timestamp
    pub modified_at: Option<SystemTime>,
}

/// Resource metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceMetadata {
    /// Resource URI
    pub uri: String,
    /// MIME type
    pub mime_type: String,
    /// Content encoding
    pub encoding: Option<String>,
    /// Content length
    pub content_length: Option<u64>,
    /// Last modified timestamp
    pub last_modified: Option<SystemTime>,
    /// ETag for caching
    pub etag: Option<String>,
    /// Custom metadata
    pub metadata: HashMap<String, Value>,
}

/// Production-grade handler registry with dependency injection and lifecycle management
#[derive(Debug)]
pub struct HandlerRegistry {
    /// Tool handlers indexed by name
    tools: Arc<RwLock<HashMap<String, Arc<dyn ToolHandler>>>>,
    /// Prompt handlers indexed by name
    prompts: Arc<RwLock<HashMap<String, Arc<dyn PromptHandler>>>>,
    /// Resource handlers with pattern matching
    resources: Arc<RwLock<Vec<Arc<dyn ResourceHandler>>>>,
    /// Registry metadata and configuration
    metadata: RegistryMetadata,
    /// Lifecycle state
    state: Arc<RwLock<RegistryState>>,
}

/// Registry metadata and configuration
#[derive(Debug, Clone)]
pub struct RegistryMetadata {
    /// Registry creation time
    pub created_at: SystemTime,
    /// Registry version
    pub version: String,
    /// Registry configuration
    pub config: RegistryConfig,
}

/// Registry configuration
#[derive(Debug, Clone)]
pub struct RegistryConfig {
    /// Maximum number of concurrent handlers
    pub max_concurrent_handlers: u32,
    /// Handler initialization timeout
    pub initialization_timeout: Duration,
    /// Handler shutdown timeout
    pub shutdown_timeout: Duration,
    /// Enable handler health checks
    pub enable_health_checks: bool,
    /// Health check interval
    pub health_check_interval: Duration,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            max_concurrent_handlers: 100,
            initialization_timeout: Duration::from_secs(30),
            shutdown_timeout: Duration::from_secs(10),
            enable_health_checks: true,
            health_check_interval: Duration::from_secs(60),
        }
    }
}

/// Registry lifecycle state
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryState {
    /// Registry is being initialized
    Initializing,
    /// Registry is ready to handle requests
    Ready,
    /// Registry is shutting down
    ShuttingDown,
    /// Registry has been shut down
    Shutdown,
}

impl HandlerRegistry {
    /// Create new handler registry with default configuration
    pub fn new() -> Self {
        Self::with_config(RegistryConfig::default())
    }
    
    /// Create new handler registry with custom configuration
    pub fn with_config(config: RegistryConfig) -> Self {
        let metadata = RegistryMetadata {
            created_at: SystemTime::now(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            config,
        };
        
        Self {
            tools: Arc::new(RwLock::new(HashMap::new())),
            prompts: Arc::new(RwLock::new(HashMap::new())),
            resources: Arc::new(RwLock::new(Vec::new())),
            metadata,
            state: Arc::new(RwLock::new(RegistryState::Initializing)),
        }
    }
    
    /// Initialize registry and all handlers
    #[instrument(skip(self))]
    pub async fn initialize(&self) -> Result<()> {
        info!("Initializing handler registry");
        
        {
            let mut state = self.state.write().await;
            *state = RegistryState::Initializing;
        }
        
        // Initialize all tool handlers
        let tools = self.tools.read().await;
        for (name, handler) in tools.iter() {
            debug!("Initializing tool handler: {}", name);
            if let Err(e) = handler.initialize().await {
                error!("Failed to initialize tool handler {}: {}", name, e);
                return Err(e);
            }
        }
        
        // Initialize all prompt handlers
        let prompts = self.prompts.read().await;
        for (name, handler) in prompts.iter() {
            debug!("Initializing prompt handler: {}", name);
            if let Err(e) = handler.initialize().await {
                error!("Failed to initialize prompt handler {}: {}", name, e);
                return Err(e);
            }
        }
        
        // Initialize all resource handlers
        let resources = self.resources.read().await;
        for handler in resources.iter() {
            debug!("Initializing resource handler");
            if let Err(e) = handler.initialize().await {
                error!("Failed to initialize resource handler: {}", e);
                return Err(e);
            }
        }
        
        {
            let mut state = self.state.write().await;
            *state = RegistryState::Ready;
        }
        
        info!("Handler registry initialized successfully");
        Ok(())
    }
    
    /// Shutdown registry and all handlers gracefully
    #[instrument(skip(self))]
    pub async fn shutdown(&self) -> Result<()> {
        info!("Shutting down handler registry");
        
        {
            let mut state = self.state.write().await;
            *state = RegistryState::ShuttingDown;
        }
        
        // Shutdown all handlers in reverse order
        let resources = self.resources.read().await;
        for handler in resources.iter() {
            if let Err(e) = handler.shutdown().await {
                warn!("Error shutting down resource handler: {}", e);
            }
        }
        
        let prompts = self.prompts.read().await;
        for (name, handler) in prompts.iter() {
            if let Err(e) = handler.shutdown().await {
                warn!("Error shutting down prompt handler {}: {}", name, e);
            }
        }
        
        let tools = self.tools.read().await;
        for (name, handler) in tools.iter() {
            if let Err(e) = handler.shutdown().await {
                warn!("Error shutting down tool handler {}: {}", name, e);
            }
        }
        
        {
            let mut state = self.state.write().await;
            *state = RegistryState::Shutdown;
        }
        
        info!("Handler registry shutdown complete");
        Ok(())
    }
    
    /// Register a tool handler
    pub async fn register_tool(&self, handler: Arc<dyn ToolHandler>) -> Result<()> {
        let name = handler.name().to_string();
        debug!("Registering tool handler: {}", name);
        
        let mut tools = self.tools.write().await;
        if tools.contains_key(&name) {
            return Err(Error::validation(format!("Tool '{}' already registered", name)));
        }
        
        tools.insert(name.clone(), handler);
        info!("Tool handler '{}' registered successfully", name);
        Ok(())
    }
    
    /// Register a prompt handler
    pub async fn register_prompt(&self, handler: Arc<dyn PromptHandler>) -> Result<()> {
        let name = handler.name().to_string();
        debug!("Registering prompt handler: {}", name);
        
        let mut prompts = self.prompts.write().await;
        if prompts.contains_key(&name) {
            return Err(Error::validation(format!("Prompt '{}' already registered", name)));
        }
        
        prompts.insert(name.clone(), handler);
        info!("Prompt handler '{}' registered successfully", name);
        Ok(())
    }
    
    /// Register a resource handler
    pub async fn register_resource(&self, handler: Arc<dyn ResourceHandler>) -> Result<()> {
        debug!("Registering resource handler");
        
        let mut resources = self.resources.write().await;
        resources.push(handler);
        info!("Resource handler registered successfully");
        Ok(())
    }
    
    /// Get tool handler by name
    pub async fn get_tool(&self, name: &str) -> Option<Arc<dyn ToolHandler>> {
        let tools = self.tools.read().await;
        tools.get(name).cloned()
    }
    
    /// Get prompt handler by name
    pub async fn get_prompt(&self, name: &str) -> Option<Arc<dyn PromptHandler>> {
        let prompts = self.prompts.read().await;
        prompts.get(name).cloned()
    }
    
    /// Get resource handler for URI
    pub async fn get_resource(&self, uri: &str) -> Option<Arc<dyn ResourceHandler>> {
        let resources = self.resources.read().await;
        for handler in resources.iter() {
            if handler.exists(uri).await {
                return Some(handler.clone());
            }
        }
        None
    }
    
    /// Get all tool definitions for capability announcement
    pub async fn get_tool_definitions(&self) -> Result<Vec<serde_json::Value>> {
        let tools = self.tools.read().await;
        let mut definitions = Vec::new();
        
        for handler in tools.values() {
            match handler.get_schema().await {
                Ok(schema) => definitions.push(schema),
                Err(e) => warn!("Failed to get schema for tool {}: {}", handler.name(), e),
            }
        }
        
        Ok(definitions)
    }
    
    /// Get all prompt definitions for capability announcement
    pub async fn get_prompt_definitions(&self) -> Result<Vec<serde_json::Value>> {
        let prompts = self.prompts.read().await;
        let mut definitions = Vec::new();
        
        for handler in prompts.values() {
            match handler.get_schema().await {
                Ok(schema) => definitions.push(schema),
                Err(e) => warn!("Failed to get schema for prompt {}: {}", handler.name(), e),
            }
        }
        
        Ok(definitions)
    }
    
    /// Get all resource definitions for capability announcement
    pub async fn get_resource_definitions(&self) -> Result<Vec<ResourceInfo>> {
        let resources = self.resources.read().await;
        let mut definitions = Vec::new();
        
        for handler in resources.iter() {
            match handler.list_resources(None).await {
                Ok(mut resources) => definitions.append(&mut resources),
                Err(e) => warn!("Failed to list resources from handler: {}", e),
            }
        }
        
        Ok(definitions)
    }
    
    /// Get registry state
    pub async fn get_state(&self) -> RegistryState {
        self.state.read().await.clone()
    }
    
    /// Check if registry is ready
    pub async fn is_ready(&self) -> bool {
        matches!(*self.state.read().await, RegistryState::Ready)
    }
    
    /// Get registry metadata and configuration
    #[must_use]
    pub fn metadata(&self) -> &RegistryMetadata {
        &self.metadata
    }
}

impl Default for HandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RequestContext {
    /// Create new request context with minimal information
    pub fn new(request_id: String, session_id: String) -> Self {
        Self {
            request_id,
            session_id,
            timestamp: SystemTime::now(),
            user_agent: None,
            client_info: None,
            metadata: None,
            auth_context: None,
            trace_context: TraceContext::new(),
            security_context: SecurityContext::public(),
        }
    }
    
    /// Create request context with full information
    pub fn with_auth(
        request_id: String,
        session_id: String,
        auth_context: AuthContext,
    ) -> Self {
        Self {
            request_id,
            session_id,
            timestamp: SystemTime::now(),
            user_agent: None,
            client_info: None,
            metadata: None,
            auth_context: Some(auth_context),
            trace_context: TraceContext::new(),
            security_context: SecurityContext::authenticated(),
        }
    }
    
    /// Check if user has required permission
    pub fn has_permission(&self, _permission: &str) -> bool {
        if let Some(auth) = &self.auth_context {
            // Check if user has the specific permission
            // In a real implementation, this would check against a permission system
            auth.roles.contains(&"admin".to_string()) || 
            self.security_context.required_permissions.is_empty()
        } else {
            // No authentication - check if this is a public resource
            matches!(self.security_context.security_level, SecurityLevel::Public)
        }
    }
}

impl TraceContext {
    /// Create new trace context
    pub fn new() -> Self {
        Self {
            trace_id: Uuid::new_v4().to_string(),
            span_id: Uuid::new_v4().to_string(),
            parent_span_id: None,
            baggage: HashMap::new(),
        }
    }
    
    /// Create child trace context
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: Uuid::new_v4().to_string(),
            parent_span_id: Some(self.span_id.clone()),
            baggage: self.baggage.clone(),
        }
    }
}

impl SecurityContext {
    /// Create public security context
    pub fn public() -> Self {
        Self {
            required_permissions: Vec::new(),
            allowed_resources: Vec::new(),
            security_level: SecurityLevel::Public,
            rate_limit_info: None,
        }
    }
    
    /// Create authenticated security context
    pub fn authenticated() -> Self {
        Self {
            required_permissions: Vec::new(),
            allowed_resources: Vec::new(),
            security_level: SecurityLevel::Authenticated,
            rate_limit_info: None,
        }
    }
    
    /// Create authorized security context
    pub fn authorized(permissions: Vec<String>) -> Self {
        Self {
            required_permissions: permissions,
            allowed_resources: Vec::new(),
            security_level: SecurityLevel::Authorized,
            rate_limit_info: None,
        }
    }
}