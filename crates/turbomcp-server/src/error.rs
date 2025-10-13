//! Server error types and handling

/// Result type for server operations
pub type ServerResult<T> = Result<T, ServerError>;

/// Comprehensive server error types
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServerError {
    /// Protocol-level error from client or protocol layer
    ///
    /// This variant preserves the original protocol error, including error codes
    /// like `-1` for user rejection. This ensures transparency when forwarding
    /// client errors (e.g., sampling/elicitation rejections) back through the
    /// server to calling clients.
    ///
    /// When converting to `turbomcp_protocol::Error`, this variant is unwrapped
    /// directly to preserve error semantics and codes.
    #[error("Protocol error: {0}")]
    Protocol(Box<turbomcp_protocol::Error>),

    /// Core errors
    #[error("Core error: {0}")]
    Core(#[from] turbomcp_protocol::registry::RegistryError),

    /// Transport layer errors
    #[error("Transport error: {0}")]
    Transport(#[from] turbomcp_transport::TransportError),

    /// Handler registration errors
    #[error("Handler error: {message}")]
    Handler {
        /// Error message
        message: String,
        /// Optional error context
        context: Option<String>,
    },

    /// Configuration errors
    #[error("Configuration error: {message}")]
    Configuration {
        /// Error message
        message: String,
        /// Configuration key that caused the error
        key: Option<String>,
    },

    /// Authentication errors
    #[error("Authentication error: {message}")]
    Authentication {
        /// Error message
        message: String,
        /// Authentication method that failed
        method: Option<String>,
    },

    /// Authorization errors
    #[error("Authorization error: {message}")]
    Authorization {
        /// Error message
        message: String,
        /// Resource being accessed
        resource: Option<String>,
    },

    /// Rate limiting errors
    #[error("Rate limit exceeded: {message}")]
    RateLimit {
        /// Error message
        message: String,
        /// Retry after seconds
        retry_after: Option<u64>,
    },

    /// Server lifecycle errors
    #[error("Lifecycle error: {0}")]
    Lifecycle(String),

    /// Server shutdown errors
    #[error("Shutdown error: {0}")]
    Shutdown(String),

    /// Middleware errors
    #[error("Middleware error: {name}: {message}")]
    Middleware {
        /// Middleware name
        name: String,
        /// Error message
        message: String,
    },

    /// Registry errors
    #[error("Registry error: {0}")]
    Registry(String),

    /// Routing errors
    #[error("Routing error: {message}")]
    Routing {
        /// Error message
        message: String,
        /// Request method that failed
        method: Option<String>,
    },

    /// Resource not found
    #[error("Resource not found: {resource}")]
    NotFound {
        /// Resource that was not found
        resource: String,
    },

    /// Internal server errors
    #[error("Internal server error: {0}")]
    Internal(String),

    /// IO errors
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization errors
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Timeout errors
    #[error("Timeout error: {operation} timed out after {timeout_ms}ms")]
    Timeout {
        /// Operation that timed out
        operation: String,
        /// Timeout in milliseconds
        timeout_ms: u64,
    },

    /// Resource exhaustion
    #[error("Resource exhausted: {resource}")]
    ResourceExhausted {
        /// Resource type
        resource: String,
        /// Current usage
        current: Option<usize>,
        /// Maximum allowed
        max: Option<usize>,
    },
}

impl ServerError {
    /// Create a new handler error
    pub fn handler(message: impl Into<String>) -> Self {
        Self::Handler {
            message: message.into(),
            context: None,
        }
    }

    /// Create a handler error with context
    pub fn handler_with_context(message: impl Into<String>, context: impl Into<String>) -> Self {
        Self::Handler {
            message: message.into(),
            context: Some(context.into()),
        }
    }

    /// Create a new configuration error
    pub fn configuration(message: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
            key: None,
        }
    }

    /// Create a configuration error with key
    pub fn configuration_with_key(message: impl Into<String>, key: impl Into<String>) -> Self {
        Self::Configuration {
            message: message.into(),
            key: Some(key.into()),
        }
    }

    /// Create a new authentication error
    pub fn authentication(message: impl Into<String>) -> Self {
        Self::Authentication {
            message: message.into(),
            method: None,
        }
    }

    /// Create an authentication error with method
    pub fn authentication_with_method(
        message: impl Into<String>,
        method: impl Into<String>,
    ) -> Self {
        Self::Authentication {
            message: message.into(),
            method: Some(method.into()),
        }
    }

    /// Create a new authorization error
    pub fn authorization(message: impl Into<String>) -> Self {
        Self::Authorization {
            message: message.into(),
            resource: None,
        }
    }

    /// Create an authorization error with resource
    pub fn authorization_with_resource(
        message: impl Into<String>,
        resource: impl Into<String>,
    ) -> Self {
        Self::Authorization {
            message: message.into(),
            resource: Some(resource.into()),
        }
    }

    /// Create a new rate limit error
    pub fn rate_limit(message: impl Into<String>) -> Self {
        Self::RateLimit {
            message: message.into(),
            retry_after: None,
        }
    }

    /// Create a rate limit error with retry after
    pub fn rate_limit_with_retry(message: impl Into<String>, retry_after: u64) -> Self {
        Self::RateLimit {
            message: message.into(),
            retry_after: Some(retry_after),
        }
    }

    /// Create a new middleware error
    pub fn middleware(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Middleware {
            name: name.into(),
            message: message.into(),
        }
    }

    /// Create a new routing error
    pub fn routing(message: impl Into<String>) -> Self {
        Self::Routing {
            message: message.into(),
            method: None,
        }
    }

    /// Create a routing error with method
    pub fn routing_with_method(message: impl Into<String>, method: impl Into<String>) -> Self {
        Self::Routing {
            message: message.into(),
            method: Some(method.into()),
        }
    }

    /// Create a not found error
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::NotFound {
            resource: resource.into(),
        }
    }

    /// Create a timeout error
    pub fn timeout(operation: impl Into<String>, timeout_ms: u64) -> Self {
        Self::Timeout {
            operation: operation.into(),
            timeout_ms,
        }
    }

    /// Create a resource exhausted error
    pub fn resource_exhausted(resource: impl Into<String>) -> Self {
        Self::ResourceExhausted {
            resource: resource.into(),
            current: None,
            max: None,
        }
    }

    /// Create a resource exhausted error with usage info
    pub fn resource_exhausted_with_usage(
        resource: impl Into<String>,
        current: usize,
        max: usize,
    ) -> Self {
        Self::ResourceExhausted {
            resource: resource.into(),
            current: Some(current),
            max: Some(max),
        }
    }

    /// Check if this error is retryable
    #[must_use]
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Timeout { .. } | Self::ResourceExhausted { .. } | Self::RateLimit { .. }
        )
    }

    /// Check if this error should cause server shutdown
    #[must_use]
    pub const fn is_fatal(&self) -> bool {
        matches!(
            self,
            Self::Lifecycle(_) | Self::Shutdown(_) | Self::Internal(_)
        )
    }

    /// Get error code for JSON-RPC responses
    #[must_use]
    pub fn error_code(&self) -> i32 {
        let code = match self {
            // Preserve protocol error codes directly
            Self::Protocol(protocol_err) => {
                let extracted_code = protocol_err.jsonrpc_error_code();
                tracing::info!(
                    "🔍 [ServerError::error_code] Protocol variant - extracted code: {}, kind: {:?}",
                    extracted_code,
                    protocol_err.kind
                );
                extracted_code
            }

            // Map server errors to JSON-RPC codes
            Self::Core(_) => -32603,
            Self::NotFound { .. } => -32004,
            Self::Authentication { .. } => -32008,
            Self::Authorization { .. } => -32005,
            Self::RateLimit { .. } => -32009,
            Self::ResourceExhausted { .. } => -32010,
            Self::Timeout { .. } => -32603,
            Self::Handler { .. } => -32002,
            Self::Transport(_) => -32603,
            Self::Configuration { .. } => -32015,
            Self::Lifecycle(_) => -32603,
            Self::Shutdown(_) => -32603,
            Self::Middleware { .. } => -32603,
            Self::Registry(_) => -32603,
            Self::Routing { .. } => -32603,
            Self::Internal(_) => -32603,
            Self::Io(_) => -32603,
            Self::Serialization(_) => -32602,
        };
        tracing::info!(
            "🔍 [ServerError::error_code] Returning code: {} for variant: {:?}",
            code,
            std::mem::discriminant(self)
        );
        code
    }
}

/// Error recovery strategies
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorRecovery {
    /// Retry the operation
    Retry,
    /// Skip and continue
    Skip,
    /// Fail immediately
    Fail,
    /// Graceful degradation
    Degrade,
}

/// Error context for detailed error reporting
#[derive(Debug, Clone)]
pub struct ErrorContext {
    /// Error category
    pub category: String,
    /// Operation being performed
    pub operation: String,
    /// Request ID if applicable
    pub request_id: Option<String>,
    /// Client ID if applicable
    pub client_id: Option<String>,
    /// Additional metadata
    pub metadata: std::collections::HashMap<String, String>,
}

impl ErrorContext {
    /// Create a new error context
    pub fn new(category: impl Into<String>, operation: impl Into<String>) -> Self {
        Self {
            category: category.into(),
            operation: operation.into(),
            request_id: None,
            client_id: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Add request ID to context
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Add client ID to context
    pub fn with_client_id(mut self, client_id: impl Into<String>) -> Self {
        self.client_id = Some(client_id.into());
        self
    }

    /// Add metadata to context
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

// Conversion from core errors to server errors
impl From<Box<turbomcp_protocol::Error>> for ServerError {
    fn from(core_error: Box<turbomcp_protocol::Error>) -> Self {
        use turbomcp_protocol::ErrorKind;

        match core_error.kind {
            // MCP-specific errors
            ErrorKind::UserRejected => Self::Handler {
                message: core_error.message,
                context: core_error.context.operation,
            },
            ErrorKind::ToolNotFound | ErrorKind::PromptNotFound | ErrorKind::ResourceNotFound => {
                Self::NotFound {
                    resource: core_error.message,
                }
            }
            ErrorKind::ToolExecutionFailed => Self::Handler {
                message: core_error.message,
                context: core_error.context.operation,
            },
            ErrorKind::ResourceAccessDenied => Self::Authorization {
                message: core_error.message,
                resource: core_error.context.component,
            },
            ErrorKind::CapabilityNotSupported => Self::Handler {
                message: format!("Capability not supported: {}", core_error.message),
                context: None,
            },
            ErrorKind::ProtocolVersionMismatch => Self::Configuration {
                message: core_error.message,
                key: Some("protocol_version".to_string()),
            },
            ErrorKind::ServerOverloaded => Self::ResourceExhausted {
                resource: "server_capacity".to_string(),
                current: None,
                max: None,
            },

            // Deprecated (backwards compatibility)
            #[allow(deprecated)]
            ErrorKind::Handler => Self::Handler {
                message: core_error.message,
                context: core_error.context.operation,
            },
            #[allow(deprecated)]
            ErrorKind::NotFound => Self::NotFound {
                resource: core_error.message,
            },

            // General errors
            ErrorKind::Authentication => Self::Authentication {
                message: core_error.message,
                method: None,
            },
            ErrorKind::PermissionDenied => Self::Authorization {
                message: core_error.message,
                resource: None,
            },
            ErrorKind::BadRequest | ErrorKind::Validation => Self::Handler {
                message: format!("Validation error: {}", core_error.message),
                context: None,
            },
            ErrorKind::Timeout => Self::Timeout {
                operation: core_error
                    .context
                    .operation
                    .unwrap_or_else(|| "unknown".to_string()),
                timeout_ms: 30000, // Default timeout
            },
            ErrorKind::RateLimited => Self::RateLimit {
                message: core_error.message,
                retry_after: None,
            },
            ErrorKind::Configuration => Self::Configuration {
                message: core_error.message,
                key: None,
            },
            ErrorKind::Transport => {
                Self::Internal(format!("Transport error: {}", core_error.message))
            }
            ErrorKind::Serialization => {
                Self::Internal(format!("Serialization error: {}", core_error.message))
            }
            ErrorKind::Protocol => {
                Self::Internal(format!("Protocol error: {}", core_error.message))
            }
            ErrorKind::Unavailable => Self::ResourceExhausted {
                resource: "service".to_string(),
                current: None,
                max: None,
            },
            ErrorKind::ExternalService => {
                Self::Internal(format!("External service error: {}", core_error.message))
            }
            ErrorKind::Cancelled => {
                Self::Internal(format!("Operation cancelled: {}", core_error.message))
            }
            ErrorKind::Internal => Self::Internal(core_error.message),
            ErrorKind::Security => Self::Authorization {
                message: format!("Security error: {}", core_error.message),
                resource: None,
            },
        }
    }
}

// Conversion from server errors to protocol errors
///
/// This conversion preserves protocol errors directly when they come from clients
/// (ServerError::Protocol variant), ensuring error codes like `-1` for user rejection
/// are maintained through the server layer.
///
/// # Error Code Preservation
///
/// When a client returns an error (e.g., user rejects sampling with code `-1`),
/// the server receives it as `ServerError::Protocol(Error{ kind: UserRejected })`.
/// This conversion unwraps it directly, preserving the original error code when
/// the error is sent back to calling clients.
impl From<ServerError> for Box<turbomcp_protocol::Error> {
    fn from(server_error: ServerError) -> Self {
        match server_error {
            // Unwrap protocol errors directly to preserve error codes
            ServerError::Protocol(protocol_err) => protocol_err,

            // Map other server errors to appropriate protocol errors
            ServerError::Transport(transport_err) => {
                turbomcp_protocol::Error::transport(format!("Transport error: {}", transport_err))
            }
            ServerError::Handler { message, context } => {
                turbomcp_protocol::Error::internal(format!(
                    "Handler error{}: {}",
                    context
                        .as_ref()
                        .map(|c| format!(" ({})", c))
                        .unwrap_or_default(),
                    message
                ))
            }
            ServerError::Core(err) => {
                turbomcp_protocol::Error::internal(format!("Core error: {}", err))
            }
            ServerError::Configuration { message, .. } => {
                turbomcp_protocol::Error::configuration(message)
            }
            ServerError::Authentication { message, .. } => {
                turbomcp_protocol::Error::new(turbomcp_protocol::ErrorKind::Authentication, message)
            }
            ServerError::Authorization { message, .. } => turbomcp_protocol::Error::new(
                turbomcp_protocol::ErrorKind::PermissionDenied,
                message,
            ),
            ServerError::RateLimit { message, .. } => {
                turbomcp_protocol::Error::rate_limited(message)
            }
            ServerError::Timeout {
                operation,
                timeout_ms,
            } => turbomcp_protocol::Error::timeout(format!(
                "Operation '{}' timed out after {}ms",
                operation, timeout_ms
            )),
            ServerError::NotFound { resource } => turbomcp_protocol::Error::new(
                turbomcp_protocol::ErrorKind::ResourceNotFound,
                format!("Resource not found: {}", resource),
            ),
            ServerError::ResourceExhausted { resource, .. } => turbomcp_protocol::Error::new(
                turbomcp_protocol::ErrorKind::Unavailable,
                format!("Resource exhausted: {}", resource),
            ),
            ServerError::Internal(message) => turbomcp_protocol::Error::internal(message),
            ServerError::Lifecycle(message) => {
                turbomcp_protocol::Error::internal(format!("Lifecycle error: {}", message))
            }
            ServerError::Shutdown(message) => {
                turbomcp_protocol::Error::internal(format!("Shutdown error: {}", message))
            }
            ServerError::Middleware { name, message } => turbomcp_protocol::Error::internal(
                format!("Middleware error ({}): {}", name, message),
            ),
            ServerError::Registry(message) => {
                turbomcp_protocol::Error::internal(format!("Registry error: {}", message))
            }
            ServerError::Routing { message, .. } => {
                turbomcp_protocol::Error::internal(format!("Routing error: {}", message))
            }
            ServerError::Io(err) => {
                turbomcp_protocol::Error::internal(format!("IO error: {}", err))
            }
            ServerError::Serialization(err) => turbomcp_protocol::Error::new(
                turbomcp_protocol::ErrorKind::Serialization,
                format!("Serialization error: {}", err),
            ),
        }
    }
}

// Comprehensive tests in separate file (tokio/axum pattern)
#[cfg(test)]
mod tests;
