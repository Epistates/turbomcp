//! Version-stable, handler-facing types — the surface user handlers speak.
//!
//! Handlers must not couple to a wire version. They return these neutral types;
//! the `VersionDispatcher` widens them to the *active* version's generated wire
//! type, filling version-specific required fields (`resultType`, `cacheScope`,
//! `ttlMs`, …) with spec defaults the handler shouldn't have to know about.
//!
//! This is the small, deliberately hand-curated subset the plan calls
//! `neutral/` (§3) — distinct from the full per-version generated surface. It
//! grows one method-family at a time as phases land; Phase 2 covers the
//! `tools/*` family and discovery. Because the trait signatures in
//! `turbomcp-server` are expressed in these types, wiring the second wire
//! version (Phase 5) adds conversions here without changing any handler.
//!
//! Conversions are intentionally one-directional (neutral → wire) and total
//! (`From`, never failing): a handler can always be serialized to the wire.

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use serde_json::{Map, Value};
use turbomcp_core::{McpError, ProtocolVersion};

use crate::v2025_06_18::types as v06;
use crate::v2025_11_25::types as legacy;
use crate::v2026_07_28::types as v0728;

/// Canonical draft `resultType` wire strings.
///
/// The draft schema made `ResultType` an open string (SEP-2322: extensible
/// result types); these are the two values the spec defines. Clients MUST
/// treat an absent field as `"complete"`.
pub mod result_type {
    /// The request completed; the result carries the final content.
    pub const COMPLETE: &str = "complete";
    /// The request needs more input; the result is an `InputRequiredResult`.
    pub const INPUT_REQUIRED: &str = "input_required";
}

/// A neutral content block. The enum is `#[non_exhaustive]` so further block
/// kinds (embedded resources, resource links) slot in without breaking callers,
/// and each variant is `#[non_exhaustive]` so future per-block spec fields slot
/// in too — construct via [`Content::text`] & co. and match with `..`.
/// (`PartialEq` only: block annotations carry an `f64` priority.)
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Content {
    /// Plain UTF-8 text.
    #[non_exhaustive]
    Text {
        /// The text.
        text: String,
        /// Display annotations for this block (audience/priority/lastModified).
        annotations: Option<Annotations>,
        /// Arbitrary `_meta` for this block (namespaced keys per the spec's
        /// `_meta` conventions). Empty = absent on the wire.
        meta: Map<String, Value>,
    },
    /// Base64-encoded image data with its MIME type (e.g. `image/png`).
    #[non_exhaustive]
    Image {
        /// Base64-encoded image bytes.
        data: String,
        /// The image MIME type.
        mime_type: String,
        /// Display annotations for this block (audience/priority/lastModified).
        annotations: Option<Annotations>,
        /// Arbitrary `_meta` for this block. Empty = absent on the wire.
        meta: Map<String, Value>,
    },
    /// Base64-encoded audio data with its MIME type (e.g. `audio/wav`).
    #[non_exhaustive]
    Audio {
        /// Base64-encoded audio bytes.
        data: String,
        /// The audio MIME type.
        mime_type: String,
        /// Display annotations for this block (audience/priority/lastModified).
        annotations: Option<Annotations>,
        /// Arbitrary `_meta` for this block. Empty = absent on the wire.
        meta: Map<String, Value>,
    },
    /// An embedded resource: the resource's contents inline (text or a base64
    /// blob), carried in a message rather than referenced by URI. The block
    /// carries its own annotations/`_meta`, distinct from the contents'
    /// [`ResourceContents`] `_meta`.
    #[non_exhaustive]
    Resource {
        /// The embedded resource contents.
        contents: ResourceContents,
        /// Display annotations for this block (audience/priority/lastModified).
        annotations: Option<Annotations>,
        /// Arbitrary `_meta` for this block. Empty = absent on the wire.
        meta: Map<String, Value>,
    },
    /// A link to a resource by URI + descriptor (name/title/MIME/size), without
    /// its contents — the client can `resources/read` it. Boxed: a full
    /// [`Resource`] (annotations/icons/`_meta`) would otherwise dominate the
    /// enum's size, taxing every text block in a content vector. The wire
    /// block's annotations/`_meta` are the resource's own fields.
    ResourceLink(Box<Resource>),
}

impl Content {
    /// A text content block.
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text {
            text: s.into(),
            annotations: None,
            meta: Map::new(),
        }
    }

    /// An image content block from base64 data and a MIME type.
    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Image {
            data: data.into(),
            mime_type: mime_type.into(),
            annotations: None,
            meta: Map::new(),
        }
    }

    /// An audio content block from base64 data and a MIME type.
    pub fn audio(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Audio {
            data: data.into(),
            mime_type: mime_type.into(),
            annotations: None,
            meta: Map::new(),
        }
    }

    /// An embedded-resource content block carrying the resource contents inline.
    pub fn resource(contents: ResourceContents) -> Self {
        Self::Resource {
            contents,
            annotations: None,
            meta: Map::new(),
        }
    }

    /// A resource-link content block referencing a resource by URI.
    pub fn resource_link(resource: Resource) -> Self {
        Self::ResourceLink(Box::new(resource))
    }

    /// Set this block's display annotations (builder style). On a resource
    /// link, sets the linked resource's annotations — on the wire they are the
    /// same fields.
    #[must_use]
    pub fn with_annotations(mut self, a: Annotations) -> Self {
        match &mut self {
            Self::Text { annotations, .. }
            | Self::Image { annotations, .. }
            | Self::Audio { annotations, .. }
            | Self::Resource { annotations, .. } => *annotations = Some(a),
            Self::ResourceLink(r) => r.annotations = Some(a),
        }
        self
    }

    /// Add one `_meta` entry to this block (builder style). On a resource
    /// link, adds to the linked resource's `_meta` — on the wire they are the
    /// same field.
    #[must_use]
    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        match &mut self {
            Self::Text { meta, .. }
            | Self::Image { meta, .. }
            | Self::Audio { meta, .. }
            | Self::Resource { meta, .. } => {
                meta.insert(key.into(), value);
            }
            Self::ResourceLink(r) => {
                r.meta.insert(key.into(), value);
            }
        }
        self
    }
}

/// A tool descriptor.
/// Whether a tool supports being run as an asynchronous task (`2025-11-25` core
/// Tasks). Mirrors the wire `execution.taskSupport`; the draft models Tasks as a
/// server-directed extension instead, so this rides only the legacy wire.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskSupport {
    /// The tool must not be run as a task.
    Forbidden,
    /// The tool may be run as a task at the client's request.
    Optional,
    /// The tool is always run as a task.
    Required,
}

/// A tool the server offers, as `tools/list` reports it.
///
/// The `#[tool]` macro builds these from a method signature: `input_schema`
/// and `output_schema` are constructed by generated schema derivation code, so the advertised
/// contract can't drift from the handler that serves it.
///
/// (`PartialEq` only: the schemas are [`Value`]s, which may hold floats.)
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Tool {
    /// Programmatic identifier (what `tools/call` references).
    pub name: String,
    /// Optional human-facing display name.
    pub title: Option<String>,
    /// Optional natural-language description (a hint to the model).
    pub description: Option<String>,
    /// JSON Schema object describing the tool's arguments
    /// (e.g. `{"type":"object","properties":{…}}`).
    pub input_schema: Value,
    /// Optional JSON Schema object describing the tool's structured result
    /// (`structuredContent`). Generated from a `Json<T>` return type.
    pub output_schema: Option<Value>,
    /// Per-tool `2025-11-25` task support (`#[tool(task)]`). `None` leaves it to
    /// the server's global Tasks policy; the draft wire ignores it.
    pub task_support: Option<TaskSupport>,
    /// Behavior hints for clients (`readOnlyHint`, `destructiveHint`, …).
    /// Hints, not guarantees — clients MUST NOT treat them as security
    /// boundaries. Carried losslessly on both wire versions.
    pub annotations: Option<ToolAnnotations>,
    /// Sized icons a client can display for this tool (both wire versions).
    pub icons: Vec<Icon>,
    /// Arbitrary `_meta` for this tool (namespaced keys per the spec's `_meta`
    /// conventions — e.g. server-local tags a catalog policy keys off).
    /// Empty = absent on the wire. Carried losslessly on both wire versions.
    pub meta: Map<String, Value>,
}

/// Behavior hints describing a [`Tool`] to clients (the spec's
/// `ToolAnnotations`). Every field is a **hint**, not a guarantee: clients
/// must not rely on them for safety decisions about untrusted servers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ToolAnnotations {
    /// Human-readable title (display precedence: `Tool::title`, then this,
    /// then `Tool::name`).
    pub title: Option<String>,
    /// `true` = the tool does not modify its environment (default `false`).
    pub read_only_hint: Option<bool>,
    /// `true` = updates may be destructive; `false` = additive only.
    /// Meaningful only when not read-only (default `true`).
    pub destructive_hint: Option<bool>,
    /// `true` = repeat calls with the same arguments have no additional
    /// effect. Meaningful only when not read-only (default `false`).
    pub idempotent_hint: Option<bool>,
    /// `true` = interacts with an "open world" of external entities (e.g. web
    /// search); `false` = a closed domain (default `true`).
    pub open_world_hint: Option<bool>,
}

impl ToolAnnotations {
    /// Annotations with every hint unset.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the tool read-only (does not modify its environment).
    #[must_use]
    pub fn read_only(mut self) -> Self {
        self.read_only_hint = Some(true);
        self
    }

    /// Set the destructive hint.
    #[must_use]
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive_hint = Some(destructive);
        self
    }

    /// Set the idempotent hint.
    #[must_use]
    pub fn idempotent(mut self, idempotent: bool) -> Self {
        self.idempotent_hint = Some(idempotent);
        self
    }

    /// Set the open-world hint.
    #[must_use]
    pub fn open_world(mut self, open_world: bool) -> Self {
        self.open_world_hint = Some(open_world);
        self
    }
}

/// A sized icon resource (`ui` display hint on tools, resources, prompts).
/// `src` is an HTTP(S) URL or a base64 `data:` URI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Icon {
    /// URI of the icon resource (HTTP/HTTPS or `data:`).
    pub src: String,
    /// MIME type override when the source's is missing or generic.
    pub mime_type: Option<String>,
    /// `"WxH"` strings (e.g. `"48x48"`) or `"any"`; empty = any size.
    pub sizes: Vec<String>,
    /// The color scheme this icon is designed for; `None` = suits both.
    pub theme: Option<IconTheme>,
}

impl Icon {
    /// An icon with the given source URI (any size, MIME inferred).
    pub fn new(src: impl Into<String>) -> Self {
        Self {
            src: src.into(),
            mime_type: None,
            sizes: Vec::new(),
            theme: None,
        }
    }
}

/// The color scheme an [`Icon`] is designed for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconTheme {
    /// For light backgrounds.
    Light,
    /// For dark backgrounds.
    Dark,
}

impl Tool {
    /// A tool with the given name and argument schema; no title/description.
    pub fn new(name: impl Into<String>, input_schema: Value) -> Self {
        Self {
            name: name.into(),
            title: None,
            description: None,
            input_schema,
            output_schema: None,
            task_support: None,
            annotations: None,
            icons: Vec::new(),
            meta: Map::new(),
        }
    }

    /// Set the behavior-hint annotations (builder style).
    #[must_use]
    pub fn with_annotations(mut self, annotations: ToolAnnotations) -> Self {
        self.annotations = Some(annotations);
        self
    }

    /// Add a display icon (builder style).
    #[must_use]
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icons.push(icon);
        self
    }

    /// Set a namespaced `_meta` entry (builder style) — e.g.
    /// `.with_meta_entry("com.example/tags", json!(["read"]))`.
    #[must_use]
    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        self.meta.insert(key.into(), value);
        self
    }

    /// Set the description (builder style).
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the output schema (builder style) — the JSON Schema for the tool's
    /// `structuredContent`.
    #[must_use]
    pub fn with_output_schema(mut self, output_schema: Value) -> Self {
        self.output_schema = Some(output_schema);
        self
    }

    /// Set the per-tool task support (builder style) — `#[tool(task)]`.
    #[must_use]
    pub fn with_task_support(mut self, task_support: TaskSupport) -> Self {
        self.task_support = Some(task_support);
        self
    }

    /// Set the title (builder style).
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// Result of `tools/list`.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ListToolsResult {
    /// The tools offered.
    pub tools: Vec<Tool>,
    /// Opaque pagination cursor; `Some` means more pages follow.
    pub next_cursor: Option<String>,
    /// Cache policy (SEP-2549); `None` = the server's configured default.
    pub cache: Option<CachePolicy>,
}

impl ListToolsResult {
    /// A single-page result over `tools`.
    pub fn new(tools: Vec<Tool>) -> Self {
        Self {
            tools,
            next_cursor: None,
            cache: None,
        }
    }

    /// Set this result's cache policy (wins over the server default).
    #[must_use]
    pub fn with_cache(mut self, cache: CachePolicy) -> Self {
        self.cache = Some(cache);
        self
    }
}

impl Cacheable for ListToolsResult {
    fn cache_policy_mut(&mut self) -> &mut Option<CachePolicy> {
        &mut self.cache
    }
}

/// Decoded `tools/call` arguments (the framework strips wire `_meta` into
/// `RequestContext` before the handler sees this).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct CallToolParams {
    /// Name of the tool to invoke.
    pub name: String,
    /// Arguments object (may be empty).
    pub arguments: Map<String, Value>,
}

impl CallToolParams {
    /// Construct from a tool name and arguments object.
    pub fn new(name: impl Into<String>, arguments: Map<String, Value>) -> Self {
        Self {
            name: name.into(),
            arguments,
        }
    }
}

/// Result of `tools/call`. Per spec, tool-level failure is `is_error`, *not* a
/// JSON-RPC error (so the model can see and self-correct).
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct CallToolResult {
    /// Unstructured result content.
    pub content: Vec<Content>,
    /// `true` if the tool itself failed.
    pub is_error: bool,
    /// Optional structured result conforming to the tool's output schema.
    pub structured_content: Option<Value>,
}

impl CallToolResult {
    /// A successful result carrying the given content blocks (text, image, audio).
    pub fn new(content: Vec<Content>) -> Self {
        Self {
            content,
            is_error: false,
            structured_content: None,
        }
    }

    /// A successful result carrying a single text block.
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            content: alloc::vec![Content::text(s)],
            is_error: false,
            structured_content: None,
        }
    }

    /// A failed result (`is_error = true`) carrying a single text block.
    pub fn error(s: impl Into<String>) -> Self {
        Self {
            content: alloc::vec![Content::text(s)],
            is_error: true,
            structured_content: None,
        }
    }
}

// ---- pagination ---------------------------------------------------------------

/// Inbound parameters shared by every `*/list` method: an opaque pagination
/// cursor (absent on the first page). The framework decodes it from the wire;
/// handlers echo a `next_cursor` in their result to advertise another page.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ListParams {
    /// Cursor returned by a previous page, or `None` for the first page.
    pub cursor: Option<String>,
}

impl ListParams {
    /// First-page request (no cursor).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Request continuing from `cursor`.
    #[must_use]
    pub fn with_cursor(cursor: impl Into<String>) -> Self {
        Self {
            cursor: Some(cursor.into()),
        }
    }

    /// Refuse a cursor on a listing that is always one unbounded page.
    ///
    /// "Invalid cursors **SHOULD** result in an error with code -32602." A
    /// listing that never sets `next_cursor` never issued one, so any cursor
    /// it is handed is invalid by construction — returning the full list
    /// instead makes a client's paging loop look like it worked and quietly
    /// re-reads page one forever.
    ///
    /// `listing` names the method for the error message.
    ///
    /// # Errors
    /// [`McpError::InvalidParams`] when a cursor is present.
    pub fn reject_unknown_cursor(&self, listing: &str) -> Result<(), McpError> {
        match &self.cursor {
            None => Ok(()),
            Some(cursor) => Err(McpError::invalid_params(alloc::format!(
                "unknown cursor `{cursor}`: {listing} returns a single page and \
                 issues no cursor"
            ))),
        }
    }
}

// ---- subscriptions (`subscriptions/listen`, 2026-07-28) ------------------------

/// What a client asks to be notified about on a `subscriptions/listen` stream.
///
/// Draft-only: `2026-07-28` replaced both `resources/subscribe` and the HTTP
/// GET stream with one long-lived subscription carrying this filter. The
/// server intersects it with the capabilities it actually registered and
/// reports the agreed subset in the acknowledgement, so asking for something
/// unsupported narrows the subscription rather than failing it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SubscriptionFilter {
    /// Notify on `notifications/tools/list_changed`.
    pub tools_list_changed: bool,
    /// Notify on `notifications/resources/list_changed`.
    pub resources_list_changed: bool,
    /// Notify on `notifications/prompts/list_changed`.
    pub prompts_list_changed: bool,
    /// Notify on `notifications/resources/updated` for these URIs — the
    /// replacement for `2025-11-25`'s `resources/subscribe`.
    pub resource_subscriptions: Vec<String>,
}

impl SubscriptionFilter {
    /// An empty filter — subscribe to nothing, then opt in.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribe to every `*_list_changed` notification.
    #[must_use]
    pub fn all_list_changed() -> Self {
        Self {
            tools_list_changed: true,
            resources_list_changed: true,
            prompts_list_changed: true,
            resource_subscriptions: Vec::new(),
        }
    }

    /// Also watch `uri` for `notifications/resources/updated`.
    #[must_use]
    pub fn with_resource(mut self, uri: impl Into<String>) -> Self {
        self.resource_subscriptions.push(uri.into());
        self
    }
}

// ---- caching (SEP-2549) --------------------------------------------------------

/// Whether shared intermediaries may cache a response (SEP-2549).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CacheScope {
    /// Only the requesting client may cache the response.
    Private,
    /// Shared intermediaries may cache the response.
    Public,
}

/// A freshness declaration for a cacheable result (SEP-2549): `ttl_ms` is the
/// server's freshness hint in milliseconds (`0` = immediately stale — clients
/// should not cache), `scope` controls shared-intermediary caching. Carried on
/// the `2026-07-28` wire (`ttlMs`/`cacheScope`, required on every cacheable
/// result); the `2025-11-25` wire has no cache fields, so the policy is
/// dropped there.
///
/// Servers configure a default per capability via `ServerBuilder::cache_policy`
/// (in `turbomcp-server`); a handler-set policy on a result wins over that
/// default. Complements — never replaces — the `*_list_changed` notifications.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CachePolicy {
    /// Freshness hint in milliseconds; `0` means immediately stale.
    pub ttl_ms: u64,
    /// Who may cache the response.
    pub scope: CacheScope,
}

impl CachePolicy {
    /// Immediately stale + private: the conservative wire default.
    pub const NO_CACHE: CachePolicy = CachePolicy {
        ttl_ms: 0,
        scope: CacheScope::Private,
    };

    /// A private (client-only) cache window of `ttl`.
    #[must_use]
    pub fn private(ttl: core::time::Duration) -> Self {
        Self {
            ttl_ms: u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX),
            scope: CacheScope::Private,
        }
    }

    /// A public (shared-intermediary) cache window of `ttl`.
    #[must_use]
    pub fn public(ttl: core::time::Duration) -> Self {
        Self {
            ttl_ms: u64::try_from(ttl.as_millis()).unwrap_or(u64::MAX),
            scope: CacheScope::Public,
        }
    }

    /// Build from the wire fields (`ttlMs` + a decoded [`CacheScope`]).
    #[must_use]
    pub fn from_wire(ttl_ms: u64, scope: CacheScope) -> Self {
        Self { ttl_ms, scope }
    }
}

/// A neutral result that carries a [`CachePolicy`] (the SEP-2549
/// `CacheableResult` surface: the four `*/list` results plus
/// `resources/read`). The dispatcher uses this to fill the server's configured
/// default when a handler didn't set one.
pub trait Cacheable {
    /// The result's cache policy slot (`None` = use the server default).
    fn cache_policy_mut(&mut self) -> &mut Option<CachePolicy>;
}

// ---- resources ----------------------------------------------------------------

/// A resource descriptor (`resources/list`).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct Resource {
    /// The resource URI (what `resources/read` references).
    pub uri: String,
    /// Programmatic identifier / fallback display name.
    pub name: String,
    /// Optional human-facing display name.
    pub title: Option<String>,
    /// Optional natural-language description (a hint to the model).
    pub description: Option<String>,
    /// MIME type, if known.
    pub mime_type: Option<String>,
    /// Raw content size in bytes (before any encoding), if known.
    pub size: Option<u64>,
    /// Display/consumption annotations (audience, priority, last-modified).
    pub annotations: Option<Annotations>,
    /// Sized icons a client can display for this resource.
    pub icons: Vec<Icon>,
    /// Arbitrary namespaced `_meta` (empty = absent on the wire).
    pub meta: Map<String, Value>,
}

impl Resource {
    /// A resource with the given URI and name.
    pub fn new(uri: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            name: name.into(),
            title: None,
            description: None,
            mime_type: None,
            size: None,
            annotations: None,
            icons: Vec::new(),
            meta: Map::new(),
        }
    }

    /// Set the title (builder style).
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the description (builder style).
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the MIME type (builder style).
    #[must_use]
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }

    /// Set the display/consumption annotations (builder style).
    #[must_use]
    pub fn with_annotations(mut self, annotations: Annotations) -> Self {
        self.annotations = Some(annotations);
        self
    }

    /// Add a display icon (builder style).
    #[must_use]
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icons.push(icon);
        self
    }

    /// Set a namespaced `_meta` entry (builder style).
    #[must_use]
    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        self.meta.insert(key.into(), value);
        self
    }
}

/// Display/consumption annotations on resources and resource templates (the
/// spec's `Annotations`): who the item is for, how important it is, and when
/// it last changed. All advisory.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Annotations {
    /// Intended audience(s); empty = unspecified.
    pub audience: Vec<Role>,
    /// Importance, `0.0` (least) ..= `1.0` (most important).
    pub priority: Option<f64>,
    /// ISO 8601 timestamp of the last modification.
    pub last_modified: Option<String>,
}

impl Annotations {
    /// Annotations with every field unset.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an intended audience role.
    #[must_use]
    pub fn for_audience(mut self, role: Role) -> Self {
        self.audience.push(role);
        self
    }

    /// Set the priority (`0.0` ..= `1.0`).
    #[must_use]
    pub fn priority(mut self, priority: f64) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Set the last-modified timestamp (ISO 8601).
    #[must_use]
    pub fn last_modified(mut self, when: impl Into<String>) -> Self {
        self.last_modified = Some(when.into());
        self
    }
}

/// The contents of a read resource (`resources/read`): UTF-8 text or a
/// base64-encoded binary blob. `#[non_exhaustive]` (enum and variants) so
/// future kinds and per-variant spec fields slot in — construct via
/// [`ResourceContents::text`]/[`blob`](ResourceContents::blob) and match with
/// `..`.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum ResourceContents {
    /// Text contents.
    #[non_exhaustive]
    Text {
        /// URI these contents belong to.
        uri: String,
        /// MIME type, if known.
        mime_type: Option<String>,
        /// The text.
        text: String,
        /// Arbitrary `_meta` for these contents (namespaced keys per the
        /// spec's `_meta` conventions — e.g. the Apps extension's `_meta.ui`).
        /// Empty = absent on the wire.
        meta: Map<String, Value>,
    },
    /// Binary contents, base64-encoded.
    #[non_exhaustive]
    Blob {
        /// URI these contents belong to.
        uri: String,
        /// MIME type, if known.
        mime_type: Option<String>,
        /// Base64-encoded bytes.
        blob: String,
        /// Arbitrary `_meta` for these contents. Empty = absent on the wire.
        meta: Map<String, Value>,
    },
}

impl ResourceContents {
    /// Text contents for `uri` (no MIME type).
    pub fn text(uri: impl Into<String>, text: impl Into<String>) -> Self {
        Self::Text {
            uri: uri.into(),
            mime_type: None,
            text: text.into(),
            meta: Map::new(),
        }
    }

    /// Base64 binary contents for `uri` (no MIME type).
    pub fn blob(uri: impl Into<String>, blob: impl Into<String>) -> Self {
        Self::Blob {
            uri: uri.into(),
            mime_type: None,
            blob: blob.into(),
            meta: Map::new(),
        }
    }

    /// Set the MIME type on either variant (builder style).
    #[must_use]
    pub fn with_mime_type(mut self, mime: impl Into<String>) -> Self {
        match &mut self {
            Self::Text { mime_type, .. } | Self::Blob { mime_type, .. } => {
                *mime_type = Some(mime.into());
            }
        }
        self
    }

    /// Add one `_meta` entry to either variant (builder style).
    #[must_use]
    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        match &mut self {
            Self::Text { meta, .. } | Self::Blob { meta, .. } => {
                meta.insert(key.into(), value);
            }
        }
        self
    }
}

/// Result of `resources/list`.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ListResourcesResult {
    /// The resources offered.
    pub resources: Vec<Resource>,
    /// Opaque pagination cursor; `Some` means more pages follow.
    pub next_cursor: Option<String>,
    /// Cache policy (SEP-2549); `None` = the server's configured default.
    pub cache: Option<CachePolicy>,
}

impl ListResourcesResult {
    /// A single-page result over `resources`.
    pub fn new(resources: Vec<Resource>) -> Self {
        Self {
            resources,
            next_cursor: None,
            cache: None,
        }
    }

    /// Set this result's cache policy (wins over the server default).
    #[must_use]
    pub fn with_cache(mut self, cache: CachePolicy) -> Self {
        self.cache = Some(cache);
        self
    }
}

impl Cacheable for ListResourcesResult {
    fn cache_policy_mut(&mut self) -> &mut Option<CachePolicy> {
        &mut self.cache
    }
}

/// Result of `resources/read`.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ReadResourceResult {
    /// One or more content items (a single resource may expand to several).
    pub contents: Vec<ResourceContents>,
    /// Cache policy (SEP-2549); `None` = the server's configured default.
    pub cache: Option<CachePolicy>,
}

impl ReadResourceResult {
    /// A result carrying the given contents.
    pub fn new(contents: Vec<ResourceContents>) -> Self {
        Self {
            contents,
            cache: None,
        }
    }

    /// Fill in `mime_type` on any contents that did not set one.
    ///
    /// `#[resource(mime_type = "…")]` reached `resources/list` but never the
    /// read, so a client that consulted the catalogue and then read the
    /// resource got two different answers: the declared type and nothing.
    /// A handler that set its own wins — the declaration is the default, not
    /// an override.
    #[must_use]
    pub fn with_default_mime_type(mut self, mime_type: &str) -> Self {
        for contents in &mut self.contents {
            let slot = match contents {
                ResourceContents::Text { mime_type, .. }
                | ResourceContents::Blob { mime_type, .. } => mime_type,
            };
            if slot.is_none() {
                *slot = Some(mime_type.into());
            }
        }
        self
    }

    /// A result carrying a single text item.
    pub fn text(uri: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            contents: alloc::vec![ResourceContents::text(uri, text)],
            cache: None,
        }
    }

    /// Set this result's cache policy (wins over the server default).
    #[must_use]
    pub fn with_cache(mut self, cache: CachePolicy) -> Self {
        self.cache = Some(cache);
        self
    }
}

impl Cacheable for ReadResourceResult {
    fn cache_policy_mut(&mut self) -> &mut Option<CachePolicy> {
        &mut self.cache
    }
}

/// A resource template (`resources/templates/list`): a URI Template (RFC 6570)
/// describing a family of resources.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ResourceTemplate {
    /// URI Template (e.g. `file://{path}`).
    pub uri_template: String,
    /// Programmatic identifier / fallback display name.
    pub name: String,
    /// Optional human-facing display name.
    pub title: Option<String>,
    /// Optional natural-language description.
    pub description: Option<String>,
    /// MIME type shared by all matching resources, if uniform.
    pub mime_type: Option<String>,
    /// Display/consumption annotations (audience, priority, last-modified).
    pub annotations: Option<Annotations>,
    /// Sized icons a client can display for matching resources.
    pub icons: Vec<Icon>,
    /// Arbitrary namespaced `_meta` (empty = absent on the wire).
    pub meta: Map<String, Value>,
}

impl ResourceTemplate {
    /// A template with the given URI Template and name.
    pub fn new(uri_template: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            uri_template: uri_template.into(),
            name: name.into(),
            title: None,
            description: None,
            mime_type: None,
            annotations: None,
            icons: Vec::new(),
            meta: Map::new(),
        }
    }

    /// Set the display/consumption annotations (builder style).
    #[must_use]
    pub fn with_annotations(mut self, annotations: Annotations) -> Self {
        self.annotations = Some(annotations);
        self
    }

    /// Add a display icon (builder style).
    #[must_use]
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icons.push(icon);
        self
    }

    /// Set a namespaced `_meta` entry (builder style).
    #[must_use]
    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        self.meta.insert(key.into(), value);
        self
    }

    /// Set the description (builder style).
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the title (builder style).
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set the MIME type (builder style).
    #[must_use]
    pub fn with_mime_type(mut self, mime_type: impl Into<String>) -> Self {
        self.mime_type = Some(mime_type.into());
        self
    }
}

/// Result of `resources/templates/list`.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ListResourceTemplatesResult {
    /// The templates offered.
    pub resource_templates: Vec<ResourceTemplate>,
    /// Opaque pagination cursor; `Some` means more pages follow.
    pub next_cursor: Option<String>,
    /// Cache policy (SEP-2549); `None` = the server's configured default.
    pub cache: Option<CachePolicy>,
}

impl ListResourceTemplatesResult {
    /// A single-page result over `resource_templates`.
    pub fn new(resource_templates: Vec<ResourceTemplate>) -> Self {
        Self {
            resource_templates,
            next_cursor: None,
            cache: None,
        }
    }

    /// Set this result's cache policy (wins over the server default).
    #[must_use]
    pub fn with_cache(mut self, cache: CachePolicy) -> Self {
        self.cache = Some(cache);
        self
    }
}

impl Cacheable for ListResourceTemplatesResult {
    fn cache_policy_mut(&mut self) -> &mut Option<CachePolicy> {
        &mut self.cache
    }
}

/// `resources/read` parameters (the framework strips wire `_meta` into
/// `RequestContext` before the handler sees this).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ReadResourceParams {
    /// URI of the resource to read.
    pub uri: String,
}

impl ReadResourceParams {
    /// Construct from a URI.
    pub fn new(uri: impl Into<String>) -> Self {
        Self { uri: uri.into() }
    }
}

// ---- prompts ------------------------------------------------------------------

/// Who authored a prompt message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The end user.
    User,
    /// The model.
    Assistant,
}

/// A declared prompt argument (used for templating and completion).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct PromptArgument {
    /// Programmatic identifier / fallback display name.
    pub name: String,
    /// Optional human-facing display name.
    pub title: Option<String>,
    /// Optional natural-language description.
    pub description: Option<String>,
    /// Whether the argument must be provided.
    pub required: bool,
}

impl PromptArgument {
    /// An optional argument with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: None,
            description: None,
            required: false,
        }
    }

    /// Mark the argument required (builder style).
    #[must_use]
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Set the description (builder style).
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the title (builder style).
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
}

/// A prompt descriptor (`prompts/list`).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Prompt {
    /// Programmatic identifier (what `prompts/get` references).
    pub name: String,
    /// Optional human-facing display name.
    pub title: Option<String>,
    /// Optional natural-language description.
    pub description: Option<String>,
    /// Declared arguments for templating.
    pub arguments: Vec<PromptArgument>,
    /// Sized icons a client can display for this prompt.
    pub icons: Vec<Icon>,
    /// Arbitrary namespaced `_meta` (empty = absent on the wire).
    pub meta: Map<String, Value>,
}

impl Prompt {
    /// A prompt with the given name and no arguments.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: None,
            description: None,
            arguments: Vec::new(),
            icons: Vec::new(),
            meta: Map::new(),
        }
    }

    /// Add a display icon (builder style).
    #[must_use]
    pub fn with_icon(mut self, icon: Icon) -> Self {
        self.icons.push(icon);
        self
    }

    /// Set a namespaced `_meta` entry (builder style).
    #[must_use]
    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        self.meta.insert(key.into(), value);
        self
    }

    /// Set the description (builder style).
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the title (builder style).
    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Append a declared argument (builder style).
    #[must_use]
    pub fn with_argument(mut self, argument: PromptArgument) -> Self {
        self.arguments.push(argument);
        self
    }
}

/// A single message in a rendered prompt.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct PromptMessage {
    /// Who authored the message.
    pub role: Role,
    /// The message content.
    pub content: Content,
}

impl PromptMessage {
    /// A user-authored message.
    pub fn user(content: Content) -> Self {
        Self {
            role: Role::User,
            content,
        }
    }

    /// An assistant-authored message.
    pub fn assistant(content: Content) -> Self {
        Self {
            role: Role::Assistant,
            content,
        }
    }

    /// A user-authored text message.
    pub fn user_text(text: impl Into<String>) -> Self {
        Self::user(Content::text(text))
    }

    /// An assistant-authored text message.
    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self::assistant(Content::text(text))
    }
}

/// Result of `prompts/list`.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct ListPromptsResult {
    /// The prompts offered.
    pub prompts: Vec<Prompt>,
    /// Opaque pagination cursor; `Some` means more pages follow.
    pub next_cursor: Option<String>,
    /// Cache policy (SEP-2549); `None` = the server's configured default.
    pub cache: Option<CachePolicy>,
}

impl ListPromptsResult {
    /// A single-page result over `prompts`.
    pub fn new(prompts: Vec<Prompt>) -> Self {
        Self {
            prompts,
            next_cursor: None,
            cache: None,
        }
    }

    /// Set this result's cache policy (wins over the server default).
    #[must_use]
    pub fn with_cache(mut self, cache: CachePolicy) -> Self {
        self.cache = Some(cache);
        self
    }
}

impl Cacheable for ListPromptsResult {
    fn cache_policy_mut(&mut self) -> &mut Option<CachePolicy> {
        &mut self.cache
    }
}

/// Result of `prompts/get`: a rendered prompt as a message sequence.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct GetPromptResult {
    /// Optional description of the rendered prompt.
    pub description: Option<String>,
    /// The messages.
    pub messages: Vec<PromptMessage>,
}

impl GetPromptResult {
    /// A result carrying the given messages.
    pub fn new(messages: Vec<PromptMessage>) -> Self {
        Self {
            description: None,
            messages,
        }
    }

    /// Set the description (builder style).
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// `prompts/get` parameters.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct GetPromptParams {
    /// Name of the prompt to render.
    pub name: String,
    /// Templating arguments (string→string per spec).
    pub arguments: BTreeMap<String, String>,
}

impl GetPromptParams {
    /// Construct from a prompt name and arguments.
    pub fn new(name: impl Into<String>, arguments: BTreeMap<String, String>) -> Self {
        Self {
            name: name.into(),
            arguments,
        }
    }
}

// ---- elicitation (client interaction) ------------------------------------------

/// What a handler asks the user for via `ctx.client.elicit(…)` (form mode).
///
/// On the draft this is packaged into an `InputRequiredResult` (MRTR,
/// SEP-2322); on `2025-11-25` it goes out as an inline `elicitation/create`
/// request. For URL-mode (out-of-band) elicitation see [`ElicitUrlParams`].
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ElicitParams {
    /// The message presented to the user describing what is being requested.
    pub message: String,
    /// The requested form schema — the spec's restricted JSON Schema subset
    /// (top-level primitive properties only, no nesting).
    pub requested_schema: Value,
}

impl ElicitParams {
    /// An elicitation showing `message` and requesting `requested_schema`.
    pub fn new(message: impl Into<String>, requested_schema: Value) -> Self {
        Self {
            message: message.into(),
            requested_schema,
        }
    }

    /// Check [`requested_schema`](Self::requested_schema) against the spec's
    /// restricted subset: a flat object whose properties are primitives, plus
    /// the one array form (a string-enum multi-select).
    ///
    /// The server runs this before sending. A client cannot render what the
    /// subset excludes, so a nested object comes back as an empty form with
    /// nothing said about why — naming the property here is the difference
    /// between a two-minute fix and an afternoon.
    ///
    /// # Errors
    /// A description of the first property outside the subset.
    pub fn validate(&self) -> Result<(), String> {
        validate_requested_schema(&self.requested_schema)
    }
}

/// A URL-mode elicitation (`mode: "url"`): the client shows `message` and
/// directs the user to `url` (e.g. an OAuth consent page); the response carries
/// an [`ElicitAction`] but no form content.
///
/// `elicitation_id` is a server-unique opaque id required by **both** wires
/// (the draft briefly dropped it; the 2026-07-28 RC restored it as required,
/// pairing it with `notifications/elicitation/complete`). The server mints
/// one when unset.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ElicitUrlParams {
    /// The message explaining why the interaction is needed.
    pub message: String,
    /// A server-unique opaque id (minted when unset; see the type docs).
    pub elicitation_id: Option<String>,
    /// The URL the user should navigate to.
    pub url: String,
}

impl ElicitUrlParams {
    /// A URL-mode elicitation showing `message` and directing the user to `url`.
    pub fn new(message: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            elicitation_id: None,
            url: url.into(),
        }
    }

    /// Set an explicit elicitation id (one is minted when unset). Clients
    /// treat it as opaque; `notifications/elicitation/complete` references it.
    #[must_use]
    pub fn with_elicitation_id(mut self, id: impl Into<String>) -> Self {
        self.elicitation_id = Some(id.into());
        self
    }
}

/// The user's action in response to an elicitation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElicitAction {
    /// The user submitted the form / confirmed the action.
    Accept,
    /// The user explicitly declined.
    Decline,
    /// The user dismissed without an explicit choice.
    Cancel,
}

/// The primitive JSON Schema types a form-mode elicitation may request.
///
/// "Form mode elicitation schemas are limited to flat objects with primitive
/// properties only … complex nested structures, arrays of objects (beyond
/// enums), and other advanced JSON Schema features are intentionally not
/// supported to simplify client user experience."
const ELICIT_PRIMITIVES: [&str; 4] = ["string", "number", "integer", "boolean"];

/// Check a form-mode `requestedSchema` against the spec's restricted subset.
///
/// A client cannot render what the subset excludes, so a server that sends a
/// nested object gets back an empty form and no explanation. Catching it where
/// the request is built names the offending property instead.
///
/// # Errors
/// A description of the first property that falls outside the subset.
fn validate_requested_schema(schema: &Value) -> Result<(), String> {
    // The wire type makes both `type` and `properties` required, so this is
    // the schema's own floor rather than an extra rule.
    if schema.get("type").and_then(Value::as_str) != Some("object") {
        return Err("requestedSchema must be an object schema (`type: \"object\"`)".into());
    }
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return Err("requestedSchema must carry a `properties` object".into());
    };
    for (name, property) in properties {
        let Some(ty) = property.get("type").and_then(Value::as_str) else {
            return Err(alloc::format!(
                "requestedSchema property `{name}` has no `type`; the form subset \
                 has no place for a `$ref`, `oneOf` or an untyped property"
            ));
        };
        if ELICIT_PRIMITIVES.contains(&ty) {
            continue;
        }
        // The one non-primitive the subset allows: a multi-select, which is an
        // array whose items are a string enum.
        if ty == "array" {
            // SEP-1330 spells a multi-select two ways: a plain string `enum`,
            // or an `anyOf` of `{const, title}` alternatives when the options
            // need display labels. Both are still a closed list of scalars,
            // which is what keeps the form renderable.
            if property.get("items").is_some_and(is_enum_items) {
                continue;
            }
            return Err(alloc::format!(
                "requestedSchema property `{name}` is an array, which the form \
                 subset allows only as a multi-select (`items` must be a string \
                 `enum`, or an `anyOf` of `const` alternatives)"
            ));
        }
        return Err(alloc::format!(
            "requestedSchema property `{name}` has type `{ty}`; the form subset \
             is flat and primitive ({ELICIT_PRIMITIVES:?}, plus a string-enum array)"
        ));
    }
    Ok(())
}

/// Whether a multi-select's `items` is a closed list of scalars, in either
/// shape SEP-1330 defines.
fn is_enum_items(items: &Value) -> bool {
    let plain_enum =
        items.get("type").and_then(Value::as_str) == Some("string") && items.get("enum").is_some();
    let labelled = items
        .get("anyOf")
        .and_then(Value::as_array)
        .is_some_and(|alts| !alts.is_empty() && alts.iter().all(|a| a.get("const").is_some()));
    plain_enum || labelled
}

/// What the client answered to an elicitation.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct ElicitOutcome {
    /// The user's action.
    pub action: ElicitAction,
    /// Submitted form values (present only on `Accept` in form mode).
    pub content: Map<String, Value>,
}

impl ElicitOutcome {
    /// An outcome with the given action and submitted content.
    #[must_use]
    pub fn new(action: ElicitAction, content: Map<String, Value>) -> Self {
        Self { action, content }
    }

    /// Whether the user accepted.
    #[must_use]
    pub fn accepted(&self) -> bool {
        self.action == ElicitAction::Accept
    }
}

// ---- roots (`roots/list`) ------------------------------------------------------

/// A filesystem boundary the client exposes to the server.
///
/// "This **MUST** be a `file://` URI" — the one shape rule the roots spec
/// states, and the reason parsing goes through [`Root::from_wire`] rather than
/// a bare deserialize: a root is a *permission* statement, so a server that
/// accepted `https://…` or a bare path would be acting on a boundary the
/// client never drew.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Root {
    /// The root's `file://` URI.
    pub uri: String,
    /// Optional human-readable name.
    pub name: Option<String>,
    /// Arbitrary `_meta`. Empty = absent on the wire.
    pub meta: Map<String, Value>,
}

impl Root {
    /// A root at `uri`, which must be a `file://` URI — anything else is
    /// `None`, because there is no sound way to interpret it.
    pub fn new(uri: impl Into<String>) -> Option<Self> {
        let uri = uri.into();
        uri.starts_with("file://").then_some(Self {
            uri,
            name: None,
            meta: Map::new(),
        })
    }

    /// Set the display name (builder style).
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Parse one wire root, rejecting a non-`file://` URI.
    #[must_use]
    pub fn from_wire(value: &Value) -> Option<Self> {
        let root = Self::new(value.get("uri")?.as_str()?)?;
        Some(Self {
            name: value.get("name").and_then(Value::as_str).map(Into::into),
            meta: value
                .get("_meta")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            ..root
        })
    }

    /// Parse a `roots/list` result, dropping entries that are not `file://`
    /// URIs. Dropping rather than failing is deliberate: one malformed root
    /// should not cost the handler the roots the client got right.
    #[must_use]
    pub fn list_from_wire(value: &Value) -> Vec<Self> {
        value
            .get("roots")
            .and_then(Value::as_array)
            .map(|roots| roots.iter().filter_map(Self::from_wire).collect())
            .unwrap_or_default()
    }

    /// Render a `roots/list` result from a set of roots.
    #[must_use]
    pub fn list_to_wire(roots: &[Self]) -> Value {
        let mut out = Map::new();
        out.insert(
            "roots".into(),
            Value::Array(roots.iter().map(Self::to_wire).collect()),
        );
        Value::Object(out)
    }

    /// Render one root.
    #[must_use]
    pub fn to_wire(&self) -> Value {
        let mut out = Map::new();
        out.insert("uri".into(), Value::String(self.uri.clone()));
        if let Some(n) = &self.name {
            out.insert("name".into(), Value::String(n.clone()));
        }
        if !self.meta.is_empty() {
            out.insert("_meta".into(), Value::Object(self.meta.clone()));
        }
        Value::Object(out)
    }
}

// ---- sampling (`sampling/createMessage`) ---------------------------------------

/// Why a sampling conversation could not be put on a wire.
///
/// Sampling is the one neutral family whose rendering is fallible, and
/// deliberately so. `2025-06-18` predates multi-block messages and agentic
/// sampling entirely; quietly dropping a `ToolUse` block to fit would hand the
/// model a conversation with a hole in it, and neither end could tell. The
/// spec's two tool-use MUSTs are checked here for the same reason — a
/// half-answered tool call is not something a provider API can be handed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SamplingError {
    /// The target revision's schema has no shape for this.
    Unsupported {
        /// What could not be rendered.
        feature: String,
        /// The revision it was being rendered for.
        version: ProtocolVersion,
    },
    /// The conversation breaks one of the spec's tool-use rules.
    Invalid(String),
}

impl core::fmt::Display for SamplingError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Unsupported { feature, version } => {
                write!(f, "{feature} cannot be represented on {version}")
            }
            Self::Invalid(why) => f.write_str(why),
        }
    }
}

impl core::error::Error for SamplingError {}

/// A tool call the model wants to make (`ToolUseContent`).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct ToolUse {
    /// Correlation id; the answering [`ToolResult`] repeats it as
    /// [`tool_use_id`](ToolResult::tool_use_id).
    pub id: String,
    /// The tool to call.
    pub name: String,
    /// Arguments, conforming to the tool's input schema.
    pub input: Map<String, Value>,
    /// Arbitrary `_meta`; clients SHOULD carry it into subsequent turns, since
    /// providers key their prompt caches off it. Empty = absent on the wire.
    pub meta: Map<String, Value>,
}

impl ToolUse {
    /// A call to `name`, correlated by `id`, with no arguments yet.
    pub fn new(id: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input: Map::new(),
            meta: Map::new(),
        }
    }

    /// Set the call arguments (builder style).
    #[must_use]
    pub fn with_input(mut self, input: Map<String, Value>) -> Self {
        self.input = input;
        self
    }

    /// Add one `_meta` entry (builder style).
    #[must_use]
    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        self.meta.insert(key.into(), value);
        self
    }
}

/// The outcome of a [`ToolUse`], fed back into the next turn
/// (`ToolResultContent`).
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct ToolResult {
    /// The [`ToolUse::id`] this answers.
    pub tool_use_id: String,
    /// Unstructured result content, the same shape a `tools/call` returns.
    pub content: Vec<Content>,
    /// Structured result, conforming to the tool's `outputSchema` if it has one.
    pub structured_content: Map<String, Value>,
    /// Whether the call failed; the content then describes the failure.
    pub is_error: Option<bool>,
    /// Arbitrary `_meta`, preserved across turns like [`ToolUse::meta`].
    pub meta: Map<String, Value>,
}

impl ToolResult {
    /// A successful result for the call with this id.
    pub fn new(tool_use_id: impl Into<String>, content: Vec<Content>) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            content,
            ..Self::default()
        }
    }

    /// A failed result: `is_error` set, with `message` as the content.
    pub fn error(tool_use_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            tool_use_id: tool_use_id.into(),
            content: alloc::vec![Content::text(message)],
            is_error: Some(true),
            ..Self::default()
        }
    }

    /// Attach a structured result (builder style).
    #[must_use]
    pub fn with_structured_content(mut self, structured: Map<String, Value>) -> Self {
        self.structured_content = structured;
        self
    }
}

/// A block inside a [`SamplingMessage`].
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum SamplingContent {
    /// Text, image or audio. `SamplingMessageContentBlock` is those three plus
    /// the two tool blocks below — an embedded resource or a resource link is
    /// not in the union on any revision, so rendering one is a
    /// [`SamplingError`] rather than a block no client can parse.
    Media(Content),
    /// The model asked to call a tool. `2025-11-25` and later.
    ToolUse(ToolUse),
    /// The outcome of a tool call. `2025-11-25` and later. Boxed: it carries a
    /// whole content vector, which would otherwise size every text block in
    /// every message.
    ToolResult(Box<ToolResult>),
}

impl SamplingContent {
    /// A text block.
    pub fn text(s: impl Into<String>) -> Self {
        Self::Media(Content::text(s))
    }

    /// An image block from base64 data and a MIME type.
    pub fn image(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Media(Content::image(data, mime_type))
    }

    /// An audio block from base64 data and a MIME type.
    pub fn audio(data: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self::Media(Content::audio(data, mime_type))
    }

    /// A tool-call block.
    pub fn tool_use(call: ToolUse) -> Self {
        Self::ToolUse(call)
    }

    /// A tool-result block.
    pub fn tool_result(result: ToolResult) -> Self {
        Self::ToolResult(Box::new(result))
    }

    /// The call id, if this is a [`ToolUse`].
    #[must_use]
    pub fn tool_use_id(&self) -> Option<&str> {
        match self {
            Self::ToolUse(u) => Some(&u.id),
            _ => None,
        }
    }

    /// The id this answers, if this is a [`ToolResult`].
    #[must_use]
    pub fn answers_tool_use(&self) -> Option<&str> {
        match self {
            Self::ToolResult(r) => Some(&r.tool_use_id),
            _ => None,
        }
    }
}

/// One message in a sampling conversation.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct SamplingMessage {
    /// Who authored it.
    pub role: Role,
    /// Its content blocks. A lone block renders as a bare object — the only
    /// shape `2025-06-18` accepts, and one every later client reads too —
    /// while several render as an array.
    pub content: Vec<SamplingContent>,
    /// Arbitrary `_meta`. `2025-11-25` and later; empty = absent on the wire.
    pub meta: Map<String, Value>,
}

impl SamplingMessage {
    /// A message carrying the given blocks.
    #[must_use]
    pub fn new(role: Role, content: Vec<SamplingContent>) -> Self {
        Self {
            role,
            content,
            meta: Map::new(),
        }
    }

    /// A message carrying one text block.
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self::new(role, alloc::vec![SamplingContent::text(text)])
    }

    /// Add one `_meta` entry (builder style).
    #[must_use]
    pub fn with_meta_entry(mut self, key: impl Into<String>, value: Value) -> Self {
        self.meta.insert(key.into(), value);
        self
    }
}

/// How much surrounding MCP context to attach to the prompt.
///
/// [`ThisServer`](Self::ThisServer) and [`AllServers`](Self::AllServers) are
/// soft-deprecated: servers SHOULD NOT send them unless the client declared
/// `sampling.context`, which the server-side handle enforces before the
/// request leaves.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IncludeContext {
    /// No extra context. The default, and the only value that is safe to send
    /// to a client which declared bare `sampling`.
    #[default]
    None,
    /// Context from the calling server only.
    ThisServer,
    /// Context from every server the client is connected to.
    AllServers,
}

impl IncludeContext {
    /// The wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::ThisServer => "thisServer",
            Self::AllServers => "allServers",
        }
    }

    /// Parse a wire string; unknown values are `None`.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "none" => Some(Self::None),
            "thisServer" => Some(Self::ThisServer),
            "allServers" => Some(Self::AllServers),
            _ => None,
        }
    }
}

/// How the model may use the offered tools.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ToolChoice {
    /// The model decides. The wire default.
    #[default]
    Auto,
    /// The model must use at least one tool before finishing.
    Required,
    /// The model must not use any tool.
    None,
}

impl ToolChoice {
    /// The wire string.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Required => "required",
            Self::None => "none",
        }
    }

    /// Parse a wire string; unknown values are `None`.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "required" => Some(Self::Required),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

/// Server hints for which model to pick. Advisory — the client MAY ignore them.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct ModelPreferences {
    /// Model-name substrings, most preferred first (`claude-3-5-sonnet`,
    /// `sonnet`, `claude`). A wire hint with no `name` says nothing, so it is
    /// dropped on the way in rather than becoming an empty hint.
    pub hints: Vec<String>,
    /// 0…1: how much cost matters.
    pub cost_priority: Option<f64>,
    /// 0…1: how much latency matters.
    pub speed_priority: Option<f64>,
    /// 0…1: how much capability matters.
    pub intelligence_priority: Option<f64>,
}

impl ModelPreferences {
    /// Preferences that hint at the given model names, in order.
    pub fn hinting<I, S>(hints: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            hints: hints.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }
}

/// Params of `sampling/createMessage`: the conversation to continue.
#[derive(Clone, Debug, Default, PartialEq)]
#[non_exhaustive]
pub struct CreateMessageParams {
    /// The conversation so far.
    pub messages: Vec<SamplingMessage>,
    /// Cap on tokens sampled; the client MAY sample fewer.
    pub max_tokens: i64,
    /// System prompt; the client MAY modify or drop it.
    pub system_prompt: Option<String>,
    /// How much MCP context to attach. Needs the client's `sampling.context`
    /// for anything but [`IncludeContext::None`].
    pub include_context: Option<IncludeContext>,
    /// Sampling temperature.
    pub temperature: Option<f64>,
    /// Sequences that stop generation.
    pub stop_sequences: Vec<String>,
    /// Provider-specific passthrough metadata.
    pub metadata: Map<String, Value>,
    /// Model selection hints.
    pub model_preferences: Option<ModelPreferences>,
    /// Tools the model may call. Needs the client's `sampling.tools`, and has
    /// no shape before `2025-11-25`.
    pub tools: Vec<Tool>,
    /// How the model may use [`tools`](Self::tools). Needs `sampling.tools`.
    pub tool_choice: Option<ToolChoice>,
}

impl CreateMessageParams {
    /// A request to continue `messages`, sampling at most `max_tokens`.
    #[must_use]
    pub fn new(messages: Vec<SamplingMessage>, max_tokens: i64) -> Self {
        Self {
            messages,
            max_tokens,
            ..Self::default()
        }
    }

    /// Set the system prompt (builder style).
    #[must_use]
    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Ask for MCP context to be attached (builder style).
    #[must_use]
    pub fn with_include_context(mut self, include: IncludeContext) -> Self {
        self.include_context = Some(include);
        self
    }

    /// Set the sampling temperature (builder style).
    #[must_use]
    pub fn with_temperature(mut self, temperature: f64) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Set the stop sequences (builder style).
    #[must_use]
    pub fn with_stop_sequences(mut self, stop: Vec<String>) -> Self {
        self.stop_sequences = stop;
        self
    }

    /// Set the model preferences (builder style).
    #[must_use]
    pub fn with_model_preferences(mut self, prefs: ModelPreferences) -> Self {
        self.model_preferences = Some(prefs);
        self
    }

    /// Offer tools to the model (builder style). Agentic sampling: the client
    /// must have declared `sampling.tools`.
    #[must_use]
    pub fn with_tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = tools;
        self
    }

    /// Constrain how the model uses the offered tools (builder style).
    #[must_use]
    pub fn with_tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Set provider passthrough metadata (builder style).
    #[must_use]
    pub fn with_metadata(mut self, metadata: Map<String, Value>) -> Self {
        self.metadata = metadata;
        self
    }

    /// Whether this request needs the client's `sampling.tools` capability.
    #[must_use]
    pub fn uses_tools(&self) -> bool {
        !self.tools.is_empty() || self.tool_choice.is_some()
    }

    /// Whether this request needs the client's `sampling.context` capability.
    #[must_use]
    pub fn uses_context(&self) -> bool {
        !matches!(self.include_context, None | Some(IncludeContext::None))
    }

    /// Check the two tool-use rules the spec states as MUSTs.
    ///
    /// A message carrying tool results must carry *only* tool results (provider
    /// APIs put them on a dedicated role), and every tool use must be answered
    /// by the very next message, one result per call, before the conversation
    /// moves on. [`to_wire`](Self::to_wire) runs this, so a malformed
    /// conversation cannot reach a client by forgetting to ask.
    pub fn validate(&self) -> Result<(), SamplingError> {
        validate_sampling_messages(&self.messages)
    }

    /// Render for `version`, or say why it cannot be rendered.
    pub fn to_wire(&self, version: &ProtocolVersion) -> Result<Value, SamplingError> {
        self.validate()?;
        let legacy_or_newer = !matches!(version, ProtocolVersion::V2025_06_18);
        if !legacy_or_newer && self.uses_tools() {
            return Err(SamplingError::Unsupported {
                feature: "agentic sampling (`tools` / `toolChoice`)".into(),
                version: version.clone(),
            });
        }
        let mut messages = Vec::with_capacity(self.messages.len());
        for m in &self.messages {
            messages.push(sampling_message_wire(m, version)?);
        }
        let mut out = Map::new();
        out.insert("messages".into(), Value::Array(messages));
        out.insert("maxTokens".into(), Value::from(self.max_tokens));
        if let Some(p) = &self.system_prompt {
            out.insert("systemPrompt".into(), Value::String(p.clone()));
        }
        if let Some(c) = self.include_context {
            out.insert("includeContext".into(), Value::String(c.as_str().into()));
        }
        if let Some(t) = self.temperature
            && let Some(n) = serde_json::Number::from_f64(t)
        {
            out.insert("temperature".into(), Value::Number(n));
        }
        if !self.stop_sequences.is_empty() {
            out.insert(
                "stopSequences".into(),
                Value::Array(
                    self.stop_sequences
                        .iter()
                        .map(|s| Value::String(s.clone()))
                        .collect(),
                ),
            );
        }
        if !self.metadata.is_empty() {
            out.insert("metadata".into(), Value::Object(self.metadata.clone()));
        }
        if let Some(p) = &self.model_preferences {
            out.insert("modelPreferences".into(), model_preferences_wire(p));
        }
        if !self.tools.is_empty() {
            let mut tools = Vec::with_capacity(self.tools.len());
            for t in &self.tools {
                tools.push(tool_wire(t.clone(), version)?);
            }
            out.insert("tools".into(), Value::Array(tools));
        }
        if let Some(c) = self.tool_choice {
            let mut choice = Map::new();
            choice.insert("mode".into(), Value::String(c.as_str().into()));
            out.insert("toolChoice".into(), Value::Object(choice));
        }
        Ok(Value::Object(out))
    }

    /// Parse inbound params. Version-agnostic and tolerant of both the bare-
    /// object and array forms of `content`: what a client must reject is
    /// decided by its declared capabilities, not by re-deriving the revision.
    pub fn from_wire(value: &Value) -> Result<Self, SamplingError> {
        let obj = value
            .as_object()
            .ok_or_else(|| SamplingError::Invalid("sampling params must be an object".into()))?;
        let messages = obj
            .get("messages")
            .and_then(Value::as_array)
            .ok_or_else(|| SamplingError::Invalid("sampling params need `messages`".into()))?
            .iter()
            .map(sampling_message_from_wire)
            .collect::<Result<Vec<_>, _>>()?;
        let max_tokens = obj
            .get("maxTokens")
            .and_then(Value::as_i64)
            .ok_or_else(|| SamplingError::Invalid("sampling params need `maxTokens`".into()))?;
        Ok(Self {
            messages,
            max_tokens,
            system_prompt: obj
                .get("systemPrompt")
                .and_then(Value::as_str)
                .map(Into::into),
            include_context: obj
                .get("includeContext")
                .and_then(Value::as_str)
                .and_then(IncludeContext::from_wire),
            temperature: obj.get("temperature").and_then(Value::as_f64),
            stop_sequences: obj
                .get("stopSequences")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(Into::into))
                        .collect()
                })
                .unwrap_or_default(),
            metadata: obj
                .get("metadata")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            model_preferences: obj.get("modelPreferences").map(model_preferences_from_wire),
            tools: obj
                .get("tools")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(tool_from_wire).collect())
                .unwrap_or_default(),
            tool_choice: obj
                .get("toolChoice")
                .and_then(|c| c.get("mode"))
                .and_then(Value::as_str)
                .and_then(ToolChoice::from_wire),
        })
    }
}

/// Result of `sampling/createMessage`: the model's turn.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct CreateMessageResult {
    /// Always `Assistant` in practice — the model authored it.
    pub role: Role,
    /// What the model produced. `ToolUse` blocks mean it wants to call tools,
    /// and pair with `stop_reason: "toolUse"`.
    pub content: Vec<SamplingContent>,
    /// The model that generated it.
    pub model: String,
    /// Why sampling stopped: `endTurn`, `stopSequence`, `maxTokens`, `toolUse`,
    /// or a provider-specific string.
    pub stop_reason: Option<String>,
    /// Arbitrary `_meta`. Empty = absent on the wire.
    pub meta: Map<String, Value>,
}

impl CreateMessageResult {
    /// A result carrying the given blocks.
    pub fn new(model: impl Into<String>, content: Vec<SamplingContent>) -> Self {
        Self {
            role: Role::Assistant,
            content,
            model: model.into(),
            stop_reason: None,
            meta: Map::new(),
        }
    }

    /// A plain text answer that ended the turn.
    pub fn text(model: impl Into<String>, text: impl Into<String>) -> Self {
        Self {
            stop_reason: Some("endTurn".into()),
            ..Self::new(model, alloc::vec![SamplingContent::text(text)])
        }
    }

    /// Set the stop reason (builder style).
    #[must_use]
    pub fn with_stop_reason(mut self, reason: impl Into<String>) -> Self {
        self.stop_reason = Some(reason.into());
        self
    }

    /// The tool calls the model asked for, in order.
    pub fn tool_uses(&self) -> impl Iterator<Item = &ToolUse> {
        self.content.iter().filter_map(|c| match c {
            SamplingContent::ToolUse(u) => Some(u),
            _ => None,
        })
    }

    /// Render for `version`, or say why it cannot be rendered.
    pub fn to_wire(&self, version: &ProtocolVersion) -> Result<Value, SamplingError> {
        let mut out = Map::new();
        out.insert("role".into(), Value::String(role_wire(self.role).into()));
        out.insert(
            "content".into(),
            sampling_content_wire(&self.content, version)?,
        );
        out.insert("model".into(), Value::String(self.model.clone()));
        if let Some(r) = &self.stop_reason {
            out.insert("stopReason".into(), Value::String(r.clone()));
        }
        if !self.meta.is_empty() && !matches!(version, ProtocolVersion::V2025_06_18) {
            out.insert("_meta".into(), Value::Object(self.meta.clone()));
        }
        Ok(Value::Object(out))
    }

    /// Parse an inbound result, tolerating both `content` forms.
    pub fn from_wire(value: &Value) -> Result<Self, SamplingError> {
        let obj = value
            .as_object()
            .ok_or_else(|| SamplingError::Invalid("sampling result must be an object".into()))?;
        let model = obj
            .get("model")
            .and_then(Value::as_str)
            .ok_or_else(|| SamplingError::Invalid("sampling result needs `model`".into()))?
            .into();
        let content = sampling_content_from_wire(obj.get("content").unwrap_or(&Value::Null))?;
        if content.iter().any(|c| c.answers_tool_use().is_some()) {
            return Err(SamplingError::Invalid(
                "a sampling result is the assistant's turn, so it cannot carry tool results".into(),
            ));
        }
        Ok(Self {
            role: obj
                .get("role")
                .and_then(Value::as_str)
                .and_then(role_from_wire)
                .unwrap_or(Role::Assistant),
            content,
            model,
            stop_reason: obj
                .get("stopReason")
                .and_then(Value::as_str)
                .map(Into::into),
            meta: obj
                .get("_meta")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
        })
    }
}

/// The spec's two tool-use MUSTs, checked over a whole conversation.
///
/// In two passes, so the complaint names the real problem: an unbalanced
/// conversation whose answer is *also* mixed should be reported as mixed, which
/// is the thing the author can act on.
fn validate_sampling_messages(messages: &[SamplingMessage]) -> Result<(), SamplingError> {
    // "When a user message contains tool results, it MUST contain ONLY tool
    // results" — provider APIs put them on a dedicated role, so a mixed message
    // has nowhere to go.
    for (i, message) in messages.iter().enumerate() {
        let results = message
            .content
            .iter()
            .filter(|c| c.answers_tool_use().is_some())
            .count();
        if results > 0 && results != message.content.len() {
            return Err(SamplingError::Invalid(alloc::format!(
                "message {i} mixes tool results with other content; a message \
                 carrying tool results must carry nothing else"
            )));
        }
    }

    for (i, message) in messages.iter().enumerate() {
        let pending: Vec<&str> = message
            .content
            .iter()
            .filter_map(SamplingContent::tool_use_id)
            .collect();
        if pending.is_empty() {
            continue;
        }
        // "Every assistant message containing ToolUseContent blocks MUST be
        // followed by a user message that consists entirely of
        // ToolResultContent blocks … before any other message."
        let Some(answer) = messages.get(i + 1) else {
            return Err(SamplingError::Invalid(alloc::format!(
                "message {i} makes {} tool call(s) that the conversation never \
                 answers; every tool use must be resolved before sampling continues",
                pending.len()
            )));
        };
        let answered: BTreeMap<&str, ()> = answer
            .content
            .iter()
            .filter_map(|c| c.answers_tool_use().map(|id| (id, ())))
            .collect();
        if answered.len() != answer.content.len() {
            return Err(SamplingError::Invalid(alloc::format!(
                "message {} must consist entirely of tool results, because \
                 message {i} makes tool calls",
                i + 1
            )));
        }
        for id in pending {
            if !answered.contains_key(id) {
                return Err(SamplingError::Invalid(alloc::format!(
                    "tool call `{id}` in message {i} has no matching tool result \
                     in message {}",
                    i + 1
                )));
            }
        }
    }
    Ok(())
}

/// `{ role, content, _meta? }` for one message.
fn sampling_message_wire(
    message: &SamplingMessage,
    version: &ProtocolVersion,
) -> Result<Value, SamplingError> {
    let mut out = Map::new();
    out.insert("role".into(), Value::String(role_wire(message.role).into()));
    out.insert(
        "content".into(),
        sampling_content_wire(&message.content, version)?,
    );
    // `SamplingMessage._meta` arrived with `2025-11-25`.
    if !message.meta.is_empty() && !matches!(version, ProtocolVersion::V2025_06_18) {
        out.insert("_meta".into(), Value::Object(message.meta.clone()));
    }
    Ok(Value::Object(out))
}

/// A lone block renders bare, several render as an array. `2025-06-18` has no
/// array form at all, so more than one block is a step-down failure there
/// rather than a truncation.
fn sampling_content_wire(
    blocks: &[SamplingContent],
    version: &ProtocolVersion,
) -> Result<Value, SamplingError> {
    let single_only = matches!(version, ProtocolVersion::V2025_06_18);
    if single_only && blocks.len() > 1 {
        return Err(SamplingError::Unsupported {
            feature: "a sampling message with more than one content block".into(),
            version: version.clone(),
        });
    }
    let mut rendered = Vec::with_capacity(blocks.len());
    for b in blocks {
        rendered.push(sampling_block_wire(b, version)?);
    }
    Ok(match rendered.len() {
        1 => rendered.remove(0),
        _ => Value::Array(rendered),
    })
}

fn sampling_block_wire(
    block: &SamplingContent,
    version: &ProtocolVersion,
) -> Result<Value, SamplingError> {
    let tool_blocks = !matches!(version, ProtocolVersion::V2025_06_18);
    match block {
        SamplingContent::Media(
            c @ (Content::Text { .. } | Content::Image { .. } | Content::Audio { .. }),
        ) => content_block_wire(c.clone(), version),
        SamplingContent::Media(_) => Err(SamplingError::Unsupported {
            feature: "an embedded resource or resource link in a sampling message".into(),
            version: version.clone(),
        }),
        _ if !tool_blocks => Err(SamplingError::Unsupported {
            feature: "tool-use content in a sampling message".into(),
            version: version.clone(),
        }),
        SamplingContent::ToolUse(u) => {
            let mut out = Map::new();
            out.insert("type".into(), Value::String("tool_use".into()));
            out.insert("id".into(), Value::String(u.id.clone()));
            out.insert("name".into(), Value::String(u.name.clone()));
            out.insert("input".into(), Value::Object(u.input.clone()));
            if !u.meta.is_empty() {
                out.insert("_meta".into(), Value::Object(u.meta.clone()));
            }
            Ok(Value::Object(out))
        }
        SamplingContent::ToolResult(r) => {
            let mut content = Vec::with_capacity(r.content.len());
            for c in &r.content {
                content.push(content_block_wire(c.clone(), version)?);
            }
            let mut out = Map::new();
            out.insert("type".into(), Value::String("tool_result".into()));
            out.insert("toolUseId".into(), Value::String(r.tool_use_id.clone()));
            out.insert("content".into(), Value::Array(content));
            if !r.structured_content.is_empty() {
                out.insert(
                    "structuredContent".into(),
                    Value::Object(r.structured_content.clone()),
                );
            }
            if let Some(e) = r.is_error {
                out.insert("isError".into(), Value::Bool(e));
            }
            if !r.meta.is_empty() {
                out.insert("_meta".into(), Value::Object(r.meta.clone()));
            }
            Ok(Value::Object(out))
        }
    }
}

/// One [`Content`] block on `version`'s wire, reusing the same conversions
/// every other content position uses so a sampling block and a tool-result
/// block cannot drift apart.
fn content_block_wire(content: Content, version: &ProtocolVersion) -> Result<Value, SamplingError> {
    let json = match version {
        ProtocolVersion::V2026_07_28 => serde_json::to_value(v0728::ContentBlock::from(content)),
        ProtocolVersion::V2025_06_18 => {
            serde_json::to_value(v06::ContentBlock::from(legacy::ContentBlock::from(content)))
        }
        _ => serde_json::to_value(legacy::ContentBlock::from(content)),
    };
    json.map_err(|e| SamplingError::Invalid(alloc::format!("content block is not JSON: {e}")))
}

fn tool_wire(tool: Tool, version: &ProtocolVersion) -> Result<Value, SamplingError> {
    let json = match version {
        ProtocolVersion::V2026_07_28 => serde_json::to_value(v0728::Tool::from(tool)),
        ProtocolVersion::V2025_06_18 => serde_json::to_value(v06::Tool::from(tool)),
        _ => serde_json::to_value(legacy::Tool::from(tool)),
    };
    json.map_err(|e| SamplingError::Invalid(alloc::format!("tool is not JSON: {e}")))
}

fn tool_from_wire(value: &Value) -> Option<Tool> {
    serde_json::from_value::<legacy::Tool>(value.clone())
        .ok()
        .map(Into::into)
}

fn model_preferences_wire(prefs: &ModelPreferences) -> Value {
    let mut out = Map::new();
    if !prefs.hints.is_empty() {
        out.insert(
            "hints".into(),
            Value::Array(
                prefs
                    .hints
                    .iter()
                    .map(|h| {
                        let mut hint = Map::new();
                        hint.insert("name".into(), Value::String(h.clone()));
                        Value::Object(hint)
                    })
                    .collect(),
            ),
        );
    }
    for (key, value) in [
        ("costPriority", prefs.cost_priority),
        ("speedPriority", prefs.speed_priority),
        ("intelligencePriority", prefs.intelligence_priority),
    ] {
        if let Some(v) = value
            && let Some(n) = serde_json::Number::from_f64(v)
        {
            out.insert(key.into(), Value::Number(n));
        }
    }
    Value::Object(out)
}

fn model_preferences_from_wire(value: &Value) -> ModelPreferences {
    ModelPreferences {
        hints: value
            .get("hints")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|h| h.get("name").and_then(Value::as_str).map(Into::into))
                    .collect()
            })
            .unwrap_or_default(),
        cost_priority: value.get("costPriority").and_then(Value::as_f64),
        speed_priority: value.get("speedPriority").and_then(Value::as_f64),
        intelligence_priority: value.get("intelligencePriority").and_then(Value::as_f64),
    }
}

fn sampling_message_from_wire(value: &Value) -> Result<SamplingMessage, SamplingError> {
    Ok(SamplingMessage {
        role: value
            .get("role")
            .and_then(Value::as_str)
            .and_then(role_from_wire)
            .ok_or_else(|| SamplingError::Invalid("a sampling message needs `role`".into()))?,
        content: sampling_content_from_wire(value.get("content").unwrap_or(&Value::Null))?,
        meta: value
            .get("_meta")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default(),
    })
}

fn sampling_content_from_wire(value: &Value) -> Result<Vec<SamplingContent>, SamplingError> {
    match value {
        Value::Array(blocks) => blocks.iter().map(sampling_block_from_wire).collect(),
        Value::Null => Err(SamplingError::Invalid(
            "a sampling message needs `content`".into(),
        )),
        one => Ok(alloc::vec![sampling_block_from_wire(one)?]),
    }
}

fn sampling_block_from_wire(value: &Value) -> Result<SamplingContent, SamplingError> {
    match value.get("type").and_then(Value::as_str) {
        Some("tool_use") => Ok(SamplingContent::ToolUse(ToolUse {
            id: value
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| SamplingError::Invalid("a tool_use block needs `id`".into()))?
                .into(),
            name: value
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| SamplingError::Invalid("a tool_use block needs `name`".into()))?
                .into(),
            input: value
                .get("input")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            meta: value
                .get("_meta")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
        })),
        Some("tool_result") => Ok(SamplingContent::ToolResult(Box::new(ToolResult {
            tool_use_id: value
                .get("toolUseId")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    SamplingError::Invalid("a tool_result block needs `toolUseId`".into())
                })?
                .into(),
            content: value
                .get("content")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(content_block_from_wire).collect())
                .unwrap_or_default(),
            structured_content: value
                .get("structuredContent")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
            is_error: value.get("isError").and_then(Value::as_bool),
            meta: value
                .get("_meta")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default(),
        }))),
        _ => content_block_from_wire(value)
            .map(SamplingContent::Media)
            .ok_or_else(|| SamplingError::Invalid("unrecognized sampling content block".into())),
    }
}

/// Parse one content block through the `2025-11-25` wire, which is the widest
/// of the three for the kinds sampling allows.
fn content_block_from_wire(value: &Value) -> Option<Content> {
    serde_json::from_value::<legacy::ContentBlock>(value.clone())
        .ok()
        .map(Into::into)
}

fn role_wire(role: Role) -> &'static str {
    match role {
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn role_from_wire(s: &str) -> Option<Role> {
    match s {
        "user" => Some(Role::User),
        "assistant" => Some(Role::Assistant),
        _ => None,
    }
}

// ---- completions --------------------------------------------------------------

/// Result of `completion/complete`: up to 100 suggested values.
#[derive(Clone, Debug, Default)]
#[non_exhaustive]
pub struct CompleteResult {
    /// Suggested completion values (spec caps at 100).
    pub values: Vec<String>,
    /// Total available, which may exceed `values.len()`.
    pub total: Option<u32>,
    /// Whether more values exist beyond those returned.
    pub has_more: Option<bool>,
}

impl CompleteResult {
    /// A result carrying the given values.
    pub fn new(values: Vec<String>) -> Self {
        Self {
            values,
            total: None,
            has_more: None,
        }
    }

    /// Set the total count (builder style).
    #[must_use]
    pub fn with_total(mut self, total: u32) -> Self {
        self.total = Some(total);
        self
    }

    /// Set the has-more flag (builder style).
    #[must_use]
    pub fn with_has_more(mut self, has_more: bool) -> Self {
        self.has_more = Some(has_more);
        self
    }

    /// Cut the values to the schema's `maxItems: 100`, flagging the rest.
    ///
    /// Run on the way to every wire, because the cap is a schema constraint
    /// and a handler returning 101 otherwise emits a response its own client
    /// will refuse to parse. `has_more` is exactly the field for what was cut,
    /// so nothing is lost but the surplus itself — and `total`, if the handler
    /// set one, still reports how many there really are.
    fn capped(mut self) -> Self {
        if self.values.len() > MAX_COMPLETION_VALUES {
            self.values.truncate(MAX_COMPLETION_VALUES);
            self.has_more = Some(true);
        }
        self
    }
}

/// `CompleteResult.completion.values` is `maxItems: 100` on every revision.
const MAX_COMPLETION_VALUES: usize = 100;

/// What a completion request is completing against.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum CompletionReference {
    /// An argument of a prompt, by prompt name.
    Prompt {
        /// Prompt name.
        name: String,
    },
    /// A variable of a resource template, by URI (template).
    ResourceTemplate {
        /// Resource URI or URI template.
        uri: String,
    },
}

/// The argument being completed: its name and the partial value typed so far.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct CompletionArgument {
    /// Name of the argument.
    pub name: String,
    /// Partial value entered so far.
    pub value: String,
}

impl CompletionArgument {
    /// Construct from a name and partial value.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

/// `completion/complete` parameters.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct CompleteParams {
    /// What is being completed (a prompt or a resource template).
    pub reference: CompletionReference,
    /// The argument and its partial value.
    pub argument: CompletionArgument,
    /// Previously-resolved arguments (for multi-variable templates).
    pub context_arguments: BTreeMap<String, String>,
}

impl CompleteParams {
    /// Construct from a reference and the argument being completed.
    pub fn new(reference: CompletionReference, argument: CompletionArgument) -> Self {
        Self {
            reference,
            argument,
            context_arguments: BTreeMap::new(),
        }
    }
}

// ---- client capabilities --------------------------------------------------

/// What a client can answer when the server calls back.
///
/// Derived from which handlers a client registers rather than written by hand,
/// for the same reason `#[server]` derives the server's: a capability object
/// that disagrees with the implementation is a bug neither side can see. Over-
/// declaring makes the server send requests the client refuses; under-declaring
/// makes it skip features the client implements, silently, because refusing to
/// send what was not declared is a spec MUST (SEP-2322) and the server obeys it.
///
/// The *sub*-capabilities matter as much as the top-level ones: a client that
/// declares `elicitation` without `url` is saying it can render a form and not
/// a consent page, and a server that ignores the difference will strand the
/// user on an interaction they were never shown.
///
/// Render with [`to_wire`](Self::to_wire), which drops what a revision predates
/// (`2025-06-18` has no elicitation or sampling sub-capabilities; `2026-07-28`
/// has no `roots.listChanged`) so one declaration is correct on every wire.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ClientCapabilities {
    /// Set when the client can answer `elicitation/create`.
    pub elicitation: Option<ElicitationCapability>,
    /// Set when the client can answer `sampling/createMessage`.
    pub sampling: Option<SamplingCapability>,
    /// Set when the client can answer `roots/list`.
    pub roots: Option<RootsCapability>,
    /// Non-standard capabilities, passed through untouched.
    pub experimental: Option<Map<String, Value>>,
    /// Extensions this client participates in, keyed by extension id. Only the
    /// `2026-07-28` shape has this field, so it is dropped on the older wires.
    pub extensions: Option<Map<String, Value>>,
}

/// Which elicitation modes a client can present.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ElicitationCapability {
    /// The client can render a form from `requestedSchema`.
    pub form: bool,
    /// The client can send the user out of band to a URL.
    pub url: bool,
}

/// Which sampling features a client supports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SamplingCapability {
    /// The client honours `includeContext`. Undeclared, the spec says servers
    /// SHOULD send only `includeContext: "none"` (or omit it).
    pub context: bool,
    /// The client honours `tools` / `toolChoice` (agentic sampling).
    pub tools: bool,
}

/// How a client exposes its roots.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RootsCapability {
    /// The client emits `notifications/roots/list_changed` when its roots
    /// change. Not part of the `2026-07-28` shape, so dropped on that wire.
    pub list_changed: bool,
}

impl ElicitationCapability {
    /// A client that can render a form but not navigate to a URL.
    #[must_use]
    pub fn form() -> Self {
        Self {
            form: true,
            url: false,
        }
    }

    /// Declare URL-mode support.
    #[must_use]
    pub fn with_url(mut self, url: bool) -> Self {
        self.url = url;
        self
    }
}

impl SamplingCapability {
    /// Sampling with neither `context` nor `tools`.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare that the client honours `includeContext`.
    #[must_use]
    pub fn with_context(mut self, context: bool) -> Self {
        self.context = context;
        self
    }

    /// Declare that the client honours `tools` / `toolChoice`.
    #[must_use]
    pub fn with_tools(mut self, tools: bool) -> Self {
        self.tools = tools;
        self
    }
}

impl RootsCapability {
    /// Roots without change notifications.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Declare that the client emits `notifications/roots/list_changed`.
    #[must_use]
    pub fn with_list_changed(mut self, list_changed: bool) -> Self {
        self.list_changed = list_changed;
        self
    }
}

impl ClientCapabilities {
    /// An empty declaration: the client answers no server→client requests.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Render for `version`, dropping the fields that revision predates.
    ///
    /// Always an object, never `null`: "I support nothing" is `{}`, which is a
    /// statement, where a missing `capabilities` is a malformed handshake.
    #[must_use]
    pub fn to_wire(&self, version: ProtocolVersion) -> Value {
        // `2025-06-18` has the three capabilities but none of their
        // sub-objects; declaring a sub-object there would assert something its
        // schema cannot express.
        let sub_capabilities = !matches!(version, ProtocolVersion::V2025_06_18);
        let mut out = Map::new();
        if let Some(e) = self.elicitation {
            let mut o = Map::new();
            if sub_capabilities {
                if e.form {
                    o.insert("form".into(), Value::Object(Map::new()));
                }
                if e.url {
                    o.insert("url".into(), Value::Object(Map::new()));
                }
            }
            out.insert("elicitation".into(), Value::Object(o));
        }
        if let Some(s) = self.sampling {
            let mut o = Map::new();
            if sub_capabilities {
                if s.context {
                    o.insert("context".into(), Value::Object(Map::new()));
                }
                if s.tools {
                    o.insert("tools".into(), Value::Object(Map::new()));
                }
            }
            out.insert("sampling".into(), Value::Object(o));
        }
        if let Some(r) = self.roots {
            let mut o = Map::new();
            // `2026-07-28` dropped `listChanged` from the roots capability.
            if r.list_changed && !matches!(version, ProtocolVersion::V2026_07_28) {
                o.insert("listChanged".into(), Value::Bool(true));
            }
            out.insert("roots".into(), Value::Object(o));
        }
        if let Some(x) = &self.experimental
            && !x.is_empty()
        {
            out.insert("experimental".into(), Value::Object(x.clone()));
        }
        if let Some(x) = &self.extensions
            && !x.is_empty()
            && matches!(version, ProtocolVersion::V2026_07_28)
        {
            out.insert("extensions".into(), Value::Object(x.clone()));
        }
        Value::Object(out)
    }
}

// ---- neutral → 2026-07-28 wire conversions --------------------------------

impl From<Content> for v0728::ContentBlock {
    fn from(c: Content) -> Self {
        match c {
            Content::Text {
                text,
                annotations,
                meta,
            } => v0728::ContentBlock::TextContent(v0728::TextContent {
                annotations: annotations.map(Into::into),
                meta: (!meta.is_empty()).then_some(v0728::MetaObject(meta)),
                text,
                type_: "text".to_string(),
            }),
            Content::Image {
                data,
                mime_type,
                annotations,
                meta,
            } => v0728::ContentBlock::ImageContent(v0728::ImageContent {
                annotations: annotations.map(Into::into),
                data,
                meta: (!meta.is_empty()).then_some(v0728::MetaObject(meta)),
                mime_type,
                type_: "image".to_string(),
            }),
            Content::Audio {
                data,
                mime_type,
                annotations,
                meta,
            } => v0728::ContentBlock::AudioContent(v0728::AudioContent {
                annotations: annotations.map(Into::into),
                data,
                meta: (!meta.is_empty()).then_some(v0728::MetaObject(meta)),
                mime_type,
                type_: "audio".to_string(),
            }),
            Content::Resource {
                contents,
                annotations,
                meta,
            } => v0728::ContentBlock::EmbeddedResource(v0728::EmbeddedResource {
                annotations: annotations.map(Into::into),
                meta: (!meta.is_empty()).then_some(v0728::MetaObject(meta)),
                resource: contents.into(),
                type_: "resource".to_string(),
            }),
            Content::ResourceLink(r) => {
                let r = *r;
                v0728::ContentBlock::ResourceLink(v0728::ResourceLink {
                    annotations: r.annotations.map(Into::into),
                    description: r.description,
                    icons: r.icons.into_iter().map(Into::into).collect(),
                    meta: (!r.meta.is_empty()).then_some(v0728::MetaObject(r.meta)),
                    mime_type: r.mime_type,
                    name: r.name,
                    size: r.size.map(|s| i64::try_from(s).unwrap_or(i64::MAX)),
                    title: r.title,
                    type_: "resource_link".to_string(),
                    uri: r.uri,
                })
            }
        }
    }
}

impl From<Tool> for v0728::Tool {
    fn from(t: Tool) -> Self {
        // The neutral `input_schema` is a JSON Schema object; deserialize it
        // into the typed wrapper. A non-object or schema-less value falls back
        // to an empty object schema rather than failing the conversion.
        let input_schema =
            serde_json::from_value(t.input_schema).unwrap_or(v0728::ToolInputSchema {
                schema: None,
                type_: "object".to_string(),
                extra: Map::new(),
            });
        v0728::Tool {
            annotations: t.annotations.map(Into::into),
            description: t.description,
            icons: t.icons.into_iter().map(Into::into).collect(),
            input_schema,
            meta: (!t.meta.is_empty()).then_some(v0728::MetaObject(t.meta)),
            name: t.name,
            // The draft `ToolOutputSchema` is a permissive bag ($schema + a
            // flattened `extra`), so any JSON Schema object deserializes into it.
            output_schema: t.output_schema.and_then(|v| serde_json::from_value(v).ok()),
            title: t.title,
        }
    }
}

impl From<ToolAnnotations> for v0728::ToolAnnotations {
    fn from(a: ToolAnnotations) -> Self {
        v0728::ToolAnnotations {
            destructive_hint: a.destructive_hint,
            idempotent_hint: a.idempotent_hint,
            open_world_hint: a.open_world_hint,
            read_only_hint: a.read_only_hint,
            title: a.title,
        }
    }
}

impl From<Icon> for v0728::Icon {
    fn from(i: Icon) -> Self {
        v0728::Icon {
            mime_type: i.mime_type,
            sizes: i.sizes,
            src: i.src,
            theme: i.theme.map(|t| match t {
                IconTheme::Light => v0728::IconTheme::Light,
                IconTheme::Dark => v0728::IconTheme::Dark,
            }),
        }
    }
}

impl From<SubscriptionFilter> for v0728::SubscriptionFilter {
    fn from(f: SubscriptionFilter) -> Self {
        // The wire models each flag as `Option<bool>` where absent means "not
        // requested"; send `Some(true)` only for what was asked for, so the
        // filter on the wire says exactly what the caller meant.
        v0728::SubscriptionFilter {
            tools_list_changed: f.tools_list_changed.then_some(true),
            resources_list_changed: f.resources_list_changed.then_some(true),
            prompts_list_changed: f.prompts_list_changed.then_some(true),
            resource_subscriptions: f.resource_subscriptions,
        }
    }
}

impl From<v0728::SubscriptionFilter> for SubscriptionFilter {
    fn from(f: v0728::SubscriptionFilter) -> Self {
        SubscriptionFilter {
            tools_list_changed: f.tools_list_changed.unwrap_or(false),
            resources_list_changed: f.resources_list_changed.unwrap_or(false),
            prompts_list_changed: f.prompts_list_changed.unwrap_or(false),
            resource_subscriptions: f.resource_subscriptions,
        }
    }
}

impl From<ListToolsResult> for v0728::ListToolsResult {
    fn from(r: ListToolsResult) -> Self {
        let cache = r.cache.unwrap_or(CachePolicy::NO_CACHE);
        v0728::ListToolsResult {
            cache_scope: match cache.scope {
                CacheScope::Public => v0728::ListToolsResultCacheScope::Public,
                CacheScope::Private => v0728::ListToolsResultCacheScope::Private,
            },
            meta: None,
            next_cursor: r.next_cursor,
            result_type: result_type::COMPLETE.to_string(),
            tools: r.tools.into_iter().map(Into::into).collect(),
            ttl_ms: cache.ttl_ms,
        }
    }
}

impl From<CallToolResult> for v0728::CallToolResult {
    fn from(r: CallToolResult) -> Self {
        v0728::CallToolResult {
            content: r.content.into_iter().map(Into::into).collect(),
            is_error: Some(r.is_error),
            meta: None,
            result_type: result_type::COMPLETE.to_string(),
            structured_content: r.structured_content,
        }
    }
}

// resources

impl From<Resource> for v0728::Resource {
    fn from(r: Resource) -> Self {
        v0728::Resource {
            annotations: r.annotations.map(Into::into),
            description: r.description,
            icons: r.icons.into_iter().map(Into::into).collect(),
            meta: (!r.meta.is_empty()).then_some(v0728::MetaObject(r.meta)),
            mime_type: r.mime_type,
            name: r.name,
            size: r.size.map(|s| i64::try_from(s).unwrap_or(i64::MAX)),
            title: r.title,
            uri: r.uri,
        }
    }
}

impl From<Annotations> for v0728::Annotations {
    fn from(a: Annotations) -> Self {
        v0728::Annotations {
            audience: a.audience.into_iter().map(Into::into).collect(),
            last_modified: a.last_modified,
            priority: a.priority,
        }
    }
}

impl From<ResourceContents> for v0728::ReadResourceResultContentsItem {
    fn from(c: ResourceContents) -> Self {
        match c {
            ResourceContents::Text {
                uri,
                mime_type,
                text,
                meta,
            } => v0728::ReadResourceResultContentsItem::TextResourceContents(
                v0728::TextResourceContents {
                    meta: (!meta.is_empty()).then_some(v0728::MetaObject(meta)),
                    mime_type,
                    text,
                    uri,
                },
            ),
            ResourceContents::Blob {
                uri,
                mime_type,
                blob,
                meta,
            } => v0728::ReadResourceResultContentsItem::BlobResourceContents(
                v0728::BlobResourceContents {
                    blob,
                    meta: (!meta.is_empty()).then_some(v0728::MetaObject(meta)),
                    mime_type,
                    uri,
                },
            ),
        }
    }
}

impl From<ResourceContents> for v0728::EmbeddedResourceResource {
    fn from(c: ResourceContents) -> Self {
        match c {
            ResourceContents::Text {
                uri,
                mime_type,
                text,
                meta,
            } => {
                v0728::EmbeddedResourceResource::TextResourceContents(v0728::TextResourceContents {
                    meta: (!meta.is_empty()).then_some(v0728::MetaObject(meta)),
                    mime_type,
                    text,
                    uri,
                })
            }
            ResourceContents::Blob {
                uri,
                mime_type,
                blob,
                meta,
            } => {
                v0728::EmbeddedResourceResource::BlobResourceContents(v0728::BlobResourceContents {
                    blob,
                    meta: (!meta.is_empty()).then_some(v0728::MetaObject(meta)),
                    mime_type,
                    uri,
                })
            }
        }
    }
}

impl From<ListResourcesResult> for v0728::ListResourcesResult {
    fn from(r: ListResourcesResult) -> Self {
        let cache = r.cache.unwrap_or(CachePolicy::NO_CACHE);
        v0728::ListResourcesResult {
            cache_scope: match cache.scope {
                CacheScope::Public => v0728::ListResourcesResultCacheScope::Public,
                CacheScope::Private => v0728::ListResourcesResultCacheScope::Private,
            },
            meta: None,
            next_cursor: r.next_cursor,
            resources: r.resources.into_iter().map(Into::into).collect(),
            result_type: result_type::COMPLETE.to_string(),
            ttl_ms: cache.ttl_ms,
        }
    }
}

impl From<ReadResourceResult> for v0728::ReadResourceResult {
    fn from(r: ReadResourceResult) -> Self {
        let cache = r.cache.unwrap_or(CachePolicy::NO_CACHE);
        v0728::ReadResourceResult {
            cache_scope: match cache.scope {
                CacheScope::Public => v0728::ReadResourceResultCacheScope::Public,
                CacheScope::Private => v0728::ReadResourceResultCacheScope::Private,
            },
            contents: r.contents.into_iter().map(Into::into).collect(),
            meta: None,
            result_type: result_type::COMPLETE.to_string(),
            ttl_ms: cache.ttl_ms,
        }
    }
}

impl From<ResourceTemplate> for v0728::ResourceTemplate {
    fn from(t: ResourceTemplate) -> Self {
        v0728::ResourceTemplate {
            annotations: t.annotations.map(Into::into),
            description: t.description,
            icons: t.icons.into_iter().map(Into::into).collect(),
            meta: (!t.meta.is_empty()).then_some(v0728::MetaObject(t.meta)),
            mime_type: t.mime_type,
            name: t.name,
            title: t.title,
            uri_template: t.uri_template,
        }
    }
}

impl From<ListResourceTemplatesResult> for v0728::ListResourceTemplatesResult {
    fn from(r: ListResourceTemplatesResult) -> Self {
        let cache = r.cache.unwrap_or(CachePolicy::NO_CACHE);
        v0728::ListResourceTemplatesResult {
            cache_scope: match cache.scope {
                CacheScope::Public => v0728::ListResourceTemplatesResultCacheScope::Public,
                CacheScope::Private => v0728::ListResourceTemplatesResultCacheScope::Private,
            },
            meta: None,
            next_cursor: r.next_cursor,
            resource_templates: r.resource_templates.into_iter().map(Into::into).collect(),
            result_type: result_type::COMPLETE.to_string(),
            ttl_ms: cache.ttl_ms,
        }
    }
}

// prompts

impl From<Role> for v0728::Role {
    fn from(r: Role) -> Self {
        match r {
            Role::User => v0728::Role::User,
            Role::Assistant => v0728::Role::Assistant,
        }
    }
}

impl From<PromptArgument> for v0728::PromptArgument {
    fn from(a: PromptArgument) -> Self {
        v0728::PromptArgument {
            description: a.description,
            name: a.name,
            required: Some(a.required),
            title: a.title,
        }
    }
}

impl From<Prompt> for v0728::Prompt {
    fn from(p: Prompt) -> Self {
        v0728::Prompt {
            arguments: p.arguments.into_iter().map(Into::into).collect(),
            description: p.description,
            icons: p.icons.into_iter().map(Into::into).collect(),
            meta: (!p.meta.is_empty()).then_some(v0728::MetaObject(p.meta)),
            name: p.name,
            title: p.title,
        }
    }
}

impl From<PromptMessage> for v0728::PromptMessage {
    fn from(m: PromptMessage) -> Self {
        v0728::PromptMessage {
            content: m.content.into(),
            role: m.role.into(),
        }
    }
}

impl From<ListPromptsResult> for v0728::ListPromptsResult {
    fn from(r: ListPromptsResult) -> Self {
        let cache = r.cache.unwrap_or(CachePolicy::NO_CACHE);
        v0728::ListPromptsResult {
            cache_scope: match cache.scope {
                CacheScope::Public => v0728::ListPromptsResultCacheScope::Public,
                CacheScope::Private => v0728::ListPromptsResultCacheScope::Private,
            },
            meta: None,
            next_cursor: r.next_cursor,
            prompts: r.prompts.into_iter().map(Into::into).collect(),
            result_type: result_type::COMPLETE.to_string(),
            ttl_ms: cache.ttl_ms,
        }
    }
}

impl From<GetPromptResult> for v0728::GetPromptResult {
    fn from(r: GetPromptResult) -> Self {
        v0728::GetPromptResult {
            description: r.description,
            messages: r.messages.into_iter().map(Into::into).collect(),
            meta: None,
            result_type: result_type::COMPLETE.to_string(),
        }
    }
}

// completions

impl From<CompleteResult> for v0728::CompleteResult {
    fn from(r: CompleteResult) -> Self {
        let r = r.capped();
        v0728::CompleteResult {
            completion: v0728::CompleteResultCompletion {
                has_more: r.has_more,
                total: r.total.map(i64::from),
                values: r.values,
            },
            meta: None,
            result_type: result_type::COMPLETE.to_string(),
        }
    }
}

// ---- neutral → 2025-11-25 wire conversions ------------------------------------
//
// The legacy mirror of the draft conversions above. Differences from the draft
// wire that these conversions absorb so handlers never see them:
// - no `resultType` / `cacheScope` / `ttlMs` (the draft's caching envelope
//   doesn't exist in 2025-11-25);
// - `_meta` is a plain map (skipped when empty), not an `Option`;
// - `CallToolResult.structuredContent` is an *object* by schema, so a neutral
//   non-object `structured_content` value cannot be represented and is dropped
//   (the draft wire keeps any JSON value — see `From<CallToolResult>` below);
// - `ToolInputSchema` is closed (`properties`/`required`/`$schema`/`type`):
//   any other top-level schema keywords a handler put in `input_schema` are
//   not representable on this wire version and do not survive conversion.

impl From<Content> for legacy::ContentBlock {
    fn from(c: Content) -> Self {
        match c {
            Content::Text {
                text,
                annotations,
                meta,
            } => legacy::ContentBlock::TextContent(legacy::TextContent {
                annotations: annotations.map(Into::into),
                meta,
                text,
                type_: "text".to_string(),
            }),
            Content::Image {
                data,
                mime_type,
                annotations,
                meta,
            } => legacy::ContentBlock::ImageContent(legacy::ImageContent {
                annotations: annotations.map(Into::into),
                data,
                meta,
                mime_type,
                type_: "image".to_string(),
            }),
            Content::Audio {
                data,
                mime_type,
                annotations,
                meta,
            } => legacy::ContentBlock::AudioContent(legacy::AudioContent {
                annotations: annotations.map(Into::into),
                data,
                meta,
                mime_type,
                type_: "audio".to_string(),
            }),
            Content::Resource {
                contents,
                annotations,
                meta,
            } => legacy::ContentBlock::EmbeddedResource(legacy::EmbeddedResource {
                annotations: annotations.map(Into::into),
                meta,
                resource: contents.into(),
                type_: "resource".to_string(),
            }),
            Content::ResourceLink(r) => {
                let r = *r;
                legacy::ContentBlock::ResourceLink(legacy::ResourceLink {
                    annotations: r.annotations.map(Into::into),
                    description: r.description,
                    icons: r.icons.into_iter().map(Into::into).collect(),
                    meta: r.meta,
                    mime_type: r.mime_type,
                    name: r.name,
                    size: r.size.map(|s| i64::try_from(s).unwrap_or(i64::MAX)),
                    title: r.title,
                    type_: "resource_link".to_string(),
                    uri: r.uri,
                })
            }
        }
    }
}

impl From<TaskSupport> for legacy::ToolExecutionTaskSupport {
    fn from(ts: TaskSupport) -> Self {
        match ts {
            TaskSupport::Forbidden => legacy::ToolExecutionTaskSupport::Forbidden,
            TaskSupport::Optional => legacy::ToolExecutionTaskSupport::Optional,
            TaskSupport::Required => legacy::ToolExecutionTaskSupport::Required,
        }
    }
}

impl From<legacy::ToolExecutionTaskSupport> for TaskSupport {
    fn from(ts: legacy::ToolExecutionTaskSupport) -> Self {
        match ts {
            legacy::ToolExecutionTaskSupport::Forbidden => TaskSupport::Forbidden,
            legacy::ToolExecutionTaskSupport::Optional => TaskSupport::Optional,
            legacy::ToolExecutionTaskSupport::Required => TaskSupport::Required,
        }
    }
}

impl From<Tool> for legacy::Tool {
    fn from(t: Tool) -> Self {
        // Deserialize the neutral JSON Schema into the (closed) legacy wrapper;
        // a non-object value falls back to an empty object schema. `execution`
        // (task support) is left unset here — the dispatcher patches it when
        // the server has Tasks enabled, since a pure conversion can't know.
        let input_schema =
            serde_json::from_value(t.input_schema).unwrap_or(legacy::ToolInputSchema {
                properties: BTreeMap::new(),
                required: Vec::new(),
                schema: None,
                type_: "object".to_string(),
                extra: Map::new(),
            });
        legacy::Tool {
            annotations: t.annotations.map(Into::into),
            description: t.description,
            // A declared `#[tool(task)]` sets per-tool task support; otherwise
            // left unset for the dispatcher to patch under a global Tasks policy.
            execution: t.task_support.map(|ts| legacy::ToolExecution {
                task_support: Some(ts.into()),
            }),
            icons: t.icons.into_iter().map(Into::into).collect(),
            input_schema,
            meta: t.meta,
            name: t.name,
            // The legacy `ToolOutputSchema` requires `type: "object"` literally
            // — but the generated `type_` is a plain `String`, so a schema like
            // `{"type":"array"}` (from `-> Json<Vec<T>>`, legal on 2026-07-28
            // where outputSchema is any JSON Schema) deserializes and would be
            // re-emitted verbatim onto a wire that forbids it.
            //
            // Worse, it would be advertised and then unsatisfiable: the
            // `CallToolResult` conversion below can only carry an *object*
            // `structuredContent`, so a non-object result is dropped and the
            // tool breaks "Servers MUST provide structured results that conform
            // to this schema". Dropping the advertisement is the honest
            // step-down — on this wire the tool returns its text mirror only.
            output_schema: t
                .output_schema
                .and_then(|v| serde_json::from_value::<legacy::ToolOutputSchema>(v).ok())
                .filter(|s| s.type_ == "object"),
            title: t.title,
        }
    }
}

impl From<ToolAnnotations> for legacy::ToolAnnotations {
    fn from(a: ToolAnnotations) -> Self {
        legacy::ToolAnnotations {
            destructive_hint: a.destructive_hint,
            idempotent_hint: a.idempotent_hint,
            open_world_hint: a.open_world_hint,
            read_only_hint: a.read_only_hint,
            title: a.title,
        }
    }
}

impl From<Icon> for legacy::Icon {
    fn from(i: Icon) -> Self {
        legacy::Icon {
            mime_type: i.mime_type,
            sizes: i.sizes,
            src: i.src,
            theme: i.theme.map(|t| match t {
                IconTheme::Light => legacy::IconTheme::Light,
                IconTheme::Dark => legacy::IconTheme::Dark,
            }),
        }
    }
}

impl From<ListToolsResult> for legacy::ListToolsResult {
    fn from(r: ListToolsResult) -> Self {
        legacy::ListToolsResult {
            meta: Map::new(),
            next_cursor: r.next_cursor,
            tools: r.tools.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<CallToolResult> for legacy::CallToolResult {
    fn from(r: CallToolResult) -> Self {
        // The legacy wire requires `structuredContent` to be a JSON object;
        // a non-object neutral value is dropped (documented above).
        let structured_content = match r.structured_content {
            Some(Value::Object(map)) => map,
            _ => Map::new(),
        };
        legacy::CallToolResult {
            content: r.content.into_iter().map(Into::into).collect(),
            is_error: Some(r.is_error),
            meta: Map::new(),
            structured_content,
        }
    }
}

// resources

impl From<Resource> for legacy::Resource {
    fn from(r: Resource) -> Self {
        legacy::Resource {
            annotations: r.annotations.map(Into::into),
            description: r.description,
            icons: r.icons.into_iter().map(Into::into).collect(),
            meta: r.meta,
            mime_type: r.mime_type,
            name: r.name,
            size: r.size.map(|s| i64::try_from(s).unwrap_or(i64::MAX)),
            title: r.title,
            uri: r.uri,
        }
    }
}

impl From<Annotations> for legacy::Annotations {
    fn from(a: Annotations) -> Self {
        legacy::Annotations {
            audience: a.audience.into_iter().map(Into::into).collect(),
            last_modified: a.last_modified,
            priority: a.priority,
        }
    }
}

impl From<ResourceContents> for legacy::ReadResourceResultContentsItem {
    fn from(c: ResourceContents) -> Self {
        match c {
            ResourceContents::Text {
                uri,
                mime_type,
                text,
                meta,
            } => legacy::ReadResourceResultContentsItem::TextResourceContents(
                legacy::TextResourceContents {
                    meta,
                    mime_type,
                    text,
                    uri,
                },
            ),
            ResourceContents::Blob {
                uri,
                mime_type,
                blob,
                meta,
            } => legacy::ReadResourceResultContentsItem::BlobResourceContents(
                legacy::BlobResourceContents {
                    blob,
                    meta,
                    mime_type,
                    uri,
                },
            ),
        }
    }
}

impl From<ResourceContents> for legacy::EmbeddedResourceResource {
    fn from(c: ResourceContents) -> Self {
        match c {
            ResourceContents::Text {
                uri,
                mime_type,
                text,
                meta,
            } => legacy::EmbeddedResourceResource::TextResourceContents(
                legacy::TextResourceContents {
                    meta,
                    mime_type,
                    text,
                    uri,
                },
            ),
            ResourceContents::Blob {
                uri,
                mime_type,
                blob,
                meta,
            } => legacy::EmbeddedResourceResource::BlobResourceContents(
                legacy::BlobResourceContents {
                    blob,
                    meta,
                    mime_type,
                    uri,
                },
            ),
        }
    }
}

impl From<ListResourcesResult> for legacy::ListResourcesResult {
    fn from(r: ListResourcesResult) -> Self {
        legacy::ListResourcesResult {
            meta: Map::new(),
            next_cursor: r.next_cursor,
            resources: r.resources.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<ReadResourceResult> for legacy::ReadResourceResult {
    fn from(r: ReadResourceResult) -> Self {
        legacy::ReadResourceResult {
            contents: r.contents.into_iter().map(Into::into).collect(),
            meta: Map::new(),
        }
    }
}

impl From<ResourceTemplate> for legacy::ResourceTemplate {
    fn from(t: ResourceTemplate) -> Self {
        legacy::ResourceTemplate {
            annotations: t.annotations.map(Into::into),
            description: t.description,
            icons: t.icons.into_iter().map(Into::into).collect(),
            meta: t.meta,
            mime_type: t.mime_type,
            name: t.name,
            title: t.title,
            uri_template: t.uri_template,
        }
    }
}

impl From<ListResourceTemplatesResult> for legacy::ListResourceTemplatesResult {
    fn from(r: ListResourceTemplatesResult) -> Self {
        legacy::ListResourceTemplatesResult {
            meta: Map::new(),
            next_cursor: r.next_cursor,
            resource_templates: r.resource_templates.into_iter().map(Into::into).collect(),
        }
    }
}

// prompts

impl From<Role> for legacy::Role {
    fn from(r: Role) -> Self {
        match r {
            Role::User => legacy::Role::User,
            Role::Assistant => legacy::Role::Assistant,
        }
    }
}

impl From<PromptArgument> for legacy::PromptArgument {
    fn from(a: PromptArgument) -> Self {
        legacy::PromptArgument {
            description: a.description,
            name: a.name,
            required: Some(a.required),
            title: a.title,
        }
    }
}

impl From<Prompt> for legacy::Prompt {
    fn from(p: Prompt) -> Self {
        legacy::Prompt {
            arguments: p.arguments.into_iter().map(Into::into).collect(),
            description: p.description,
            icons: p.icons.into_iter().map(Into::into).collect(),
            meta: p.meta,
            name: p.name,
            title: p.title,
        }
    }
}

impl From<PromptMessage> for legacy::PromptMessage {
    fn from(m: PromptMessage) -> Self {
        legacy::PromptMessage {
            content: m.content.into(),
            role: m.role.into(),
        }
    }
}

impl From<ListPromptsResult> for legacy::ListPromptsResult {
    fn from(r: ListPromptsResult) -> Self {
        legacy::ListPromptsResult {
            meta: Map::new(),
            next_cursor: r.next_cursor,
            prompts: r.prompts.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<GetPromptResult> for legacy::GetPromptResult {
    fn from(r: GetPromptResult) -> Self {
        legacy::GetPromptResult {
            description: r.description,
            messages: r.messages.into_iter().map(Into::into).collect(),
            meta: Map::new(),
        }
    }
}

// completions

impl From<CompleteResult> for legacy::CompleteResult {
    fn from(r: CompleteResult) -> Self {
        let r = r.capped();
        legacy::CompleteResult {
            completion: legacy::CompleteResultCompletion {
                has_more: r.has_more,
                total: r.total.map(i64::from),
                values: r.values,
            },
            meta: Map::new(),
        }
    }
}

// ---- wire → neutral conversions (the client's inbound path) -------------------
//
// The inverse of every conversion above: a client deserializes a server's
// result into the negotiated version's wire type (codegenned, spec-exact) and
// narrows it to the neutral common subset. Version-specific envelope fields
// (`resultType` / `cacheScope` / `ttlMs` / `_meta` / annotations / icons) are
// dropped — they aren't part of the cross-version surface a client consumes.
//
// **Content is fully modeled.** Neutral `Content` covers every wire content
// block kind — text, image, audio, embedded resource, and resource link — so
// inbound conversion is total (no lossy JSON-text fallback).

impl From<v0728::ContentBlock> for Content {
    fn from(c: v0728::ContentBlock) -> Self {
        match c {
            v0728::ContentBlock::TextContent(t) => Content::Text {
                text: t.text,
                annotations: t.annotations.map(Into::into),
                meta: t.meta.map(|m| m.0).unwrap_or_default(),
            },
            v0728::ContentBlock::ImageContent(i) => Content::Image {
                data: i.data,
                mime_type: i.mime_type,
                annotations: i.annotations.map(Into::into),
                meta: i.meta.map(|m| m.0).unwrap_or_default(),
            },
            v0728::ContentBlock::AudioContent(a) => Content::Audio {
                data: a.data,
                mime_type: a.mime_type,
                annotations: a.annotations.map(Into::into),
                meta: a.meta.map(|m| m.0).unwrap_or_default(),
            },
            v0728::ContentBlock::EmbeddedResource(e) => Content::Resource {
                contents: e.resource.into(),
                annotations: e.annotations.map(Into::into),
                meta: e.meta.map(|m| m.0).unwrap_or_default(),
            },
            v0728::ContentBlock::ResourceLink(l) => Content::ResourceLink(Box::new(l.into())),
        }
    }
}

impl From<v0728::Tool> for Tool {
    fn from(t: v0728::Tool) -> Self {
        Tool {
            name: t.name,
            title: t.title,
            description: t.description,
            input_schema: serde_json::to_value(&t.input_schema)
                .unwrap_or_else(|_| Value::Object(Map::new())),
            output_schema: t.output_schema.and_then(|s| serde_json::to_value(s).ok()),
            // The draft models Tasks as a server-directed extension, not a
            // per-tool wire field.
            task_support: None,
            annotations: t.annotations.map(Into::into),
            icons: t.icons.into_iter().map(Into::into).collect(),
            meta: t.meta.map(|m| m.0).unwrap_or_default(),
        }
    }
}

impl From<v0728::ToolAnnotations> for ToolAnnotations {
    fn from(a: v0728::ToolAnnotations) -> Self {
        ToolAnnotations {
            title: a.title,
            read_only_hint: a.read_only_hint,
            destructive_hint: a.destructive_hint,
            idempotent_hint: a.idempotent_hint,
            open_world_hint: a.open_world_hint,
        }
    }
}

impl From<v0728::Icon> for Icon {
    fn from(i: v0728::Icon) -> Self {
        Icon {
            src: i.src,
            mime_type: i.mime_type,
            sizes: i.sizes,
            theme: i.theme.map(|t| match t {
                v0728::IconTheme::Light => IconTheme::Light,
                v0728::IconTheme::Dark => IconTheme::Dark,
            }),
        }
    }
}

impl From<v0728::ListToolsResult> for ListToolsResult {
    fn from(r: v0728::ListToolsResult) -> Self {
        ListToolsResult {
            tools: r.tools.into_iter().map(Into::into).collect(),
            next_cursor: r.next_cursor,
            cache: Some(CachePolicy::from_wire(
                r.ttl_ms,
                match r.cache_scope {
                    v0728::ListToolsResultCacheScope::Public => CacheScope::Public,
                    v0728::ListToolsResultCacheScope::Private => CacheScope::Private,
                },
            )),
        }
    }
}

impl From<v0728::CallToolResult> for CallToolResult {
    fn from(r: v0728::CallToolResult) -> Self {
        CallToolResult {
            content: r.content.into_iter().map(Into::into).collect(),
            is_error: r.is_error.unwrap_or(false),
            structured_content: r.structured_content,
        }
    }
}

impl From<v0728::Resource> for Resource {
    fn from(r: v0728::Resource) -> Self {
        Resource {
            uri: r.uri,
            name: r.name,
            title: r.title,
            description: r.description,
            mime_type: r.mime_type,
            size: r.size.map(|s| u64::try_from(s).unwrap_or(0)),
            annotations: r.annotations.map(Into::into),
            icons: r.icons.into_iter().map(Into::into).collect(),
            meta: r.meta.map(|m| m.0).unwrap_or_default(),
        }
    }
}

impl From<v0728::Annotations> for Annotations {
    fn from(a: v0728::Annotations) -> Self {
        Annotations {
            audience: a.audience.into_iter().map(Into::into).collect(),
            priority: a.priority,
            last_modified: a.last_modified,
        }
    }
}

impl From<v0728::ResourceLink> for Resource {
    fn from(l: v0728::ResourceLink) -> Self {
        Resource {
            uri: l.uri,
            name: l.name,
            title: l.title,
            description: l.description,
            mime_type: l.mime_type,
            size: l.size.map(|s| u64::try_from(s).unwrap_or(0)),
            annotations: l.annotations.map(Into::into),
            icons: l.icons.into_iter().map(Into::into).collect(),
            meta: l.meta.map(|m| m.0).unwrap_or_default(),
        }
    }
}

impl From<v0728::EmbeddedResourceResource> for ResourceContents {
    fn from(r: v0728::EmbeddedResourceResource) -> Self {
        match r {
            v0728::EmbeddedResourceResource::TextResourceContents(t) => ResourceContents::Text {
                uri: t.uri,
                mime_type: t.mime_type,
                text: t.text,
                meta: t.meta.map(|m| m.0).unwrap_or_default(),
            },
            v0728::EmbeddedResourceResource::BlobResourceContents(b) => ResourceContents::Blob {
                uri: b.uri,
                mime_type: b.mime_type,
                blob: b.blob,
                meta: b.meta.map(|m| m.0).unwrap_or_default(),
            },
        }
    }
}

impl From<v0728::ReadResourceResultContentsItem> for ResourceContents {
    fn from(c: v0728::ReadResourceResultContentsItem) -> Self {
        match c {
            v0728::ReadResourceResultContentsItem::TextResourceContents(t) => {
                ResourceContents::Text {
                    uri: t.uri,
                    mime_type: t.mime_type,
                    text: t.text,
                    meta: t.meta.map(|m| m.0).unwrap_or_default(),
                }
            }
            v0728::ReadResourceResultContentsItem::BlobResourceContents(b) => {
                ResourceContents::Blob {
                    uri: b.uri,
                    mime_type: b.mime_type,
                    blob: b.blob,
                    meta: b.meta.map(|m| m.0).unwrap_or_default(),
                }
            }
        }
    }
}

impl From<v0728::ListResourcesResult> for ListResourcesResult {
    fn from(r: v0728::ListResourcesResult) -> Self {
        ListResourcesResult {
            resources: r.resources.into_iter().map(Into::into).collect(),
            next_cursor: r.next_cursor,
            cache: Some(CachePolicy::from_wire(
                r.ttl_ms,
                match r.cache_scope {
                    v0728::ListResourcesResultCacheScope::Public => CacheScope::Public,
                    v0728::ListResourcesResultCacheScope::Private => CacheScope::Private,
                },
            )),
        }
    }
}

impl From<v0728::ReadResourceResult> for ReadResourceResult {
    fn from(r: v0728::ReadResourceResult) -> Self {
        ReadResourceResult {
            contents: r.contents.into_iter().map(Into::into).collect(),
            cache: Some(CachePolicy::from_wire(
                r.ttl_ms,
                match r.cache_scope {
                    v0728::ReadResourceResultCacheScope::Public => CacheScope::Public,
                    v0728::ReadResourceResultCacheScope::Private => CacheScope::Private,
                },
            )),
        }
    }
}

impl From<v0728::ResourceTemplate> for ResourceTemplate {
    fn from(t: v0728::ResourceTemplate) -> Self {
        ResourceTemplate {
            uri_template: t.uri_template,
            name: t.name,
            title: t.title,
            description: t.description,
            mime_type: t.mime_type,
            annotations: t.annotations.map(Into::into),
            icons: t.icons.into_iter().map(Into::into).collect(),
            meta: t.meta.map(|m| m.0).unwrap_or_default(),
        }
    }
}

impl From<v0728::ListResourceTemplatesResult> for ListResourceTemplatesResult {
    fn from(r: v0728::ListResourceTemplatesResult) -> Self {
        ListResourceTemplatesResult {
            resource_templates: r.resource_templates.into_iter().map(Into::into).collect(),
            next_cursor: r.next_cursor,
            cache: Some(CachePolicy::from_wire(
                r.ttl_ms,
                match r.cache_scope {
                    v0728::ListResourceTemplatesResultCacheScope::Public => CacheScope::Public,
                    v0728::ListResourceTemplatesResultCacheScope::Private => CacheScope::Private,
                },
            )),
        }
    }
}

impl From<v0728::Role> for Role {
    fn from(r: v0728::Role) -> Self {
        match r {
            v0728::Role::User => Role::User,
            v0728::Role::Assistant => Role::Assistant,
        }
    }
}

impl From<v0728::PromptArgument> for PromptArgument {
    fn from(a: v0728::PromptArgument) -> Self {
        PromptArgument {
            name: a.name,
            title: a.title,
            description: a.description,
            required: a.required.unwrap_or(false),
        }
    }
}

impl From<v0728::Prompt> for Prompt {
    fn from(p: v0728::Prompt) -> Self {
        Prompt {
            name: p.name,
            title: p.title,
            description: p.description,
            arguments: p.arguments.into_iter().map(Into::into).collect(),
            icons: p.icons.into_iter().map(Into::into).collect(),
            meta: p.meta.map(|m| m.0).unwrap_or_default(),
        }
    }
}

impl From<v0728::PromptMessage> for PromptMessage {
    fn from(m: v0728::PromptMessage) -> Self {
        PromptMessage {
            role: m.role.into(),
            content: m.content.into(),
        }
    }
}

impl From<v0728::ListPromptsResult> for ListPromptsResult {
    fn from(r: v0728::ListPromptsResult) -> Self {
        ListPromptsResult {
            prompts: r.prompts.into_iter().map(Into::into).collect(),
            next_cursor: r.next_cursor,
            cache: Some(CachePolicy::from_wire(
                r.ttl_ms,
                match r.cache_scope {
                    v0728::ListPromptsResultCacheScope::Public => CacheScope::Public,
                    v0728::ListPromptsResultCacheScope::Private => CacheScope::Private,
                },
            )),
        }
    }
}

impl From<v0728::GetPromptResult> for GetPromptResult {
    fn from(r: v0728::GetPromptResult) -> Self {
        GetPromptResult {
            description: r.description,
            messages: r.messages.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v0728::CompleteResult> for CompleteResult {
    fn from(r: v0728::CompleteResult) -> Self {
        CompleteResult {
            values: r.completion.values,
            total: r
                .completion
                .total
                .map(|t| u32::try_from(t).unwrap_or(u32::MAX)),
            has_more: r.completion.has_more,
        }
    }
}

// ---- 2025-11-25 wire → neutral ------------------------------------------------

impl From<legacy::ContentBlock> for Content {
    fn from(c: legacy::ContentBlock) -> Self {
        match c {
            legacy::ContentBlock::TextContent(t) => Content::Text {
                text: t.text,
                annotations: t.annotations.map(Into::into),
                meta: t.meta,
            },
            legacy::ContentBlock::ImageContent(i) => Content::Image {
                data: i.data,
                mime_type: i.mime_type,
                annotations: i.annotations.map(Into::into),
                meta: i.meta,
            },
            legacy::ContentBlock::AudioContent(a) => Content::Audio {
                data: a.data,
                mime_type: a.mime_type,
                annotations: a.annotations.map(Into::into),
                meta: a.meta,
            },
            legacy::ContentBlock::EmbeddedResource(e) => Content::Resource {
                contents: e.resource.into(),
                annotations: e.annotations.map(Into::into),
                meta: e.meta,
            },
            legacy::ContentBlock::ResourceLink(l) => Content::ResourceLink(Box::new(l.into())),
        }
    }
}

impl From<legacy::Tool> for Tool {
    fn from(t: legacy::Tool) -> Self {
        Tool {
            name: t.name,
            title: t.title,
            description: t.description,
            input_schema: serde_json::to_value(&t.input_schema)
                .unwrap_or_else(|_| Value::Object(Map::new())),
            output_schema: t.output_schema.and_then(|s| serde_json::to_value(s).ok()),
            task_support: t.execution.and_then(|e| e.task_support).map(Into::into),
            annotations: t.annotations.map(Into::into),
            icons: t.icons.into_iter().map(Into::into).collect(),
            meta: t.meta,
        }
    }
}

impl From<legacy::ToolAnnotations> for ToolAnnotations {
    fn from(a: legacy::ToolAnnotations) -> Self {
        ToolAnnotations {
            title: a.title,
            read_only_hint: a.read_only_hint,
            destructive_hint: a.destructive_hint,
            idempotent_hint: a.idempotent_hint,
            open_world_hint: a.open_world_hint,
        }
    }
}

impl From<legacy::Icon> for Icon {
    fn from(i: legacy::Icon) -> Self {
        Icon {
            src: i.src,
            mime_type: i.mime_type,
            sizes: i.sizes,
            theme: i.theme.map(|t| match t {
                legacy::IconTheme::Light => IconTheme::Light,
                legacy::IconTheme::Dark => IconTheme::Dark,
            }),
        }
    }
}

impl From<legacy::ListToolsResult> for ListToolsResult {
    fn from(r: legacy::ListToolsResult) -> Self {
        ListToolsResult {
            tools: r.tools.into_iter().map(Into::into).collect(),
            next_cursor: r.next_cursor,
            // The 2025-11-25 wire has no cache fields (SEP-2549 is draft-only).
            cache: None,
        }
    }
}

impl From<legacy::CallToolResult> for CallToolResult {
    fn from(r: legacy::CallToolResult) -> Self {
        CallToolResult {
            content: r.content.into_iter().map(Into::into).collect(),
            is_error: r.is_error.unwrap_or(false),
            structured_content: if r.structured_content.is_empty() {
                None
            } else {
                Some(Value::Object(r.structured_content))
            },
        }
    }
}

impl From<legacy::Resource> for Resource {
    fn from(r: legacy::Resource) -> Self {
        Resource {
            uri: r.uri,
            name: r.name,
            title: r.title,
            description: r.description,
            mime_type: r.mime_type,
            size: r.size.map(|s| u64::try_from(s).unwrap_or(0)),
            annotations: r.annotations.map(Into::into),
            icons: r.icons.into_iter().map(Into::into).collect(),
            meta: r.meta,
        }
    }
}

impl From<legacy::Annotations> for Annotations {
    fn from(a: legacy::Annotations) -> Self {
        Annotations {
            audience: a.audience.into_iter().map(Into::into).collect(),
            priority: a.priority,
            last_modified: a.last_modified,
        }
    }
}

impl From<legacy::ResourceLink> for Resource {
    fn from(l: legacy::ResourceLink) -> Self {
        Resource {
            uri: l.uri,
            name: l.name,
            title: l.title,
            description: l.description,
            mime_type: l.mime_type,
            size: l.size.map(|s| u64::try_from(s).unwrap_or(0)),
            annotations: l.annotations.map(Into::into),
            icons: l.icons.into_iter().map(Into::into).collect(),
            meta: l.meta,
        }
    }
}

impl From<legacy::EmbeddedResourceResource> for ResourceContents {
    fn from(r: legacy::EmbeddedResourceResource) -> Self {
        match r {
            legacy::EmbeddedResourceResource::TextResourceContents(t) => ResourceContents::Text {
                uri: t.uri,
                mime_type: t.mime_type,
                text: t.text,
                meta: t.meta,
            },
            legacy::EmbeddedResourceResource::BlobResourceContents(b) => ResourceContents::Blob {
                uri: b.uri,
                mime_type: b.mime_type,
                blob: b.blob,
                meta: b.meta,
            },
        }
    }
}

impl From<legacy::ReadResourceResultContentsItem> for ResourceContents {
    fn from(c: legacy::ReadResourceResultContentsItem) -> Self {
        match c {
            legacy::ReadResourceResultContentsItem::TextResourceContents(t) => {
                ResourceContents::Text {
                    uri: t.uri,
                    mime_type: t.mime_type,
                    text: t.text,
                    meta: t.meta,
                }
            }
            legacy::ReadResourceResultContentsItem::BlobResourceContents(b) => {
                ResourceContents::Blob {
                    uri: b.uri,
                    mime_type: b.mime_type,
                    blob: b.blob,
                    meta: b.meta,
                }
            }
        }
    }
}

impl From<legacy::ListResourcesResult> for ListResourcesResult {
    fn from(r: legacy::ListResourcesResult) -> Self {
        ListResourcesResult {
            resources: r.resources.into_iter().map(Into::into).collect(),
            next_cursor: r.next_cursor,
            cache: None,
        }
    }
}

impl From<legacy::ReadResourceResult> for ReadResourceResult {
    fn from(r: legacy::ReadResourceResult) -> Self {
        ReadResourceResult {
            contents: r.contents.into_iter().map(Into::into).collect(),
            cache: None,
        }
    }
}

impl From<legacy::ResourceTemplate> for ResourceTemplate {
    fn from(t: legacy::ResourceTemplate) -> Self {
        ResourceTemplate {
            uri_template: t.uri_template,
            name: t.name,
            title: t.title,
            description: t.description,
            mime_type: t.mime_type,
            annotations: t.annotations.map(Into::into),
            icons: t.icons.into_iter().map(Into::into).collect(),
            meta: t.meta,
        }
    }
}

impl From<legacy::ListResourceTemplatesResult> for ListResourceTemplatesResult {
    fn from(r: legacy::ListResourceTemplatesResult) -> Self {
        ListResourceTemplatesResult {
            resource_templates: r.resource_templates.into_iter().map(Into::into).collect(),
            next_cursor: r.next_cursor,
            cache: None,
        }
    }
}

impl From<legacy::Role> for Role {
    fn from(r: legacy::Role) -> Self {
        match r {
            legacy::Role::User => Role::User,
            legacy::Role::Assistant => Role::Assistant,
        }
    }
}

impl From<legacy::PromptArgument> for PromptArgument {
    fn from(a: legacy::PromptArgument) -> Self {
        PromptArgument {
            name: a.name,
            title: a.title,
            description: a.description,
            required: a.required.unwrap_or(false),
        }
    }
}

impl From<legacy::Prompt> for Prompt {
    fn from(p: legacy::Prompt) -> Self {
        Prompt {
            name: p.name,
            title: p.title,
            description: p.description,
            arguments: p.arguments.into_iter().map(Into::into).collect(),
            icons: p.icons.into_iter().map(Into::into).collect(),
            meta: p.meta,
        }
    }
}

impl From<legacy::PromptMessage> for PromptMessage {
    fn from(m: legacy::PromptMessage) -> Self {
        PromptMessage {
            role: m.role.into(),
            content: m.content.into(),
        }
    }
}

impl From<legacy::ListPromptsResult> for ListPromptsResult {
    fn from(r: legacy::ListPromptsResult) -> Self {
        ListPromptsResult {
            prompts: r.prompts.into_iter().map(Into::into).collect(),
            next_cursor: r.next_cursor,
            cache: None,
        }
    }
}

impl From<legacy::GetPromptResult> for GetPromptResult {
    fn from(r: legacy::GetPromptResult) -> Self {
        GetPromptResult {
            description: r.description,
            messages: r.messages.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<legacy::CompleteResult> for CompleteResult {
    fn from(r: legacy::CompleteResult) -> Self {
        CompleteResult {
            values: r.completion.values,
            total: r
                .completion
                .total
                .map(|t| u32::try_from(t).unwrap_or(u32::MAX)),
            has_more: r.completion.has_more,
        }
    }
}

// ---- 2025-06-18 ----------------------------------------------------------------
//
// `2025-06-18` is `2025-11-25` minus a closed list of additions, so its
// conversions are a step down from the `legacy` ones rather than a third
// hand-written set — see [`crate::v2025_06_18::convert`] for why, and for the
// per-type detail of what each step drops. These are only the entry points the
// dispatcher and client name; everything they touch is converted there.

/// Both directions of the neutral bridge for one result type, routed through
/// the `2025-11-25` wire.
macro_rules! neutral_via_legacy {
    ($($ty:ident),+ $(,)?) => {$(
        impl From<$ty> for v06::$ty {
            fn from(n: $ty) -> Self {
                legacy::$ty::from(n).into()
            }
        }

        impl From<v06::$ty> for $ty {
            fn from(w: v06::$ty) -> Self {
                legacy::$ty::from(w).into()
            }
        }
    )+};
}

neutral_via_legacy!(
    // Results — the dispatch surface.
    ListToolsResult,
    CallToolResult,
    ListResourcesResult,
    ListResourceTemplatesResult,
    ReadResourceResult,
    ListPromptsResult,
    GetPromptResult,
    CompleteResult,
    // Elements, matching what the `2025-11-25` family exposes.
    Tool,
    ToolAnnotations,
    Resource,
    ResourceTemplate,
    Prompt,
    PromptArgument,
    PromptMessage,
    Annotations,
    Role,
);

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// One declaration, three wires. A revision that cannot express a
    /// sub-capability must not be told about it: `2025-06-18` has bare
    /// `elicitation`/`sampling` objects, and `2026-07-28` dropped
    /// `roots.listChanged`.
    #[test]
    fn capabilities_render_per_revision() {
        let caps = ClientCapabilities {
            elicitation: Some(ElicitationCapability {
                form: true,
                url: true,
            }),
            sampling: Some(SamplingCapability {
                context: true,
                tools: true,
            }),
            roots: Some(RootsCapability { list_changed: true }),
            experimental: None,
            extensions: None,
        };
        assert_eq!(
            caps.to_wire(ProtocolVersion::V2025_11_25),
            json!({
                "elicitation": { "form": {}, "url": {} },
                "sampling": { "context": {}, "tools": {} },
                "roots": { "listChanged": true },
            })
        );
        assert_eq!(
            caps.to_wire(ProtocolVersion::V2025_06_18),
            json!({
                "elicitation": {},
                "sampling": {},
                "roots": { "listChanged": true },
            })
        );
        assert_eq!(
            caps.to_wire(ProtocolVersion::V2026_07_28),
            json!({
                "elicitation": { "form": {}, "url": {} },
                "sampling": { "context": {}, "tools": {} },
                "roots": {},
            })
        );
    }

    /// Declaring nothing is `{}`, and a capability with no sub-capability is
    /// still the capability. Both are statements a server acts on, so neither
    /// may collapse to an absent key.
    #[test]
    fn an_empty_declaration_is_still_an_object() {
        assert_eq!(
            ClientCapabilities::new().to_wire(ProtocolVersion::V2025_11_25),
            json!({})
        );
        let form_only = ClientCapabilities {
            elicitation: Some(ElicitationCapability {
                form: true,
                url: false,
            }),
            ..ClientCapabilities::new()
        };
        assert_eq!(
            form_only.to_wire(ProtocolVersion::V2025_11_25),
            json!({ "elicitation": { "form": {} } }),
            "a form-only client must not read as url-capable"
        );
    }

    /// A non-object `outputSchema` is legal on `2026-07-28` and forbidden on
    /// the legacy wires, so it must not survive the step-down.
    ///
    /// `-> Json<Vec<T>>` produces `{"type":"array", …}`. The legacy
    /// `ToolOutputSchema.type` is an unchecked `String`, so it used to
    /// round-trip onto a wire whose schema requires the literal `"object"` —
    /// and the paired `structuredContent` was dropped on the same call, leaving
    /// the tool advertising a schema it could never satisfy.
    #[test]
    fn a_non_object_output_schema_does_not_reach_the_legacy_wires() {
        let array = Tool::new("rows", json!({"type": "object", "properties": {}}))
            .with_output_schema(json!({ "type": "array", "items": { "type": "string" } }));

        let draft: v0728::Tool = array.clone().into();
        assert!(
            draft.output_schema.is_some(),
            "2026-07-28 allows any JSON Schema, so it keeps it"
        );

        let legacy: legacy::Tool = array.into();
        assert!(
            legacy.output_schema.is_none(),
            "a wire that requires type=object must not be handed type=array"
        );

        // An object schema still crosses, so this drops nothing it shouldn't.
        let object = Tool::new("stats", json!({"type": "object", "properties": {}}))
            .with_output_schema(
                json!({ "type": "object", "properties": { "n": { "type": "integer" } } }),
            );
        let legacy: legacy::Tool = object.into();
        assert_eq!(
            legacy.output_schema.expect("object schemas survive").type_,
            "object"
        );
    }

    #[test]
    fn tool_widens_to_draft_wire() {
        let neutral = Tool::new("echo", json!({"type": "object", "properties": {}}))
            .with_description("Echoes input");
        let wire: v0728::Tool = neutral.into();
        assert_eq!(wire.name, "echo");
        assert_eq!(wire.input_schema.type_, "object");
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["name"], "echo");
        assert_eq!(v["inputSchema"]["type"], "object");
    }

    #[test]
    fn call_result_carries_result_type_and_is_error() {
        let wire: v0728::CallToolResult = CallToolResult::error("boom").into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["resultType"], "complete");
        assert_eq!(v["isError"], true);
        assert_eq!(v["content"][0]["type"], "text");
        assert_eq!(v["content"][0]["text"], "boom");
    }

    #[test]
    fn list_result_fills_draft_required_fields() {
        let wire: v0728::ListToolsResult =
            ListToolsResult::new(alloc::vec![Tool::new("a", json!({"type": "object"}))]).into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["resultType"], "complete");
        assert_eq!(v["cacheScope"], "private");
        assert_eq!(v["ttlMs"], 0);
        assert_eq!(v["tools"][0]["name"], "a");
    }

    #[test]
    fn read_resource_text_widens_to_wire_union() {
        let wire: v0728::ReadResourceResult = ReadResourceResult::text("file://a", "hi").into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["resultType"], "complete");
        assert_eq!(v["cacheScope"], "private");
        assert_eq!(v["contents"][0]["uri"], "file://a");
        assert_eq!(v["contents"][0]["text"], "hi");
    }

    #[test]
    fn list_resources_and_templates_fill_required_fields() {
        let res: v0728::ListResourcesResult = ListResourcesResult::new(alloc::vec![
            Resource::new("file://a", "a").with_mime_type("text/plain"),
        ])
        .into();
        let v = serde_json::to_value(&res).unwrap();
        assert_eq!(v["resultType"], "complete");
        assert_eq!(v["resources"][0]["mimeType"], "text/plain");

        let templates: v0728::ListResourceTemplatesResult = ListResourceTemplatesResult::new(
            alloc::vec![ResourceTemplate::new("file://{path}", "files",)],
        )
        .into();
        let v = serde_json::to_value(&templates).unwrap();
        assert_eq!(v["resourceTemplates"][0]["uriTemplate"], "file://{path}");
        assert_eq!(v["cacheScope"], "private");
    }

    #[test]
    fn prompt_get_widens_with_roles() {
        let wire: v0728::GetPromptResult = GetPromptResult::new(alloc::vec![
            PromptMessage::user_text("hello"),
            PromptMessage::assistant_text("hi there"),
        ])
        .with_description("greeting")
        .into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["resultType"], "complete");
        assert_eq!(v["description"], "greeting");
        assert_eq!(v["messages"][0]["role"], "user");
        assert_eq!(v["messages"][0]["content"]["text"], "hello");
        assert_eq!(v["messages"][1]["role"], "assistant");
    }

    #[test]
    fn list_prompts_carries_arguments() {
        let wire: v0728::ListPromptsResult = ListPromptsResult::new(alloc::vec![
            Prompt::new("summarize")
                .with_description("Summarize text")
                .with_argument(PromptArgument::new("text").required(true)),
        ])
        .into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["prompts"][0]["name"], "summarize");
        assert_eq!(v["prompts"][0]["arguments"][0]["name"], "text");
        assert_eq!(v["prompts"][0]["arguments"][0]["required"], true);
    }

    #[test]
    fn complete_result_nests_completion() {
        let wire: v0728::CompleteResult = CompleteResult::new(alloc::vec!["foo".to_string()])
            .with_total(1)
            .with_has_more(false)
            .into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["resultType"], "complete");
        assert_eq!(v["completion"]["values"][0], "foo");
        assert_eq!(v["completion"]["total"], 1);
        assert_eq!(v["completion"]["hasMore"], false);
    }

    // ---- legacy (2025-11-25) conversions ----------------------------------

    #[test]
    fn legacy_tool_widens_without_draft_envelope() {
        let wire: legacy::ListToolsResult = ListToolsResult::new(alloc::vec![
            Tool::new(
                "echo",
                json!({"type": "object", "properties": {"msg": {"type": "string"}}, "required": ["msg"]}),
            )
            .with_description("Echoes input"),
        ])
        .into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["tools"][0]["name"], "echo");
        assert_eq!(v["tools"][0]["inputSchema"]["type"], "object");
        assert_eq!(
            v["tools"][0]["inputSchema"]["properties"]["msg"]["type"],
            "string"
        );
        assert_eq!(v["tools"][0]["inputSchema"]["required"][0], "msg");
        // The draft caching envelope must not leak onto the legacy wire.
        let obj = v.as_object().unwrap();
        assert!(!obj.contains_key("resultType"));
        assert!(!obj.contains_key("cacheScope"));
        assert!(!obj.contains_key("ttlMs"));
    }

    #[test]
    fn legacy_call_result_keeps_object_structured_content_drops_non_object() {
        let mut ok = CallToolResult::text("done");
        ok.structured_content = Some(json!({"answer": 42}));
        let wire: legacy::CallToolResult = ok.into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["isError"], false);
        assert_eq!(v["content"][0]["text"], "done");
        assert_eq!(v["structuredContent"]["answer"], 42);

        let mut bad = CallToolResult::text("done");
        bad.structured_content = Some(json!(7)); // not an object: unrepresentable
        let wire: legacy::CallToolResult = bad.into();
        let v = serde_json::to_value(&wire).unwrap();
        assert!(v.as_object().unwrap().get("structuredContent").is_none());
    }

    #[test]
    fn legacy_resources_round_trip() {
        let wire: legacy::ListResourcesResult = ListResourcesResult::new(alloc::vec![
            Resource::new("file://a", "a").with_mime_type("text/plain"),
        ])
        .into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["resources"][0]["uri"], "file://a");
        assert_eq!(v["resources"][0]["mimeType"], "text/plain");

        let wire: legacy::ReadResourceResult = ReadResourceResult::text("file://a", "hi").into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["contents"][0]["text"], "hi");

        let wire: legacy::ListResourceTemplatesResult = ListResourceTemplatesResult::new(
            alloc::vec![ResourceTemplate::new("file://{path}", "files")],
        )
        .into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["resourceTemplates"][0]["uriTemplate"], "file://{path}");
    }

    #[test]
    fn legacy_prompts_and_completion_round_trip() {
        let wire: legacy::ListPromptsResult = ListPromptsResult::new(alloc::vec![
            Prompt::new("summarize").with_argument(PromptArgument::new("text").required(true)),
        ])
        .into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["prompts"][0]["arguments"][0]["required"], true);

        let wire: legacy::GetPromptResult =
            GetPromptResult::new(alloc::vec![PromptMessage::user_text("hello")]).into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["messages"][0]["role"], "user");
        assert_eq!(v["messages"][0]["content"]["type"], "text");

        let wire: legacy::CompleteResult = CompleteResult::new(alloc::vec!["x".to_string()])
            .with_total(5)
            .into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["completion"]["total"], 5);
        assert_eq!(v["completion"]["values"][0], "x");
    }

    // ---- wire → neutral (the client's inbound path) -----------------------
    //
    // Round-trip the common subset: neutral → version wire → neutral must
    // preserve every field neutral models. The version envelope (resultType,
    // cacheScope, …) is dropped on the way back, which is the point.

    #[test]
    fn draft_tools_round_trip_through_neutral() {
        let original = ListToolsResult::new(alloc::vec![
            Tool::new("echo", json!({"type": "object"}))
                .with_title("Echo")
                .with_description("Echoes input"),
        ]);
        let wire: v0728::ListToolsResult = original.into();
        let back: ListToolsResult = wire.into();
        assert_eq!(back.tools.len(), 1);
        assert_eq!(back.tools[0].name, "echo");
        assert_eq!(back.tools[0].title.as_deref(), Some("Echo"));
        assert_eq!(back.tools[0].description.as_deref(), Some("Echoes input"));
        assert_eq!(back.tools[0].input_schema["type"], "object");
    }

    #[test]
    fn legacy_tools_round_trip_through_neutral() {
        let original =
            ListToolsResult::new(alloc::vec![Tool::new("add", json!({"type": "object"}))]);
        let wire: legacy::ListToolsResult = original.into();
        let back: ListToolsResult = wire.into();
        assert_eq!(back.tools[0].name, "add");
    }

    /// A tool carrying every metadata surface — annotations (all five hints),
    /// icons (with theme), and namespaced `_meta`.
    fn metadata_tool() -> Tool {
        Tool::new("audit", json!({"type": "object"}))
            .with_title("Audit")
            .with_annotations(
                ToolAnnotations::new()
                    .read_only()
                    .destructive(false)
                    .idempotent(true)
                    .open_world(false),
            )
            .with_icon(Icon {
                src: "https://example.com/audit.png".into(),
                mime_type: Some("image/png".into()),
                sizes: alloc::vec!["48x48".into()],
                theme: Some(IconTheme::Dark),
            })
            .with_meta_entry("com.example/tags", json!(["read", "safety"]))
    }

    fn assert_metadata_preserved(back: &Tool) {
        let a = back.annotations.as_ref().expect("annotations survive");
        assert_eq!(a.read_only_hint, Some(true));
        assert_eq!(a.destructive_hint, Some(false));
        assert_eq!(a.idempotent_hint, Some(true));
        assert_eq!(a.open_world_hint, Some(false));
        assert_eq!(back.icons.len(), 1);
        assert_eq!(back.icons[0].src, "https://example.com/audit.png");
        assert_eq!(back.icons[0].mime_type.as_deref(), Some("image/png"));
        assert_eq!(back.icons[0].sizes, alloc::vec!["48x48".to_string()]);
        assert_eq!(back.icons[0].theme, Some(IconTheme::Dark));
        assert_eq!(back.meta["com.example/tags"], json!(["read", "safety"]));
    }

    #[test]
    fn tool_annotations_icons_and_meta_round_trip_the_draft_wire() {
        let wire: v0728::Tool = metadata_tool().into();
        let v = serde_json::to_value(&wire).unwrap();
        // Exact spec wire names.
        assert_eq!(v["annotations"]["readOnlyHint"], json!(true));
        assert_eq!(v["annotations"]["destructiveHint"], json!(false));
        assert_eq!(v["annotations"]["idempotentHint"], json!(true));
        assert_eq!(v["annotations"]["openWorldHint"], json!(false));
        assert_eq!(v["icons"][0]["src"], "https://example.com/audit.png");
        assert_eq!(v["icons"][0]["theme"], "dark");
        assert_eq!(v["_meta"]["com.example/tags"], json!(["read", "safety"]));
        let back: Tool = wire.into();
        assert_metadata_preserved(&back);
    }

    #[test]
    fn tool_annotations_icons_and_meta_round_trip_the_legacy_wire() {
        let wire: legacy::Tool = metadata_tool().into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["annotations"]["readOnlyHint"], json!(true));
        assert_eq!(v["icons"][0]["theme"], "dark");
        assert_eq!(v["_meta"]["com.example/tags"], json!(["read", "safety"]));
        let back: Tool = wire.into();
        assert_metadata_preserved(&back);
    }

    #[test]
    fn absent_tool_metadata_stays_absent_on_the_wire() {
        let wire: v0728::Tool = Tool::new("plain", json!({"type": "object"})).into();
        let v = serde_json::to_value(&wire).unwrap();
        assert!(v.get("annotations").is_none(), "no annotations key: {v}");
        assert!(v.get("icons").is_none(), "no icons key: {v}");
        assert!(v.get("_meta").is_none(), "no _meta key: {v}");
    }

    /// A resource carrying every metadata surface: spec `Annotations`
    /// (audience/priority/lastModified), an icon, and namespaced `_meta`.
    fn metadata_resource() -> Resource {
        Resource::new("mem://doc", "doc")
            .with_annotations(
                Annotations::new()
                    .for_audience(Role::User)
                    .priority(0.75)
                    .last_modified("2026-07-20T00:00:00Z"),
            )
            .with_icon(Icon::new("https://example.com/doc.png"))
            .with_meta_entry("com.example/tags", json!(["docs"]))
    }

    fn assert_resource_metadata(back: &Resource) {
        let a = back.annotations.as_ref().expect("annotations survive");
        assert_eq!(a.audience, alloc::vec![Role::User]);
        assert_eq!(a.priority, Some(0.75));
        assert_eq!(a.last_modified.as_deref(), Some("2026-07-20T00:00:00Z"));
        assert_eq!(back.icons[0].src, "https://example.com/doc.png");
        assert_eq!(back.meta["com.example/tags"], json!(["docs"]));
    }

    #[test]
    fn resource_metadata_round_trips_both_wires() {
        let draft_wire: v0728::Resource = metadata_resource().into();
        let v = serde_json::to_value(&draft_wire).unwrap();
        assert_eq!(v["annotations"]["audience"], json!(["user"]));
        assert_eq!(v["annotations"]["priority"], json!(0.75));
        assert_eq!(v["annotations"]["lastModified"], "2026-07-20T00:00:00Z");
        assert_eq!(v["_meta"]["com.example/tags"], json!(["docs"]));
        let back: Resource = draft_wire.into();
        assert_resource_metadata(&back);

        let legacy_wire: legacy::Resource = metadata_resource().into();
        let back: Resource = legacy_wire.into();
        assert_resource_metadata(&back);
    }

    #[test]
    fn resource_template_and_prompt_metadata_round_trip_both_wires() {
        let template = ResourceTemplate::new("file://{path}", "files")
            .with_annotations(Annotations::new().for_audience(Role::Assistant))
            .with_meta_entry("com.example/kind", json!("fs"));
        let draft_wire: v0728::ResourceTemplate = template.clone().into();
        let back: ResourceTemplate = draft_wire.into();
        assert_eq!(
            back.annotations.as_ref().unwrap().audience,
            alloc::vec![Role::Assistant]
        );
        assert_eq!(back.meta["com.example/kind"], json!("fs"));
        let legacy_wire: legacy::ResourceTemplate = template.into();
        let back: ResourceTemplate = legacy_wire.into();
        assert_eq!(back.meta["com.example/kind"], json!("fs"));

        let prompt = Prompt::new("summarize")
            .with_icon(Icon::new("https://example.com/p.png"))
            .with_meta_entry("com.example/category", json!("text"));
        let draft_wire: v0728::Prompt = prompt.clone().into();
        let back: Prompt = draft_wire.into();
        assert_eq!(back.icons[0].src, "https://example.com/p.png");
        assert_eq!(back.meta["com.example/category"], json!("text"));
        let legacy_wire: legacy::Prompt = prompt.into();
        let back: Prompt = legacy_wire.into();
        assert_eq!(back.icons[0].src, "https://example.com/p.png");
    }

    #[test]
    fn resource_link_content_carries_metadata_both_wires() {
        let content = Content::ResourceLink(Box::new(metadata_resource()));
        let wire: v0728::ContentBlock = content.clone().into();
        let back: Content = wire.into();
        let Content::ResourceLink(r) = back else {
            panic!("resource link survives");
        };
        assert_resource_metadata(&r);

        let wire: legacy::ContentBlock = content.into();
        let back: Content = wire.into();
        let Content::ResourceLink(r) = back else {
            panic!("resource link survives");
        };
        assert_resource_metadata(&r);
    }

    #[test]
    fn content_block_annotations_and_meta_round_trip_both_wires() {
        let annotations = Annotations::new()
            .for_audience(Role::User)
            .priority(0.5)
            .last_modified("2026-07-21T00:00:00Z");
        for content in [
            Content::text("hi"),
            Content::image("aGk=", "image/png"),
            Content::audio("aGk=", "audio/wav"),
        ] {
            let content = content
                .with_annotations(annotations.clone())
                .with_meta_entry("com.example/source", json!("cache"));

            let wire: v0728::ContentBlock = content.clone().into();
            let v = serde_json::to_value(&wire).unwrap();
            // Exact spec wire names on the block itself.
            assert_eq!(v["annotations"]["audience"], json!(["user"]), "{v}");
            assert_eq!(v["annotations"]["priority"], json!(0.5));
            assert_eq!(v["annotations"]["lastModified"], "2026-07-21T00:00:00Z");
            assert_eq!(v["_meta"]["com.example/source"], json!("cache"));
            let back: Content = wire.into();
            assert_eq!(back, content);

            let wire: legacy::ContentBlock = content.clone().into();
            let v = serde_json::to_value(&wire).unwrap();
            assert_eq!(v["annotations"]["priority"], json!(0.5));
            assert_eq!(v["_meta"]["com.example/source"], json!("cache"));
            let back: Content = wire.into();
            assert_eq!(back, content);
        }
    }

    #[test]
    fn embedded_resource_block_and_contents_meta_round_trip_both_wires() {
        // Block-level annotations/_meta and the inner contents' _meta are
        // distinct spec surfaces; both must survive.
        let contents = ResourceContents::text("ui://app", "<html></html>")
            .with_mime_type("text/html;profile=mcp-app")
            .with_meta_entry("io.modelcontextprotocol/ui", json!({"prefersBorder": true}));
        let content = Content::resource(contents)
            .with_annotations(Annotations::new().for_audience(Role::User))
            .with_meta_entry("com.example/origin", json!("embedded"));

        let wire: v0728::ContentBlock = content.clone().into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["_meta"]["com.example/origin"], json!("embedded"));
        assert_eq!(
            v["resource"]["_meta"]["io.modelcontextprotocol/ui"]["prefersBorder"],
            json!(true)
        );
        let back: Content = wire.into();
        assert_eq!(back, content);

        let wire: legacy::ContentBlock = content.clone().into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["_meta"]["com.example/origin"], json!("embedded"));
        assert_eq!(
            v["resource"]["_meta"]["io.modelcontextprotocol/ui"]["prefersBorder"],
            json!(true)
        );
        let back: Content = wire.into();
        assert_eq!(back, content);
    }

    #[test]
    fn read_resource_contents_meta_round_trips_both_wires() {
        let make = || {
            ReadResourceResult::new(alloc::vec![
                ResourceContents::text("file://a", "hi")
                    .with_meta_entry("com.example/etag", json!("abc")),
                ResourceContents::blob("file://b", "Zm9v")
                    .with_meta_entry("com.example/etag", json!("def")),
            ])
        };
        let assert_meta = |back: &ReadResourceResult| {
            let (ResourceContents::Text { meta, .. }, ResourceContents::Blob { meta: bmeta, .. }) =
                (&back.contents[0], &back.contents[1])
            else {
                panic!("variants survive");
            };
            assert_eq!(meta["com.example/etag"], json!("abc"));
            assert_eq!(bmeta["com.example/etag"], json!("def"));
        };

        let wire: v0728::ReadResourceResult = make().into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["contents"][0]["_meta"]["com.example/etag"], json!("abc"));
        let back: ReadResourceResult = wire.into();
        assert_meta(&back);

        let wire: legacy::ReadResourceResult = make().into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["contents"][1]["_meta"]["com.example/etag"], json!("def"));
        let back: ReadResourceResult = wire.into();
        assert_meta(&back);
    }

    #[test]
    fn absent_content_metadata_stays_absent_on_the_wire() {
        let wire: v0728::ContentBlock = Content::text("plain").into();
        let v = serde_json::to_value(&wire).unwrap();
        assert!(v.get("annotations").is_none(), "no annotations key: {v}");
        assert!(v.get("_meta").is_none(), "no _meta key: {v}");

        let wire: legacy::ContentBlock = Content::text("plain").into();
        let v = serde_json::to_value(&wire).unwrap();
        assert!(v.get("annotations").is_none(), "no annotations key: {v}");
        assert!(v.get("_meta").is_none(), "no _meta key: {v}");

        let wire: v0728::ReadResourceResult = ReadResourceResult::text("file://a", "hi").into();
        let v = serde_json::to_value(&wire).unwrap();
        assert!(v["contents"][0].get("_meta").is_none(), "no _meta key: {v}");
    }

    #[test]
    fn draft_call_result_round_trips_content_and_is_error() {
        let original = CallToolResult::error("boom");
        let wire: v0728::CallToolResult = original.into();
        let back: CallToolResult = wire.into();
        assert!(back.is_error);
        assert_eq!(back.content.len(), 1);
        assert!(matches!(&back.content[0], Content::Text { text, .. } if text == "boom"));
    }

    #[test]
    fn legacy_call_result_object_structured_content_round_trips() {
        let mut original = CallToolResult::text("ok");
        original.structured_content = Some(json!({"answer": 42}));
        let wire: legacy::CallToolResult = original.into();
        let back: CallToolResult = wire.into();
        assert!(!back.is_error);
        assert_eq!(back.structured_content, Some(json!({"answer": 42})));
    }

    #[test]
    fn read_resource_round_trips_text_and_blob() {
        let original = ReadResourceResult::new(alloc::vec![
            ResourceContents::text("file://a", "hi").with_mime_type("text/plain"),
            ResourceContents::blob("file://b", "Zm9v"),
        ]);
        let wire: v0728::ReadResourceResult = original.into();
        let back: ReadResourceResult = wire.into();
        assert_eq!(back.contents.len(), 2);
        assert!(
            matches!(&back.contents[0], ResourceContents::Text { uri, text, mime_type, .. }
                if uri == "file://a" && text == "hi" && mime_type.as_deref() == Some("text/plain"))
        );
        assert!(
            matches!(&back.contents[1], ResourceContents::Blob { uri, blob, .. }
                if uri == "file://b" && blob == "Zm9v")
        );
    }

    #[test]
    fn prompts_and_completion_round_trip() {
        let prompts = ListPromptsResult::new(alloc::vec![
            Prompt::new("summarize")
                .with_description("Summarize text")
                .with_argument(PromptArgument::new("text").required(true)),
        ]);
        let wire: v0728::ListPromptsResult = prompts.into();
        let back: ListPromptsResult = wire.into();
        assert_eq!(back.prompts[0].name, "summarize");
        assert!(back.prompts[0].arguments[0].required);

        let get = GetPromptResult::new(alloc::vec![PromptMessage::user_text("hello")])
            .with_description("greeting");
        let wire: legacy::GetPromptResult = get.into();
        let back: GetPromptResult = wire.into();
        assert_eq!(back.description.as_deref(), Some("greeting"));
        assert!(matches!(&back.messages[0].content, Content::Text { text, .. } if text == "hello"));
        assert!(matches!(back.messages[0].role, Role::User));

        let complete = CompleteResult::new(alloc::vec!["foo".to_string()])
            .with_total(1)
            .with_has_more(false);
        let wire: v0728::CompleteResult = complete.into();
        let back: CompleteResult = wire.into();
        assert_eq!(back.values, alloc::vec!["foo".to_string()]);
        assert_eq!(back.total, Some(1));
        assert_eq!(back.has_more, Some(false));
    }

    #[test]
    fn image_and_audio_content_round_trip() {
        for content in [
            Content::image("Zm9v", "image/png"),
            Content::audio("YmFy", "audio/wav"),
        ] {
            // draft
            let draft_block: v0728::ContentBlock = content.clone().into();
            assert_eq!(Content::from(draft_block), content);
            // legacy
            let legacy_block: legacy::ContentBlock = content.clone().into();
            assert_eq!(Content::from(legacy_block), content);
        }
    }

    #[test]
    fn resource_and_resource_link_content_round_trip() {
        for content in [
            Content::resource(
                ResourceContents::text("file://x", "hi").with_mime_type("text/plain"),
            ),
            Content::resource(
                ResourceContents::blob("file://y", "Zm9v").with_mime_type("image/png"),
            ),
            Content::resource_link(
                Resource::new("file://x", "x")
                    .with_title("X")
                    .with_mime_type("text/plain"),
            ),
        ] {
            // draft
            let draft_block: v0728::ContentBlock = content.clone().into();
            assert_eq!(Content::from(draft_block), content);
            // legacy
            let legacy_block: legacy::ContentBlock = content.clone().into();
            assert_eq!(Content::from(legacy_block), content);
        }
    }

    #[test]
    fn task_support_round_trips_the_legacy_wire_and_drops_on_draft() {
        for (ts, wire_str) in [
            (TaskSupport::Forbidden, "forbidden"),
            (TaskSupport::Optional, "optional"),
            (TaskSupport::Required, "required"),
        ] {
            let make = || Tool::new("t", json!({"type": "object"})).with_task_support(ts);
            let wire: legacy::Tool = make().into();
            let v = serde_json::to_value(&wire).unwrap();
            assert_eq!(v["execution"]["taskSupport"], wire_str);
            let back: Tool = wire.into();
            assert_eq!(back.task_support, Some(ts));

            // The draft models Tasks as an extension, not a per-tool wire field.
            let wire: v0728::Tool = make().into();
            let v = serde_json::to_value(&wire).unwrap();
            assert!(v.get("execution").is_none(), "no draft execution key: {v}");
            let back: Tool = wire.into();
            assert_eq!(back.task_support, None);
        }
        // A legacy tool without `execution` reads back as None.
        let wire: legacy::Tool = Tool::new("t", json!({"type": "object"})).into();
        let back: Tool = wire.into();
        assert_eq!(back.task_support, None);
    }

    #[test]
    fn draft_structured_content_keeps_non_object_values() {
        // The draft (unlike legacy) allows any JSON value here — a future
        // narrowing to objects must fail this, mirroring the legacy-drop test.
        for sc in [json!(7), json!([1, 2, 3]), json!("str"), json!(true)] {
            let mut r = CallToolResult::text("ok");
            r.structured_content = Some(sc.clone());
            let wire: v0728::CallToolResult = r.into();
            let v = serde_json::to_value(&wire).unwrap();
            assert_eq!(v["structuredContent"], sc);
            let back: CallToolResult = wire.into();
            assert_eq!(back.structured_content, Some(sc));
        }
    }

    #[test]
    fn cache_policy_round_trips_the_draft_wire_and_drops_on_legacy() {
        // Outbound + inbound Public arm.
        let result = ListToolsResult::new(alloc::vec![])
            .with_cache(CachePolicy::public(core::time::Duration::from_secs(60)));
        let wire: v0728::ListToolsResult = result.into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["ttlMs"], 60_000);
        assert_eq!(v["cacheScope"], "public");
        let back: ListToolsResult = wire.into();
        assert_eq!(
            back.cache,
            Some(CachePolicy::from_wire(60_000, CacheScope::Public))
        );

        // resources/read carries the same envelope (Private arm).
        let rr = ReadResourceResult::text("file://a", "hi")
            .with_cache(CachePolicy::private(core::time::Duration::from_millis(500)));
        let wire: v0728::ReadResourceResult = rr.into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["ttlMs"], 500);
        assert_eq!(v["cacheScope"], "private");
        let back: ReadResourceResult = wire.into();
        assert_eq!(
            back.cache,
            Some(CachePolicy::from_wire(500, CacheScope::Private))
        );

        // The legacy wire has no cache fields: dropped outbound, None inbound.
        let result = ListToolsResult::new(alloc::vec![])
            .with_cache(CachePolicy::public(core::time::Duration::from_secs(60)));
        let wire: legacy::ListToolsResult = result.into();
        let back: ListToolsResult = wire.into();
        assert_eq!(back.cache, None);

        // Pinned asymmetry: neutral `cache: None` re-enters from the draft
        // wire as the explicit conservative default (the wire always carries
        // the fields), never as None.
        let wire: v0728::ListToolsResult = ListToolsResult::new(alloc::vec![]).into();
        let back: ListToolsResult = wire.into();
        assert_eq!(back.cache, Some(CachePolicy::NO_CACHE));
    }

    #[test]
    fn legacy_annotations_wire_names_are_exact() {
        // The draft names are asserted elsewhere; pin the legacy JSON too so a
        // codegen rename regression (e.g. lastModified -> last_modified) fails.
        let wire: legacy::Resource = metadata_resource().into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["annotations"]["audience"], json!(["user"]));
        assert_eq!(v["annotations"]["priority"], json!(0.75));
        assert_eq!(v["annotations"]["lastModified"], "2026-07-20T00:00:00Z");

        let content = Content::text("hi").with_annotations(
            Annotations::new()
                .for_audience(Role::User)
                .last_modified("2026-07-21T00:00:00Z"),
        );
        let wire: legacy::ContentBlock = content.into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["annotations"]["audience"], json!(["user"]));
        assert_eq!(v["annotations"]["lastModified"], "2026-07-21T00:00:00Z");
    }

    #[test]
    fn absent_tool_metadata_stays_absent_on_the_legacy_wire() {
        let wire: legacy::Tool = Tool::new("plain", json!({"type": "object"})).into();
        let v = serde_json::to_value(&wire).unwrap();
        assert!(v.get("annotations").is_none(), "no annotations key: {v}");
        assert!(v.get("icons").is_none(), "no icons key: {v}");
        assert!(v.get("_meta").is_none(), "no _meta key: {v}");
        assert!(v.get("execution").is_none(), "no execution key: {v}");
    }

    #[test]
    fn resource_size_round_trips_and_clamps() {
        let mut resource = Resource::new("file://big", "big");
        resource.size = Some(4096);
        for wire_v in [
            serde_json::to_value(v0728::Resource::from(resource.clone())).unwrap(),
            serde_json::to_value(legacy::Resource::from(resource.clone())).unwrap(),
        ] {
            assert_eq!(wire_v["size"], 4096);
        }
        let back: Resource = v0728::Resource::from(resource.clone()).into();
        assert_eq!(back.size, Some(4096));
        let back: Resource = legacy::Resource::from(resource.clone()).into();
        assert_eq!(back.size, Some(4096));

        // Documented clamp: the wire types `size` as i64, so a u64 beyond
        // i64::MAX saturates — identically on Resource…
        resource.size = Some(u64::MAX);
        let back: Resource = v0728::Resource::from(resource.clone()).into();
        assert_eq!(back.size, Some(u64::try_from(i64::MAX).unwrap()));
        // …and on ResourceLink (the invariant is "never panic, never go
        // negative, never silently drop").
        let wire: v0728::ContentBlock = Content::resource_link(resource).into();
        let Content::ResourceLink(r) = Content::from(wire) else {
            panic!("resource link survives");
        };
        assert_eq!(r.size, Some(u64::try_from(i64::MAX).unwrap()));
    }

    #[test]
    fn icon_theme_light_round_trips_both_wires() {
        let tool = Tool::new("t", json!({"type": "object"})).with_icon(Icon {
            src: "https://example.com/i.png".into(),
            mime_type: None,
            sizes: alloc::vec![],
            theme: Some(IconTheme::Light),
        });
        let wire: v0728::Tool = tool.clone().into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["icons"][0]["theme"], "light");
        let back: Tool = wire.into();
        assert_eq!(back.icons[0].theme, Some(IconTheme::Light));

        let wire: legacy::Tool = tool.into();
        let v = serde_json::to_value(&wire).unwrap();
        assert_eq!(v["icons"][0]["theme"], "light");
        let back: Tool = wire.into();
        assert_eq!(back.icons[0].theme, Some(IconTheme::Light));
    }

    #[test]
    fn empty_annotations_serialize_as_empty_object_not_empty_arrays() {
        let content = Content::text("hi").with_annotations(Annotations::new());
        for v in [
            serde_json::to_value(v0728::ContentBlock::from(content.clone())).unwrap(),
            serde_json::to_value(legacy::ContentBlock::from(content.clone())).unwrap(),
        ] {
            // Present (the caller set it) but empty — never `"audience": []`.
            assert_eq!(v["annotations"], json!({}), "{v}");
        }
        let wire: v0728::ContentBlock = content.clone().into();
        let back: Content = wire.into();
        assert_eq!(back, content);
    }

    // ---- sampling ------------------------------------------------------------

    /// One conversation, three wires, and the shape each one accepts.
    ///
    /// A lone block renders bare rather than as a one-element array: that is
    /// the only form `2025-06-18` defines, and every later client reads it too,
    /// so there is nothing to gain from the array.
    #[test]
    fn a_single_block_message_renders_bare_on_every_wire() {
        let params =
            CreateMessageParams::new(alloc::vec![SamplingMessage::text(Role::User, "hi")], 64);
        for version in ProtocolVersion::SUPPORTED {
            let wire = params.to_wire(version).expect("plain text is universal");
            assert_eq!(
                wire["messages"][0]["content"],
                json!({ "type": "text", "text": "hi" }),
                "{version}"
            );
            assert_eq!(wire["maxTokens"], 64, "{version}");
        }
    }

    /// `2025-06-18` predates multi-block content, agentic sampling and message
    /// `_meta`; each is refused rather than dropped, because a conversation
    /// with a hole in it is answered wrongly instead of failing.
    #[test]
    fn the_2025_06_18_step_down_refuses_rather_than_truncates() {
        let two_blocks = CreateMessageParams::new(
            alloc::vec![SamplingMessage::new(
                Role::User,
                alloc::vec![
                    SamplingContent::text("see this"),
                    SamplingContent::image("aGk=", "image/png"),
                ],
            )],
            64,
        );
        let agentic = CreateMessageParams::new(Vec::new(), 64)
            .with_tools(alloc::vec![Tool::new("echo", json!({ "type": "object" }))]);

        for params in [&two_blocks, &agentic] {
            assert!(matches!(
                params.to_wire(&ProtocolVersion::V2025_06_18),
                Err(SamplingError::Unsupported { .. })
            ));
            assert!(params.to_wire(&ProtocolVersion::V2025_11_25).is_ok());
            assert!(params.to_wire(&ProtocolVersion::V2026_07_28).is_ok());
        }

        // Message `_meta` is dropped rather than refused: it is advisory
        // metadata, so losing it costs a cache hint, not the conversation.
        let with_meta = CreateMessageParams::new(
            alloc::vec![
                SamplingMessage::text(Role::User, "hi").with_meta_entry("x/cache", json!("k")),
            ],
            64,
        );
        let older = with_meta.to_wire(&ProtocolVersion::V2025_06_18).unwrap();
        assert!(older["messages"][0].get("_meta").is_none());
        let newer = with_meta.to_wire(&ProtocolVersion::V2025_11_25).unwrap();
        assert_eq!(newer["messages"][0]["_meta"]["x/cache"], "k");
    }

    /// An embedded resource is in no revision's `SamplingMessageContentBlock`,
    /// so it is refused everywhere rather than emitted as a block the client
    /// cannot parse.
    #[test]
    fn a_resource_block_is_not_sampling_content_on_any_wire() {
        let params = CreateMessageParams::new(
            alloc::vec![SamplingMessage::new(
                Role::User,
                alloc::vec![SamplingContent::Media(Content::resource(
                    ResourceContents::text("file:///a", "x"),
                ))],
            )],
            8,
        );
        for version in ProtocolVersion::SUPPORTED {
            assert!(
                matches!(
                    params.to_wire(version),
                    Err(SamplingError::Unsupported { .. })
                ),
                "{version}"
            );
        }
    }

    /// The two tool-use MUSTs, and the round trip of a balanced conversation.
    #[test]
    fn tool_conversations_are_checked_and_round_trip() {
        let call = SamplingMessage::new(
            Role::Assistant,
            alloc::vec![
                SamplingContent::tool_use(ToolUse::new("c1", "echo")),
                SamplingContent::tool_use(ToolUse::new("c2", "echo")),
            ],
        );
        let one_answer = SamplingMessage::new(
            Role::User,
            alloc::vec![SamplingContent::tool_result(ToolResult::new(
                "c1",
                alloc::vec![Content::text("42")],
            ))],
        );
        let both = SamplingMessage::new(
            Role::User,
            alloc::vec![
                SamplingContent::tool_result(ToolResult::new(
                    "c1",
                    alloc::vec![Content::text("42")]
                )),
                SamplingContent::tool_result(ToolResult::error("c2", "boom")),
            ],
        );

        // A call left unanswered, and a call only half answered.
        for messages in [
            alloc::vec![call.clone()],
            alloc::vec![call.clone(), one_answer],
        ] {
            assert!(matches!(
                CreateMessageParams::new(messages, 64).validate(),
                Err(SamplingError::Invalid(_))
            ));
        }

        let balanced = CreateMessageParams::new(alloc::vec![call, both], 64);
        balanced.validate().expect("every call is answered");
        let wire = balanced.to_wire(&ProtocolVersion::V2025_11_25).unwrap();
        assert_eq!(wire["messages"][0]["content"][0]["type"], "tool_use");
        assert_eq!(wire["messages"][1]["content"][1]["isError"], true);

        let back = CreateMessageParams::from_wire(&wire).expect("round trip");
        assert_eq!(back.messages, balanced.messages);
    }

    /// Params and results survive the wire in both directions, including the
    /// fields the older revisions share.
    #[test]
    fn sampling_params_and_results_round_trip() {
        let params =
            CreateMessageParams::new(alloc::vec![SamplingMessage::text(Role::User, "hi")], 32)
                .with_system_prompt("be brief")
                .with_temperature(0.25)
                .with_stop_sequences(alloc::vec!["STOP".into()])
                .with_include_context(IncludeContext::ThisServer)
                .with_model_preferences(ModelPreferences::hinting(["sonnet"]))
                .with_tool_choice(ToolChoice::Required);

        let wire = params.to_wire(&ProtocolVersion::V2026_07_28).unwrap();
        assert_eq!(wire["includeContext"], "thisServer");
        assert_eq!(wire["toolChoice"], json!({ "mode": "required" }));
        assert_eq!(wire["modelPreferences"]["hints"][0]["name"], "sonnet");
        assert_eq!(CreateMessageParams::from_wire(&wire).unwrap(), params);

        let result = CreateMessageResult::new(
            "m",
            alloc::vec![SamplingContent::tool_use(ToolUse::new("c1", "echo"))],
        )
        .with_stop_reason("toolUse");
        let wire = result.to_wire(&ProtocolVersion::V2025_11_25).unwrap();
        assert_eq!(wire["stopReason"], "toolUse");
        assert_eq!(CreateMessageResult::from_wire(&wire).unwrap(), result);
        assert_eq!(result.tool_uses().count(), 1);

        // A result is the assistant's turn, so it cannot carry tool results.
        assert!(
            CreateMessageResult::from_wire(&json!({
                "model": "m",
                "role": "assistant",
                "content": [{ "type": "tool_result", "toolUseId": "c1", "content": [] }],
            }))
            .is_err()
        );
    }

    /// Which sub-capability a request needs is a property of the request.
    #[test]
    fn a_requests_declared_needs_come_from_what_it_carries() {
        let plain = CreateMessageParams::new(Vec::new(), 8);
        assert!(!plain.uses_tools() && !plain.uses_context());
        assert!(
            !plain
                .clone()
                .with_include_context(IncludeContext::None)
                .uses_context(),
            "`none` is the undeclared-safe value"
        );
        assert!(
            plain
                .clone()
                .with_include_context(IncludeContext::AllServers)
                .uses_context()
        );
        assert!(
            plain
                .clone()
                .with_tool_choice(ToolChoice::None)
                .uses_tools()
        );
        assert!(
            plain
                .with_tools(alloc::vec![Tool::new("t", json!({}))])
                .uses_tools()
        );
    }

    /// The schema caps `completion.values` at 100. A handler that returns more
    /// gets the surplus cut and `hasMore` set, rather than a response its own
    /// client refuses to parse.
    #[test]
    fn a_completion_over_the_cap_is_cut_and_flagged() {
        let many =
            CompleteResult::new((0..150).map(|i| alloc::format!("v{i}")).collect()).with_total(150);
        let draft: v0728::CompleteResult = many.clone().into();
        assert_eq!(draft.completion.values.len(), 100);
        assert_eq!(draft.completion.has_more, Some(true));
        assert_eq!(draft.completion.total, Some(150), "the real count survives");

        let legacy_wire: legacy::CompleteResult = many.into();
        assert_eq!(legacy_wire.completion.values.len(), 100);
        assert_eq!(legacy_wire.completion.has_more, Some(true));

        // At or under the cap nothing is touched, including an explicit
        // `hasMore: false`.
        let exact = CompleteResult::new((0..100).map(|i| alloc::format!("v{i}")).collect())
            .with_has_more(false);
        let wire: v0728::CompleteResult = exact.into();
        assert_eq!(wire.completion.values.len(), 100);
        assert_eq!(wire.completion.has_more, Some(false));
    }

    // ---- elicitation ---------------------------------------------------------

    /// "Form mode elicitation schemas are limited to flat objects with
    /// primitive properties only."
    ///
    /// A client cannot render what the subset excludes, so an unchecked nested
    /// object reaches the user as an empty form with nothing said about why.
    #[test]
    fn a_form_schema_outside_the_restricted_subset_is_rejected() {
        let ok = ElicitParams::new(
            "?",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "minLength": 1 },
                    "age": { "type": "integer" },
                    "score": { "type": "number" },
                    "agree": { "type": "boolean" },
                    "colors": {
                        "type": "array",
                        "items": { "type": "string", "enum": ["red", "green"] },
                    },
                    // SEP-1330's labelled forms: `oneOf` for single-select,
                    // `anyOf` items for multi-select.
                    "shade": {
                        "type": "string",
                        "oneOf": [
                            { "const": "#FF0000", "title": "Red" },
                            { "const": "#00FF00", "title": "Green" },
                        ],
                    },
                    "palette": {
                        "type": "array",
                        "items": { "anyOf": [
                            { "const": "#FF0000", "title": "Red" },
                            { "const": "#00FF00", "title": "Green" },
                        ]},
                    },
                },
            }),
        );
        ok.validate().expect("the whole allowed subset");

        for (label, schema) in [
            ("not an object", json!({ "type": "string" })),
            (
                "nested object",
                json!({ "type": "object", "properties": {
                    "address": { "type": "object", "properties": {} },
                }}),
            ),
            (
                "array of objects",
                json!({ "type": "object", "properties": {
                    "rows": { "type": "array", "items": { "type": "object" } },
                }}),
            ),
            (
                "array without an enum",
                json!({ "type": "object", "properties": {
                    "tags": { "type": "array", "items": { "type": "string" } },
                }}),
            ),
            (
                "untyped property",
                json!({ "type": "object", "properties": {
                    "whatever": { "$ref": "#/$defs/Thing" },
                }}),
            ),
        ] {
            assert!(
                ElicitParams::new("?", schema).validate().is_err(),
                "{label} should be refused"
            );
        }
    }

    // ---- roots ---------------------------------------------------------------

    /// "This MUST be a `file://` URI." A root is a permission statement, so a
    /// non-file one is refused at construction and dropped on parse rather
    /// than handed to a handler that would act on it.
    #[test]
    fn only_file_uri_roots_exist() {
        assert!(Root::new("file:///work").is_some());
        for bad in ["https://example.com", "/work", "FILE:///work", ""] {
            assert!(Root::new(bad).is_none(), "{bad}");
        }

        let roots = Root::list_from_wire(&json!({ "roots": [
            { "uri": "file:///ok", "name": "ok" },
            { "uri": "https://example.com/evil" },
            { "name": "no uri at all" },
        ]}));
        assert_eq!(roots.len(), 1, "only the file:// root survives");
        assert_eq!(roots[0].uri, "file:///ok");
        assert_eq!(roots[0].name.as_deref(), Some("ok"));
        assert_eq!(
            Root::list_to_wire(&roots),
            json!({ "roots": [{ "uri": "file:///ok", "name": "ok" }] })
        );
    }
}
