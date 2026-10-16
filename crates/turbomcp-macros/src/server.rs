//! Expansion of `#[server]`.
//!
//! The driver parses the annotated `impl` block, classifies each method by its
//! `#[tool]` / `#[resource]` / `#[prompt]` marker, and emits:
//! - the user's `impl` block, cleaned of the marker/parameter helper attributes;
//! - `impl McpServerCore` (from the `name`/`version` args), whose `register`
//!   registers the capabilities found, so every `into_server()` has them;
//! - one capability trait impl per kind present (`WithTools`, `WithResources`,
//!   `WithPrompts`) — so advertised capabilities are derived from what's written;
//! - per-tool argument structs (deriving `Deserialize` + `JsonSchema`) that back
//!   compile-time schema generation and pre-call validation;
//! - inherent `into_server()` (the trait method, without the import) and
//!   `run_stdio()` entry points.
//!
//! All generated paths are rooted at `::turbomcp` so the macro works from any
//! downstream crate.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, format_ident, quote, quote_spanned};
use syn::ext::IdentExt as _;
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, ExprLit, FnArg, Ident, ImplItem, ImplItemFn, ItemImpl, Lit, LitStr, Meta, Pat,
    Token, Type, parse2,
};

/// Entry point called by the `#[proc_macro_attribute]` shim.
pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let args = ServerArgs::parse(attr)?;
    let mut block: ItemImpl = parse2(item)?;
    let self_ty = (*block.self_ty).clone();
    let generics = block.generics.clone();
    let target = Target {
        ty: &self_ty,
        generics: &generics,
    };

    // Classify methods and collect handler models. Clean the marker attributes
    // off the methods in place so the re-emitted impl compiles.
    let mut tools = Vec::new();
    let mut resources = Vec::new();
    let mut prompts = Vec::new();
    let mut completion: Option<CompletionHandler> = None;

    for item in &mut block.items {
        let ImplItem::Fn(f) = item else { continue };
        let Some(kind) = take_marker(&mut f.attrs)? else {
            continue;
        };
        match kind {
            Marker::Handler { kind, desc, args } => {
                let mut h = Handler::parse(f, desc, kind)?;
                h.apply(*args);
                h.validate_metadata()?;
                match &h.kind {
                    HandlerKind::Tool => tools.push(h),
                    HandlerKind::Prompt => prompts.push(h),
                    HandlerKind::Resource { .. } => resources.push(h),
                }
            }
            Marker::Completion => {
                if completion.is_some() {
                    return Err(syn::Error::new(
                        f.sig.span(),
                        "a #[server] may declare at most one #[completion] handler",
                    ));
                }
                completion = Some(CompletionHandler::parse(f)?);
            }
        }
        // Strip per-parameter helper attributes from the re-emitted method.
        strip_param_attrs(f);
    }

    // The wire name is what clients call; the spec constrains it, and dispatch
    // matches on it — so an out-of-spec or shadowed name is a compile error.
    for t in &tools {
        validate_tool_name(&t.wire_name(), t.wire_name_span())?;
    }
    reject_duplicates(&tools, |h| h.wire_name(), "tool name", NAME_REMEDY)?;
    reject_duplicates(&prompts, |h| h.wire_name(), "prompt name", NAME_REMEDY)?;
    reject_duplicates(
        &resources,
        Handler::resource_uri,
        "resource URI",
        "give each resource a distinct URI",
    )?;

    let tools_impl = (!tools.is_empty()).then(|| gen_tools_impl(&target, &tools));
    let resources_impl = (!resources.is_empty()).then(|| gen_resources_impl(&target, &resources));
    let prompts_impl = (!prompts.is_empty()).then(|| gen_prompts_impl(&target, &prompts));
    let completions_impl = completion
        .as_ref()
        .map(|c| gen_completions_impl(&target, c));

    let mut registrations = TokenStream::new();
    if !tools.is_empty() {
        registrations.extend(quote!(.with_tools()));
    }
    if !resources.is_empty() {
        registrations.extend(quote!(.with_resources()));
    }
    if !prompts.is_empty() {
        registrations.extend(quote!(.with_prompts()));
    }
    if completion.is_some() {
        registrations.extend(quote!(.with_completions()));
    }
    let core_impl = gen_core_impl(&target, &args, &registrations)?;
    let entry_head = target.head(None);

    let entry_impl = quote! {
        #entry_head {
            /// Build a configurable server with this type's capabilities
            /// registered. The same as
            /// [`IntoServerBuilder::into_server`](::turbomcp::IntoServerBuilder::into_server),
            /// here so it needs no import.
            pub fn into_server(self) -> ::turbomcp::ServerBuilder<Self> {
                ::turbomcp::ServerBuilder::new(self)
            }

            /// Serve over stdio until the peer closes stdin.
            ///
            /// Dual-stack by default: the connection is wrapped in a
            /// [`LegacySessionAdapter`](::turbomcp::LegacySessionAdapter), so
            /// both stateless `2026-07-28` clients and stateful
            /// `2025-11-25` (`initialize`-handshake) clients are served.
            ///
            /// Needs a builder setting or middleware?
            /// `into_server()…serve(stdio())` serves the same way.
            pub async fn run_stdio(self) -> ::core::result::Result<(), ::turbomcp::ProtocolError> {
                self.into_server().serve(::turbomcp::stdio()).await
            }
        }
    };

    let generated = quote! {
        #core_impl
        #tools_impl
        #resources_impl
        #prompts_impl
        #completions_impl
        #entry_impl
    };
    let generated = crate::resolve_facade(generated)?;
    Ok(quote! { #block #generated })
}

/// The type a `#[server]` impl is for, with the impl's generics: every
/// generated impl repeats them, so `impl<S: Store> App<S>` serves as well
/// as a concrete type.
struct Target<'a> {
    ty: &'a Type,
    generics: &'a syn::Generics,
}

impl Target<'_> {
    /// `impl<…> Trait for Ty<…> where …` (or the inherent `impl<…> Ty<…>`).
    fn head(&self, trait_path: Option<TokenStream>) -> TokenStream {
        let (impl_generics, _, where_clause) = self.generics.split_for_impl();
        let ty = self.ty;
        match trait_path {
            Some(t) => quote!(impl #impl_generics #t for #ty #where_clause),
            None => quote!(impl #impl_generics #ty #where_clause),
        }
    }
}

const NAME_REMEDY: &str = "rename one, or give it a distinct `name = \"…\"`";

/// Reject two handlers claiming one wire identity — dispatch matches on it, so
/// the second could never run. Tools and prompts are keyed by wire name (they
/// are separate namespaces, hence one call per kind); resources by URI.
fn reject_duplicates(
    handlers: &[Handler],
    key: impl Fn(&Handler) -> String,
    what: &str,
    remedy: &str,
) -> syn::Result<()> {
    let mut seen: std::collections::HashMap<String, &Ident> = std::collections::HashMap::new();
    for h in handlers {
        let k = key(h);
        if let Some(first) = seen.get(&k) {
            return Err(syn::Error::new(
                h.method.span(),
                format!(
                    "two handlers claim the {what} `{k}`; it is already claimed by \
                     the method `{first}`. Dispatch matches on it, so the second \
                     would never run — {remedy}."
                ),
            ));
        }
        seen.insert(k, &h.method);
    }
    Ok(())
}

/// Check a tool's wire name against the spec's naming rules (`server/tools`,
/// identical in `2025-11-25` and the draft): 1–128 characters drawn from ASCII
/// letters, digits, `_`, `-`, and `.`.
///
/// The spec states these as SHOULDs, but they are enforced here. Clients apply
/// their own name patterns and reject or mangle what falls outside this set, and
/// a compile error is a far better place to discover that than a production
/// `tools/call`. A server that genuinely must use another name can implement
/// [`WithTools`](turbomcp::WithTools) by hand.
fn validate_tool_name(name: &str, span: Span) -> syn::Result<()> {
    let len = name.chars().count();
    if len == 0 {
        return Err(syn::Error::new(span, "a tool name must not be empty"));
    }
    if len > 128 {
        return Err(syn::Error::new(
            span,
            format!("tool name `{name}` is {len} characters; the spec allows at most 128"),
        ));
    }
    if let Some(c) = name
        .chars()
        .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
    {
        return Err(syn::Error::new(
            span,
            format!(
                "tool name `{name}` contains `{c}`, which the spec does not permit: a \
                 tool name may use ASCII letters, digits, `_`, `-`, and `.` \
                 (e.g. `getUser`, `DATA_EXPORT_v2`, `admin.tools.list`)"
            ),
        ));
    }
    Ok(())
}

// ---- attribute parsing -------------------------------------------------------

struct ServerArgs {
    /// Any `&str`-valued expression: a literal, `env!(…)`, `concat!(…)`,
    /// `include_str!(…)`, a `const`. Defaults to the crate's name.
    name: Option<Expr>,
    /// Likewise; defaults to the crate's version.
    version: Option<Expr>,
    title: Option<Expr>,
    instructions: Option<Expr>,
    /// `description = …` (`2025-11-25` onwards).
    description: Option<Expr>,
    /// `website_url = …` (`2025-11-25` onwards).
    website_url: Option<Expr>,
    /// `icon(…)`, each a `neutral::Icon` expression.
    icons: Vec<TokenStream>,
    /// Wire strings from `protocols(…)`, paired with their spans for error
    /// reporting. Empty means "unset" — the server keeps the crate default
    /// (`ProtocolVersion::SUPPORTED`).
    protocols: Vec<LitStr>,
}

impl ServerArgs {
    fn parse(attr: TokenStream) -> syn::Result<Self> {
        // `Meta` rather than `MetaNameValue`: `protocols(…)` is a list, like
        // `#[tool(scopes(…))]`.
        let metas = Punctuated::<Meta, Token![,]>::parse_terminated.parse2(attr.clone())?;
        let mut name = None;
        let mut version = None;
        let mut title = None;
        let mut instructions = None;
        let mut description = None;
        let mut website_url = None;
        let mut icons = Vec::new();
        let mut protocols = Vec::new();
        for m in metas {
            let key = m
                .path()
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
            if key == "icon" {
                icons.push(icon_arg(&m)?);
                continue;
            }
            if key == "protocols" {
                let Meta::List(list) = &m else {
                    return Err(syn::Error::new(
                        m.span(),
                        "expected `protocols(\"2025-11-25\", …)`",
                    ));
                };
                let lits =
                    list.parse_args_with(Punctuated::<LitStr, Token![,]>::parse_terminated)?;
                if lits.is_empty() {
                    return Err(syn::Error::new(
                        list.span(),
                        "`protocols(…)` needs at least one version — a server that \
                         accepts none can answer nothing",
                    ));
                }
                protocols = lits.into_iter().collect();
                continue;
            }
            let Meta::NameValue(nv) = &m else {
                return Err(syn::Error::new(
                    m.span(),
                    format!("expected `{key} = \"…\"`"),
                ));
            };
            let val = nv.value.clone();
            match key.as_str() {
                "name" => name = Some(val),
                "version" => version = Some(val),
                "title" => title = Some(val),
                "instructions" => instructions = Some(val),
                "description" => description = Some(val),
                "website_url" => website_url = Some(val),
                other => {
                    return Err(syn::Error::new(
                        m.path().span(),
                        format!(
                            "unknown #[server] argument `{other}` (expected name, version, title, \
                             description, website_url, icon, instructions, protocols)"
                        ),
                    ));
                }
            }
        }
        Ok(Self {
            name,
            version,
            title,
            instructions,
            description,
            website_url,
            icons,
            protocols,
        })
    }
}

/// The `ProtocolVersion` variant path for a wire string in `protocols(…)`.
///
/// The macro can't reach `ProtocolVersion::SUPPORTED` (it would be a runtime
/// value, and `supported_versions` returns `&'static [_]`), so the mapping
/// lives here. **When the draft freezes and `Draft` becomes a dated variant,
/// this table moves with it** — `protocols_accepts_every_supported_version`
/// in the facade tests fails if the two fall out of step.
fn protocol_variant(wire: &LitStr) -> syn::Result<proc_macro2::TokenStream> {
    match wire.value().as_str() {
        "2025-06-18" => Ok(quote! { ::turbomcp::ProtocolVersion::V2025_06_18 }),
        "2025-11-25" => Ok(quote! { ::turbomcp::ProtocolVersion::V2025_11_25 }),
        "2026-07-28" => Ok(quote! { ::turbomcp::ProtocolVersion::V2026_07_28 }),
        other => Err(syn::Error::new(
            wire.span(),
            format!(
                "`{other}` is not a protocol version this build serves \
                 (expected \"2025-06-18\", \"2025-11-25\", or \"2026-07-28\")"
            ),
        )),
    }
}

/// A parsed marker attribute. `#[completion]` takes no arguments and has no
/// handler model of its own; the other three share one.
enum Marker {
    Handler {
        kind: HandlerKind,
        /// The resolved description: an explicit `description = …`, else the
        /// bare-string shorthand, else the doc comment.
        desc: Option<Desc>,
        args: Box<MarkerArgs>,
    },
    Completion,
}

/// Behavior-hint flags from `#[tool(read_only, destructive = false, …)]` —
/// each maps to the spec's `ToolAnnotations` (`None` = hint not declared).
#[derive(Default, Clone, Copy)]
struct ToolHints {
    read_only: Option<bool>,
    destructive: Option<bool>,
    idempotent: Option<bool>,
    open_world: Option<bool>,
}

impl ToolHints {
    fn any(&self) -> bool {
        self.read_only.is_some()
            || self.destructive.is_some()
            || self.idempotent.is_some()
            || self.open_world.is_some()
    }
}

/// Which marker an attribute belongs to. Gates the keys it accepts, so a
/// misplaced one is reported against *that* marker rather than the union of
/// everything any marker takes.
#[derive(Clone, Copy)]
enum MarkerKind {
    Tool,
    Prompt,
    Resource,
}

impl MarkerKind {
    fn label(self) -> &'static str {
        match self {
            Self::Tool => "#[tool]",
            Self::Prompt => "#[prompt]",
            Self::Resource => "#[resource]",
        }
    }

    /// Whether this marker accepts the named key.
    fn accepts(self, key: &str) -> bool {
        match self {
            Self::Tool => matches!(
                key,
                "description"
                    | "name"
                    | "title"
                    | "task"
                    | "scopes"
                    | "tags"
                    | "schema_extend"
                    | "read_only"
                    | "destructive"
                    | "idempotent"
                    | "open_world"
                    | "icon"
                    | "meta"
                    | "output_schema"
            ),
            Self::Prompt => matches!(
                key,
                "description" | "name" | "title" | "tags" | "icon" | "meta"
            ),
            Self::Resource => matches!(
                key,
                "description"
                    | "name"
                    | "title"
                    | "mime_type"
                    | "tags"
                    | "icon"
                    | "meta"
                    | "size"
                    | "audience"
                    | "priority"
            ),
        }
    }

    /// The keys this marker accepts, for the "expected …" half of an error.
    fn expected(self) -> &'static str {
        match self {
            Self::Tool => {
                "`description = \"…\"`, `name = \"…\"`, `title = \"…\"`, `task`, \
                 `scopes(\"…\", …)`, `tags(\"…\", …)`, `schema_extend = \"{…}\"`, \
                 `icon(…)`, `meta(\"key\" = …)`, `output_schema = Type`, or a behavior hint \
                 (`read_only`, `destructive`, `idempotent`, `open_world`)"
            }
            Self::Prompt => {
                "`description = \"…\"`, `name = \"…\"`, `title = \"…\"`, `tags(\"…\", …)`, \
                 `icon(…)`, or `meta(\"key\" = …)`"
            }
            Self::Resource => {
                "`description = \"…\"`, `name = \"…\"`, `title = \"…\"`, `mime_type = \"…\"`, \
                 `tags(\"…\", …)`, `icon(…)`, `meta(\"key\" = …)`, `size = N`, \
                 `audience(\"user\", …)`, or `priority = 0.0..=1.0`"
            }
        }
    }
}

/// One argument inside a marker attribute, with the span to blame for it.
///
/// Parsing is deliberately marker-agnostic — [`Punctuated::parse_terminated`]
/// takes a plain `Parse` impl, which can't see the marker — so an unrecognized
/// key becomes [`ArgKind::Unknown`] and the per-marker gate runs in
/// [`MarkerArgs::parse`], where the marker *is* known.
struct MarkerArg {
    span: Span,
    kind: ArgKind,
}

/// A bare string (the URI on `#[resource]`, a description shorthand elsewhere),
/// `description` / `name` / `title` / `mime_type = "…"`, the `task` flag,
/// `scopes("…", …)` (required OAuth scopes), `tags("…", …)` (categorization),
/// or a behavior hint — `read_only` / `destructive` / `idempotent` /
/// `open_world`, each optionally `= true|false` (bare = `true`).
enum ArgKind {
    Positional(LitStr),
    Desc(Expr),
    Name(LitStr),
    Title(Expr),
    MimeType(Expr),
    /// `task` / `task = "optional"` (`false`) or `task = "required"` (`true`).
    Task(bool),
    Scopes(Vec<String>),
    Tags(Vec<String>),
    Hint(HintKind, bool),
    SchemaExtend(LitStr),
    /// An `neutral::Icon` expression.
    Icon(TokenStream),
    /// `meta("key" = <json>, …)`: keys and `json!` value tokens.
    Meta(Vec<(LitStr, TokenStream)>),
    OutputSchema(Type),
    Size(Expr),
    Audience(Vec<TokenStream>),
    Priority(Expr),
    Unknown(String),
}

impl ArgKind {
    /// The key this argument was written as, for the per-marker gate.
    fn key(&self) -> &str {
        match self {
            Self::Positional(_) => "",
            Self::Desc(_) => "description",
            Self::Name(_) => "name",
            Self::Title(_) => "title",
            Self::MimeType(_) => "mime_type",
            Self::Task(_) => "task",
            Self::Scopes(_) => "scopes",
            Self::Tags(_) => "tags",
            Self::Hint(k, _) => k.key(),
            Self::SchemaExtend(_) => "schema_extend",
            Self::Icon(_) => "icon",
            Self::Meta(_) => "meta",
            Self::OutputSchema(_) => "output_schema",
            Self::Size(_) => "size",
            Self::Audience(_) => "audience",
            Self::Priority(_) => "priority",
            Self::Unknown(k) => k,
        }
    }
}

#[derive(Clone, Copy)]
enum HintKind {
    ReadOnly,
    Destructive,
    Idempotent,
    OpenWorld,
}

impl HintKind {
    fn from_key(key: &str) -> Option<Self> {
        match key {
            "read_only" => Some(Self::ReadOnly),
            "destructive" => Some(Self::Destructive),
            "idempotent" => Some(Self::Idempotent),
            "open_world" => Some(Self::OpenWorld),
            _ => None,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Destructive => "destructive",
            Self::Idempotent => "idempotent",
            Self::OpenWorld => "open_world",
        }
    }
}

impl Parse for MarkerArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(LitStr) {
            let s: LitStr = input.parse()?;
            return Ok(Self {
                span: s.span(),
                kind: ArgKind::Positional(s),
            });
        }
        let meta: Meta = input.parse()?;
        let span = meta.path().span();
        let key = meta
            .path()
            .get_ident()
            .map(ToString::to_string)
            .unwrap_or_default();
        // `task` is a flag; the rest take a value, so their shape is checked here
        // and the marker gate (which key belongs on which marker) runs later.
        let kind = match key.as_str() {
            "task" => ArgKind::Task(task_required(&meta)?),
            "description" => ArgKind::Desc(name_value_expr(&meta, &key)?),
            "name" => ArgKind::Name(name_value_lit(&meta, &key)?),
            "title" => ArgKind::Title(name_value_expr(&meta, &key)?),
            "mime_type" => ArgKind::MimeType(name_value_expr(&meta, &key)?),
            "scopes" => ArgKind::Scopes(string_list(&meta, &key)?),
            "tags" => ArgKind::Tags(nonempty_string_list(&meta, &key)?),
            "schema_extend" => ArgKind::SchemaExtend(name_value_lit(&meta, &key)?),
            "icon" => ArgKind::Icon(icon_arg(&meta)?),
            "meta" => ArgKind::Meta(meta_arg(&meta)?),
            "output_schema" => {
                ArgKind::OutputSchema(parse2(name_value_expr(&meta, &key)?.to_token_stream())?)
            }
            "size" => ArgKind::Size(name_value_expr(&meta, &key)?),
            "audience" => ArgKind::Audience(audience_arg(&meta)?),
            "priority" => ArgKind::Priority(priority_arg(&meta)?),
            _ => match HintKind::from_key(&key) {
                Some(hint) => ArgKind::Hint(hint, hint_value(&meta)?),
                None => ArgKind::Unknown(key),
            },
        };
        Ok(Self { span, kind })
    }
}

/// `.with_meta_entry(key, [values…])`, or nothing when `values` is empty — an
/// empty array in `_meta` would be a distinct-looking value every reader then
/// has to special-case.
fn string_meta(values: &[String], key: TokenStream) -> Option<TokenStream> {
    if values.is_empty() {
        return None;
    }
    Some(quote!(.with_meta_entry(
        #key,
        ::turbomcp::__macros::serde_json::json!([#(#values),*]),
    )))
}

/// The strings inside a list-shaped argument — `scopes("a", "b")`,
/// `tags("a", "b")`.
fn string_list(meta: &Meta, key: &str) -> syn::Result<Vec<String>> {
    match meta {
        Meta::List(list) => {
            let lits = list.parse_args_with(Punctuated::<LitStr, Token![,]>::parse_terminated)?;
            Ok(lits.iter().map(LitStr::value).collect())
        }
        _ => Err(syn::Error::new(
            meta.path().span(),
            format!("expected `{key}(\"…\", …)`"),
        )),
    }
}

/// [`string_list`], rejecting an empty string. A tag is a lookup key — an empty
/// one can't be matched deliberately, so it is always a mistake, and catching it
/// here beats shipping an unmatchable `_meta` entry.
fn nonempty_string_list(meta: &Meta, key: &str) -> syn::Result<Vec<String>> {
    let Meta::List(list) = meta else {
        return string_list(meta, key);
    };
    let lits = list.parse_args_with(Punctuated::<LitStr, Token![,]>::parse_terminated)?;
    for lit in &lits {
        if lit.value().is_empty() {
            return Err(syn::Error::new(
                lit.span(),
                format!("a {key} entry is empty"),
            ));
        }
    }
    Ok(lits.iter().map(LitStr::value).collect())
}

/// The boolean a behavior hint declares: bare (`read_only`) means `true`;
/// `destructive = false` declares the hint *false*, which is distinct from
/// leaving it unset — the spec's defaults differ.
fn hint_value(meta: &Meta) -> syn::Result<bool> {
    match meta {
        Meta::Path(_) => Ok(true),
        Meta::NameValue(nv) => match &nv.value {
            Expr::Lit(ExprLit {
                lit: Lit::Bool(b), ..
            }) => Ok(b.value),
            other => Err(syn::Error::new(other.span(), "expected `true` or `false`")),
        },
        Meta::List(l) => Err(syn::Error::new(
            l.span(),
            "behavior hints take no list — use the bare flag or `= true|false`",
        )),
    }
}

/// The string literal of `key = "…"`, blaming the key when the shape is wrong.
fn name_value_lit(meta: &Meta, key: &str) -> syn::Result<LitStr> {
    let Meta::NameValue(nv) = meta else {
        return Err(syn::Error::new(
            meta.span(),
            format!("expected `{key} = \"…\"`"),
        ));
    };
    match &nv.value {
        Expr::Lit(ExprLit {
            lit: Lit::Str(s), ..
        }) => Ok(s.clone()),
        other => Err(syn::Error::new(other.span(), "expected a string literal")),
    }
}

/// `task` alone or `task = "optional"` is `false`; `task = "required"` is
/// `true`; anything else is an error at the value.
fn task_required(meta: &Meta) -> syn::Result<bool> {
    match meta {
        Meta::Path(_) => Ok(false),
        Meta::NameValue(_) => {
            let value = name_value_str(meta, "task")?;
            match value.as_str() {
                "optional" => Ok(false),
                "required" => Ok(true),
                _ => Err(syn::Error::new(
                    meta.span(),
                    format!(
                        "`task` is `task`, `task = \"optional\"` or `task = \"required\"`, not `{value}`"
                    ),
                )),
            }
        }
        Meta::List(_) => Err(syn::Error::new(
            meta.span(),
            "`task` is `task`, `task = \"optional\"` or `task = \"required\"`",
        )),
    }
}

/// The value of `key = …`: any expression (a literal, `env!`, `concat!`,
/// `include_str!`, a `const`), blaming the key when the shape is wrong.
fn name_value_expr(meta: &Meta, key: &str) -> syn::Result<Expr> {
    match meta {
        Meta::NameValue(nv) => Ok(nv.value.clone()),
        _ => Err(syn::Error::new(
            meta.span(),
            format!("expected `{key} = …`"),
        )),
    }
}

/// `icon("src")` or `icon(src = …, mime_type = …, sizes("48x48", …),
/// theme = "light" | "dark")`, as a `neutral::Icon` expression.
fn icon_arg(meta: &Meta) -> syn::Result<TokenStream> {
    let Meta::List(list) = meta else {
        return Err(syn::Error::new(
            meta.span(),
            "expected `icon(\"https://…\")` or `icon(src = …, …)`",
        ));
    };
    if let Ok(src) = parse2::<LitStr>(list.tokens.clone()) {
        return Ok(quote!(::turbomcp::neutral::Icon::new(#src)));
    }
    let parts = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated)?;
    let mut src = None;
    let mut sets = Vec::new();
    for part in &parts {
        let key = part
            .path()
            .get_ident()
            .map(ToString::to_string)
            .unwrap_or_default();
        match key.as_str() {
            "src" => src = Some(name_value_expr(part, "src")?),
            "mime_type" => {
                let m = name_value_expr(part, "mime_type")?;
                sets.push(quote!(__icon.mime_type = ::core::option::Option::Some(::core::convert::Into::into(#m));));
            }
            "sizes" => {
                let sizes = string_list(part, "sizes")?;
                sets.push(
                    quote!(__icon.sizes = ::std::vec![#(::std::string::String::from(#sizes)),*];),
                );
            }
            "theme" => {
                let theme = name_value_lit(part, "theme")?;
                let variant = match theme.value().as_str() {
                    "light" => quote!(Light),
                    "dark" => quote!(Dark),
                    _ => {
                        return Err(syn::Error::new(
                            theme.span(),
                            "an icon's `theme` is \"light\" or \"dark\"",
                        ));
                    }
                };
                sets.push(quote!(__icon.theme = ::core::option::Option::Some(::turbomcp::neutral::IconTheme::#variant);));
            }
            other => {
                return Err(syn::Error::new(
                    part.path().span(),
                    format!("unknown icon key `{other}` (expected src, mime_type, sizes, theme)"),
                ));
            }
        }
    }
    let src = src.ok_or_else(|| syn::Error::new(list.span(), "an icon needs a `src`"))?;
    Ok(quote!({
        #[allow(unused_mut)]
        let mut __icon = ::turbomcp::neutral::Icon::new(#src);
        #(#sets)*
        __icon
    }))
}

/// `meta("key" = <json>, …)`. A value is anything `json!` takes: a literal,
/// `{ … }`, `[ … ]`, an expression. The spec reserves the
/// `io.modelcontextprotocol/` prefix, and TurboMCP sets `io.turbomcp/` itself
/// (`tags(…)`, `scopes(…)`), so neither is accepted here.
fn meta_arg(meta: &Meta) -> syn::Result<Vec<(LitStr, TokenStream)>> {
    struct Entry(LitStr, TokenStream);
    impl Parse for Entry {
        fn parse(input: ParseStream) -> syn::Result<Self> {
            let key: LitStr = input.parse()?;
            input.parse::<Token![=]>()?;
            let mut value = TokenStream::new();
            while !input.is_empty() && !input.peek(Token![,]) {
                let tt: proc_macro2::TokenTree = input.parse()?;
                value.extend([tt]);
            }
            if value.is_empty() {
                return Err(input.error("expected a value after `=`"));
            }
            Ok(Self(key, value))
        }
    }
    let Meta::List(list) = meta else {
        return Err(syn::Error::new(
            meta.span(),
            "expected `meta(\"key\" = value, …)`",
        ));
    };
    let entries = list.parse_args_with(Punctuated::<Entry, Token![,]>::parse_terminated)?;
    entries
        .into_iter()
        .map(|Entry(key, value)| {
            let k = key.value();
            if k.starts_with("io.modelcontextprotocol/") || k.starts_with("io.turbomcp/") {
                return Err(syn::Error::new(
                    key.span(),
                    format!(
                        "`{k}` is under a reserved `_meta` prefix (the spec's or TurboMCP's own)"
                    ),
                ));
            }
            Ok((key, value))
        })
        .collect()
}

/// `audience("user", "assistant")`, as `neutral::Role` paths.
fn audience_arg(meta: &Meta) -> syn::Result<Vec<TokenStream>> {
    let Meta::List(list) = meta else {
        return string_list(meta, "audience").map(|_| Vec::new());
    };
    let lits = list.parse_args_with(Punctuated::<LitStr, Token![,]>::parse_terminated)?;
    lits.iter()
        .map(|lit| match lit.value().as_str() {
            "user" => Ok(quote!(::turbomcp::neutral::Role::User)),
            "assistant" => Ok(quote!(::turbomcp::neutral::Role::Assistant)),
            _ => Err(syn::Error::new(
                lit.span(),
                "an audience is \"user\" or \"assistant\"",
            )),
        })
        .collect()
}

/// `priority = 0.0..=1.0`, checked when it is a literal.
fn priority_arg(meta: &Meta) -> syn::Result<Expr> {
    let value = name_value_expr(meta, "priority")?;
    if let Expr::Lit(ExprLit { lit, .. }) = &value {
        let number = match lit {
            Lit::Float(f) => f.base10_parse::<f64>().ok(),
            Lit::Int(i) => i.base10_parse::<f64>().ok(),
            _ => None,
        };
        if !number.is_some_and(|n| (0.0..=1.0).contains(&n)) {
            return Err(syn::Error::new(
                value.span(),
                "`priority` is a number from 0.0 to 1.0",
            ));
        }
    }
    Ok(value)
}

fn name_value_str(meta: &Meta, key: &str) -> syn::Result<String> {
    name_value_lit(meta, key).map(|s| s.value())
}

/// Reject a `schema_extend` payload that isn't a JSON object, at the literal.
///
/// Parsing it here rather than at runtime means a typo is a compile error on
/// the offending string instead of a tool whose `inputSchema` is silently
/// wrong. The keys are deliberately *not* validated against a JSON Schema
/// vocabulary — SEP-2106's whole point is that a server passes through
/// keywords it does not itself interpret.
fn validate_schema_extend(lit: &LitStr) -> syn::Result<()> {
    let text = lit.value();
    match serde_json::from_str::<serde_json::Value>(&text) {
        // An input schema is `type: "object"` by definition; overriding that
        // (or the dialect) can only produce a schema no client accepts.
        Ok(serde_json::Value::Object(map)) => {
            match ["type", "$schema"]
                .into_iter()
                .find(|k| map.contains_key(*k))
            {
                Some(key) => Err(syn::Error::new(
                    lit.span(),
                    format!(
                        "`schema_extend` can't set `{key}`: a tool's input schema is always an object"
                    ),
                )),
                None => Ok(()),
            }
        }
        Ok(_) => Err(syn::Error::new(
            lit.span(),
            "`schema_extend` must be a JSON object of schema keywords, e.g. \
             `schema_extend = r#\"{\"allOf\": [...]}\"#`",
        )),
        Err(e) => Err(syn::Error::new(
            lit.span(),
            format!("`schema_extend` is not valid JSON: {e}"),
        )),
    }
}

/// Everything a marker attribute can declare. Which fields may be set is gated
/// per marker by [`MarkerKind::accepts`], so one struct serves all three without
/// letting `#[prompt(task)]` through.
#[derive(Default)]
struct MarkerArgs {
    /// The bare string, if any: the URI on `#[resource]`, otherwise a
    /// description shorthand.
    positional: Option<LitStr>,
    desc: Option<Expr>,
    /// `name = "…"` — kept as a literal so a bad name is reported at the
    /// literal rather than at the method.
    name: Option<LitStr>,
    title: Option<Expr>,
    mime_type: Option<Expr>,
    task: bool,
    task_required: bool,
    scopes: Vec<String>,
    tags: Vec<String>,
    hints: ToolHints,
    /// `schema_extend = "{…}"` — extra top-level JSON Schema keywords merged
    /// into the generated `inputSchema`. Kept as a literal so a malformed
    /// object is reported at the string.
    schema_extend: Option<LitStr>,
    icons: Vec<TokenStream>,
    meta: Vec<(LitStr, TokenStream)>,
    output_schema: Option<Type>,
    size: Option<Expr>,
    audience: Vec<TokenStream>,
    priority: Option<Expr>,
}

impl MarkerArgs {
    /// Parse `#[m]`, `#[m = "…"]`, `#[m("…")]`, `#[m(key = "…", …)]`, and
    /// combinations, rejecting keys the marker does not accept.
    fn parse(attr: &Attribute, marker: MarkerKind) -> syn::Result<Self> {
        let mut parsed = Self::default();
        match &attr.meta {
            Meta::Path(_) => return Ok(parsed),
            Meta::NameValue(nv) => {
                parsed.desc = Some(nv.value.clone());
                return Ok(parsed);
            }
            Meta::List(_) => {}
        }
        let args = attr.parse_args_with(Punctuated::<MarkerArg, Token![,]>::parse_terminated)?;
        for a in args {
            let key = a.kind.key();
            if !key.is_empty() && !marker.accepts(key) {
                return Err(syn::Error::new(
                    a.span,
                    format!(
                        "`{key}` is not valid on {} — expected {}",
                        marker.label(),
                        marker.expected()
                    ),
                ));
            }
            match a.kind {
                ArgKind::Positional(s) => {
                    if parsed.positional.is_some() {
                        return Err(syn::Error::new(
                            a.span,
                            format!("{} takes at most one bare string", marker.label()),
                        ));
                    }
                    parsed.positional = Some(s);
                }
                ArgKind::Desc(s) => parsed.desc = Some(s),
                ArgKind::Name(s) => parsed.name = Some(s),
                ArgKind::Title(s) => parsed.title = Some(s),
                ArgKind::MimeType(s) => parsed.mime_type = Some(s),
                ArgKind::Task(required) => {
                    parsed.task = true;
                    parsed.task_required = required;
                }
                ArgKind::Scopes(s) => parsed.scopes = s,
                ArgKind::Tags(t) => parsed.tags = t,
                ArgKind::Hint(HintKind::ReadOnly, v) => parsed.hints.read_only = Some(v),
                ArgKind::Hint(HintKind::Destructive, v) => parsed.hints.destructive = Some(v),
                ArgKind::Hint(HintKind::Idempotent, v) => parsed.hints.idempotent = Some(v),
                ArgKind::Hint(HintKind::OpenWorld, v) => parsed.hints.open_world = Some(v),
                ArgKind::SchemaExtend(s) => {
                    validate_schema_extend(&s)?;
                    parsed.schema_extend = Some(s);
                }
                ArgKind::Icon(icon) => parsed.icons.push(icon),
                ArgKind::Meta(entries) => parsed.meta.extend(entries),
                ArgKind::OutputSchema(ty) => parsed.output_schema = Some(ty),
                ArgKind::Size(size) => parsed.size = Some(size),
                ArgKind::Audience(a) => parsed.audience = a,
                ArgKind::Priority(p) => parsed.priority = Some(p),
                // Unreachable: the gate above rejects unknown keys.
                ArgKind::Unknown(_) => {}
            }
        }
        Ok(parsed)
    }
}

/// Whether `attr` is the helper attribute `name`, however it is spelled:
/// `#[tool]`, `#[turbomcp::tool]`, or through a renamed dependency
/// (`#[sdk::tool]`). Matching the bare identifier alone let the qualified
/// spellings through as plain methods that registered nothing.
fn attr_named(attr: &Attribute, name: &str) -> bool {
    attr.path().segments.last().is_some_and(|s| s.ident == name)
}

const MARKERS: [&str; 4] = ["tool", "prompt", "resource", "completion"];

/// Find and remove a `#[tool]` / `#[prompt]` / `#[resource(...)]` / `#[completion]`
/// marker from a method's attributes, returning its parsed form (and `None` for
/// plain methods).
fn take_marker(attrs: &mut Vec<Attribute>) -> syn::Result<Option<Marker>> {
    let is_marker = |a: &Attribute| MARKERS.iter().any(|m| attr_named(a, m));
    let Some(pos) = attrs.iter().position(is_marker) else {
        return Ok(None);
    };
    let attr = attrs.remove(pos);
    // One method is one component. A second marker used to be left on the
    // method, where it did nothing: `#[tool] #[prompt]` registered a tool.
    if let Some(extra) = attrs.iter().find(|a| is_marker(a)) {
        return Err(syn::Error::new(
            extra.span(),
            "a method takes one of #[tool], #[resource], #[prompt] or #[completion], not several",
        ));
    }
    let doc = doc_comment(attrs);
    if attr_named(&attr, "completion") {
        return Ok(Some(Marker::Completion));
    }
    let marker = if attr_named(&attr, "tool") {
        MarkerKind::Tool
    } else if attr_named(&attr, "prompt") {
        MarkerKind::Prompt
    } else {
        MarkerKind::Resource
    };
    let mut args = MarkerArgs::parse(&attr, marker)?;
    let kind = match marker {
        MarkerKind::Tool => HandlerKind::Tool,
        MarkerKind::Prompt => HandlerKind::Prompt,
        // On `#[resource]` the bare string is the URI, and it is required.
        MarkerKind::Resource => {
            let uri = args.positional.take().ok_or_else(|| {
                syn::Error::new(
                    attr.span(),
                    "#[resource(\"uri\")] requires a string URI argument",
                )
            })?;
            HandlerKind::Resource {
                uri: uri.value(),
                span: uri.span(),
            }
        }
    };
    // Elsewhere the bare string is the description shorthand; an explicit
    // `description = "…"` wins over it, and both win over the doc comment.
    let desc = args
        .desc
        .take()
        .map(|e| Desc::Explicit(Box::new(e)))
        .or_else(|| {
            args.positional.as_ref().map(|lit| {
                Desc::Explicit(Box::new(Expr::Lit(ExprLit {
                    attrs: Vec::new(),
                    lit: Lit::Str(lit.clone()),
                })))
            })
        })
        .or(doc);
    Ok(Some(Marker::Handler {
        kind,
        desc,
        args: Box::new(args),
    }))
}

// ---- handler model -----------------------------------------------------------

enum HandlerKind {
    Tool,
    Prompt,
    Resource { uri: String, span: Span },
}

struct ArgParam {
    ident: Ident,
    ty: Type,
    description: Option<String>,
    is_header: bool,
    is_option: bool,
}

enum Slot {
    Ctx,
    Arg(usize),
    /// The `Params<T>` holding the whole input, bound as `__tmcp_whole`.
    Params,
}

struct Handler {
    kind: HandlerKind,
    method: Ident,
    /// `name = "…"` from the marker: the name this handler answers to on the
    /// wire. `None` means the Rust method name. Decoupled on purpose — the
    /// wire name is a public contract, and welding it to a Rust identifier
    /// makes an internal rename a breaking change for clients (and rules out
    /// names that aren't valid identifiers, like `search.web`).
    wire_name: Option<LitStr>,
    description: Option<Desc>,
    args: Vec<ArgParam>,
    /// Ordered call sites (skipping the receiver) so the call can be rebuilt.
    slots: Vec<Slot>,
    /// The declared return type (`None` for `-> ()`), used to detect a
    /// `Json<T>` result and generate the tool's `outputSchema`.
    ret_ty: Option<Type>,
    /// `#[tool(task)]`: the tool may run as a task (on `2025-11-25` core Tasks
    /// and the `2026-07-28` Tasks extension alike). Tools only.
    task: bool,
    /// `#[tool(task = "required")]`: it can only run as a task.
    task_required: bool,
    /// `#[tool(scopes(…))]`: OAuth scopes the caller must hold. Tools only.
    scopes: Vec<String>,
    /// `tags(…)`: categorization for catalog policy, carried in the
    /// component's `_meta`. All three kinds.
    tags: Vec<String>,
    /// `title = …`: human-facing display name (all three kinds).
    title: Option<Expr>,
    /// `#[resource(mime_type = …)]`: the MIME type of what this resource
    /// serves, when it is known statically. Resources only.
    mime_type: Option<Expr>,
    /// `#[tool(read_only, …)]` behavior hints → `ToolAnnotations`. Tools only.
    hints: ToolHints,
    /// `#[tool(schema_extend = "{…}")]`: extra top-level JSON Schema keywords
    /// merged into the generated `inputSchema`. Tools only.
    schema_extend: Option<LitStr>,
    /// `Params<T>`: `T`, the handler's whole input. Tools and prompts.
    params: Option<Type>,
    /// `icon(…)`, each a `neutral::Icon` expression. All three kinds.
    icons: Vec<TokenStream>,
    /// `meta("key" = …)`. All three kinds.
    meta: Vec<(LitStr, TokenStream)>,
    /// `#[tool(output_schema = T)]`: `T`'s schema as the tool's
    /// `outputSchema`, for a tool returning a `CallToolResult`.
    output_schema: Option<Type>,
    /// `#[resource(size = N)]`, fixed resources only.
    size: Option<Expr>,
    /// `#[resource(audience(…), priority = …)]` → its `Annotations`.
    audience: Vec<TokenStream>,
    priority: Option<Expr>,
}

impl Handler {
    /// Apply the marker's arguments. Keys that don't belong to this handler's
    /// kind were already rejected at parse time, so they are simply absent.
    fn apply(&mut self, args: MarkerArgs) {
        self.wire_name = args.name;
        self.title = args.title;
        self.mime_type = args.mime_type;
        self.task = args.task;
        self.task_required = args.task_required;
        self.scopes = args.scopes;
        self.tags = args.tags;
        self.hints = args.hints;
        self.schema_extend = args.schema_extend;
        self.icons = args.icons;
        self.meta = args.meta;
        self.output_schema = args.output_schema;
        self.size = args.size;
        self.audience = args.audience;
        self.priority = args.priority;
    }

    /// Metadata that contradicts the handler: an `output_schema` beside a
    /// `Json<T>` return (whose schema already is the output schema), a
    /// `size` on a template (it describes many resources).
    fn validate_metadata(&self) -> syn::Result<()> {
        if let Some(explicit) = &self.output_schema
            && self.ret_ty.as_ref().and_then(json_output_inner).is_some()
        {
            return Err(syn::Error::new_spanned(
                explicit,
                "this tool returns `Json<T>`, whose schema is already its `outputSchema`; \
                 drop `output_schema`",
            ));
        }
        if let (Some(size), HandlerKind::Resource { uri, .. }) = (&self.size, &self.kind)
            && uri.contains('{')
        {
            return Err(syn::Error::new_spanned(
                size,
                "`size` describes one resource; a URI template stands for many",
            ));
        }
        Ok(())
    }

    /// `.with_icon(…)` per icon and `.with_meta_entry(…)` per `meta` entry:
    /// every component kind has both builders.
    fn icons_and_meta(&self) -> TokenStream {
        let icons = self.icons.iter().map(|i| quote!(.with_icon(#i)));
        let meta = self.meta.iter().map(
            |(k, v)| quote!(.with_meta_entry(#k, ::turbomcp::__macros::serde_json::json!(#v))),
        );
        quote!(#(#icons)* #(#meta)*)
    }

    /// `.with_meta_entry(TAGS, [...])`, or nothing when this handler declared no
    /// tags — an empty array in `_meta` would be a lie a filter has to special-case.
    fn tags_meta(&self) -> Option<TokenStream> {
        string_meta(&self.tags, quote!(::turbomcp::__macros::TAGS_META_KEY))
    }

    /// `.with_meta_entry(SCOPES, [...])` for `#[tool(scopes(…))]`.
    ///
    /// The requirement is enforced in the call arm regardless; publishing it
    /// here is what lets a catalog policy filter `tools/list` by it, and what
    /// carries it across a mount.
    fn scopes_meta(&self) -> Option<TokenStream> {
        string_meta(&self.scopes, quote!(::turbomcp::__macros::SCOPES_META_KEY))
    }

    /// The name this handler answers to on the wire.
    ///
    /// Defaults to the Rust method name with any raw-identifier prefix removed:
    /// `#[tool] async fn r#type` is a tool named `type`, not `r#type` (which
    /// isn't a name the spec permits).
    fn wire_name(&self) -> String {
        self.wire_name
            .as_ref()
            .map_or_else(|| self.method.unraw().to_string(), LitStr::value)
    }

    /// The span to blame for a wire-name problem: the literal when the name was
    /// given explicitly, the method identifier when it was derived.
    fn wire_name_span(&self) -> Span {
        self.wire_name
            .as_ref()
            .map_or_else(|| self.method.span(), LitStr::span)
    }

    /// This resource's URI. Panics on any other kind — callers filter first.
    fn resource_uri(&self) -> String {
        match &self.kind {
            HandlerKind::Resource { uri, .. } => uri.clone(),
            _ => unreachable!("resource_uri on a non-resource handler"),
        }
    }

    fn parse(f: &ImplItemFn, description: Option<Desc>, kind: HandlerKind) -> syn::Result<Self> {
        if f.sig.asyncness.is_none() {
            return Err(syn::Error::new(
                f.sig.span(),
                "handler methods must be `async`",
            ));
        }
        let mut args = Vec::new();
        let mut slots = Vec::new();
        let mut params: Option<Type> = None;
        let expected = match &kind {
            HandlerKind::Tool => ("#[tool]", "CallToolContext"),
            HandlerKind::Prompt => ("#[prompt]", "GetPromptContext"),
            HandlerKind::Resource { .. } => ("#[resource]", "ReadResourceContext"),
        };
        for input in &f.sig.inputs {
            match input {
                FnArg::Receiver(r) => check_receiver(r)?,
                FnArg::Typed(pt) => {
                    if check_context(&pt.ty, expected)? {
                        if slots.iter().any(|s| matches!(s, Slot::Ctx)) {
                            return Err(syn::Error::new(
                                pt.ty.span(),
                                "a handler takes its context once",
                            ));
                        }
                        slots.push(Slot::Ctx);
                        continue;
                    }
                    if let Some(inner) = params_inner(&pt.ty) {
                        if matches!(kind, HandlerKind::Resource { .. }) {
                            return Err(syn::Error::new_spanned(
                                &pt.ty,
                                "a #[resource] binds its URI template's variables; `Params<T>` \
                                 is for tools and prompts",
                            ));
                        }
                        if params.is_some() || !args.is_empty() {
                            return Err(syn::Error::new_spanned(
                                &pt.ty,
                                "`Params<T>` is the handler's whole input: take it alone \
                                 (beside an optional context)",
                            ));
                        }
                        params = Some(inner.clone());
                        slots.push(Slot::Params);
                        continue;
                    }
                    if params.is_some() {
                        return Err(syn::Error::new_spanned(
                            &pt.ty,
                            "`Params<T>` is the handler's whole input: put this field in `T`",
                        ));
                    }
                    let Pat::Ident(pi) = &*pt.pat else {
                        return Err(syn::Error::new(
                            pt.pat.span(),
                            "handler arguments are plain identifiers (destructure a `Params<T>` \
                             if you want a pattern)",
                        ));
                    };
                    let description = param_description(&pt.attrs)?;
                    let is_header = pt.attrs.iter().any(|a| attr_named(a, "mcp_header"));
                    let is_option = type_is_option(&pt.ty);
                    slots.push(Slot::Arg(args.len()));
                    args.push(ArgParam {
                        ident: pi.ident.clone(),
                        ty: (*pt.ty).clone(),
                        description,
                        is_header,
                        is_option,
                    });
                }
            }
        }

        if let HandlerKind::Resource { uri, span } = &kind {
            let vars = template_vars(uri).map_err(|e| {
                syn::Error::new(*span, format!("invalid URI template `{uri}`: {e}"))
            })?;
            if vars.is_empty() && !args.is_empty() {
                return Err(syn::Error::new(
                    f.sig.span(),
                    "a fixed-URI #[resource] takes only `&self` and an optional context; \
                     use a URI template (e.g. `#[resource(\"file://{path}\")]`) to accept args",
                ));
            }
            // Every handler argument must name a template variable, and one the
            // URI may leave out must be able to say so.
            for a in &args {
                let name = a.ident.unraw().to_string();
                let Some(var) = vars.iter().find(|v| v.name == name) else {
                    return Err(syn::Error::new(
                        a.ident.span(),
                        format!(
                            "resource argument `{name}` does not match any variable in the URI template `{uri}`"
                        ),
                    ));
                };
                if !var.required && !a.is_option {
                    return Err(syn::Error::new(
                        a.ident.span(),
                        format!(
                            "`{name}` is optional in the URI template `{uri}` (a URI may leave \
                             it out), so take it as an `Option<…>`"
                        ),
                    ));
                }
            }
        }

        let ret_ty = match &f.sig.output {
            syn::ReturnType::Type(_, ty) => Some((**ty).clone()),
            syn::ReturnType::Default => None,
        };

        Ok(Self {
            kind,
            method: f.sig.ident.clone(),
            wire_name: None,
            description,
            args,
            slots,
            ret_ty,
            task: false,
            task_required: false,
            scopes: Vec::new(),
            tags: Vec::new(),
            title: None,
            mime_type: None,
            hints: ToolHints::default(),
            schema_extend: None,
            params,
            icons: Vec::new(),
            meta: Vec::new(),
            output_schema: None,
            size: None,
            audience: Vec::new(),
            priority: None,
        })
    }

    /// Reconstruct the call argument list mapping `Ctx` → `ctx` and `Arg(i)` to
    /// the given per-argument expression (e.g. `__args.name` or a local).
    fn call_args(&self, arg_expr: impl Fn(&ArgParam) -> TokenStream) -> Vec<TokenStream> {
        self.slots
            .iter()
            .map(|slot| match slot {
                Slot::Ctx => quote!(__tmcp_ctx),
                Slot::Arg(i) => arg_expr(&self.args[*i]),
                Slot::Params => quote!(::turbomcp::Params(__tmcp_whole)),
            })
            .collect()
    }
}

// ---- codegen: McpServerCore --------------------------------------------------

fn gen_core_impl(
    target: &Target,
    args: &ServerArgs,
    registrations: &TokenStream,
) -> syn::Result<TokenStream> {
    // Unset, the server is the crate: its `Cargo.toml` name and version,
    // so a release bump can't leave the advertised version behind.
    let name = args.name.as_ref().map_or_else(
        || quote!(::core::env!("CARGO_PKG_NAME")),
        ToTokens::to_token_stream,
    );
    let version = args.version.as_ref().map_or_else(
        || quote!(::core::env!("CARGO_PKG_VERSION")),
        ToTokens::to_token_stream,
    );
    let title_set = args.title.as_ref().map(
        |t| quote!(__info.title = ::core::option::Option::Some(::core::convert::Into::into(#t));),
    );
    let description_set = args
        .description
        .as_ref()
        .map(|d| quote!(__info = __info.with_description(#d);));
    let website_set = args
        .website_url
        .as_ref()
        .map(|u| quote!(__info = __info.with_website_url(#u);));
    let icons_set = (!args.icons.is_empty()).then(|| {
        let icons = &args.icons;
        quote!(__info = ::turbomcp::__macros::with_server_icons(__info, ::std::vec![#(#icons),*]);)
    });
    let instructions_fn = args.instructions.as_ref().map(|i| {
        quote! {
            fn instructions(&self) -> ::core::option::Option<::std::string::String> {
                ::core::option::Option::Some(::core::convert::Into::into(#i))
            }
        }
    });
    // `protocols(…)` narrows the dual-stack default. A `static` (not a
    // promoted temporary) because `ProtocolVersion` owns a `String` in its
    // `Unknown` variant, so it has a destructor and can't be const-promoted.
    let protocols_fn = if args.protocols.is_empty() {
        None
    } else {
        let variants = args
            .protocols
            .iter()
            .map(protocol_variant)
            .collect::<syn::Result<Vec<_>>>()?;
        Some(quote! {
            fn supported_versions(&self) -> &'static [::turbomcp::ProtocolVersion] {
                static SUPPORTED: &[::turbomcp::ProtocolVersion] = &[#(#variants),*];
                SUPPORTED
            }
        })
    };
    let core_head = target.head(Some(quote!(::turbomcp::McpServerCore)));
    Ok(quote! {
        #core_head {
            fn server_info(&self) -> ::turbomcp::Implementation {
                #[allow(unused_mut)]
                let mut __info = ::turbomcp::Implementation::new(#name, #version);
                #title_set
                #description_set
                #website_set
                #icons_set
                __info
            }
            #instructions_fn
            #protocols_fn
            fn register(
                router: ::turbomcp::MethodRouter<Self>,
            ) -> ::turbomcp::MethodRouter<Self> {
                router #registrations
            }
        }
    })
}

// ---- codegen: tools ----------------------------------------------------------

fn gen_tools_impl(target: &Target, tools: &[Handler]) -> TokenStream {
    let self_ty = target.ty;
    let head = target.head(Some(quote!(::turbomcp::WithTools)));
    let arg_structs = tools
        .iter()
        .filter(|t| t.params.is_none())
        .map(|t| gen_args_struct(self_ty, t));
    // Marking any tool `#[tool(task)]` makes tasks opt-in for this server:
    // the rest say `forbidden` themselves (the spec's default), as data on
    // each tool, so it holds wherever the tools are served, mounted included.
    let opt_in = tools.iter().any(|t| t.task);
    // One definition per tool, behind a hidden associated fn that both the
    // list and the lookup use: its schema is generated, and computed, once.
    let tool_fn = |t: &Handler| format_ident!("__tmcp_tool_{}", t.method.unraw());
    let definitions = tools.iter().map(|t| {
        let f = tool_fn(t);
        let entry = gen_tool_list_entry(self_ty, t, opt_in);
        quote! {
            #[doc(hidden)]
            #[allow(non_snake_case)]
            fn #f() -> &'static ::turbomcp::neutral::Tool #entry
        }
    });
    let inherent = target.head(None);
    let list_entries = tools.iter().map(|t| {
        let f = tool_fn(t);
        quote!(::core::clone::Clone::clone(Self::#f()))
    });
    let call_arms = tools.iter().map(|t| gen_tool_call_arm(self_ty, t));
    let lookup_arms = tools.iter().map(|t| {
        let name = t.wire_name();
        let f = tool_fn(t);
        quote!(#name => Some(::core::clone::Clone::clone(Self::#f())),)
    });

    quote! {
        #(#arg_structs)*

        #inherent {
            #(#definitions)*
        }

        #head {
            async fn lookup_tool(&self, _ctx: &::turbomcp::ListToolsContext, name: String)
                -> ::turbomcp::McpResult<Option<::turbomcp::neutral::Tool>> {
                Ok(match name.as_str() { #(#lookup_arms)* _ => None })
            }
            async fn list_tools(
                &self,
                _ctx: &::turbomcp::ListToolsContext,
                params: ::turbomcp::neutral::ListParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::ListToolsResult> {
                params.reject_unknown_cursor("tools/list")?;
                ::core::result::Result::Ok(::turbomcp::neutral::ListToolsResult::new(
                    ::std::vec![ #(#list_entries),* ],
                ))
            }

            #[allow(unused_variables)]
            async fn call_tool(
                &self,
                __tmcp_ctx: &::turbomcp::CallToolContext,
                __tmcp_params: ::turbomcp::neutral::CallToolParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::CallToolResult> {
                match __tmcp_params.name.as_str() {
                    #(#call_arms)*
                    other => ::core::result::Result::Ok(
                        ::turbomcp::neutral::CallToolResult::error(
                            ::std::format!("unknown tool: {}", other)
                        )
                    ),
                }
            }
        }
    }
}

/// The generated argument struct's name.
///
/// Qualified by the *server type*, not just the method: two `#[server]` impls
/// in one module may each have a tool of the same name, and a module-scoped
/// name would collide — surfacing as a baffling "`__Tmcp_foo_Args` is defined
/// multiple times" pointing at the attribute rather than the real cause.
fn args_struct_ident(self_ty: &Type, t: &Handler) -> Ident {
    format_ident!("__Tmcp_{}_{}_Args", type_tag(self_ty), t.method)
}

/// An identifier-safe tag for a server type: its path segments joined
/// (`foo::Bar` → `foo_Bar`, so `a::Bar` and `b::Bar` in one module don't
/// collide), or a hash of the tokens for anything that isn't a plain path.
fn type_tag(self_ty: &Type) -> Ident {
    if let Type::Path(p) = self_ty
        && p.qself.is_none()
    {
        let joined: Vec<String> = p
            .path
            .segments
            .iter()
            .map(|s| s.ident.unraw().to_string())
            .collect();
        return format_ident!("{}", joined.join("_"));
    }
    use std::hash::{Hash as _, Hasher as _};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    self_ty.to_token_stream().to_string().hash(&mut hasher);
    format_ident!("Ty{:016x}", hasher.finish())
}

fn gen_args_struct(self_ty: &Type, t: &Handler) -> TokenStream {
    let ident = args_struct_ident(self_ty, t);
    let fields = t.args.iter().map(|a| {
        let name = &a.ident;
        let ty = &a.ty;
        let desc = a
            .description
            .as_ref()
            .map(|d| quote!(#[schemars(description = #d)]));
        quote! { #desc pub #name: #ty, }
    });
    quote! {
        #[derive(
            ::turbomcp::__macros::serde::Deserialize,
            ::turbomcp::__macros::schemars::JsonSchema,
        )]
        #[serde(crate = "::turbomcp::__macros::serde")]
        #[schemars(crate = "::turbomcp::__macros::schemars")]
        #[allow(non_camel_case_types, dead_code)]
        struct #ident { #(#fields)* }
    }
}

fn gen_tool_list_entry(self_ty: &Type, t: &Handler, opt_in: bool) -> TokenStream {
    let ident = args_struct_ident(self_ty, t);
    // The input schema: a `Params<T>`'s `T`, else the generated argument
    // struct.
    let input_ty = t
        .params
        .as_ref()
        .map_or_else(|| quote!(#ident), ToTokens::to_token_stream);
    let name = t.wire_name();
    let desc = t.description.as_ref().map(|d| {
        let d = d.tokens();
        quote!(.with_description(#d))
    });
    let header_marks = t.args.iter().filter(|a| a.is_header).map(|a| {
        let prop = a.ident.unraw().to_string();
        // Spanned at the parameter's type, so a `#[mcp_header] lat: f64` is
        // an error on `f64` naming the rule, not a tool that every conforming
        // HTTP client has to drop from `tools/list`.
        let ty = &a.ty;
        let check = quote_spanned!(ty.span()=>
            ::turbomcp::__macros::assert_header_param::<#ty>();
        );
        quote!(#check ::turbomcp::__macros::mark_mcp_header(&mut __schema, #prop);)
    });
    // A `-> Json<T>` (optionally inside `McpResult<_>`) return produces the
    // tool's outputSchema from `T` (requires `T: schemars::JsonSchema`).
    let output_ty = t
        .output_schema
        .as_ref()
        .or_else(|| t.ret_ty.as_ref().and_then(json_output_inner));
    let output_schema = output_ty.map(|inner| {
        quote!(.with_output_schema(
            ::turbomcp::__macros::normalize_input_schema(
                ::turbomcp::__macros::serde_json::to_value(
                    ::turbomcp::__macros::schemars::schema_for!(#inner)
                ).unwrap_or_else(|_| ::turbomcp::__macros::serde_json::Value::Object(
                    ::core::default::Default::default()
                ))
            )
        ))
    });
    // `#[tool(task)]` advertises per-tool task support: `Optional`, or
    // `Required` for `task = "required"`.
    let task_support = if t.task {
        Some(if t.task_required {
            quote!(.with_task_support(::turbomcp::neutral::TaskSupport::Required))
        } else {
            quote!(.with_task_support(::turbomcp::neutral::TaskSupport::Optional))
        })
    } else {
        opt_in.then(|| quote!(.with_task_support(::turbomcp::neutral::TaskSupport::Forbidden)))
    };
    let title = t.title.as_ref().map(|s| quote!(.with_title(#s)));
    // Behavior hints (`read_only`, `destructive`, …) → `ToolAnnotations`.
    let annotations = t.hints.any().then(|| {
        let set = [
            ("read_only_hint", t.hints.read_only),
            ("destructive_hint", t.hints.destructive),
            ("idempotent_hint", t.hints.idempotent),
            ("open_world_hint", t.hints.open_world),
        ]
        .into_iter()
        .filter_map(|(field, v)| {
            let field = Ident::new(field, proc_macro2::Span::call_site());
            v.map(|v| quote!(__annotations.#field = ::core::option::Option::Some(#v);))
        })
        .collect::<Vec<_>>();
        quote!(.with_annotations({
            let mut __annotations = ::turbomcp::neutral::ToolAnnotations::new();
            #(#set)*
            __annotations
        }))
    });
    let tags = t.tags_meta();
    let scopes = t.scopes_meta();
    let extras = t.icons_and_meta();
    // `schema_extend` is applied last so an explicit keyword wins over the
    // derived one (including `additionalProperties`).
    let schema_extend = t
        .schema_extend
        .as_ref()
        .map(|s| quote!(__schema = ::turbomcp::__macros::extend_object_schema(__schema, #s);));
    quote! {
        {
            static __TOOL: ::std::sync::LazyLock<::turbomcp::neutral::Tool> = ::std::sync::LazyLock::new(|| {
            let mut __schema = ::turbomcp::__macros::close_object_schema(
                ::turbomcp::__macros::normalize_input_schema(
                    ::turbomcp::__macros::serde_json::to_value(
                        ::turbomcp::__macros::schemars::schema_for!(#input_ty)
                    ).unwrap_or_else(|_| ::turbomcp::__macros::serde_json::Value::Object(
                        ::core::default::Default::default()
                    ))
                )
            );
            #schema_extend
            #(#header_marks)*
            ::turbomcp::neutral::Tool::new(#name, __schema)
                #desc #title #annotations #output_schema #task_support #tags #scopes #extras
            });
            &*__TOOL
        }
    }
}

fn gen_tool_call_arm(self_ty: &Type, t: &Handler) -> TokenStream {
    let ident = args_struct_ident(self_ty, t);
    let name = t.wire_name();
    let method = &t.method;
    let call_args = t.call_args(|a| {
        let f = &a.ident;
        quote!(__args.#f)
    });
    // `#[tool(scopes(…))]`: deny the call unless the caller holds every scope.
    // The error names all of them (one step-up asks for everything the tool
    // needs); the router makes it a tool error and a step-up challenge.
    let scope_guard = (!t.scopes.is_empty()).then(|| {
        let scopes = &t.scopes;
        quote! {
            if !__tmcp_ctx.base.has_scopes(&[#(#scopes),*]) {
                return ::core::result::Result::Err(
                    ::turbomcp::McpError::insufficient_scope([#(#scopes),*])
                );
            }
        }
    });
    let binding = if let Some(ty) = &t.params {
        quote!(__tmcp_whole: #ty)
    } else {
        quote!(__args: #ident)
    };
    quote! {
        #name => {
            #scope_guard
            let #binding = match ::turbomcp::__macros::serde_json::from_value(
                ::turbomcp::__macros::serde_json::Value::Object(__tmcp_params.arguments)
            ) {
                ::core::result::Result::Ok(a) => a,
                ::core::result::Result::Err(e) => {
                    return ::core::result::Result::Ok(
                        ::turbomcp::neutral::CallToolResult::error(
                            ::std::format!("invalid arguments for tool '{}': {}", #name, e)
                        )
                    );
                }
            };
            ::turbomcp::IntoCallToolResult::into_call_tool_result_for(
                self.#method(#(#call_args),*).await,
                __tmcp_ctx,
            )
        }
    }
}

// ---- codegen: resources ------------------------------------------------------

fn gen_resources_impl(target: &Target, resources: &[Handler]) -> TokenStream {
    let head = target.head(Some(quote!(::turbomcp::WithResources)));
    let is_template = |r: &Handler| {
        let HandlerKind::Resource { uri, .. } = &r.kind else {
            unreachable!()
        };
        uri.contains('{')
    };
    let fixed: Vec<&Handler> = resources.iter().filter(|r| !is_template(r)).collect();
    let templated: Vec<&Handler> = resources.iter().filter(|r| is_template(r)).collect();

    // resources/list — concrete resources only (templates go to templates/list).
    let list_entries = fixed.iter().map(|r| {
        let uri = r.resource_uri();
        let name = r.wire_name();
        let meta = resource_metadata(r);
        quote!(::turbomcp::neutral::Resource::new(#uri, #name) #meta)
    });

    // resources/templates/list — parameterized URIs.
    let template_entries = templated.iter().map(|r| {
        let uri = r.resource_uri();
        let name = r.wire_name();
        let meta = resource_metadata(r);
        quote!(::turbomcp::neutral::ResourceTemplate::new(#uri, #name) #meta)
    });

    let fixed_arms = fixed.iter().map(|r| {
        let uri = r.resource_uri();
        let method = &r.method;
        let call_args = r.call_args(|_| quote!(compile_error!("fixed resource takes no args")));
        let default_mime = declared_mime_default(r);
        quote! {
            #uri => return ::turbomcp::IntoReadResourceResult::into_read_resource_result(
                self.#method(#(#call_args),*).await,
                #uri,
            ) #default_mime,
        }
    });

    // Each templated resource: try to match the incoming URI, bind vars by name.
    let template_matches = templated.iter().map(|r| {
        let uri = r.resource_uri();
        let method = &r.method;
        let default_mime = declared_mime_default(r);
        let extracts = r.args.iter().map(|a| {
            let arg_name = a.ident.unraw().to_string();
            // A variable the URI left out is simply absent.
            parsed_binding(
                a,
                &quote!(__vars.iter().find(|(k, _)| k == #arg_name).map(|(_, v)| v)),
                &quote!(
                    return ::core::result::Result::Err(::turbomcp::McpError::internal(
                        ::std::format!("template var '{}' missing", #arg_name)
                    ))
                ),
                Some(&quote!(::turbomcp::McpError::resource_not_found(__uri))),
            )
        });
        let call_args = r.call_args(|a| {
            let f = &a.ident;
            quote!(#f)
        });
        quote! {
            if let ::core::option::Option::Some(__vars) =
                ::turbomcp::__macros::match_uri_template(#uri, __uri)
            {
                #(#extracts)*
                return ::turbomcp::IntoReadResourceResult::into_read_resource_result(
                    self.#method(#(#call_args),*).await,
                    __uri,
                ) #default_mime;
            }
        }
    });

    let templates_list_fn = (!templated.is_empty()).then(|| {
        quote! {
            async fn list_resource_templates(
                &self,
                _ctx: &::turbomcp::ListResourceTemplatesContext,
                params: ::turbomcp::neutral::ListParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::ListResourceTemplatesResult> {
                params.reject_unknown_cursor("resources/templates/list")?;
                ::core::result::Result::Ok(
                    ::turbomcp::neutral::ListResourceTemplatesResult::new(
                        ::std::vec![ #(#template_entries),* ],
                    )
                )
            }
        }
    });

    // Every URI checked against the matcher's own grammar at compile time,
    // blamed on its attribute.
    let grammar_checks = resources.iter().map(|r| {
        let uri = r.resource_uri();
        let HandlerKind::Resource { span, .. } = &r.kind else {
            unreachable!("resources only")
        };
        quote_spanned! {*span=>
            const _: () = if let ::core::result::Result::Err(__why) =
                ::turbomcp::__macros::validate_uri_template(#uri)
            {
                ::core::panic!("{}", __why);
            };
        }
    });
    quote! {
        #(#grammar_checks)*

        #head {
            async fn list_resources(
                &self,
                _ctx: &::turbomcp::ListResourcesContext,
                params: ::turbomcp::neutral::ListParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::ListResourcesResult> {
                params.reject_unknown_cursor("resources/list")?;
                ::core::result::Result::Ok(::turbomcp::neutral::ListResourcesResult::new(
                    ::std::vec![ #(#list_entries),* ],
                ))
            }

            #templates_list_fn

            #[allow(unused_variables)]
            async fn read_resource(
                &self,
                __tmcp_ctx: &::turbomcp::ReadResourceContext,
                __tmcp_params: ::turbomcp::neutral::ReadResourceParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::ReadResourceResult> {
                let __uri = __tmcp_params.uri.as_str();
                match __uri {
                    #(#fixed_arms)*
                    _ => {}
                }
                #(#template_matches)*
                ::core::result::Result::Err(
                    ::turbomcp::McpError::resource_not_found(__uri)
                )
            }
        }
    }
}

/// The tail that carries a declared `mime_type` into the *read* result.
///
/// `resources/list` advertised it and `resources/read` did not, so a client
/// that consulted the catalogue and then read the resource got two different
/// answers. Applied as a default: a handler returning its own `ResourceContents`
/// with a MIME type keeps it.
fn declared_mime_default(r: &Handler) -> TokenStream {
    match r.mime_type.as_ref() {
        Some(m) => quote!(.map(|__r| __r.with_default_mime_type(#m))),
        None => quote!(),
    }
}

/// The builder tail carrying a resource's optional metadata. `Resource` and
/// `ResourceTemplate` expose the same setters, so one generator serves both.
fn resource_metadata(r: &Handler) -> TokenStream {
    let desc = r.description.as_ref().map(|d| {
        let d = d.tokens();
        quote!(.with_description(#d))
    });
    let title = r.title.as_ref().map(|t| quote!(.with_title(#t)));
    let mime = r.mime_type.as_ref().map(|m| quote!(.with_mime_type(#m)));
    let tags = r.tags_meta();
    let extras = r.icons_and_meta();
    let size = r.size.as_ref().map(|n| quote!(.with_size(#n)));
    let annotations = (!r.audience.is_empty() || r.priority.is_some()).then(|| {
        let audience = &r.audience;
        let priority = r
            .priority
            .as_ref()
            .map(|p| quote!(__annotations.priority = ::core::option::Option::Some(#p);));
        quote!(.with_annotations({
            let mut __annotations = ::turbomcp::neutral::Annotations::new();
            __annotations.audience = ::std::vec![#(#audience),*];
            #priority
            __annotations
        }))
    });
    quote!(#desc #title #mime #size #annotations #tags #extras)
}

// ---- codegen: prompts --------------------------------------------------------

fn gen_prompts_impl(target: &Target, prompts: &[Handler]) -> TokenStream {
    let head = target.head(Some(quote!(::turbomcp::WithPrompts)));
    let list_entries = prompts.iter().map(gen_prompt_list_entry);
    let get_arms = prompts.iter().map(gen_prompt_get_arm);
    quote! {
        #head {
            async fn list_prompts(
                &self,
                _ctx: &::turbomcp::ListPromptsContext,
                params: ::turbomcp::neutral::ListParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::ListPromptsResult> {
                params.reject_unknown_cursor("prompts/list")?;
                ::core::result::Result::Ok(::turbomcp::neutral::ListPromptsResult::new(
                    ::std::vec![ #(#list_entries),* ],
                ))
            }

            #[allow(unused_variables)]
            async fn get_prompt(
                &self,
                __tmcp_ctx: &::turbomcp::GetPromptContext,
                __tmcp_params: ::turbomcp::neutral::GetPromptParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::GetPromptResult> {
                match __tmcp_params.name.as_str() {
                    #(#get_arms)*
                    other => ::core::result::Result::Err(
                        ::turbomcp::McpError::invalid_params(
                            ::std::format!("unknown prompt: {}", other)
                        )
                    ),
                }
            }
        }
    }
}

fn gen_prompt_list_entry(p: &Handler) -> TokenStream {
    let name = p.wire_name();
    let desc = p.description.as_ref().map(|d| {
        let d = d.tokens();
        quote!(.with_description(#d))
    });
    let title = p.title.as_ref().map(|t| quote!(.with_title(#t)));
    let args = p.args.iter().map(|a| {
        let arg_name = a.ident.unraw().to_string();
        let req = (!a.is_option).then(|| quote!(.required(true)));
        let adesc = a
            .description
            .as_ref()
            .map(|d| quote!(.with_description(#d)));
        quote!(.with_argument(::turbomcp::neutral::PromptArgument::new(#arg_name) #req #adesc))
    });
    let tags = p.tags_meta();
    let extras = p.icons_and_meta();
    let prompt =
        quote!(::turbomcp::neutral::Prompt::new(#name) #desc #title #tags #extras #(#args)*);
    // A `Params<T>` prompt's arguments are `T`'s properties.
    match &p.params {
        Some(ty) => quote! {{
            let mut __prompt = #prompt;
            __prompt.arguments = ::turbomcp::__macros::prompt_arguments(
                &::turbomcp::__macros::serde_json::to_value(
                    ::turbomcp::__macros::schemars::schema_for!(#ty)
                ).unwrap_or_default()
            );
            __prompt
        }},
        None => prompt,
    }
}

/// `let #ident: #ty = …` from a string-valued `source`
/// (`Option<&String>`): parsed with `FromStr` (spanned at the type, so a
/// type that can't be parsed is an error there), `None` for an absent
/// `Option<…>` and `missing` otherwise.
///
/// `refuse` replaces a parse failure's error (a template capture that
/// doesn't parse names no resource, so it is not found, not invalid).
fn parsed_binding(
    a: &ArgParam,
    source: &TokenStream,
    missing: &TokenStream,
    refuse: Option<&TokenStream>,
) -> TokenStream {
    let ident = &a.ident;
    let ty = &a.ty;
    let arg_name = a.ident.unraw().to_string();
    let inner = option_inner(ty);
    let map_err = refuse.map(|e| quote!(.map_err(|_| #e)));
    let parse = quote_spanned! {inner.span()=>
        ::turbomcp::__macros::parse_argument::<#inner>(#arg_name, __v) #map_err ?
    };
    if a.is_option {
        quote! {
            let #ident: #ty = match #source {
                ::core::option::Option::Some(__v) => ::core::option::Option::Some(#parse),
                ::core::option::Option::None => ::core::option::Option::None,
            };
        }
    } else {
        quote! {
            let #ident: #ty = match #source {
                ::core::option::Option::Some(__v) => #parse,
                ::core::option::Option::None => #missing,
            };
        }
    }
}

fn gen_prompt_get_arm(p: &Handler) -> TokenStream {
    let name = p.wire_name();
    let method = &p.method;
    let extracts = p.args.iter().map(|a| {
        let arg_name = a.ident.unraw().to_string();
        parsed_binding(
            a,
            &quote!(__tmcp_params.arguments.get(#arg_name)),
            &quote!(
                return ::core::result::Result::Err(::turbomcp::McpError::invalid_params(
                    ::std::format!("missing required prompt argument '{}'", #arg_name)
                ))
            ),
            None,
        )
    });
    // A `Params<T>` prompt: its string arguments, deserialized into `T`.
    let whole = p.params.as_ref().map(|ty| {
        quote! {
            let __tmcp_whole: #ty = match ::turbomcp::__macros::serde_json::from_value(
                ::turbomcp::__macros::serde_json::Value::Object(
                    ::core::iter::Iterator::collect(::core::iter::Iterator::map(
                        __tmcp_params.arguments.into_iter(),
                        |(k, v)| (k, ::turbomcp::__macros::serde_json::Value::String(v)),
                    ))
                )
            ) {
                ::core::result::Result::Ok(w) => w,
                ::core::result::Result::Err(e) => return ::core::result::Result::Err(
                    ::turbomcp::McpError::invalid_params(
                        ::std::format!("arguments for prompt '{}': {}", #name, e)
                    )
                ),
            };
        }
    });
    let call_args = p.call_args(|a| {
        let f = &a.ident;
        quote!(#f)
    });
    quote! {
        #name => {
            #(#extracts)*
            #whole
            ::turbomcp::IntoGetPromptResult::into_get_prompt_result(
                self.#method(#(#call_args),*).await
            )
        }
    }
}

// ---- codegen: completions ----------------------------------------------------

/// The single `#[completion]` handler: its method name and whether it takes a
/// `&CompleteContext` (so the generated delegation passes `ctx` or not).
struct CompletionHandler {
    method: Ident,
    wants_ctx: bool,
}

impl CompletionHandler {
    fn parse(f: &ImplItemFn) -> syn::Result<Self> {
        if f.sig.asyncness.is_none() {
            return Err(syn::Error::new(
                f.sig.span(),
                "handler methods must be `async`",
            ));
        }
        let mut wants_ctx = false;
        let mut value_params = 0usize;
        for input in &f.sig.inputs {
            let pt = match input {
                FnArg::Receiver(r) => {
                    check_receiver(r)?;
                    continue;
                }
                FnArg::Typed(pt) => pt,
            };
            if check_context(&pt.ty, ("#[completion]", "CompleteContext"))? {
                wants_ctx = true;
            } else {
                value_params += 1;
            }
        }
        if value_params != 1 {
            return Err(syn::Error::new(
                f.sig.span(),
                "a #[completion] handler takes exactly one `neutral::CompleteParams` \
                 argument (plus an optional `&CompleteContext`)",
            ));
        }
        Ok(Self {
            method: f.sig.ident.clone(),
            wants_ctx,
        })
    }
}

fn gen_completions_impl(target: &Target, c: &CompletionHandler) -> TokenStream {
    let head = target.head(Some(quote!(::turbomcp::WithCompletions)));
    let method = &c.method;
    let call = if c.wants_ctx {
        quote!(self.#method(__tmcp_ctx, __tmcp_params))
    } else {
        quote!(self.#method(__tmcp_params))
    };
    quote! {
        #head {
            #[allow(unused_variables)]
            async fn complete(
                &self,
                __tmcp_ctx: &::turbomcp::CompleteContext,
                __tmcp_params: ::turbomcp::neutral::CompleteParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::CompleteResult> {
                #call.await
            }
        }
    }
}

// ---- small helpers -----------------------------------------------------------

/// One variable of a resource URI template.
struct TemplateVar {
    name: String,
    /// Whether every matching URI binds it. `{var}` and `{+var}` do; the
    /// other operators expand to nothing when the variable is undefined, so a
    /// URI may leave theirs out.
    required: bool,
}

/// The variables of an RFC 6570 template, for binding handler arguments.
///
/// This only finds them. The grammar is the runtime matcher's
/// (`turbomcp_server::uri_template::validate`), which the generated code
/// evaluates in a `const` so a template the matcher could never read fails to
/// compile; the obvious mistakes are reported here first, at the attribute.
fn template_vars(uri: &str) -> Result<Vec<TemplateVar>, String> {
    let mut vars = Vec::new();
    let mut rest = uri;
    while let Some(open) = rest.find(['{', '}']) {
        if rest.as_bytes()[open] == b'}' {
            return Err("unmatched `}`".into());
        }
        let close = rest[open..]
            .find('}')
            .map(|c| open + c)
            .ok_or("unclosed `{`")?;
        let body = &rest[open + 1..close];
        let (required, list) = match body.chars().next() {
            Some('+') => (true, &body[1..]),
            Some('#' | '.' | '/' | ';' | '?' | '&') => (false, &body[1..]),
            _ => (true, body),
        };
        for spec in list.split(',') {
            let name = spec
                .strip_suffix('*')
                .or_else(|| spec.split_once(':').map(|(name, _)| name))
                .unwrap_or(spec);
            vars.push(TemplateVar {
                name: name.to_owned(),
                required,
            });
        }
        rest = &rest[close + 1..];
    }
    Ok(vars)
}

/// A component's description.
enum Desc {
    /// `description = …` or the bare-string shorthand: used as written.
    Explicit(Box<Expr>),
    /// The doc comment's attributes in order (`#[doc = include_str!(…)]`
    /// included), shaped for a model at first use by `doc_description`.
    Doc(Vec<Expr>),
}

impl Desc {
    /// An expression for the description (`impl Into<String>`).
    fn tokens(&self) -> TokenStream {
        match self {
            Self::Explicit(e) => e.to_token_stream(),
            Self::Doc(parts) => {
                let mut joined = Vec::new();
                for (i, part) in parts.iter().enumerate() {
                    if i > 0 {
                        joined.push(quote!("\n"));
                    }
                    joined.push(part.to_token_stream());
                }
                quote! {{
                    static __DOC: ::std::sync::LazyLock<::std::string::String> =
                        ::std::sync::LazyLock::new(|| {
                            ::turbomcp::__macros::doc_description(::core::concat!(#(#joined),*))
                        });
                    ::core::clone::Clone::clone(&*__DOC)
                }}
            }
        }
    }
}

/// The doc comment, `#[doc = "…"]` and `#[doc = include_str!(…)]` alike.
fn doc_comment(attrs: &[Attribute]) -> Option<Desc> {
    let parts: Vec<Expr> = attrs
        .iter()
        .filter(|a| a.path().is_ident("doc"))
        .filter_map(|a| match &a.meta {
            Meta::NameValue(nv) => Some(nv.value.clone()),
            _ => None,
        })
        .collect();
    (!parts.is_empty()).then_some(Desc::Doc(parts))
}

/// `#[description("…")]` on a parameter.
fn param_description(attrs: &[Attribute]) -> syn::Result<Option<String>> {
    for a in attrs {
        if attr_named(a, "description") {
            let s = a.parse_args::<syn::LitStr>()?;
            return Ok(Some(s.value()));
        }
    }
    Ok(None)
}

/// Remove parameter helper attributes (`#[description]`, `#[mcp_header]`) so the
/// re-emitted method compiles.
fn strip_param_attrs(f: &mut ImplItemFn) {
    for input in &mut f.sig.inputs {
        if let FnArg::Typed(pt) = input {
            pt.attrs
                .retain(|a| !attr_named(a, "description") && !attr_named(a, "mcp_header"));
        }
    }
}

/// A handler's receiver must be `&self`: the server is cloned into every
/// connection and called concurrently, so mutable state belongs behind a
/// lock, and a by-value `self` would be consumed by the first call.
fn check_receiver(r: &syn::Receiver) -> syn::Result<()> {
    if !matches!(r.kind, syn::ReceiverKind::Reference(_, _, None)) {
        return Err(syn::Error::new_spanned(
            r,
            "a handler takes `&self`: the server serves requests concurrently, so keep \
             mutable state behind a lock (`Arc<Mutex<…>>`) instead",
        ));
    }
    Ok(())
}

/// Whether `ty` is this handler's context (`expected` is the marker and the
/// context it receives). A reference to another `…Context` is an error
/// naming the right one: the generated code would otherwise fail with a
/// type mismatch pointing at the attribute.
fn check_context(ty: &Type, (marker, expected): (&str, &str)) -> syn::Result<bool> {
    let Type::Reference(r) = ty else {
        return Ok(false);
    };
    let Type::Path(p) = &*r.elem else {
        return Ok(false);
    };
    let Some(seg) = p.path.segments.last() else {
        return Ok(false);
    };
    let name = seg.ident.to_string();
    if !name.ends_with("Context") {
        return Ok(false);
    }
    if name != expected {
        let hint = if name == "RequestContext" {
            " (the request's `RequestContext` is its `.base`)"
        } else {
            ""
        };
        return Err(syn::Error::new_spanned(
            ty,
            format!("a {marker} takes `&{expected}` as its context, not `&{name}`{hint}"),
        ));
    }
    if r.mutability.is_some() {
        return Err(syn::Error::new_spanned(
            ty,
            format!("take the context as `&{expected}`, not `&mut`"),
        ));
    }
    Ok(true)
}

/// `T` of a `Params<T>` parameter.
fn params_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    (seg.ident == "Params")
        .then(|| first_generic_type(seg))
        .flatten()
}

/// `T` of an `Option<T>`, or the type itself.
fn option_inner(ty: &Type) -> &Type {
    if let Type::Path(p) = ty
        && let Some(seg) = p.path.segments.last()
        && seg.ident == "Option"
        && let Some(inner) = first_generic_type(seg)
    {
        return inner;
    }
    ty
}

/// Whether a type is `Option<…>`.
fn type_is_option(ty: &Type) -> bool {
    if let Type::Path(p) = ty
        && let Some(seg) = p.path.segments.last()
    {
        return seg.ident == "Option";
    }
    false
}

/// The first angle-bracketed generic type argument of a path segment, if any
/// (e.g. `T` of `Json<T>`).
fn first_generic_type(seg: &syn::PathSegment) -> Option<&Type> {
    let syn::PathArguments::AngleBracketed(ab) = &seg.arguments else {
        return None;
    };
    ab.args.iter().find_map(|a| match a {
        syn::GenericArgument::Type(t) => Some(t),
        _ => None,
    })
}

/// If `ty` is `Json<T>` — possibly wrapped in `Result<_, _>` / `McpResult<_>` —
/// return `T`, the type whose schema becomes the tool's `outputSchema`. Matching
/// is by the last path segment's identifier, so `turbomcp::Json<T>` works too.
fn json_output_inner(ty: &Type) -> Option<&Type> {
    let Type::Path(p) = ty else { return None };
    let seg = p.path.segments.last()?;
    match seg.ident.to_string().as_str() {
        "Json" => first_generic_type(seg),
        "Result" | "McpResult" => json_output_inner(first_generic_type(seg)?),
        _ => None,
    }
}
