//! [`ProtocolVersion`] — the single representation of an MCP protocol version.
//!
//! Ground truth (verified against `reference/modelcontextprotocol/schema/`):
//! the published versions are `2024-11-05`, `2025-03-26`, `2025-06-18`,
//! `2025-11-25`, and the in-development draft. Upstream's
//! `schema/draft/schema.ts` now pins `LATEST_PROTOCOL_VERSION = "2026-07-28"`,
//! so that is the wire string real draft-tracking peers negotiate — even though
//! the schema *content* still lives in `schema/draft/` until the dated directory
//! freezes (scheduled ~2026-07-28).
//!
//! The draft is modeled as a *channel* — [`ProtocolVersion::Draft`], not a dated
//! variant — because the spec's release date can still slip. We keep a stable
//! name and map only its wire string to the spec's current
//! `LATEST_PROTOCOL_VERSION`. At freeze we add the dated `V2026_07_28` variant,
//! repoint [`ProtocolVersion::LATEST`], and deprecate [`ProtocolVersion::Draft`]
//! in favor of it.

use alloc::string::{String, ToString};

/// An MCP protocol version.
///
/// `#[non_exhaustive]` so new versions can be added without a major bump.
/// Serializes to / deserializes from the wire string (e.g. `"2025-11-25"`,
/// `"2026-07-28"`); unrecognized strings round-trip through
/// [`ProtocolVersion::Unknown`] rather than failing to parse.
#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(from = "String", into = "String")]
#[non_exhaustive]
pub enum ProtocolVersion {
    /// `2024-11-05` — first stable revision.
    V2024_11_05,
    /// `2025-03-26` — added (and is the only version to have) JSON-RPC batches.
    V2025_03_26,
    /// `2025-06-18` — removed batches.
    V2025_06_18,
    /// `2025-11-25` — current stable; stateful, core Tasks, `initialize`/`ping`.
    V2025_11_25,
    /// The in-development **draft** channel — stateless model (`server/discover`,
    /// `subscriptions/listen`, MRTR). Its wire string tracks the draft's
    /// `LATEST_PROTOCOL_VERSION` (currently `"2026-07-28"`); the variant is named
    /// for the channel rather than the date so it survives a slip, and will be
    /// deprecated in favor of a dated variant once the spec freezes.
    Draft,
    /// Any version string this build does not recognize.
    Unknown(String),
}

impl ProtocolVersion {
    /// The latest version this build targets.
    pub const LATEST: Self = Self::Draft;

    /// Versions v4 actively supports as first-class (others may still be
    /// negotiated/named, but are not first-class dispatch targets).
    ///
    /// Chronological, which is also the order they are advertised in
    /// `server/discover` and in an unsupported-version error's `supported`
    /// list.
    pub const SUPPORTED: &'static [Self] = &[Self::V2025_06_18, Self::V2025_11_25, Self::Draft];

    /// The stateful revisions: those that negotiate with an `initialize`
    /// handshake and carry per-session state, as opposed to the draft's
    /// per-request model.
    pub const STATEFUL: &'static [Self] = &[Self::V2025_06_18, Self::V2025_11_25];

    /// Whether this version uses the `initialize` handshake and per-session
    /// state (see [`STATEFUL`](Self::STATEFUL)).
    #[must_use]
    pub fn is_stateful(&self) -> bool {
        Self::STATEFUL.contains(self)
    }

    /// Whether this version has **core** Tasks: the `tasks/*` methods, the
    /// `tasks` server capability, and `Tool.execution`.
    ///
    /// Only `2025-11-25`. `2025-06-18` predates Tasks entirely, and the draft
    /// moved them out into an extension.
    #[must_use]
    pub fn has_core_tasks(&self) -> bool {
        matches!(self, Self::V2025_11_25)
    }

    /// The wire string for this version.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::V2024_11_05 => "2024-11-05",
            Self::V2025_03_26 => "2025-03-26",
            Self::V2025_06_18 => "2025-06-18",
            Self::V2025_11_25 => "2025-11-25",
            Self::Draft => "2026-07-28",
            Self::Unknown(s) => s,
        }
    }

    /// Parse a wire string into a [`ProtocolVersion`]. Unrecognized strings
    /// become [`ProtocolVersion::Unknown`] (never an error).
    #[must_use]
    pub fn from_wire(s: &str) -> Self {
        match s {
            "2024-11-05" => Self::V2024_11_05,
            "2025-03-26" => Self::V2025_03_26,
            "2025-06-18" => Self::V2025_06_18,
            "2025-11-25" => Self::V2025_11_25,
            "2026-07-28" => Self::Draft,
            other => Self::Unknown(other.to_string()),
        }
    }

    /// Whether this build supports `self` as a first-class dispatch target.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        Self::SUPPORTED.contains(self)
    }

    /// Whether `self` names a *published* MCP protocol version this build
    /// recognizes (any variant other than [`ProtocolVersion::Unknown`]).
    ///
    /// Broader than [`is_supported`](Self::is_supported): an older revision such
    /// as `2025-03-26` is recognized but not a first-class dispatch target. A
    /// transport can tolerate a recognized version header (letting a session's
    /// negotiated version govern) while still rejecting an unrecognized string.
    #[must_use]
    pub fn is_recognized(&self) -> bool {
        !matches!(self, Self::Unknown(_))
    }
}

impl core::fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl From<String> for ProtocolVersion {
    /// Infallible: unrecognized strings map to [`ProtocolVersion::Unknown`]
    /// (reusing the owned `String`, no extra allocation).
    fn from(s: String) -> Self {
        match s.as_str() {
            "2024-11-05" => Self::V2024_11_05,
            "2025-03-26" => Self::V2025_03_26,
            "2025-06-18" => Self::V2025_06_18,
            "2025-11-25" => Self::V2025_11_25,
            "2026-07-28" => Self::Draft,
            _ => Self::Unknown(s),
        }
    }
}

impl From<ProtocolVersion> for String {
    fn from(v: ProtocolVersion) -> Self {
        match v {
            ProtocolVersion::Unknown(s) => s,
            other => other.as_str().to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn roundtrip_known_versions() {
        for v in [
            ProtocolVersion::V2025_11_25,
            ProtocolVersion::Draft,
            ProtocolVersion::V2025_06_18,
        ] {
            let s = serde_json::to_string(&v).unwrap();
            let back: ProtocolVersion = serde_json::from_str(&s).unwrap();
            assert_eq!(v, back);
        }
    }

    #[test]
    fn draft_wire_string_is_correct() {
        assert_eq!(ProtocolVersion::Draft.as_str(), "2026-07-28");
        assert_eq!(
            serde_json::to_string(&ProtocolVersion::Draft).unwrap(),
            "\"2026-07-28\""
        );
    }

    #[test]
    fn unknown_roundtrips_not_errors() {
        let v: ProtocolVersion = serde_json::from_str("\"2099-01-01\"").unwrap();
        assert_eq!(v, ProtocolVersion::Unknown("2099-01-01".to_string()));
        assert_eq!(serde_json::to_string(&v).unwrap(), "\"2099-01-01\"");
    }

    #[test]
    fn display_matches_wire_string() {
        assert_eq!(ProtocolVersion::Draft.to_string(), "2026-07-28");
        assert_eq!(ProtocolVersion::V2025_11_25.to_string(), "2025-11-25");
        assert_eq!(
            ProtocolVersion::Unknown("2099-01-01".to_string()).to_string(),
            "2099-01-01"
        );
    }

    #[test]
    fn supported_set() {
        assert!(ProtocolVersion::V2025_11_25.is_supported());
        assert!(ProtocolVersion::Draft.is_supported());
        assert!(!ProtocolVersion::V2024_11_05.is_supported());
    }

    #[test]
    fn recognized_is_broader_than_supported() {
        // Recognized but not a dispatch target (older revisions).
        assert!(ProtocolVersion::from_wire("2025-03-26").is_recognized());
        assert!(ProtocolVersion::from_wire("2024-11-05").is_recognized());
        assert!(!ProtocolVersion::from_wire("2025-03-26").is_supported());
        // Supported implies recognized.
        assert!(ProtocolVersion::V2025_11_25.is_recognized());
        assert!(ProtocolVersion::Draft.is_recognized());
        // A garbage string is neither.
        assert!(!ProtocolVersion::from_wire("nonsense").is_recognized());
    }
}
