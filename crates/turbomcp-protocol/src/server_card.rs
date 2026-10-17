//! MCP Server Cards (SEP-2127, extension `io.modelcontextprotocol/server-card`):
//! a static, public document describing a remote MCP server for discovery
//! before connecting — its identity, its HTTP endpoints and the protocol
//! versions each serves.
//!
//! The wire format is the extension's `schema.ts` (`ext-server-card`, the
//! snapshot SEP-2127 cites). A card carries no tools, resources, prompts,
//! capabilities or credentials, and is advisory: a client that connects
//! prefers what `server/discover` says where the two disagree.
//!
//! A server built with `turbomcp-transport-http` serves one at
//! `GET {path}/server-card` (`HttpConfig::with_server_card`); its client side
//! fetches and validates one (`fetch_server_card`).

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The `$schema` every card names (versioned by `vN`, not by date).
pub const SCHEMA_URL: &str =
    "https://static.modelcontextprotocol.io/schemas/v1/server-card.schema.json";

/// The media type a card is served and requested as.
pub const MEDIA_TYPE: &str = "application/mcp-server-card+json";

/// The extension identifier.
pub const EXTENSION: &str = "io.modelcontextprotocol/server-card";

/// The path suffix MCP reserves for a card, appended to the Streamable HTTP
/// endpoint's URL (`https://host/mcp` → `https://host/mcp/server-card`).
pub const PATH_SUFFIX: &str = "/server-card";

/// Where a server at `endpoint` (its Streamable HTTP URL) hosts its card by
/// default.
#[must_use]
pub fn default_url(endpoint: &str) -> String {
    let mut url = endpoint.trim_end_matches('/').to_string();
    url.push_str(PATH_SUFFIX);
    url
}

/// A Server Card.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct ServerCard {
    /// Always [`SCHEMA_URL`].
    #[serde(rename = "$schema")]
    pub schema: String,
    /// Reverse-DNS name with exactly one `/` (`com.example/weather`).
    pub name: String,
    /// The server's version (`Implementation.version`); a range is not one.
    pub version: String,
    /// What the server does, in at most 100 characters.
    pub description: String,
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Homepage or documentation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub website_url: Option<String>,
    /// Where the source lives.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<Repository>,
    /// Icons a client can display.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub icons: Vec<Icon>,
    /// The HTTP endpoints to connect to.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remotes: Vec<Remote>,
    /// Reverse-DNS-namespaced vendor metadata (never capabilities).
    #[serde(rename = "_meta", default, skip_serializing_if = "Map::is_empty")]
    pub meta: Map<String, Value>,
}

impl ServerCard {
    /// A card for the server named `name` (reverse DNS, one `/`) that does
    /// `description`. A server serving it fills `version`, `title`, `icons`,
    /// `websiteUrl` and each remote's protocol versions from what it runs,
    /// where they are left unset.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            schema: SCHEMA_URL.into(),
            name: name.into(),
            version: String::new(),
            description: description.into(),
            title: None,
            website_url: None,
            repository: None,
            icons: Vec::new(),
            remotes: Vec::new(),
            meta: Map::new(),
        }
    }

    /// Set the version.
    #[must_use]
    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.version = version.into();
        self
    }

    /// Set the display name.
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the homepage.
    #[must_use]
    pub fn with_website_url(mut self, url: impl Into<String>) -> Self {
        self.website_url = Some(url.into());
        self
    }

    /// Set the source repository.
    #[must_use]
    pub fn with_repository(mut self, repository: Repository) -> Self {
        self.repository = Some(repository);
        self
    }

    /// Add an icon.
    #[must_use]
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icons.push(icon);
        self
    }

    /// Add an endpoint.
    #[must_use]
    pub fn with_remote(mut self, remote: Remote) -> Self {
        self.remotes.push(remote);
        self
    }

    /// Add a `_meta` entry.
    #[must_use]
    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        self.meta.insert(key.into(), value);
        self
    }

    /// Check the constraints the schema and its prose put on a card.
    ///
    /// # Errors
    /// The first field that breaks one, and how.
    pub fn validate(&self) -> Result<(), ServerCardError> {
        let fail = |field: &'static str, problem: &str| {
            Err(ServerCardError {
                field,
                problem: problem.into(),
            })
        };
        if self.schema != SCHEMA_URL {
            return fail("$schema", "must be the v1 Server Card schema URL");
        }
        if !valid_name(&self.name) {
            return fail(
                "name",
                "must be 3-200 characters of reverse-DNS namespace, one `/`, and a name",
            );
        }
        if self.version.is_empty() || self.version.chars().count() > 255 {
            return fail("version", "must be 1-255 characters");
        }
        if is_version_range(&self.version) {
            return fail("version", "is a version range, not a version");
        }
        if !(1..=100).contains(&self.description.chars().count()) {
            return fail("description", "must be 1-100 characters");
        }
        if self
            .title
            .as_ref()
            .is_some_and(|t| !(1..=100).contains(&t.chars().count()))
        {
            return fail("title", "must be 1-100 characters");
        }
        if self
            .website_url
            .as_deref()
            .is_some_and(|u| !looks_like_uri(u))
        {
            return fail("websiteUrl", "must be a URI");
        }
        if self
            .repository
            .as_ref()
            .is_some_and(|r| !looks_like_uri(&r.url))
        {
            return fail("repository.url", "must be a URI");
        }
        if self.icons.iter().any(|i| !looks_like_uri(&i.src)) {
            return fail("icons[].src", "must be a URI");
        }
        if self.remotes.iter().any(|r| !valid_remote_url(&r.url)) {
            return fail(
                "remotes[].url",
                "must start with http://, https:// or a {variable}, with no whitespace",
            );
        }
        Ok(())
    }
}

/// A card that breaks a constraint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerCardError {
    /// The field, as the wire names it.
    pub field: &'static str,
    /// What is wrong with it.
    pub problem: String,
}

impl fmt::Display for ServerCardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "server card `{}` {}", self.field, self.problem)
    }
}

impl core::error::Error for ServerCardError {}

/// Where the server's source lives.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Repository {
    /// Browsable, cloneable URL.
    pub url: String,
    /// Hosting service (`"github"`).
    pub source: String,
    /// Path from the repository root to the server, in a monorepo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subfolder: Option<String>,
    /// The forge's stable repository id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl Repository {
    /// Source at `url`, hosted on `source`.
    pub fn new(url: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            source: source.into(),
            subfolder: None,
            id: None,
        }
    }
}

/// An icon.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Icon {
    /// HTTP(S) URL or `data:` URI.
    pub src: String,
    /// MIME type, when the source's is missing or generic.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    /// `WxH` sizes, or `"any"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sizes: Vec<String>,
    /// The background it is drawn for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<Theme>,
}

impl Icon {
    /// An icon at `src`.
    pub fn new(src: impl Into<String>) -> Self {
        Self {
            src: src.into(),
            mime_type: None,
            sizes: Vec::new(),
            theme: None,
        }
    }
}

/// The background an icon is designed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum Theme {
    /// A light background.
    Light,
    /// A dark background.
    Dark,
}

/// An HTTP endpoint to connect to.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Remote {
    /// The transport served there.
    #[serde(rename = "type")]
    pub transport: RemoteTransport,
    /// URL template; `{variables}` come from [`variables`](Self::variables).
    pub url: String,
    /// Headers the endpoint requires or accepts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<KeyValueInput>,
    /// The `{variables}` in `url`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, Input>,
    /// The MCP protocol versions served there.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supported_protocol_versions: Vec<String>,
}

impl Remote {
    /// A Streamable HTTP endpoint at `url`.
    pub fn streamable_http(url: impl Into<String>) -> Self {
        Self::new(RemoteTransport::StreamableHttp, url)
    }

    /// A legacy HTTP+SSE endpoint at `url`.
    pub fn sse(url: impl Into<String>) -> Self {
        Self::new(RemoteTransport::Sse, url)
    }

    fn new(transport: RemoteTransport, url: impl Into<String>) -> Self {
        Self {
            transport,
            url: url.into(),
            headers: Vec::new(),
            variables: BTreeMap::new(),
            supported_protocol_versions: Vec::new(),
        }
    }

    /// Add a header.
    #[must_use]
    pub fn with_header(mut self, header: KeyValueInput) -> Self {
        self.headers.push(header);
        self
    }

    /// Describe the `{name}` variable in the URL.
    #[must_use]
    pub fn with_variable(mut self, name: impl Into<String>, input: Input) -> Self {
        self.variables.insert(name.into(), input);
        self
    }

    /// Set the protocol versions served (a serving server fills them from
    /// what it runs when left empty).
    #[must_use]
    pub fn with_supported_protocol_versions<I, V>(mut self, versions: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<String>,
    {
        self.supported_protocol_versions = versions.into_iter().map(Into::into).collect();
        self
    }
}

/// The transport a [`Remote`] serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum RemoteTransport {
    /// Streamable HTTP.
    StreamableHttp,
    /// The legacy HTTP+SSE transport.
    Sse,
}

/// A value the user supplies or the card presets: a URL variable, or a
/// header's value.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[non_exhaustive]
pub struct Input {
    /// What the value is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Whether the connection needs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_required: Option<bool>,
    /// Whether to handle it as a secret.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_secret: Option<bool>,
    /// How to read it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<InputFormat>,
    /// The default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// An example to show while configuring.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    /// A preset value users can't change; `{variables}` are substituted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// The only values allowed.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

impl Input {
    /// An input described as `description`.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: Some(description.into()),
            ..Self::default()
        }
    }

    /// Mark it required.
    #[must_use]
    pub fn required(mut self) -> Self {
        self.is_required = Some(true);
        self
    }

    /// Mark it secret.
    #[must_use]
    pub fn secret(mut self) -> Self {
        self.is_secret = Some(true);
        self
    }

    /// Preset its value.
    #[must_use]
    pub fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Set its default.
    #[must_use]
    pub fn with_default(mut self, default: impl Into<String>) -> Self {
        self.default = Some(default.into());
        self
    }
}

/// How an [`Input`] is read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum InputFormat {
    /// Text.
    String,
    /// A decimal number.
    Number,
    /// `"true"` or `"false"`.
    Boolean,
    /// A path on the user's filesystem.
    Filepath,
}

/// A named [`Input`]: an HTTP header.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct KeyValueInput {
    /// The header name.
    pub name: String,
    /// The value's description and constraints.
    #[serde(flatten)]
    pub input: Input,
    /// The `{variables}` in its `value`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, Input>,
}

impl KeyValueInput {
    /// The header `name`, as `input` describes it.
    pub fn new(name: impl Into<String>, input: Input) -> Self {
        Self {
            name: name.into(),
            input,
            variables: BTreeMap::new(),
        }
    }

    /// Describe the `{name}` variable in its value.
    #[must_use]
    pub fn with_variable(mut self, name: impl Into<String>, input: Input) -> Self {
        self.variables.insert(name.into(), input);
        self
    }
}

/// `^[a-zA-Z0-9.-]+/[a-zA-Z0-9._-]+$`, 3 to 200 characters.
fn valid_name(name: &str) -> bool {
    let Some((namespace, server)) = name.split_once('/') else {
        return false;
    };
    (3..=200).contains(&name.len())
        && !namespace.is_empty()
        && !server.is_empty()
        && namespace
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
        && server
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// `^(https?://[^\s]+|\{[a-zA-Z_][a-zA-Z0-9_]*\}[^\s]*)$`.
fn valid_remote_url(url: &str) -> bool {
    if url.chars().any(char::is_whitespace) {
        return false;
    }
    if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        return !rest.is_empty();
    }
    let Some(rest) = url.strip_prefix('{') else {
        return false;
    };
    let Some((variable, _)) = rest.split_once('}') else {
        return false;
    };
    let mut chars = variable.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// "Version ranges are rejected (e.g., `^1.2.3`, `~1.2.3`, `>=1.2.3`, `1.x`,
/// `1.*`)."
fn is_version_range(version: &str) -> bool {
    version.starts_with(['^', '~', '>', '<', '='])
        || version
            .split(['.', '-', '+'])
            .any(|part| matches!(part, "x" | "X" | "*"))
}

/// A scheme, then something: what `format: uri` asks for.
fn looks_like_uri(uri: &str) -> bool {
    uri.split_once(':').is_some_and(|(scheme, rest)| {
        !rest.is_empty()
            && scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
    }) && !uri.chars().any(char::is_whitespace)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn card() -> ServerCard {
        ServerCard::new("com.example/weather", "Weather forecasts.").with_version("1.2.0")
    }

    #[test]
    fn a_minimal_card_validates_and_serializes_as_the_schema_spells_it() {
        let card = card().with_remote(
            Remote::streamable_http("https://example.com/mcp")
                .with_supported_protocol_versions(["2026-07-28"]),
        );
        card.validate().unwrap();
        assert_eq!(
            serde_json::to_value(&card).unwrap(),
            json!({
                "$schema": SCHEMA_URL,
                "name": "com.example/weather",
                "version": "1.2.0",
                "description": "Weather forecasts.",
                "remotes": [{
                    "type": "streamable-http",
                    "url": "https://example.com/mcp",
                    "supportedProtocolVersions": ["2026-07-28"]
                }]
            })
        );
    }

    #[test]
    fn each_constraint_names_its_field() {
        let field = |card: ServerCard| card.validate().unwrap_err().field;
        assert_eq!(
            field(ServerCard::new("weather", "x").with_version("1")),
            "name"
        );
        assert_eq!(
            field(ServerCard::new("a/b/c", "x").with_version("1")),
            "name"
        );
        assert_eq!(field(ServerCard::new("com.example/w", "x")), "version");
        assert_eq!(field(card().with_version("^1.2.3")), "version");
        assert_eq!(field(card().with_version("1.x")), "version");
        assert_eq!(
            field(ServerCard::new("com.example/w", "").with_version("1")),
            "description"
        );
        let long = "x".repeat(101);
        assert_eq!(
            field(ServerCard::new("com.example/w", long).with_version("1")),
            "description"
        );
        assert_eq!(field(card().with_title("")), "title");
        assert_eq!(field(card().with_website_url("not a uri")), "websiteUrl");
        assert_eq!(
            field(card().with_remote(Remote::sse("ftp://x"))),
            "remotes[].url"
        );
        assert_eq!(
            field(card().with_remote(Remote::sse("{9x}/mcp"))),
            "remotes[].url"
        );
        let mut wrong = card();
        wrong.schema =
            "https://static.modelcontextprotocol.io/schemas/2026-01-01/server-card.schema.json"
                .into();
        assert_eq!(field(wrong), "$schema");
    }

    #[test]
    fn a_templated_url_and_its_variables_are_accepted() {
        card()
            .with_remote(
                Remote::streamable_http("{tenant}.example.com/mcp")
                    .with_variable("tenant", Input::new("Tenant").required())
                    .with_header(
                        KeyValueInput::new(
                            "Authorization",
                            Input::new("Bearer token")
                                .required()
                                .secret()
                                .with_value("Bearer {token}"),
                        )
                        .with_variable("token", Input::new("API token").secret()),
                    ),
            )
            .validate()
            .unwrap();
    }

    #[test]
    fn the_default_url_appends_the_reserved_suffix() {
        assert_eq!(
            default_url("https://host/mcp"),
            "https://host/mcp/server-card"
        );
        assert_eq!(
            default_url("https://host/mcp/"),
            "https://host/mcp/server-card"
        );
    }
}
