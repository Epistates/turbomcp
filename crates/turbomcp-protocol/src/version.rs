//! Per-request protocol-version extraction (the modern, stateless path).
//!
//! In `2026-07-28` the version travels in each request's
//! `params._meta["io.modelcontextprotocol/protocolVersion"]` rather than being
//! negotiated once at `initialize`. This module reads that field; the
//! `VersionDispatcher` (in `turbomcp-server`) turns the result into a routing
//! decision (and consults session state instead for the legacy stateful path).

use serde_json::Value;
use turbomcp_core::ProtocolVersion;
use turbomcp_core::meta::keys::PROTOCOL_VERSION;

/// Read the protocol version from a request's `params._meta`, if present.
///
/// Returns `None` when `params`, `_meta`, or the version key is absent — the
/// dispatcher decides what an absent version means (see
/// `VersionDispatcher`'s missing-version policy, PLAN §4.9).
#[must_use]
pub fn request_protocol_version(params: Option<&Value>) -> Option<ProtocolVersion> {
    let meta = params?.get("_meta")?.as_object()?;
    let raw = meta.get(PROTOCOL_VERSION)?.as_str()?;
    Some(ProtocolVersion::from_wire(raw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_draft_version_from_meta() {
        let params = json!({
            "name": "echo",
            "_meta": { "io.modelcontextprotocol/protocolVersion": "2026-07-28" }
        });
        assert_eq!(
            request_protocol_version(Some(&params)),
            Some(ProtocolVersion::V2026_07_28)
        );
    }

    #[test]
    fn absent_meta_is_none() {
        assert_eq!(request_protocol_version(Some(&json!({"name": "x"}))), None);
        assert_eq!(request_protocol_version(None), None);
    }

    #[test]
    fn unknown_version_string_preserved() {
        let params =
            json!({ "_meta": { "io.modelcontextprotocol/protocolVersion": "9999-99-99" } });
        assert_eq!(
            request_protocol_version(Some(&params)),
            Some(ProtocolVersion::Unknown("9999-99-99".into()))
        );
    }
}
