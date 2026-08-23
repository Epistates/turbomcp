//! [`ProtocolVersion`] — the single representation of an MCP protocol version.
//!
//! Ground truth (verified against `reference/modelcontextprotocol/schema/`):
//! the published versions are `2024-11-05`, `2025-03-26`, `2025-06-18`,
//! `2025-11-25`, and `2026-07-28`.
//!
//! `2026-07-28` **froze** on 2026-07-28 (upstream tag `2026-07-28`, dated
//! directory `schema/2026-07-28/`), so it is now a dated variant like every
//! other revision: [`ProtocolVersion::V2026_07_28`]. Until then it was modeled
//! as a slip-proof *channel* named `Draft`, which survives as a deprecated
//! alias — same wire string, same dispatch — so existing code keeps compiling.
//! `schema/draft/` currently holds a byte-identical copy of the frozen schema;
//! when a genuinely new draft opens upstream, it gets its own channel again.

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
    /// `2026-07-28` — stateless model (`server/discover`,
    /// `subscriptions/listen`, MRTR), Tasks moved out to an extension.
    /// Frozen 2026-07-28; before that this was the `Draft` channel.
    V2026_07_28,
    /// Any version string this build does not recognize.
    Unknown(String),
}

impl ProtocolVersion {
    /// The in-development draft channel, before `2026-07-28` froze.
    ///
    /// Kept as an alias so code written against the pre-freeze API still
    /// compiles and dispatches identically. A future draft will get a new
    /// channel constant rather than reusing this one, which is pinned to a
    /// now-published revision.
    #[deprecated(since = "4.0.0-alpha.2", note = "the draft froze: use V2026_07_28")]
    #[allow(non_upper_case_globals, reason = "preserves the old variant spelling")]
    pub const Draft: Self = Self::V2026_07_28;

    /// The latest version this build targets.
    pub const LATEST: Self = Self::V2026_07_28;

    /// Versions v4 actively supports as first-class (others may still be
    /// negotiated/named, but are not first-class dispatch targets).
    ///
    /// Chronological, which is also the order they are advertised in
    /// `server/discover` and in an unsupported-version error's `supported`
    /// list.
    pub const SUPPORTED: &'static [Self] =
        &[Self::V2025_06_18, Self::V2025_11_25, Self::V2026_07_28];

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
            Self::V2026_07_28 => "2026-07-28",
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
            "2026-07-28" => Self::V2026_07_28,
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
            "2026-07-28" => Self::V2026_07_28,
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
            ProtocolVersion::V2026_07_28,
            ProtocolVersion::V2025_06_18,
        ] {
            let s = serde_json::to_string(&v).unwrap();
            let back: ProtocolVersion = serde_json::from_str(&s).unwrap();
            assert_eq!(v, back);
        }
    }

    #[test]
    fn frozen_wire_string_is_correct() {
        assert_eq!(ProtocolVersion::V2026_07_28.as_str(), "2026-07-28");
        assert_eq!(
            serde_json::to_string(&ProtocolVersion::V2026_07_28).unwrap(),
            "\"2026-07-28\""
        );
    }

    /// The pre-freeze spelling still resolves, and to the same revision — code
    /// written against `Draft` keeps compiling and dispatching identically.
    #[test]
    #[allow(deprecated, reason = "asserting the deprecated alias still works")]
    fn draft_alias_still_names_the_frozen_revision() {
        assert_eq!(ProtocolVersion::Draft, ProtocolVersion::V2026_07_28);
        assert_eq!(ProtocolVersion::Draft.as_str(), "2026-07-28");
    }

    #[test]
    fn unknown_roundtrips_not_errors() {
        let v: ProtocolVersion = serde_json::from_str("\"2099-01-01\"").unwrap();
        assert_eq!(v, ProtocolVersion::Unknown("2099-01-01".to_string()));
        assert_eq!(serde_json::to_string(&v).unwrap(), "\"2099-01-01\"");
    }

    #[test]
    fn display_matches_wire_string() {
        assert_eq!(ProtocolVersion::V2026_07_28.to_string(), "2026-07-28");
        assert_eq!(ProtocolVersion::V2025_11_25.to_string(), "2025-11-25");
        assert_eq!(
            ProtocolVersion::Unknown("2099-01-01".to_string()).to_string(),
            "2099-01-01"
        );
    }

    #[test]
    fn supported_set() {
        assert!(ProtocolVersion::V2025_11_25.is_supported());
        assert!(ProtocolVersion::V2026_07_28.is_supported());
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
        assert!(ProtocolVersion::V2026_07_28.is_recognized());
        // A garbage string is neither.
        assert!(!ProtocolVersion::from_wire("nonsense").is_recognized());
    }
}
