//! [`Composite`]: build one MCP server out of several, each mounted either under
//! a prefix or flat.
//!
//! ```no_run
//! # use turbomcp_server::{Composite, McpServerCore};
//! # use turbomcp_core::{Implementation, McpResult};
//! # #[derive(Clone)] struct Weather;
//! # impl McpServerCore for Weather { fn server_info(&self) -> Implementation { Implementation::new("w", "1") } }
//! # #[derive(Clone)] struct News;
//! # impl McpServerCore for News { fn server_info(&self) -> Implementation { Implementation::new("n", "1") } }
//! # fn f() -> McpResult<()> {
//! use turbomcp_server::IntoServerBuilder;
//!
//! let gateway = Composite::new(Implementation::new("gateway", "1.0.0"))
//!     .mount("weather", Weather.into_server())?
//!     .mount("news", News.into_server())?
//!     .into_server()          // -> ServerBuilder<CompositeServer>
//!     .build();               // -> the tower::Service, as usual
//! # Ok(()) }
//! ```
//!
//! # Two kinds of mount
//!
//! [`mount`](Composite::mount) namespaces: `weather.forecast`. Use it for
//! optional or third-party servers, where a caller seeing which vertical a tool
//! came from is a feature, and where a clash with something else in the process
//! is a real risk.
//!
//! [`mount_flat`](Composite::mount_flat) does not rename anything. Use it to
//! assemble one public catalogue out of several servers — splitting a large
//! server into focused ones without breaking any client, since `read_note` stays
//! `read_note`. That is the only way to decompose an existing server without a
//! breaking change, so it is the right default for a server's *own* components
//! and the wrong one for anything it merely hosts.
//!
//! They compose: a composite can carry a flat core and prefixed plugins at once.
//!
//! The trade is collisions. Prefixing makes them impossible; flat mounting makes
//! them **detected** — two mounts exposing one name fails the list that would
//! have shown both, naming the servers involved, because the protocol gives a
//! client no way to disambiguate two identical names and silently dropping one
//! would be worse. [`preflight`](CompositeServer::preflight) runs that check
//! ahead of a first request.
//!
//! # What gets namespaced, and what doesn't
//!
//! **A prefixed mount's tools and prompts are prefixed** — `weather.forecast`,
//! `news.headlines`.
//! Their names are flat, short, and chosen without knowing what else the process
//! will serve, so collisions are likely; prefixing makes them impossible. `.` is
//! the separator because the spec's name charset allows it and it already reads
//! as a namespace (`#[tool(name = "search.web")]`). A mount prefix may therefore
//! not itself contain `.`, which keeps the split back to `(mount, name)`
//! unambiguous.
//!
//! **Resource URIs are left alone.** A URI is already a namespace — scheme plus
//! authority — and it is a globally meaningful identifier a client may hand
//! elsewhere; rewriting it makes it a lie. (v3 mounted them as
//! `{prefix}://{original_uri}`, which for `config://app` under prefix `weather`
//! produces `weather://config://app`.) So two mounts claiming one URI is a real
//! ambiguity, and [`list_resources`](CompositeServer::list_resources) reports it
//! rather than silently letting one shadow the other — the same rule the
//! `#[server]` macro enforces at compile time within a single server.
//!
//! # What a mount does not bring with it
//!
//! [`mount`](Composite::mount) takes a [`ServerBuilder`] because that is where a
//! server's *capabilities* are registered — `Weather.into_server()` already
//! knows Weather has tools and no prompts. Everything else a `ServerBuilder`
//! configures (tasks, session and task backends, extensions, cache policy, the
//! MRTR state key) is dispatcher-level: there is one dispatcher, and it is the
//! composite's. Setting any of them on a mounted builder is rejected rather than
//! ignored.
//!
//! # Pagination
//!
//! A cursor only means something to the server that minted it, so the composite
//! mints its own — `{mount}:{that mount's cursor}`, where `{mount}` is the
//! prefix, or `#{index}` for a flat mount since `#` is outside the prefix
//! charset — and hands each mount only a cursor it issued. A page walks the
//! mounts in order and ends at the first
//! one reporting another page of its own, so no mount's `next_cursor` is
//! dropped and a page may span several mounts. A cursor the composite did not
//! issue, or one naming a mount that is no longer there, is refused rather than
//! quietly restarting from the beginning.
//!
//! Likewise a mount's `supported_versions()` does not narrow the composite's.
//! Handlers are version-neutral, so a sub-server pinned to one revision is not
//! protected by that pin once it is mounted — the composite's dispatcher owns
//! wire rendering. Mounting a narrower server is an error naming the fix: pin
//! the composite with [`protocols`](Composite::protocols).

use std::collections::BTreeMap;
use std::sync::Arc;

use futures::future::BoxFuture;

use turbomcp_core::{Implementation, McpError, McpResult, ProtocolVersion, RequestContext};
use turbomcp_protocol::neutral;

use crate::builder::ServerBuilder;
use crate::context::{
    CallToolContext, CompleteContext, GetPromptContext, ListPromptsContext,
    ListResourceTemplatesContext, ListResourcesContext, ListToolsContext, ReadResourceContext,
};
use crate::router::MethodRouter;
use crate::traits::{McpServerCore, WithCompletions, WithPrompts, WithResources, WithTools};

/// The separator between a mount's prefix and a component's own name.
const SEP: char = '.';

/// The separator inside a composite pagination cursor,
/// `{prefix}:{the mount's own cursor}`.
///
/// A prefix is `[A-Za-z0-9_-]+` (see [`validate_prefix`]) so it can never
/// contain `:`, which makes the split back to `(mount, own cursor)`
/// unambiguous however the mount chose to encode its half.
const CURSOR_SEP: char = ':';

/// The spec's upper bound on a tool name (`server/tools`, both revisions).
const MAX_TOOL_NAME: usize = 128;

// ---- type erasure ------------------------------------------------------------

/// The object-safe view of a mounted server: its capability set plus one
/// dispatch method per RPC, each `None` when that capability is not registered.
///
/// This exists because the capability traits use `impl Future` returns and so
/// are not dyn-compatible. [`MethodRouter`] has already erased the trait bounds
/// into stored closures, so a mount is just `(server, router)` with the server
/// type erased too.
trait Mounted: Send + Sync + 'static {
    fn has_tools(&self) -> bool;
    fn has_resources(&self) -> bool;
    fn has_prompts(&self) -> bool;
    fn has_completions(&self) -> bool;

    fn list_tools(
        &self,
        ctx: ListToolsContext,
        params: neutral::ListParams,
    ) -> Option<BoxFuture<'static, McpResult<neutral::ListToolsResult>>>;
    fn call_tool(
        &self,
        ctx: CallToolContext,
        params: neutral::CallToolParams,
    ) -> Option<BoxFuture<'static, McpResult<neutral::CallToolResult>>>;
    fn list_resources(
        &self,
        ctx: ListResourcesContext,
        params: neutral::ListParams,
    ) -> Option<BoxFuture<'static, McpResult<neutral::ListResourcesResult>>>;
    fn read_resource(
        &self,
        ctx: ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> Option<BoxFuture<'static, McpResult<neutral::ReadResourceResult>>>;
    fn list_resource_templates(
        &self,
        ctx: ListResourceTemplatesContext,
        params: neutral::ListParams,
    ) -> Option<BoxFuture<'static, McpResult<neutral::ListResourceTemplatesResult>>>;
    fn list_prompts(
        &self,
        ctx: ListPromptsContext,
        params: neutral::ListParams,
    ) -> Option<BoxFuture<'static, McpResult<neutral::ListPromptsResult>>>;
    fn get_prompt(
        &self,
        ctx: GetPromptContext,
        params: neutral::GetPromptParams,
    ) -> Option<BoxFuture<'static, McpResult<neutral::GetPromptResult>>>;
    fn complete(
        &self,
        ctx: CompleteContext,
        params: neutral::CompleteParams,
    ) -> Option<BoxFuture<'static, McpResult<neutral::CompleteResult>>>;
}

/// A concrete server plus its router, with the server type erased.
struct Erased<S> {
    server: S,
    router: MethodRouter<S>,
}

/// Forward one [`Mounted`] method to the matching `MethodRouter::dispatch_*`.
macro_rules! forward {
    ($name:ident, $dispatch:ident, $ctx:ty, $params:ty, $result:ty) => {
        fn $name(
            &self,
            ctx: $ctx,
            params: $params,
        ) -> Option<BoxFuture<'static, McpResult<$result>>> {
            self.router.$dispatch(self.server.clone(), ctx, params)
        }
    };
}

impl<S: McpServerCore> Mounted for Erased<S> {
    fn has_tools(&self) -> bool {
        self.router.has_tools()
    }
    fn has_resources(&self) -> bool {
        self.router.has_resources()
    }
    fn has_prompts(&self) -> bool {
        self.router.has_prompts()
    }
    fn has_completions(&self) -> bool {
        self.router.has_completions()
    }

    forward!(
        list_tools,
        dispatch_list_tools,
        ListToolsContext,
        neutral::ListParams,
        neutral::ListToolsResult
    );
    forward!(
        call_tool,
        dispatch_call_tool,
        CallToolContext,
        neutral::CallToolParams,
        neutral::CallToolResult
    );
    forward!(
        list_resources,
        dispatch_list_resources,
        ListResourcesContext,
        neutral::ListParams,
        neutral::ListResourcesResult
    );
    forward!(
        read_resource,
        dispatch_read_resource,
        ReadResourceContext,
        neutral::ReadResourceParams,
        neutral::ReadResourceResult
    );
    forward!(
        list_resource_templates,
        dispatch_list_resource_templates,
        ListResourceTemplatesContext,
        neutral::ListParams,
        neutral::ListResourceTemplatesResult
    );
    forward!(
        list_prompts,
        dispatch_list_prompts,
        ListPromptsContext,
        neutral::ListParams,
        neutral::ListPromptsResult
    );
    forward!(
        get_prompt,
        dispatch_get_prompt,
        GetPromptContext,
        neutral::GetPromptParams,
        neutral::GetPromptResult
    );
    forward!(
        complete,
        dispatch_complete,
        CompleteContext,
        neutral::CompleteParams,
        neutral::CompleteResult
    );
}

struct Mount {
    /// `None` for a flat mount, whose components keep their own names.
    prefix: Option<String>,
    /// Identifies this mount inside a pagination cursor. The prefix when there
    /// is one; otherwise `#{index}`, which cannot collide with a prefix because
    /// `#` is outside the prefix charset.
    cursor_id: String,
    /// The mounted server's own `server_info().name`, so a collision can name
    /// the servers involved. A flat mount has no prefix to blame.
    name: String,
    server: Box<dyn Mounted>,
}

impl Mount {
    /// How this mount is referred to in diagnostics.
    fn label(&self) -> &str {
        self.prefix.as_deref().unwrap_or(&self.name)
    }
}

// ---- the builder -------------------------------------------------------------

/// Assembles several servers into one. See [`CompositeServer`] for what a mount
/// does and does not bring with it.
pub struct Composite {
    info: Implementation,
    instructions: Option<String>,
    versions: &'static [ProtocolVersion],
    mounts: Vec<Mount>,
}

impl std::fmt::Debug for Composite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Composite")
            .field("info", &self.info)
            .field("versions", &self.versions)
            .field("mounts", &Mounts(&self.mounts))
            .finish()
    }
}

/// How the mounts are labelled, for `Debug` — a mounted server is opaque.
/// A prefixed mount shows its prefix; a flat one shows its server's name in
/// braces, so the two cannot be confused.
struct Mounts<'a>(&'a [Mount]);

impl std::fmt::Debug for Mounts<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|m| match &m.prefix {
                Some(prefix) => prefix.clone(),
                None => format!("{{{}}}", m.name),
            }))
            .finish()
    }
}

impl Composite {
    /// Start an empty composite identified by `info`.
    #[must_use]
    pub fn new(info: Implementation) -> Self {
        Self {
            info,
            instructions: None,
            versions: ProtocolVersion::SUPPORTED,
            mounts: Vec::new(),
        }
    }

    /// Guidance returned to clients in discovery, describing the composed
    /// server as a whole. A mount's own instructions are not merged — they
    /// describe a server the client is not talking to.
    #[must_use]
    pub fn instructions(mut self, instructions: impl Into<String>) -> Self {
        self.instructions = Some(instructions.into());
        self
    }

    /// Narrow the protocol revisions this composite accepts, as
    /// `#[server(protocols(…))]` does for a single server. Mounting a server
    /// that accepts fewer revisions than this set is an error.
    #[must_use]
    pub fn protocols(mut self, versions: &'static [ProtocolVersion]) -> Self {
        self.versions = versions;
        self
    }

    /// Mount `server` under `prefix`: its tools and prompts join this
    /// composite's as `{prefix}.{name}`, its resources and templates join under
    /// their own URIs.
    ///
    /// # Errors
    /// - `prefix` is empty, contains `.` (which would make the split back to
    ///   `(mount, name)` ambiguous), or contains a character outside the spec's
    ///   tool-name set (`[A-Za-z0-9_-]`) — the composed name has to remain a
    ///   legal tool name.
    /// - `prefix` is already mounted.
    /// - `server` accepts fewer protocol revisions than this composite —
    ///   handlers are version-neutral, so a sub-server's pin cannot be honored
    ///   once mounted; narrow the composite with [`protocols`](Self::protocols).
    /// - `server`'s builder carries dispatcher-level configuration, which
    ///   belongs on the composite's own builder.
    pub fn mount<S>(self, prefix: impl AsRef<str>, server: ServerBuilder<S>) -> McpResult<Self>
    where
        S: McpServerCore,
    {
        let prefix = prefix.as_ref();
        validate_prefix(prefix)?;
        if self
            .mounts
            .iter()
            .any(|m| m.prefix.as_deref() == Some(prefix))
        {
            return Err(McpError::invalid_params(format!(
                "`{prefix}` is already mounted; give each mounted server a distinct prefix"
            )));
        }
        self.push(Some(prefix.to_owned()), server)
    }

    /// Mount `server` *without* a prefix: its tools and prompts join this
    /// composite under **their own names**, unchanged.
    ///
    /// This is how a public tool catalogue is assembled from several servers
    /// without renaming anything — a client's `read_note` stays `read_note`, so
    /// splitting one large server into focused ones is not a breaking change for
    /// anyone calling it. Prefixed [`mount`](Self::mount) remains the right
    /// choice for optional or third-party servers, where namespacing is a
    /// feature. The two compose freely in one composite.
    ///
    /// # Collisions
    ///
    /// Prefixing makes a name clash impossible; flat mounting does not, so it is
    /// **detected instead of prevented**. Two flat mounts exposing one tool name
    /// makes `tools/list` fail, naming both servers — no silent shadowing, since
    /// the spec gives a client no way to disambiguate two identical names.
    ///
    /// The check happens when the list is built, not at mount time, and that is
    /// deliberate: what a server exposes depends on the request. A tool gated by
    /// [`with_visibility`](crate::ServerBuilder::with_visibility) exists for one
    /// caller and not another, so a snapshot taken at mount time — with no
    /// identity — could report a clash no caller can reach, or miss one two
    /// privileged callers hit. [`preflight`](CompositeServer::preflight) runs
    /// the same check ahead of time for whichever identity you name.
    ///
    /// # Errors
    /// - `server` accepts fewer protocol revisions than this composite —
    ///   handlers are version-neutral, so a sub-server's pin cannot be honored
    ///   once mounted; narrow the composite with [`protocols`](Self::protocols).
    /// - `server`'s builder carries dispatcher-level configuration, which
    ///   belongs on the composite's own builder.
    pub fn mount_flat<S>(self, server: ServerBuilder<S>) -> McpResult<Self>
    where
        S: McpServerCore,
    {
        self.push(None, server)
    }

    /// The half of mounting that does not care whether there is a prefix.
    fn push<S>(mut self, prefix: Option<String>, server: ServerBuilder<S>) -> McpResult<Self>
    where
        S: McpServerCore,
    {
        let at = match &prefix {
            Some(prefix) => format!("mounted at `{prefix}`"),
            None => "mounted flat".to_owned(),
        };
        if let Some(setting) = server.dispatcher_setting() {
            return Err(McpError::invalid_params(format!(
                "the server {at} sets `{setting}`, which configures the dispatcher — there \
                 is one dispatcher and it is the composite's. Move the call to the \
                 composite's own builder."
            )));
        }
        let (server, router) = server.into_parts();
        let narrowed: Vec<&str> = self
            .versions
            .iter()
            .filter(|v| !server.supported_versions().contains(v))
            .map(ProtocolVersion::as_str)
            .collect();
        if !narrowed.is_empty() {
            return Err(McpError::invalid_params(format!(
                "the server {at} does not accept {narrowed:?}, which this composite does. \
                 Handlers are version-neutral, so mounting cannot honor a sub-server's pin \
                 — narrow the composite with `.protocols(…)` instead."
            )));
        }
        // A flat mount has no prefix to name it in a cursor, so it is identified
        // by its position. `#` is outside the prefix charset, so the two spaces
        // cannot overlap.
        let cursor_id = prefix
            .clone()
            .unwrap_or_else(|| format!("#{}", self.mounts.len()));
        let name = server.server_info().name;
        self.mounts.push(Mount {
            prefix,
            cursor_id,
            name,
            server: Box::new(Erased { server, router }),
        });
        Ok(self)
    }

    /// Freeze into the server value.
    #[must_use]
    pub fn build(self) -> CompositeServer {
        CompositeServer {
            inner: Arc::new(self),
        }
    }

    /// Freeze and begin building the dispatcher, registering exactly the
    /// capabilities the mounts between them provide — so an all-tools composite
    /// advertises `tools` and nothing else, just as a single server does.
    #[must_use]
    pub fn into_server(self) -> ServerBuilder<CompositeServer> {
        self.build().into_server()
    }
}

/// A mount prefix has to survive being concatenated into a tool name, so it is
/// held to the same character set minus the separator itself.
fn validate_prefix(prefix: &str) -> McpResult<()> {
    if prefix.is_empty() {
        // An empty prefix would yield `.read_note`, which is neither flat nor
        // namespaced — so it names the method that actually means "flat".
        return Err(McpError::invalid_params(
            "a mount prefix may not be empty — use `mount_flat` to mount a server whose \
             tools and prompts keep their own names",
        ));
    }
    if let Some(bad) = prefix
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-')))
    {
        let why = if bad == SEP {
            "`.` separates the prefix from the component name, so a prefix containing one \
             would make the split ambiguous"
        } else {
            "a composed name must stay a legal tool name: ASCII letters, digits, `_`, `-`"
        };
        return Err(McpError::invalid_params(format!(
            "the mount prefix `{prefix}` contains `{bad}` — {why}"
        )));
    }
    Ok(())
}

// ---- the composed server -----------------------------------------------------

/// The server value a [`Composite`] freezes into. Implements every capability
/// trait by delegating to the mounts; which capabilities are *advertised* is
/// decided by [`into_server`](CompositeServer::into_server).
#[derive(Clone)]
pub struct CompositeServer {
    inner: Arc<Composite>,
}

impl std::fmt::Debug for CompositeServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompositeServer")
            .field("info", &self.inner.info)
            .field("mounts", &Mounts(&self.inner.mounts))
            .finish()
    }
}

impl CompositeServer {
    /// Begin building the dispatcher, registering exactly the capabilities the
    /// mounts provide. Shadows the blanket
    /// [`IntoServerBuilder::into_server`](crate::IntoServerBuilder::into_server),
    /// which would register none.
    #[must_use]
    pub fn into_server(self) -> ServerBuilder<Self> {
        let (tools, resources, prompts, completions) = (
            self.any(Mounted::has_tools),
            self.any(Mounted::has_resources),
            self.any(Mounted::has_prompts),
            self.any(Mounted::has_completions),
        );
        let mut builder = ServerBuilder::new(self);
        if tools {
            builder = builder.with_tools();
        }
        if resources {
            builder = builder.with_resources();
        }
        if prompts {
            builder = builder.with_prompts();
        }
        if completions {
            builder = builder.with_completions();
        }
        builder
    }

    fn any(&self, has: impl Fn(&dyn Mounted) -> bool) -> bool {
        self.inner.mounts.iter().any(|m| has(m.server.as_ref()))
    }

    /// Walk every list this composite serves, as `request` would see them, and
    /// report the first name or URI two mounts both claim.
    ///
    /// Flat mounts move collisions from impossible to *detected*, and detection
    /// happens when a list is built — which without this means the first real
    /// `tools/list` of a bad deploy is what surfaces the misconfiguration. Run
    /// this at startup and fail there instead.
    ///
    /// # What it can and cannot promise
    ///
    /// It checks the catalogue **as the identity in `request` sees it**. That is
    /// the honest limit: with
    /// [`with_visibility`](crate::ServerBuilder::with_visibility) installed, what
    /// a server exposes is a function of the caller, so no single check covers
    /// every caller. Pass a context per identity class that matters — an
    /// anonymous one, an admin one — rather than treating one pass as proof.
    ///
    /// Unlike a request, this drains every page, so a collision between mounts
    /// whose components land on different pages is caught too.
    ///
    /// # Errors
    /// The collision, naming both mounts and the identifier they share. Also any
    /// error a mount raises while listing, since a mount that cannot be listed
    /// cannot be checked.
    pub async fn preflight(&self, request: RequestContext) -> McpResult<()> {
        // A mount that keeps issuing cursors would otherwise hang startup. The
        // bound is far above any real catalogue, so tripping it means a mount is
        // misbehaving — which is itself worth failing on.
        const MAX_PAGES: usize = 1000;

        macro_rules! drain {
            ($ctx:expr, $list:ident, $label:literal) => {{
                let ctx = $ctx;
                let mut cursor = None;
                for page in 0.. {
                    if page == MAX_PAGES {
                        return Err(McpError::internal(format!(
                            "a mounted server is still paginating {} after {MAX_PAGES} pages \
                             — it is not terminating its cursor",
                            $label
                        )));
                    }
                    let mut params = neutral::ListParams::default();
                    params.cursor = cursor;
                    let result = self.$list(&ctx, params).await?;
                    match result.next_cursor {
                        Some(next) => cursor = Some(next),
                        None => break,
                    }
                }
            }};
        }

        drain!(ListToolsContext::new(request.clone()), list_tools, "tools");
        drain!(
            ListResourcesContext::new(request.clone()),
            list_resources,
            "resources"
        );
        drain!(
            ListResourceTemplatesContext::new(request.clone()),
            list_resource_templates,
            "resource templates"
        );
        drain!(ListPromptsContext::new(request), list_prompts, "prompts");
        Ok(())
    }

    /// Split a *prefixed* name into the mount that owns it and the name it knows
    /// itself by. `None` when no mounted prefix claims it — which includes every
    /// name belonging to a flat mount.
    fn route(&self, qualified: &str) -> Option<(&Mount, String)> {
        let (prefix, name) = qualified.split_once(SEP)?;
        let mount = self
            .inner
            .mounts
            .iter()
            .find(|m| m.prefix.as_deref() == Some(prefix))?;
        Some((mount, name.to_owned()))
    }

    /// Whether any mount is flat — i.e. whether an unprefixed name could belong
    /// to something. When none is, an unroutable name is unroutable immediately
    /// and no list is needed.
    fn has_flat(&self) -> bool {
        self.inner.mounts.iter().any(|m| m.prefix.is_none())
    }

    /// Find the flat mount exposing `name`, by asking each what it currently
    /// serves.
    ///
    /// A flat mount's names are not derivable from the name itself, and they are
    /// not fixed either — visibility policies and dynamic catalogues both make
    /// the answer request-dependent — so the only correct source is the list for
    /// *this* request.
    ///
    /// The obvious alternative, dispatching to each mount until one does not
    /// answer "unknown tool", is **unsound**: a tool-level failure is a
    /// `CallToolResult` with `is_error`, indistinguishable from not knowing the
    /// name, so a call that genuinely ran and failed would be re-dispatched to
    /// the next server — running its side effects a second time.
    async fn flat_owner<'a, T, F>(
        mounts: &'a [Mount],
        name: &str,
        mut list: F,
    ) -> McpResult<Option<&'a Mount>>
    where
        F: FnMut(&'a Mount) -> Option<BoxFuture<'static, McpResult<T>>>,
        T: Names,
    {
        for mount in mounts.iter().filter(|m| m.prefix.is_none()) {
            let Some(fut) = list(mount) else { continue };
            if fut.await?.has(name) {
                return Ok(Some(mount));
            }
        }
        Ok(None)
    }
}

/// A list result that can say whether it contains a given component name — the
/// one thing [`CompositeServer::flat_owner`] needs from it.
trait Names {
    fn has(&self, name: &str) -> bool;
}

impl Names for neutral::ListToolsResult {
    fn has(&self, name: &str) -> bool {
        self.tools.iter().any(|t| t.name == name)
    }
}

impl Names for neutral::ListPromptsResult {
    fn has(&self, name: &str) -> bool {
        self.prompts.iter().any(|p| p.name == name)
    }
}

/// `{prefix}.{name}`.
fn qualify(prefix: &str, name: &str) -> String {
    let mut out = String::with_capacity(prefix.len() + 1 + name.len());
    out.push_str(prefix);
    out.push(SEP);
    out.push_str(name);
    out
}

/// Decode a cursor this composite minted into the index of the mount to resume
/// and the cursor *that mount* issued.
///
/// A cursor is opaque to the client but not to us: it has to say which mount is
/// mid-page, because a cursor only means something to the server that minted it.
/// Handing mount A's cursor to mount B — which is what forwarding the caller's
/// `cursor` to every mount would do — asks a server to interpret another
/// server's private state.
///
/// No cursor starts at the first mount. An unparseable one, or one naming a
/// mount that no longer exists, is a client error: cursors do not survive a
/// change to the composite's shape, and continuing from the beginning would
/// silently repeat a page the caller already has.
fn resume_at(mounts: &[Mount], cursor: Option<&str>) -> McpResult<(usize, Option<String>)> {
    let Some(cursor) = cursor else {
        return Ok((0, None));
    };
    let bad = || McpError::invalid_params(format!("not a cursor this server issued: `{cursor}`"));
    let (id, own) = cursor.split_once(CURSOR_SEP).ok_or_else(bad)?;
    let at = mounts
        .iter()
        .position(|m| m.cursor_id == id)
        .ok_or_else(bad)?;
    Ok((at, (!own.is_empty()).then(|| own.to_owned())))
}

/// Wrap a mount's own cursor so the next request resumes at that mount.
fn resume_cursor(mount: &Mount, own: &str) -> String {
    let mut out = String::with_capacity(mount.cursor_id.len() + 1 + own.len());
    out.push_str(&mount.cursor_id);
    out.push(CURSOR_SEP);
    out.push_str(own);
    out
}

/// One page of a composed list.
///
/// Walks the mounts from wherever `params.cursor` left off, handing the
/// resuming mount its own cursor and every later mount none, and **stops at the
/// first mount that reports another page** — returning a cursor that names it.
/// That is what keeps a mount's `next_cursor` from being dropped: the page ends
/// where the pagination does, rather than concatenating first pages and
/// discarding the rest.
///
/// `$adapt` runs per item with the owning mount in scope, and may borrow
/// anything the caller declared before the invocation (the duplicate-URI set,
/// for the two resource lists).
macro_rules! page_through {
    ($self:ident, $ctx:ident, $params:ident, $dispatch:ident, $field:ident, $result:ty,
     |$mount:ident, $item:ident| $adapt:block) => {{
        let mounts = &$self.inner.mounts;
        let (start, mut own) = resume_at(mounts, $params.cursor.as_deref())?;
        let mut items = Vec::new();
        let mut next = None;
        for $mount in &mounts[start..] {
            let mut params = $params.clone();
            // Only the mount the cursor names gets one; the rest start fresh.
            params.cursor = own.take();
            let Some(fut) = $mount.server.$dispatch($ctx.clone(), params) else {
                continue;
            };
            let page = fut.await?;
            #[allow(unused_mut)]
            for mut $item in page.$field {
                $adapt
                items.push($item);
            }
            if let Some(cursor) = page.next_cursor {
                next = Some(resume_cursor($mount, &cursor));
                break;
            }
        }
        let mut out = <$result>::new(items);
        out.next_cursor = next;
        Ok(out)
    }};
}

impl McpServerCore for CompositeServer {
    fn server_info(&self) -> Implementation {
        self.inner.info.clone()
    }

    fn supported_versions(&self) -> &'static [ProtocolVersion] {
        self.inner.versions
    }

    fn instructions(&self) -> Option<String> {
        self.inner.instructions.clone()
    }
}

impl WithTools for CompositeServer {
    async fn list_tools(
        &self,
        ctx: &ListToolsContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        page_through!(
            self,
            ctx,
            params,
            list_tools,
            tools,
            neutral::ListToolsResult,
            |mount, tool| {
                if let Some(prefix) = &mount.prefix {
                    tool.name = qualify(prefix, &tool.name);
                }
                // The spec bounds a tool name at 128 characters and clients
                // reject or mangle what exceeds it. The macro checks a name it
                // generates, but the composed name only exists here — and a flat
                // mount may be a server this process never declared.
                if tool.name.len() > MAX_TOOL_NAME {
                    return Err(McpError::internal(format!(
                        "the tool `{}` from `{}` is {} characters, over the spec's \
                         {MAX_TOOL_NAME}-character limit{}",
                        tool.name,
                        mount.label(),
                        tool.name.len(),
                        if mount.prefix.is_some() {
                            " once mounted — use a shorter prefix"
                        } else {
                            ""
                        },
                    )));
                }
                claim(&mut seen, &tool.name, mount, Kind::Tool)?;
            }
        )
    }

    async fn call_tool(
        &self,
        ctx: &CallToolContext,
        mut params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        // A name with no prefix may still belong to a flat mount, which only its
        // current list can say. Matches what a `#[server]` impl answers for a
        // name it doesn't know: a tool-level error the model can act on, not a
        // JSON-RPC one.
        let routed = match self.route(&params.name) {
            Some(routed) => Some(routed),
            None if self.has_flat() => {
                Self::flat_owner(&self.inner.mounts, &params.name, |mount| {
                    mount
                        .server
                        .list_tools(ListToolsContext::new(ctx.base.clone()), Default::default())
                })
                .await?
                .map(|mount| (mount, params.name.clone()))
            }
            None => None,
        };
        let Some((mount, name)) = routed else {
            return Ok(neutral::CallToolResult::error(format!(
                "unknown tool: {}",
                params.name
            )));
        };
        params.name = name;
        let Some(fut) = mount.server.call_tool(ctx.clone(), params) else {
            return Ok(neutral::CallToolResult::error(format!(
                "the server mounted at `{}` serves no tools",
                mount.label()
            )));
        };
        fut.await
    }
}

impl WithResources for CompositeServer {
    async fn list_resources(
        &self,
        ctx: &ListResourcesContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourcesResult> {
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        page_through!(
            self,
            ctx,
            params,
            list_resources,
            resources,
            neutral::ListResourcesResult,
            |mount, resource| {
                claim(&mut seen, &resource.uri, mount, Kind::Resource)?;
            }
        )
    }

    async fn list_resource_templates(
        &self,
        ctx: &ListResourceTemplatesContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListResourceTemplatesResult> {
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        page_through!(
            self,
            ctx,
            params,
            list_resource_templates,
            resource_templates,
            neutral::ListResourceTemplatesResult,
            |mount, template| {
                claim(&mut seen, &template.uri_template, mount, Kind::Template)?;
            }
        )
    }

    async fn read_resource(
        &self,
        ctx: &ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> McpResult<neutral::ReadResourceResult> {
        // URIs are not prefixed, so the owning mount can't be derived from the
        // URI — ask each in turn. Only "not found" falls through: a mount that
        // owns the URI and failed for its own reasons must report that, not be
        // silently retried against a server that doesn't own it at all.
        for mount in &self.inner.mounts {
            let Some(fut) = mount.server.read_resource(ctx.clone(), params.clone()) else {
                continue;
            };
            match fut.await {
                Err(McpError::ResourceNotFound(_)) => continue,
                other => return other,
            }
        }
        Err(McpError::resource_not_found(params.uri))
    }
}

impl WithPrompts for CompositeServer {
    async fn list_prompts(
        &self,
        ctx: &ListPromptsContext,
        params: neutral::ListParams,
    ) -> McpResult<neutral::ListPromptsResult> {
        let mut seen: BTreeMap<String, String> = BTreeMap::new();
        page_through!(
            self,
            ctx,
            params,
            list_prompts,
            prompts,
            neutral::ListPromptsResult,
            |mount, prompt| {
                if let Some(prefix) = &mount.prefix {
                    prompt.name = qualify(prefix, &prompt.name);
                }
                claim(&mut seen, &prompt.name, mount, Kind::Prompt)?;
            }
        )
    }

    async fn get_prompt(
        &self,
        ctx: &GetPromptContext,
        mut params: neutral::GetPromptParams,
    ) -> McpResult<neutral::GetPromptResult> {
        let routed = match self.route(&params.name) {
            Some(routed) => Some(routed),
            None if self.has_flat() => {
                Self::flat_owner(&self.inner.mounts, &params.name, |mount| {
                    mount.server.list_prompts(
                        ListPromptsContext::new(ctx.base.clone()),
                        Default::default(),
                    )
                })
                .await?
                .map(|mount| (mount, params.name.clone()))
            }
            None => None,
        };
        let Some((mount, name)) = routed else {
            return Err(McpError::invalid_params(format!(
                "unknown prompt: {}",
                params.name
            )));
        };
        params.name = name;
        let Some(fut) = mount.server.get_prompt(ctx.clone(), params) else {
            return Err(McpError::invalid_params(format!(
                "the server mounted at `{}` serves no prompts",
                mount.label()
            )));
        };
        fut.await
    }
}

impl WithCompletions for CompositeServer {
    async fn complete(
        &self,
        ctx: &CompleteContext,
        mut params: neutral::CompleteParams,
    ) -> McpResult<neutral::CompleteResult> {
        match &mut params.reference {
            // A prompt reference names a prompt. A prefixed one routes exactly;
            // an unprefixed one is resolved the same way `get_prompt` resolves
            // it, so completion and the call it completes for agree on the owner.
            neutral::CompletionReference::Prompt { name } => {
                let routed = match self.route(name) {
                    Some(routed) => Some(routed),
                    None if self.has_flat() => {
                        Self::flat_owner(&self.inner.mounts, name, |mount| {
                            mount.server.list_prompts(
                                ListPromptsContext::new(ctx.base.clone()),
                                Default::default(),
                            )
                        })
                        .await?
                        .map(|mount| (mount, name.clone()))
                    }
                    None => None,
                };
                let Some((mount, own)) = routed else {
                    return Ok(neutral::CompleteResult::new(vec![]));
                };
                *name = own;
                match mount.server.complete(ctx.clone(), params) {
                    Some(fut) => fut.await,
                    None => Ok(neutral::CompleteResult::new(vec![])),
                }
            }
            // A resource-template reference names a URI, which is not
            // prefixed. Ask each mount that completes and take the first
            // non-empty answer: an empty completion is always a legal reply, so
            // a mount that doesn't own the template declines by returning one.
            neutral::CompletionReference::ResourceTemplate { .. } => {
                for mount in &self.inner.mounts {
                    let Some(fut) = mount.server.complete(ctx.clone(), params.clone()) else {
                        continue;
                    };
                    let result = fut.await?;
                    if !result.values.is_empty() {
                        return Ok(result);
                    }
                }
                Ok(neutral::CompleteResult::new(vec![]))
            }
            // `CompletionReference` is `#[non_exhaustive]`. A kind added later
            // has no routing rule here yet, and an empty completion is always a
            // legal reply — better than guessing a mount.
            _ => Ok(neutral::CompleteResult::new(vec![])),
        }
    }
}

/// What sort of thing a wire-visible identifier names, so a collision can say
/// what to do about it.
#[derive(Clone, Copy)]
enum Kind {
    Tool,
    Prompt,
    Resource,
    Template,
}

impl Kind {
    fn what(self) -> &'static str {
        match self {
            Self::Tool => "tool",
            Self::Prompt => "prompt",
            Self::Resource => "resource URI",
            Self::Template => "template URI",
        }
    }

    /// The fix, which differs by why the identifier is unprefixed.
    fn remedy(self) -> &'static str {
        match self {
            // Names are prefixed unless a mount is flat, so a clash means two
            // flat mounts — or a flat one against a prefixed one's product.
            Self::Tool | Self::Prompt => {
                "flat mounts do not rename, so two of them cannot expose the same name — \
                 mount one under a prefix instead"
            }
            Self::Resource | Self::Template => {
                "resource URIs are never prefixed by mounting — give each server its own \
                 scheme or authority"
            }
        }
    }
}

/// Record `id` as claimed by `mount`, refusing a second claim.
///
/// Two mounts exposing one wire-visible identifier is a genuine ambiguity:
/// nothing in the request says which was meant, and letting the first win would
/// silently hide the second — which the spec gives a client no way to detect.
/// The `#[server]` macro rejects the same collision within one server at compile
/// time; across mounts it can only be seen here.
///
/// The check is per *page*, which is everything a mount has unless it paginates.
/// Catching a collision between two mounts whose components land on different
/// pages would mean draining every mount on every request, which is the cost
/// pagination exists to avoid.
fn claim(
    seen: &mut BTreeMap<String, String>,
    id: &str,
    mount: &Mount,
    kind: Kind,
) -> McpResult<()> {
    if let Some(first) = seen.insert(id.to_owned(), mount.label().to_owned()) {
        return Err(McpError::internal(format!(
            "two mounted servers expose the {} `{id}`: `{first}` and `{}` — {}",
            kind.what(),
            mount.label(),
            kind.remedy(),
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IntoServerBuilder;

    #[derive(Clone)]
    struct Bare;

    impl McpServerCore for Bare {
        fn server_info(&self) -> Implementation {
            Implementation::new("bare", "1.0.0")
        }
    }

    #[derive(Clone)]
    struct Pinned;

    impl McpServerCore for Pinned {
        fn server_info(&self) -> Implementation {
            Implementation::new("pinned", "1.0.0")
        }
        fn supported_versions(&self) -> &'static [ProtocolVersion] {
            &[ProtocolVersion::V2025_11_25]
        }
    }

    fn composite() -> Composite {
        Composite::new(Implementation::new("gateway", "1.0.0"))
    }

    #[test]
    fn a_prefix_must_survive_being_part_of_a_tool_name() {
        for bad in ["", "we.ather", "we ather", "wéather", "weather/x"] {
            let err = composite()
                .mount(bad, Bare.into_server())
                .expect_err("`{bad}` should be rejected");
            assert!(
                err.to_string().contains("prefix"),
                "unhelpful message for `{bad}`: {err}"
            );
        }
        for good in ["weather", "weather-api", "weather_api", "v2"] {
            composite()
                .mount(good, Bare.into_server())
                .unwrap_or_else(|e| panic!("`{good}` should be accepted: {e}"));
        }
    }

    #[test]
    fn a_prefix_may_be_used_once() {
        let err = composite()
            .mount("weather", Bare.into_server())
            .unwrap()
            .mount("weather", Bare.into_server())
            .expect_err("the second mount should be rejected");
        assert!(err.to_string().contains("already mounted"), "{err}");
    }

    #[test]
    fn a_mount_may_not_be_narrower_than_the_composite() {
        // Mounting a pinned server under a full-range composite would silently
        // serve it revisions its author excluded.
        let err = composite()
            .mount("pinned", Pinned.into_server())
            .expect_err("a narrower mount should be rejected");
        assert!(err.to_string().contains("2025-06-18"), "{err}");
        assert!(err.to_string().contains(".protocols("), "{err}");

        // …and narrowing the composite to match is the fix the message names.
        composite()
            .protocols(&[ProtocolVersion::V2025_11_25])
            .mount("pinned", Pinned.into_server())
            .expect("a matching composite should accept it");
    }

    #[test]
    fn dispatcher_settings_on_a_mount_are_refused_not_ignored() {
        let err = composite()
            .mount("weather", Bare.into_server().with_tasks())
            .expect_err("a dispatcher-level setting should be rejected");
        assert!(err.to_string().contains("with_tasks"), "{err}");
        assert!(err.to_string().contains("dispatcher"), "{err}");
    }
}
