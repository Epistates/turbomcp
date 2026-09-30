//! Internal-error masking ([`ServerBuilder::mask_internal_errors`]).
//!
//! An internal error's text is written for the operator: it wraps a database
//! driver's message, an upstream response, a path. Masked, the client gets
//! `internal error (ref: <id>)` and the full text goes to `tracing` under the
//! same id, so a user's report can be matched to the log line.
//!
//! [`ServerBuilder::mask_internal_errors`]: crate::ServerBuilder::mask_internal_errors

use turbomcp_core::{Extensions, JsonRpcMessage, codes};

/// Attached to a request's extensions when the server masks internal errors,
/// so the paths that turn an error into a tool result can see it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct MaskInternalErrors;

/// Whether this request's internal errors are masked.
pub(crate) fn enabled(extensions: &Extensions) -> bool {
    extensions.contains::<MaskInternalErrors>()
}

/// Log `detail` under a fresh reference and return what the client sees.
pub(crate) fn masked(detail: &str) -> String {
    let reference = uuid::Uuid::new_v4();
    tracing::error!(error_ref = %reference, error = detail, "internal error masked from the client");
    format!("internal error (ref: {reference})")
}

/// Mask an outgoing internal-error response. Its `data` goes too: whatever a
/// handler attached to an internal error is as much the operator's as the
/// message.
pub(crate) fn mask_response(mut msg: JsonRpcMessage) -> JsonRpcMessage {
    if let JsonRpcMessage::Response(response) = &mut msg
        && let Some(error) = response.error.as_mut()
        && error.code == codes::INTERNAL_ERROR
    {
        error.message = masked(&error.message);
        error.data = None;
    }
    msg
}
