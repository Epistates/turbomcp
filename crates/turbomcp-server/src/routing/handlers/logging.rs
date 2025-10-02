//! Logging handler for MCP logging operations

use turbomcp_core::RequestContext;
use turbomcp_protocol::{
    jsonrpc::{JsonRpcRequest, JsonRpcResponse},
    types::{SetLevelRequest, SetLevelResult},
};

use super::HandlerContext;
use crate::routing::utils::{error_response, parse_params, success_response};

/// Handle set log level request
pub async fn handle_set_level(
    _context: &HandlerContext,
    request: JsonRpcRequest,
    _ctx: RequestContext,
) -> JsonRpcResponse {
    match parse_params::<SetLevelRequest>(&request) {
        Ok(_set_level_request) => {
            // TODO: Implement actual log level setting
            let result = SetLevelResult;
            success_response(&request, result)
        }
        Err(e) => error_response(&request, e),
    }
}
