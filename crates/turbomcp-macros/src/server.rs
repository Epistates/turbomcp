//! Expansion of `#[server]`.
//!
//! The driver parses the annotated `impl` block, classifies each method by its
//! `#[tool]` / `#[resource]` / `#[prompt]` marker, and emits:
//! - the user's `impl` block, cleaned of the marker/parameter helper attributes;
//! - `impl McpServerCore` (from the `name`/`version` args);
//! - one capability trait impl per kind present (`WithTools`, `WithResources`,
//!   `WithPrompts`) — so advertised capabilities are derived from what's written;
//! - per-tool argument structs (deriving `Deserialize` + `JsonSchema`) that back
//!   compile-time schema generation and pre-call validation;
//! - inherent `into_server()` (pre-registering the discovered capabilities) and
//!   `run_stdio()` entry points.
//!
//! All generated paths are rooted at `::turbomcp` so the macro works from any
//! downstream crate.

use proc_macro2::TokenStream;
use quote::{ToTokens as _, format_ident, quote};
use syn::parse::{Parse, ParseStream, Parser};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{
    Attribute, Expr, ExprLit, FnArg, Ident, ImplItem, ImplItemFn, ItemImpl, Lit, LitStr, Meta,
    MetaNameValue, Pat, Token, Type, parse2,
};

/// Entry point called by the `#[proc_macro_attribute]` shim.
pub(crate) fn expand(attr: TokenStream, item: TokenStream) -> syn::Result<TokenStream> {
    let args = ServerArgs::parse(attr)?;
    let mut block: ItemImpl = parse2(item)?;
    // The generated trait impls name the type directly, so generic parameters
    // would escape their binder. Say so plainly — otherwise the user sees
    // "cannot find type `T` in this scope" pointing at generated code.
    if !block.generics.params.is_empty() {
        return Err(syn::Error::new(
            block.generics.span(),
            "#[server] does not support generic impl blocks — the generated \
             `McpServerCore`/`WithTools` impls are for one concrete type. \
             Apply it to a concrete type (e.g. a type alias or newtype), or \
             implement the capability traits by hand.",
        ));
    }
    let self_ty = (*block.self_ty).clone();

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
            Marker::Tool {
                desc,
                name,
                task,
                scopes,
                title,
                hints,
            } => {
                let mut h = Handler::parse(f, desc, HandlerKind::Tool)?;
                h.wire_name = name;
                h.task = task;
                h.scopes = scopes;
                h.title = title;
                h.hints = hints;
                tools.push(h);
            }
            Marker::Prompt { desc, name } => {
                let mut h = Handler::parse(f, desc, HandlerKind::Prompt)?;
                h.wire_name = name;
                prompts.push(h);
            }
            Marker::Resource { uri, desc } => {
                resources.push(Handler::parse(f, desc, HandlerKind::Resource { uri })?);
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

    // `name = "…"` makes collisions expressible, and two handlers answering
    // the same wire name would silently shadow one another in dispatch.
    reject_duplicate_names(&tools, "tool")?;
    reject_duplicate_names(&prompts, "prompt")?;

    let core_impl = gen_core_impl(&self_ty, &args)?;
    let tools_impl = (!tools.is_empty()).then(|| gen_tools_impl(&self_ty, &tools));
    let resources_impl = (!resources.is_empty()).then(|| gen_resources_impl(&self_ty, &resources));
    let prompts_impl = (!prompts.is_empty()).then(|| gen_prompts_impl(&self_ty, &prompts));
    let completions_impl = completion
        .as_ref()
        .map(|c| gen_completions_impl(&self_ty, c));

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

    let entry_impl = quote! {
        impl #self_ty {
            /// Build a configurable server with this type's capabilities registered.
            pub fn into_server(self) -> ::turbomcp::ServerBuilder<Self> {
                ::turbomcp::ServerBuilder::new(self) #registrations
            }

            /// Serve over stdio until the peer closes stdin.
            ///
            /// Dual-stack by default: the connection is wrapped in a
            /// [`LegacySessionAdapter`](::turbomcp::LegacySessionAdapter), so
            /// both stateless `2026-07-28` clients and stateful
            /// `2025-11-25` (`initialize`-handshake) clients are served.
            pub async fn run_stdio(self) -> ::core::result::Result<(), ::turbomcp::ProtocolError> {
                ::turbomcp::serve_stdio(::turbomcp::LegacySessionAdapter::new(
                    self.into_server().build(),
                ))
                .await
            }
        }
    };

    Ok(quote! {
        #block
        #core_impl
        #tools_impl
        #resources_impl
        #prompts_impl
        #completions_impl
        #entry_impl
    })
}

/// Reject two handlers of the same kind answering to one wire name. Tools and
/// prompts are separate namespaces, so this is checked per kind.
fn reject_duplicate_names(handlers: &[Handler], kind: &str) -> syn::Result<()> {
    let mut seen: std::collections::HashMap<String, &Ident> = std::collections::HashMap::new();
    for h in handlers {
        let name = h.wire_name();
        if let Some(first) = seen.get(&name) {
            return Err(syn::Error::new(
                h.method.span(),
                format!(
                    "two {kind}s answer to the wire name `{name}`; it is already \
                     claimed by the method `{first}`. Wire names must be unique \
                     within a server — rename one, or give it a distinct \
                     `name = \"…\"`."
                ),
            ));
        }
        seen.insert(name, &h.method);
    }
    Ok(())
}

// ---- attribute parsing -------------------------------------------------------

struct ServerArgs {
    name: String,
    version: String,
    title: Option<String>,
    instructions: Option<String>,
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
        let mut protocols = Vec::new();
        for m in metas {
            let key = m
                .path()
                .get_ident()
                .map(ToString::to_string)
                .unwrap_or_default();
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
            let val = lit_str(&nv.value)
                .ok_or_else(|| syn::Error::new(nv.value.span(), "expected a string literal"))?;
            match key.as_str() {
                "name" => name = Some(val),
                "version" => version = Some(val),
                "title" => title = Some(val),
                "instructions" => instructions = Some(val),
                other => {
                    return Err(syn::Error::new(
                        m.path().span(),
                        format!(
                            "unknown #[server] argument `{other}` (expected name, version, title, instructions, protocols)"
                        ),
                    ));
                }
            }
        }
        Ok(Self {
            name: name
                .ok_or_else(|| syn::Error::new(attr.span(), "#[server] requires `name = \"…\"`"))?,
            version: version.ok_or_else(|| {
                syn::Error::new(attr.span(), "#[server] requires `version = \"…\"`")
            })?,
            title,
            instructions,
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
        "2025-11-25" => Ok(quote! { ::turbomcp::ProtocolVersion::V2025_11_25 }),
        "2026-07-28" => Ok(quote! { ::turbomcp::ProtocolVersion::Draft }),
        other => Err(syn::Error::new(
            wire.span(),
            format!(
                "`{other}` is not a protocol version this build serves \
                 (expected \"2025-11-25\" or \"2026-07-28\")"
            ),
        )),
    }
}

enum Marker {
    Tool {
        desc: Option<String>,
        name: Option<String>,
        task: bool,
        scopes: Vec<String>,
        title: Option<String>,
        hints: ToolHints,
    },
    Prompt {
        desc: Option<String>,
        name: Option<String>,
    },
    Resource {
        uri: String,
        desc: Option<String>,
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

/// One argument inside `#[tool(...)]`: a bare description string, `description =
/// "…"`, `title = "…"`, the `task` flag, `scopes("…", …)` (required OAuth
/// scopes), or a behavior hint — `read_only` / `destructive` / `idempotent` /
/// `open_world`, each optionally `= true|false` (bare = `true`).
enum ToolArg {
    Desc(String),
    Name(String),
    Title(String),
    Task,
    Scopes(Vec<String>),
    Hint(HintKind, bool),
}

#[derive(Clone, Copy)]
enum HintKind {
    ReadOnly,
    Destructive,
    Idempotent,
    OpenWorld,
}

fn hint_kind(meta: &Meta) -> Option<HintKind> {
    let p = meta.path();
    if p.is_ident("read_only") {
        Some(HintKind::ReadOnly)
    } else if p.is_ident("destructive") {
        Some(HintKind::Destructive)
    } else if p.is_ident("idempotent") {
        Some(HintKind::Idempotent)
    } else if p.is_ident("open_world") {
        Some(HintKind::OpenWorld)
    } else {
        None
    }
}

impl Parse for ToolArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(LitStr) {
            let s: LitStr = input.parse()?;
            return Ok(ToolArg::Desc(s.value()));
        }
        let meta: Meta = input.parse()?;
        if meta.path().is_ident("task") {
            Ok(ToolArg::Task)
        } else if meta.path().is_ident("description") {
            match meta {
                Meta::NameValue(nv) => lit_str(&nv.value)
                    .map(ToolArg::Desc)
                    .ok_or_else(|| syn::Error::new(nv.value.span(), "expected a string literal")),
                _ => Err(syn::Error::new(
                    meta.span(),
                    "expected `description = \"…\"`",
                )),
            }
        } else if meta.path().is_ident("name") {
            match meta {
                Meta::NameValue(nv) => lit_str(&nv.value)
                    .map(ToolArg::Name)
                    .ok_or_else(|| syn::Error::new(nv.value.span(), "expected a string literal")),
                _ => Err(syn::Error::new(meta.span(), "expected `name = \"…\"`")),
            }
        } else if meta.path().is_ident("title") {
            match meta {
                Meta::NameValue(nv) => lit_str(&nv.value)
                    .map(ToolArg::Title)
                    .ok_or_else(|| syn::Error::new(nv.value.span(), "expected a string literal")),
                _ => Err(syn::Error::new(meta.span(), "expected `title = \"…\"`")),
            }
        } else if meta.path().is_ident("scopes") {
            match meta {
                Meta::List(list) => {
                    let lits =
                        list.parse_args_with(Punctuated::<LitStr, Token![,]>::parse_terminated)?;
                    Ok(ToolArg::Scopes(lits.iter().map(LitStr::value).collect()))
                }
                _ => Err(syn::Error::new(meta.span(), "expected `scopes(\"…\", …)`")),
            }
        } else if let Some(kind) = hint_kind(&meta) {
            match meta {
                // Bare flag: `read_only` means the hint is true.
                Meta::Path(_) => Ok(ToolArg::Hint(kind, true)),
                // Explicit: `destructive = false` declares the hint false
                // (distinct from not declaring it — the spec defaults differ).
                Meta::NameValue(nv) => match &nv.value {
                    syn::Expr::Lit(el) => match &el.lit {
                        syn::Lit::Bool(b) => Ok(ToolArg::Hint(kind, b.value)),
                        _ => Err(syn::Error::new(
                            nv.value.span(),
                            "expected `true` or `false`",
                        )),
                    },
                    _ => Err(syn::Error::new(
                        nv.value.span(),
                        "expected `true` or `false`",
                    )),
                },
                Meta::List(l) => Err(syn::Error::new(
                    l.span(),
                    "behavior hints take no list — use the bare flag or `= true|false`",
                )),
            }
        } else {
            Err(syn::Error::new(
                meta.span(),
                "expected `description = \"…\"`, `name = \"…\"`, `title = \"…\"`, `task`, `scopes(…)`, \
                 or a behavior hint (`read_only`, `destructive`, `idempotent`, `open_world`)",
            ))
        }
    }
}

/// Everything `#[tool(...)]` can declare.
#[derive(Default)]
struct ToolMarkerArgs {
    desc: Option<String>,
    /// `#[tool(name = "…")]` — the wire name, when it should differ from the
    /// Rust method name.
    name: Option<String>,
    task: bool,
    scopes: Vec<String>,
    title: Option<String>,
    hints: ToolHints,
}

/// Parse `#[tool]`, `#[tool("…")]`, `#[tool(description = "…")]`, `#[tool(task)]`,
/// `#[tool(scopes(…))]`, `#[tool(title = "…")]`, behavior hints, and combinations.
fn parse_tool_args(attr: &Attribute) -> syn::Result<ToolMarkerArgs> {
    match &attr.meta {
        Meta::Path(_) => Ok(ToolMarkerArgs::default()),
        Meta::NameValue(nv) => Ok(ToolMarkerArgs {
            desc: lit_str(&nv.value),
            ..ToolMarkerArgs::default()
        }),
        Meta::List(_) => {
            let args = attr.parse_args_with(Punctuated::<ToolArg, Token![,]>::parse_terminated)?;
            let mut parsed = ToolMarkerArgs::default();
            for a in args {
                match a {
                    ToolArg::Desc(s) => parsed.desc = Some(s),
                    ToolArg::Name(s) => parsed.name = Some(s),
                    ToolArg::Title(s) => parsed.title = Some(s),
                    ToolArg::Task => parsed.task = true,
                    ToolArg::Scopes(s) => parsed.scopes = s,
                    ToolArg::Hint(HintKind::ReadOnly, v) => parsed.hints.read_only = Some(v),
                    ToolArg::Hint(HintKind::Destructive, v) => parsed.hints.destructive = Some(v),
                    ToolArg::Hint(HintKind::Idempotent, v) => parsed.hints.idempotent = Some(v),
                    ToolArg::Hint(HintKind::OpenWorld, v) => parsed.hints.open_world = Some(v),
                }
            }
            Ok(parsed)
        }
    }
}

/// Find and remove a `#[tool]` / `#[prompt]` / `#[resource(...)]` / `#[completion]`
/// marker from a method's attributes, returning its parsed form (and `None` for
/// plain methods).
fn take_marker(attrs: &mut Vec<Attribute>) -> syn::Result<Option<Marker>> {
    let Some(pos) = attrs.iter().position(|a| {
        let p = &a.path();
        p.is_ident("tool")
            || p.is_ident("prompt")
            || p.is_ident("resource")
            || p.is_ident("completion")
    }) else {
        return Ok(None);
    };
    let attr = attrs.remove(pos);
    let doc = doc_comment(attrs);
    if attr.path().is_ident("tool") {
        let parsed = parse_tool_args(&attr)?;
        Ok(Some(Marker::Tool {
            desc: parsed.desc.or(doc),
            name: parsed.name,
            task: parsed.task,
            scopes: parsed.scopes,
            title: parsed.title,
            hints: parsed.hints,
        }))
    } else if attr.path().is_ident("prompt") {
        let parsed = parse_prompt_args(&attr)?;
        Ok(Some(Marker::Prompt {
            desc: parsed.0.or(doc),
            name: parsed.1,
        }))
    } else if attr.path().is_ident("completion") {
        Ok(Some(Marker::Completion))
    } else {
        // #[resource("uri")] — URI is required.
        let uri = attr.parse_args::<syn::LitStr>().map_err(|_| {
            syn::Error::new(
                attr.span(),
                "#[resource(\"uri\")] requires a string URI argument",
            )
        })?;
        Ok(Some(Marker::Resource {
            uri: uri.value(),
            desc: doc,
        }))
    }
}

/// Extract a description from `#[tool("…")]` or `#[tool(description = "…")]`.
/// Parse `#[prompt]`, `#[prompt("…")]`, `#[prompt(description = "…")]`,
/// `#[prompt(name = "…")]`, and combinations, into `(description, name)`.
fn parse_prompt_args(attr: &Attribute) -> syn::Result<(Option<String>, Option<String>)> {
    match &attr.meta {
        Meta::Path(_) => Ok((None, None)),
        Meta::NameValue(nv) => Ok((lit_str(&nv.value), None)),
        Meta::List(_) => {
            // `#[prompt("…")]` — a bare description, the common shorthand.
            if let Ok(s) = attr.parse_args::<syn::LitStr>() {
                return Ok((Some(s.value()), None));
            }
            let metas =
                attr.parse_args_with(Punctuated::<MetaNameValue, Token![,]>::parse_terminated)?;
            let (mut desc, mut name) = (None, None);
            for nv in metas {
                let value = lit_str(&nv.value)
                    .ok_or_else(|| syn::Error::new(nv.value.span(), "expected a string literal"))?;
                if nv.path.is_ident("description") {
                    desc = Some(value);
                } else if nv.path.is_ident("name") {
                    name = Some(value);
                } else {
                    return Err(syn::Error::new(
                        nv.path.span(),
                        "expected `description = \"…\"` or `name = \"…\"`",
                    ));
                }
            }
            Ok((desc, name))
        }
    }
}

// ---- handler model -----------------------------------------------------------

enum HandlerKind {
    Tool,
    Prompt,
    Resource { uri: String },
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
}

struct Handler {
    kind: HandlerKind,
    method: Ident,
    /// `name = "…"` from the marker: the name this handler answers to on the
    /// wire. `None` means the Rust method name. Decoupled on purpose — the
    /// wire name is a public contract, and welding it to a Rust identifier
    /// makes an internal rename a breaking change for clients (and rules out
    /// names that aren't valid identifiers, like `search.web`).
    wire_name: Option<String>,
    description: Option<String>,
    args: Vec<ArgParam>,
    /// Ordered call sites (skipping the receiver) so the call can be rebuilt.
    slots: Vec<Slot>,
    /// The declared return type (`None` for `-> ()`), used to detect a
    /// `Json<T>` result and generate the tool's `outputSchema`.
    ret_ty: Option<Type>,
    /// `#[tool(task)]`: opt this tool into `2025-11-25` task support. Tools only.
    task: bool,
    /// `#[tool(scopes(…))]`: OAuth scopes the caller must hold. Tools only.
    scopes: Vec<String>,
    /// `#[tool(title = "…")]`: human-facing display name. Tools only.
    title: Option<String>,
    /// `#[tool(read_only, …)]` behavior hints → `ToolAnnotations`. Tools only.
    hints: ToolHints,
}

impl Handler {
    /// The name this handler answers to on the wire.
    fn wire_name(&self) -> String {
        self.wire_name
            .clone()
            .unwrap_or_else(|| self.method.to_string())
    }

    fn parse(f: &ImplItemFn, description: Option<String>, kind: HandlerKind) -> syn::Result<Self> {
        if f.sig.asyncness.is_none() {
            return Err(syn::Error::new(
                f.sig.span(),
                "handler methods must be `async`",
            ));
        }
        let mut args = Vec::new();
        let mut slots = Vec::new();
        for input in &f.sig.inputs {
            match input {
                FnArg::Receiver(_) => {} // &self
                FnArg::Typed(pt) => {
                    if is_ctx_type(&pt.ty) {
                        slots.push(Slot::Ctx);
                        continue;
                    }
                    let Pat::Ident(pi) = &*pt.pat else {
                        return Err(syn::Error::new(
                            pt.pat.span(),
                            "handler arguments must be simple identifiers",
                        ));
                    };
                    let description = param_description(&pt.attrs)?;
                    let is_header = pt.attrs.iter().any(|a| a.path().is_ident("mcp_header"));
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

        if let HandlerKind::Resource { uri } = &kind {
            let vars = template_vars(uri);
            if vars.is_empty() && !args.is_empty() {
                return Err(syn::Error::new(
                    f.sig.span(),
                    "a fixed-URI #[resource] takes only `&self` and an optional context; \
                     use a URI template (e.g. `#[resource(\"file://{path}\")]`) to accept args",
                ));
            }
            // Every handler argument must name a template variable.
            for a in &args {
                if !vars.contains(&a.ident.to_string()) {
                    return Err(syn::Error::new(
                        a.ident.span(),
                        format!(
                            "resource argument `{}` does not match any variable in the URI template `{uri}`",
                            a.ident
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
            scopes: Vec::new(),
            title: None,
            hints: ToolHints::default(),
        })
    }

    /// Reconstruct the call argument list mapping `Ctx` → `ctx` and `Arg(i)` to
    /// the given per-argument expression (e.g. `__args.name` or a local).
    fn call_args(&self, arg_expr: impl Fn(&ArgParam) -> TokenStream) -> Vec<TokenStream> {
        self.slots
            .iter()
            .map(|slot| match slot {
                Slot::Ctx => quote!(ctx),
                Slot::Arg(i) => arg_expr(&self.args[*i]),
            })
            .collect()
    }
}

// ---- codegen: McpServerCore --------------------------------------------------

fn gen_core_impl(self_ty: &Type, args: &ServerArgs) -> syn::Result<TokenStream> {
    let name = &args.name;
    let version = &args.version;
    let title_set = args.title.as_ref().map(
        |t| quote!(__info.title = ::core::option::Option::Some(::std::string::String::from(#t));),
    );
    let instructions_fn = args.instructions.as_ref().map(|i| {
        quote! {
            fn instructions(&self) -> ::core::option::Option<::std::string::String> {
                ::core::option::Option::Some(::std::string::String::from(#i))
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
    Ok(quote! {
        impl ::turbomcp::McpServerCore for #self_ty {
            fn server_info(&self) -> ::turbomcp::Implementation {
                #[allow(unused_mut)]
                let mut __info = ::turbomcp::Implementation::new(#name, #version);
                #title_set
                __info
            }
            #instructions_fn
            #protocols_fn
        }
    })
}

// ---- codegen: tools ----------------------------------------------------------

fn gen_tools_impl(self_ty: &Type, tools: &[Handler]) -> TokenStream {
    let arg_structs = tools.iter().map(|t| gen_args_struct(self_ty, t));
    let list_entries = tools.iter().map(|t| gen_tool_list_entry(self_ty, t));
    let call_arms = tools.iter().map(|t| gen_tool_call_arm(self_ty, t));

    quote! {
        #(#arg_structs)*

        impl ::turbomcp::WithTools for #self_ty {
            async fn list_tools(
                &self,
                _ctx: &::turbomcp::ListToolsContext,
                _params: ::turbomcp::neutral::ListParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::ListToolsResult> {
                ::core::result::Result::Ok(::turbomcp::neutral::ListToolsResult::new(
                    ::std::vec![ #(#list_entries),* ],
                ))
            }

            #[allow(unused_variables)]
            async fn call_tool(
                &self,
                ctx: &::turbomcp::CallToolContext,
                params: ::turbomcp::neutral::CallToolParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::CallToolResult> {
                match params.name.as_str() {
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

/// An identifier-safe tag for a server type: its final path segment
/// (`foo::Bar` → `Bar`), or a hash of the tokens for anything that isn't a
/// plain path, so the name stays unique either way.
fn type_tag(self_ty: &Type) -> Ident {
    if let Type::Path(p) = self_ty
        && let Some(seg) = p.path.segments.last()
    {
        return seg.ident.clone();
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

fn gen_tool_list_entry(self_ty: &Type, t: &Handler) -> TokenStream {
    let ident = args_struct_ident(self_ty, t);
    let name = t.wire_name();
    let desc = t
        .description
        .as_ref()
        .map(|d| quote!(.with_description(#d)));
    let header_marks = t.args.iter().filter(|a| a.is_header).map(|a| {
        let prop = a.ident.to_string();
        quote!(::turbomcp::__macros::mark_mcp_header(&mut __schema, #prop);)
    });
    // A `-> Json<T>` (optionally inside `McpResult<_>`) return produces the
    // tool's outputSchema from `T` (requires `T: schemars::JsonSchema`).
    let output_schema = t.ret_ty.as_ref().and_then(json_output_inner).map(|inner| {
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
    // `#[tool(task)]` advertises per-tool `2025-11-25` task support (Optional).
    let task_support = t
        .task
        .then(|| quote!(.with_task_support(::turbomcp::neutral::TaskSupport::Optional)));
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
    quote! {
        {
            let mut __schema = ::turbomcp::__macros::close_object_schema(
                ::turbomcp::__macros::normalize_input_schema(
                    ::turbomcp::__macros::serde_json::to_value(
                        ::turbomcp::__macros::schemars::schema_for!(#ident)
                    ).unwrap_or_else(|_| ::turbomcp::__macros::serde_json::Value::Object(
                        ::core::default::Default::default()
                    ))
                )
            );
            #(#header_marks)*
            ::turbomcp::neutral::Tool::new(#name, __schema)
                #desc #title #annotations #output_schema #task_support
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
    let scope_guard = (!t.scopes.is_empty()).then(|| {
        let scopes = &t.scopes;
        let needed = t.scopes.join(", ");
        quote! {
            if !ctx.base.identity.has_scopes(&[#(#scopes),*]) {
                return ::core::result::Result::Ok(
                    ::turbomcp::neutral::CallToolResult::error(
                        ::std::format!("insufficient scope: '{}' requires {}", #name, #needed)
                    )
                );
            }
        }
    });
    quote! {
        #name => {
            #scope_guard
            let __args: #ident = match ::turbomcp::__macros::serde_json::from_value(
                ::turbomcp::__macros::serde_json::Value::Object(params.arguments)
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
            ::turbomcp::IntoCallToolResult::into_call_tool_result(
                self.#method(#(#call_args),*).await
            )
        }
    }
}

// ---- codegen: resources ------------------------------------------------------

fn gen_resources_impl(self_ty: &Type, resources: &[Handler]) -> TokenStream {
    let is_template = |r: &Handler| {
        let HandlerKind::Resource { uri } = &r.kind else {
            unreachable!()
        };
        uri.contains('{')
    };
    let fixed: Vec<&Handler> = resources.iter().filter(|r| !is_template(r)).collect();
    let templated: Vec<&Handler> = resources.iter().filter(|r| is_template(r)).collect();

    // resources/list — concrete resources only (templates go to templates/list).
    let list_entries = fixed.iter().map(|r| {
        let HandlerKind::Resource { uri } = &r.kind else {
            unreachable!()
        };
        let name = r.method.to_string();
        let desc = r
            .description
            .as_ref()
            .map(|d| quote!(.with_description(#d)));
        quote!(::turbomcp::neutral::Resource::new(#uri, #name) #desc)
    });

    // resources/templates/list — parameterized URIs.
    let template_entries = templated.iter().map(|r| {
        let HandlerKind::Resource { uri } = &r.kind else {
            unreachable!()
        };
        let name = r.method.to_string();
        let desc = r
            .description
            .as_ref()
            .map(|d| quote!(.with_description(#d)));
        quote!(::turbomcp::neutral::ResourceTemplate::new(#uri, #name) #desc)
    });

    let fixed_arms = fixed.iter().map(|r| {
        let HandlerKind::Resource { uri } = &r.kind else {
            unreachable!()
        };
        let method = &r.method;
        let call_args = r.call_args(|_| quote!(compile_error!("fixed resource takes no args")));
        quote! {
            #uri => return ::turbomcp::IntoReadResourceResult::into_read_resource_result(
                self.#method(#(#call_args),*).await,
                #uri,
            ),
        }
    });

    // Each templated resource: try to match the incoming URI, bind vars by name.
    let template_matches = templated.iter().map(|r| {
        let HandlerKind::Resource { uri } = &r.kind else {
            unreachable!()
        };
        let method = &r.method;
        let extracts = r.args.iter().map(|a| {
            let ident = &a.ident;
            let arg_name = a.ident.to_string();
            quote! {
                let #ident: ::std::string::String = match __vars.iter()
                    .find(|(k, _)| k == #arg_name)
                {
                    ::core::option::Option::Some((_, v)) => ::core::clone::Clone::clone(v),
                    ::core::option::Option::None => return ::core::result::Result::Err(
                        ::turbomcp::McpError::internal(
                            ::std::format!("template var '{}' missing", #arg_name)
                        )
                    ),
                };
            }
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
                );
            }
        }
    });

    let templates_list_fn = (!templated.is_empty()).then(|| {
        quote! {
            async fn list_resource_templates(
                &self,
                _ctx: &::turbomcp::ListResourceTemplatesContext,
                _params: ::turbomcp::neutral::ListParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::ListResourceTemplatesResult> {
                ::core::result::Result::Ok(
                    ::turbomcp::neutral::ListResourceTemplatesResult::new(
                        ::std::vec![ #(#template_entries),* ],
                    )
                )
            }
        }
    });

    quote! {
        impl ::turbomcp::WithResources for #self_ty {
            async fn list_resources(
                &self,
                _ctx: &::turbomcp::ListResourcesContext,
                _params: ::turbomcp::neutral::ListParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::ListResourcesResult> {
                ::core::result::Result::Ok(::turbomcp::neutral::ListResourcesResult::new(
                    ::std::vec![ #(#list_entries),* ],
                ))
            }

            #templates_list_fn

            #[allow(unused_variables)]
            async fn read_resource(
                &self,
                ctx: &::turbomcp::ReadResourceContext,
                params: ::turbomcp::neutral::ReadResourceParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::ReadResourceResult> {
                let __uri = params.uri.as_str();
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

// ---- codegen: prompts --------------------------------------------------------

fn gen_prompts_impl(self_ty: &Type, prompts: &[Handler]) -> TokenStream {
    let list_entries = prompts.iter().map(gen_prompt_list_entry);
    let get_arms = prompts.iter().map(gen_prompt_get_arm);
    quote! {
        impl ::turbomcp::WithPrompts for #self_ty {
            async fn list_prompts(
                &self,
                _ctx: &::turbomcp::ListPromptsContext,
                _params: ::turbomcp::neutral::ListParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::ListPromptsResult> {
                ::core::result::Result::Ok(::turbomcp::neutral::ListPromptsResult::new(
                    ::std::vec![ #(#list_entries),* ],
                ))
            }

            #[allow(unused_variables)]
            async fn get_prompt(
                &self,
                ctx: &::turbomcp::GetPromptContext,
                params: ::turbomcp::neutral::GetPromptParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::GetPromptResult> {
                match params.name.as_str() {
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
    let desc = p
        .description
        .as_ref()
        .map(|d| quote!(.with_description(#d)));
    let args = p.args.iter().map(|a| {
        let arg_name = a.ident.to_string();
        let req = (!a.is_option).then(|| quote!(.required(true)));
        let adesc = a
            .description
            .as_ref()
            .map(|d| quote!(.with_description(#d)));
        quote!(.with_argument(::turbomcp::neutral::PromptArgument::new(#arg_name) #req #adesc))
    });
    quote!(::turbomcp::neutral::Prompt::new(#name) #desc #(#args)*)
}

fn gen_prompt_get_arm(p: &Handler) -> TokenStream {
    let name = p.wire_name();
    let method = &p.method;
    let extracts = p.args.iter().map(|a| {
        let ident = &a.ident;
        let arg_name = a.ident.to_string();
        if a.is_option {
            quote! {
                let #ident: ::core::option::Option<::std::string::String> =
                    params.arguments.get(#arg_name).cloned();
            }
        } else {
            quote! {
                let #ident: ::std::string::String = match params.arguments.get(#arg_name) {
                    ::core::option::Option::Some(v) => ::core::clone::Clone::clone(v),
                    ::core::option::Option::None => {
                        return ::core::result::Result::Err(
                            ::turbomcp::McpError::invalid_params(
                                ::std::format!("missing required prompt argument '{}'", #arg_name)
                            )
                        );
                    }
                };
            }
        }
    });
    let call_args = p.call_args(|a| {
        let f = &a.ident;
        quote!(#f)
    });
    quote! {
        #name => {
            #(#extracts)*
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
            let FnArg::Typed(pt) = input else { continue };
            if is_ctx_type(&pt.ty) {
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

fn gen_completions_impl(self_ty: &Type, c: &CompletionHandler) -> TokenStream {
    let method = &c.method;
    let call = if c.wants_ctx {
        quote!(self.#method(ctx, params))
    } else {
        quote!(self.#method(params))
    };
    quote! {
        impl ::turbomcp::WithCompletions for #self_ty {
            #[allow(unused_variables)]
            async fn complete(
                &self,
                ctx: &::turbomcp::CompleteContext,
                params: ::turbomcp::neutral::CompleteParams,
            ) -> ::turbomcp::McpResult<::turbomcp::neutral::CompleteResult> {
                #call.await
            }
        }
    }
}

// ---- small helpers -----------------------------------------------------------

/// Variable names in an RFC 6570 URI template (`{var}` / `{+var}`), in order.
fn template_vars(uri: &str) -> Vec<String> {
    let mut vars = Vec::new();
    let mut rest = uri;
    while let Some(open) = rest.find('{') {
        let Some(close_rel) = rest[open..].find('}') else {
            break;
        };
        let mut var = &rest[open + 1..open + close_rel];
        var = var.strip_prefix('+').unwrap_or(var);
        if !var.is_empty() {
            vars.push(var.to_string());
        }
        rest = &rest[open + close_rel + 1..];
    }
    vars
}

/// A string literal `Expr`, or `None`.
fn lit_str(e: &Expr) -> Option<String> {
    if let Expr::Lit(ExprLit {
        lit: Lit::Str(s), ..
    }) = e
    {
        Some(s.value())
    } else {
        None
    }
}

/// Concatenate `#[doc = "…"]` lines into a trimmed description.
fn doc_comment(attrs: &[Attribute]) -> Option<String> {
    let mut lines = Vec::new();
    for a in attrs {
        if !a.path().is_ident("doc") {
            continue;
        }
        if let Meta::NameValue(nv) = &a.meta
            && let Some(s) = lit_str(&nv.value)
        {
            lines.push(s.trim().to_string());
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines.join(" ").trim().to_string())
    }
}

/// `#[description("…")]` on a parameter.
fn param_description(attrs: &[Attribute]) -> syn::Result<Option<String>> {
    for a in attrs {
        if a.path().is_ident("description") {
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
                .retain(|a| !a.path().is_ident("description") && !a.path().is_ident("mcp_header"));
        }
    }
}

/// Whether a type is a reference to something named `…Context`.
fn is_ctx_type(ty: &Type) -> bool {
    let Type::Reference(r) = ty else { return false };
    if let Type::Path(p) = &*r.elem
        && let Some(seg) = p.path.segments.last()
    {
        return seg.ident.to_string().ends_with("Context");
    }
    false
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
