//! Request and response validation for MCP protocol compliance

use turbomcp_protocol::jsonrpc::{JsonRpcRequest, JsonRpcResponse};

use crate::{McpError, ServerErrorExt, ServerResult};

/// Validate JSON-RPC request using protocol validator
pub fn validate_request(request: &JsonRpcRequest) -> ServerResult<()> {
    // Lightweight structural validation using protocol validator
    let validator = turbomcp_protocol::validation::ProtocolValidator::new();
    match validator.validate_request(request) {
        turbomcp_protocol::validation::ValidationResult::Invalid(errors) => {
            let msg = errors
                .into_iter()
                .map(|e| {
                    format!(
                        "{}: {}{}",
                        e.code,
                        e.message,
                        e.field_path
                            .map(|p| format!(" (@ {p})"))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            Err(McpError::routing(format!("Request validation failed: {msg}"))
                .with_operation(request.method.clone()))
        }
        _ => Ok(()),
    }
}

/// Validate JSON-RPC response using protocol validator
pub fn validate_response(response: &JsonRpcResponse) -> ServerResult<()> {
    let validator = turbomcp_protocol::validation::ProtocolValidator::new();
    match validator.validate_response(response) {
        turbomcp_protocol::validation::ValidationResult::Invalid(errors) => {
            let msg = errors
                .into_iter()
                .map(|e| {
                    format!(
                        "{}: {}{}",
                        e.code,
                        e.message,
                        e.field_path
                            .map(|p| format!(" (@ {p})"))
                            .unwrap_or_default()
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            Err(McpError::routing(format!(
                "Response validation failed: {msg}"
            )))
        }
        _ => Ok(()),
    }
}

