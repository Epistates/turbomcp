//! RFC 9728 Protected Resource Metadata.
//!
//! The MCP server MUST serve this document (MCP authorization spec
//! §Authorization Server Discovery) so clients can locate the authorization
//! server(s) that issue tokens for this resource. The MCP-mandated field is
//! `authorization_servers` (≥1 entry); `resource` is the canonical resource
//! URI the `aud` claim is bound to.

use serde::{Deserialize, Serialize};

/// An RFC 9728 Protected Resource Metadata document.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResourceMetadata {
    /// The resource's canonical URI. Tokens MUST carry this in their `aud`.
    pub resource: String,
    /// Authorization servers that issue tokens for this resource (≥1, per the
    /// MCP authorization spec).
    pub authorization_servers: Vec<String>,
    /// Scopes the resource recognizes (RFC 9728 OPTIONAL). The spec says omit
    /// `offline_access` here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scopes_supported: Option<Vec<String>>,
    /// How bearer tokens may be presented (RFC 9728 OPTIONAL). MCP uses the
    /// `Authorization` header only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bearer_methods_supported: Option<Vec<String>>,
}

impl ResourceMetadata {
    /// A metadata document for `resource`, issued by `authorization_servers`.
    /// Defaults `bearer_methods_supported` to `["header"]` (MCP's only mode).
    #[must_use]
    pub fn new(
        resource: impl Into<String>,
        authorization_servers: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            resource: resource.into(),
            authorization_servers: authorization_servers.into_iter().map(Into::into).collect(),
            scopes_supported: None,
            bearer_methods_supported: Some(vec!["header".to_owned()]),
        }
    }

    /// Declare the scopes the resource recognizes (RFC 9728 `scopes_supported`).
    ///
    /// `offline_access` is left out: it is the authorization server's to
    /// offer, and servers "SHOULD NOT include `offline_access` in
    /// `scopes_supported`", since a client requesting every listed scope
    /// would then ask every user for long-lived access.
    #[must_use]
    pub fn scopes_supported(mut self, scopes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.scopes_supported = Some(
            scopes
                .into_iter()
                .map(Into::into)
                .filter(|scope: &String| scope != "offline_access")
                .collect(),
        );
        self
    }
}
