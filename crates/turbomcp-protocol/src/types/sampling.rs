//! LLM sampling types.
//!
//! These types are defined canonically in [`turbomcp_types`] (see
//! [`turbomcp_types::protocol`]). This module re-exports them plus a
//! [`StopReason`] helper for the common spec values — the on-wire type for
//! `CreateMessageResult.stopReason` is `string` per MCP 2025-11-25, so the
//! canonical field is `Option<String>`. Use
//! `Some(StopReason::EndTurn.to_string())` or `Some(StopReason::EndTurn.into())`
//! to construct it ergonomically.

use serde::{Deserialize, Serialize};

pub use turbomcp_types::{
    CreateMessageRequest, CreateMessageResult, IncludeContext, ModelHint, ModelPreferences,
    SamplingMessage, ToolChoice, ToolChoiceMode,
};

/// Known stop reasons per MCP 2025-11-25 spec.
///
/// On the wire `stopReason` is a string — vendors may return other values.
/// Use [`StopReason::as_str`], [`Display`](core::fmt::Display), or
/// `Into<String>` to build the `CreateMessageResult.stop_reason` field.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "camelCase")]
pub enum StopReason {
    /// Generation completed naturally.
    EndTurn,
    /// Hit the maximum token limit.
    MaxTokens,
    /// Matched a stop sequence.
    StopSequence,
    /// Content filtering triggered.
    ContentFilter,
    /// Model chose to invoke a tool (SEP-1577).
    ToolUse,
}

impl StopReason {
    /// The camelCase string used on the wire for this reason.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::EndTurn => "endTurn",
            Self::MaxTokens => "maxTokens",
            Self::StopSequence => "stopSequence",
            Self::ContentFilter => "contentFilter",
            Self::ToolUse => "toolUse",
        }
    }
}

impl core::fmt::Display for StopReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<StopReason> for String {
    fn from(r: StopReason) -> Self {
        r.as_str().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stop_reason_wire_values() {
        assert_eq!(StopReason::EndTurn.as_str(), "endTurn");
        assert_eq!(StopReason::MaxTokens.as_str(), "maxTokens");
        assert_eq!(StopReason::StopSequence.as_str(), "stopSequence");
        assert_eq!(StopReason::ContentFilter.as_str(), "contentFilter");
        assert_eq!(StopReason::ToolUse.as_str(), "toolUse");
    }

    #[test]
    fn stop_reason_serialization() {
        assert_eq!(
            serde_json::to_string(&StopReason::EndTurn).unwrap(),
            "\"endTurn\""
        );
        assert_eq!(
            serde_json::to_string(&StopReason::ToolUse).unwrap(),
            "\"toolUse\""
        );
    }

    #[test]
    fn stop_reason_into_string_roundtrip() {
        let s: String = StopReason::MaxTokens.into();
        assert_eq!(s, "maxTokens");
    }
}
