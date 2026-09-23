//! Server macro - generates McpHandler trait implementation.

use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{Ident, ItemImpl};

/// Helper to resolve the correct turbomcp crate path.
fn turbomcp_crate() -> TokenStream {
    match proc_macro_crate::crate_name("turbomcp") {
        Ok(proc_macro_crate::FoundCrate::Itself) => quote!(::turbomcp),
        Ok(proc_macro_crate::FoundCrate::Name(name)) => {
            let ident = Ident::new(&name, proc_macro2::Span::call_site());
            quote!(::#ident)
        }
        Err(_) => match proc_macro_crate::crate_name("turbomcp-server") {
            Ok(proc_macro_crate::FoundCrate::Itself) => quote!(::turbomcp_server),
            Ok(proc_macro_crate::FoundCrate::Name(name)) => {
                let ident = Ident::new(&name, proc_macro2::Span::call_site());
                quote!(::#ident)
            }
            Err(_) => quote!(crate),
        },
    }
}

use super::attrs::{CommonAttrs, parse_lit_str, parse_marker_attrs};
use super::tool::{
    ToolAttrs, ToolInfo, args_ident, ctx_ident, generate_annotations_code, generate_call_args,
    generate_execution_code, generate_extraction_code, generate_icons_code,
    generate_output_schema_code, generate_schema_code,
};
use syn::ext::IdentExt;

/// Information collected from analyzing the impl block.
pub struct ServerInfo {
    /// Struct name
    pub struct_name: Ident,
    /// Server name
    pub name: TokenStream,
    /// Server version
    pub version: TokenStream,
    /// Server description
    pub description: Option<TokenStream>,
    /// Human-readable title (SEP-973)
    pub title: Option<TokenStream>,
    /// Guidance returned as the `initialize` result's `instructions` field
    pub instructions: Option<TokenStream>,
    /// Homepage for this implementation
    pub website_url: Option<TokenStream>,
    /// Icon source URIs (SEP-973)
    pub icons: Vec<TokenStream>,
    /// Tool handlers
    pub tools: Vec<ToolInfo>,
    /// Resource handlers
    pub resources: Vec<ResourceInfo>,
    /// Prompt handlers
    pub prompts: Vec<PromptInfo>,
    /// Optional extension-point handlers discovered from marker attributes.
    pub extensions: ExtensionHandlers,
    /// `#[server(page_size = N)]`, if given.
    pub page_size: Option<TokenStream>,
}

/// The `McpHandler` methods a server can opt into with a marker attribute.
///
/// Each is `None` unless the impl block declares the corresponding marker, in
/// which case `#[server]` generates an override *and* advertises the matching
/// capability. Leaving one unset keeps the trait default, which reports
/// `capability_not_supported` — so what a server claims during initialization
/// always matches what it can actually serve.
///
/// `#[set_level]` is the exception: `logging` is always advertised, and the
/// trait default accepts the level, so the marker only lets a server observe it.
#[derive(Default)]
pub struct ExtensionHandlers {
    /// `#[completion]` → `complete` + `completions` capability.
    pub completion: Option<ExtensionHandler>,
    /// `#[subscribe]` → `subscribe` + `resources.subscribe` capability.
    pub subscribe: Option<ExtensionHandler>,
    /// `#[unsubscribe]` → `unsubscribe`. Required alongside `#[subscribe]`.
    pub unsubscribe: Option<ExtensionHandler>,
    /// `#[set_level]` → `set_log_level`.
    pub set_level: Option<ExtensionHandler>,
    /// `#[roots_changed]` → `on_roots_list_changed`. Advertises nothing:
    /// `roots` is a client capability, not a server one.
    pub roots_changed: Option<ExtensionHandler>,
}

/// A single marker-attributed method.
pub struct ExtensionHandler {
    /// Name of the user's method to call.
    pub fn_name: Ident,
    /// Whether the signature takes a `&RequestContext` after its value
    /// parameter, so the generated call passes the right number of arguments.
    pub takes_ctx: bool,
}

/// Resource handler info.
#[derive(Clone)]
pub struct ResourceInfo {
    /// Resource URI template
    pub uri_template: String,
    /// Resource name
    pub name: String,
    /// Resource description
    pub description: Option<String>,
    /// MIME type of the resource (HIGH-001)
    pub mime_type: Option<String>,
    /// Function name
    pub fn_name: Ident,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Version string
    pub version: Option<String>,
    /// Human-readable title (SEP-973).
    pub title: Option<String>,
    /// Icon URIs (SEP-973).
    pub icons: Vec<String>,
    /// `ResourceAnnotations` hints.
    pub annotations: ResourceAnnotationAttrs,
    /// `size = N`: the resource's size in bytes. Concrete resources only.
    pub size: Option<u64>,
}

/// `ResourceAnnotations` declared on a `#[resource]`.
///
/// Flat keys, like the tool annotation hints (`read_only = true`), rather than
/// a nested `annotations(...)` group.
#[derive(Clone, Default)]
pub struct ResourceAnnotationAttrs {
    /// `audience = ["user", "assistant"]`, as `Role` variant names.
    pub audience: Vec<Ident>,
    /// `priority = 0.0..=1.0`.
    pub priority: Option<f64>,
    /// `last_modified = "..."`, an ISO 8601 timestamp passed through as written.
    pub last_modified: Option<String>,
}

impl ResourceAnnotationAttrs {
    fn is_empty(&self) -> bool {
        self.audience.is_empty() && self.priority.is_none() && self.last_modified.is_none()
    }
}

/// Prompt handler info.
#[derive(Clone)]
pub struct PromptInfo {
    /// Prompt name
    pub name: String,
    /// Prompt description
    pub description: Option<String>,
    /// Prompt arguments (HIGH-002)
    pub arguments: Vec<PromptArgumentInfo>,
    /// Whether the handler returns `McpResult`/`Result<_, McpError>`, in which
    /// case an `Err` propagates instead of being rendered as a message.
    pub returns_mcp_error: bool,
    /// Whether the handler is `Result`-shaped at all. A fallible prompt whose
    /// error type is *not* `McpError` still has to propagate rather than render
    /// — it is just converted on the way.
    pub returns_result: bool,
    /// Function name
    pub fn_name: Ident,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Version string
    pub version: Option<String>,
    /// Human-readable title (SEP-973).
    pub title: Option<String>,
    /// Icon URIs (SEP-973).
    pub icons: Vec<String>,
}

/// Prompt argument info (HIGH-002).
#[derive(Clone)]
pub struct PromptArgumentInfo {
    /// Argument name on the wire, with any `r#` prefix removed.
    pub name: String,
    /// The parameter's identifier as written, for the generated binding.
    pub ident: Ident,
    /// Human-readable label for the argument, from `#[title("...")]`
    pub title: Option<String>,
    /// Argument description
    pub description: Option<String>,
    /// Whether the argument is required
    pub required: bool,
}

/// Parse server attributes.
///
/// Every value-bearing key accepts any expression that evaluates to something
/// `Into<String>`, not only a string literal, so server identity can come from
/// the build or the environment:
///
/// ```ignore
/// #[server(name = "calc", version = env!("CARGO_PKG_VERSION"))]
/// ```
#[derive(Default)]
pub struct ServerAttrs {
    /// Server name
    pub name: Option<syn::Expr>,
    /// Server version
    pub version: Option<syn::Expr>,
    /// Server description
    pub description: Option<syn::Expr>,
    /// Human-readable title (SEP-973)
    pub title: Option<syn::Expr>,
    /// Guidance returned as the `initialize` result's `instructions` field
    pub instructions: Option<syn::Expr>,
    /// Homepage for this implementation
    pub website_url: Option<syn::Expr>,
    /// Icon source URIs (SEP-973)
    pub icons: Vec<syn::Expr>,
    /// `page_size = N`: paginate the list methods at N entries.
    pub page_size: Option<syn::Expr>,
}

impl ServerAttrs {
    /// Parse from attribute token stream.
    pub fn parse(args: TokenStream) -> Result<Self, syn::Error> {
        let mut attrs = Self::default();

        if args.is_empty() {
            return Ok(attrs);
        }

        let Self {
            ref mut name,
            ref mut version,
            ref mut description,
            ref mut title,
            ref mut instructions,
            ref mut website_url,
            ref mut icons,
            ref mut page_size,
        } = attrs;

        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("name") {
                *name = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("version") {
                *version = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("description") {
                *description = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("title") {
                *title = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("instructions") {
                *instructions = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("website_url") {
                *website_url = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("icons") {
                let value = meta.value()?;
                let items;
                syn::bracketed!(items in value);
                *icons =
                    syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated(
                        &items,
                    )?
                    .into_iter()
                    .collect();
            } else if meta.path.is_ident("page_size") {
                *page_size = Some(meta.value()?.parse()?);
            } else if meta.path.is_ident("logging") {
                // A bare flag, still accepted so servers that declare it keep
                // compiling. It no longer changes anything: every server
                // can emit `notifications/message` through `ctx.log()`, and a
                // server that emits log notifications must declare `logging`,
                // so the capability is now always advertised.
            } else if meta.path.is_ident("transports") {
                // v3: The `transports` attribute was removed.
                //
                // Emit the migration diagnostic *before* trying to parse the
                // value, so users who write `transports = "stdio"` (string instead
                // of the v2 array form) get the migration guidance rather than a
                // generic `expected '['` diagnostic.
                return Err(syn::Error::new(
                    meta.path.span(),
                    "`transports` attribute was removed. Enable features in Cargo.toml instead:\n\
                    turbomcp = { version = \"3.1\", features = [\"http\", \"tcp\"] }\n\
                    Then call transport methods: server.run_http(\"0.0.0.0:8080\").await",
                ));
            } else if meta.path.is_ident("root") {
                // v3: Ignore `root` attribute for backward compatibility.
                // Roots configuration should be done via builder API.
                let _value: syn::LitStr = meta.value()?.parse()?;
            } else {
                // Name the accepted keys rather than dropping a typo silently:
                // `descriptio = "..."` used to compile into a server with no
                // description and no diagnostic.
                let key = meta
                    .path
                    .get_ident()
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string());
                return Err(meta.error(format!(
                    "unknown #[server] attribute key `{key}`; expected one of `name`, \
                     `version`, `description`, `title`, `instructions`, `website_url`, `icons`, \
                     `page_size`, `logging`",
                )));
            }
            Ok(())
        });

        syn::parse::Parser::parse2(parser, args)?;

        Ok(attrs)
    }
}

/// Analyze an impl block and extract server information.
pub fn analyze_impl(impl_block: &ItemImpl, attrs: &ServerAttrs) -> Result<ServerInfo, syn::Error> {
    // Extract struct name
    let struct_name = match &*impl_block.self_ty {
        syn::Type::Path(type_path) => match type_path.path.segments.last() {
            Some(segment) => segment.ident.clone(),
            None => {
                return Err(syn::Error::new_spanned(
                    &type_path.path,
                    "Expected a valid type path",
                ));
            }
        },
        _ => {
            return Err(syn::Error::new_spanned(
                &impl_block.self_ty,
                "The #[server] attribute only supports named types",
            ));
        }
    };

    let name = match &attrs.name {
        Some(expr) => quote!(#expr),
        None => {
            let literal = struct_name.to_string();
            quote!(#literal)
        }
    };
    let version = match &attrs.version {
        Some(expr) => quote!(#expr),
        None => quote!("1.0.0"),
    };
    let to_tokens = |expr: &Option<syn::Expr>| expr.as_ref().map(|e| quote!(#e));
    let description = to_tokens(&attrs.description);
    let title = to_tokens(&attrs.title);
    let instructions = to_tokens(&attrs.instructions);
    let website_url = to_tokens(&attrs.website_url);
    let icons = attrs.icons.iter().map(|e| quote!(#e)).collect();

    let mut tools = Vec::new();
    let mut resources = Vec::new();
    let mut prompts = Vec::new();
    let mut extensions = ExtensionHandlers::default();

    // Analyze methods
    for item in &impl_block.items {
        if let syn::ImplItem::Fn(method) = item {
            for attr in &method.attrs {
                if attr.path().is_ident("tool") {
                    // Parse tool attributes (description, tags, version)
                    let tool_attrs = ToolAttrs::parse(attr)?;
                    let item_fn = syn::ItemFn {
                        attrs: method.attrs.clone(),
                        vis: method.vis.clone(),
                        modifiers: method.modifiers.clone(),
                        sig: method.sig.clone(),
                        block: Box::new(syn::parse_quote!({})),
                    };
                    let tool_info = ToolInfo::from_fn(&item_fn, tool_attrs)?;
                    tools.push(tool_info);
                    break;
                } else if attr.path().is_ident("resource") {
                    let resource_attrs = extract_resource_attrs(attr)?;
                    let fn_name = method.sig.ident.clone();
                    let common = resource_attrs.common;
                    // An explicit description wins over the doc comment, as on
                    // `#[tool]`.
                    let description = common
                        .description
                        .or_else(|| extract_doc_comments(&method.attrs));
                    resources.push(ResourceInfo {
                        uri_template: resource_attrs.uri_template,
                        name: fn_name.unraw().to_string(),
                        description,
                        mime_type: resource_attrs.mime_type,
                        fn_name,
                        tags: common.tags,
                        version: common.version,
                        title: common.title,
                        icons: common.icons,
                        annotations: resource_attrs.annotations,
                        size: resource_attrs.size,
                    });
                    break;
                } else if attr.path().is_ident("prompt") {
                    let fn_name = method.sig.ident.clone();
                    let common = extract_prompt_attrs(attr)?;
                    // An explicit description wins over the doc comment, as on
                    // `#[tool]`. It used to be the other way round here, so a
                    // `description = "..."` next to any `///` line was ignored.
                    let description = common
                        .description
                        .or_else(|| extract_doc_comments(&method.attrs));
                    let arguments = extract_prompt_arguments(&method.sig);
                    prompts.push(PromptInfo {
                        name: fn_name.unraw().to_string(),
                        description,
                        arguments,
                        returns_mcp_error: returns_mcp_error(&method.sig),
                        returns_result: returns_result(&method.sig),
                        fn_name,
                        tags: common.tags,
                        version: common.version,
                        title: common.title,
                        icons: common.icons,
                    });
                    break;
                } else if let Some(slot) = extension_slot(attr, &mut extensions) {
                    if slot.is_some() {
                        return Err(syn::Error::new_spanned(
                            attr,
                            format!(
                                "duplicate #[{}] handler; a server may declare at most one",
                                attr.path()
                                    .get_ident()
                                    .map_or_else(|| "extension".to_string(), ToString::to_string)
                            ),
                        ));
                    }
                    *slot = Some(ExtensionHandler {
                        fn_name: method.sig.ident.clone(),
                        takes_ctx: signature_takes_context(&method.sig),
                    });
                    break;
                }
            }
        }
    }

    Ok(ServerInfo {
        struct_name,
        name,
        version,
        description,
        title,
        instructions,
        website_url,
        icons,
        tools,
        resources,
        prompts,
        extensions,
        page_size: attrs.page_size.as_ref().map(|expr| quote!(#expr)),
    })
}

/// Reject marker combinations that would advertise a method the server cannot
/// serve.
///
/// `#[subscribe]` advertises `resources.subscribe`, and a client that holds a
/// subscription must be able to cancel it. Generating a no-op `unsubscribe`
/// would answer success while the handler's own bookkeeping kept the
/// subscription alive, so the server would go on sending updates the client
/// asked to stop — the same "accepted and then ignored" failure a no-op
/// `logging/setLevel` would be. Only the author knows how to undo what their
/// `#[subscribe]` did, so the pairing is enforced here instead.
fn validate_extensions(extensions: &ExtensionHandlers) -> Result<(), syn::Error> {
    if let (Some(subscribe), None) = (&extensions.subscribe, &extensions.unsubscribe) {
        return Err(syn::Error::new_spanned(
            &subscribe.fn_name,
            "#[subscribe] requires a matching #[unsubscribe] handler\n\n\
             Declaring #[subscribe] advertises `resources.subscribe`, which commits the \
             server to answering `resources/unsubscribe` as well:\n\
             \n\
             #[unsubscribe]\n\
             async fn unwatch(&self, uri: String) -> McpResult<()> { ... }",
        ));
    }
    Ok(())
}

/// Map a marker attribute to its slot in [`ExtensionHandlers`].
///
/// Returns `None` for attributes that are not extension markers, so the caller
/// can keep scanning.
fn extension_slot<'a>(
    attr: &syn::Attribute,
    extensions: &'a mut ExtensionHandlers,
) -> Option<&'a mut Option<ExtensionHandler>> {
    if attr.path().is_ident("completion") {
        Some(&mut extensions.completion)
    } else if attr.path().is_ident("subscribe") {
        Some(&mut extensions.subscribe)
    } else if attr.path().is_ident("unsubscribe") {
        Some(&mut extensions.unsubscribe)
    } else if attr.path().is_ident("set_level") {
        Some(&mut extensions.set_level)
    } else if attr.path().is_ident("roots_changed") {
        Some(&mut extensions.roots_changed)
    } else {
        None
    }
}

/// Whether a marker-attributed method accepts a `&RequestContext`.
///
/// The context parameter is optional on every extension handler, so the
/// generated dispatch has to know whether to pass it.
fn signature_takes_context(sig: &syn::Signature) -> bool {
    sig.inputs.iter().any(|arg| match arg {
        syn::FnArg::Typed(pat_type) => is_request_context_type(&pat_type.ty),
        syn::FnArg::Receiver(_) => false,
    })
}

/// Resource attribute parsed info (HIGH-001).
pub struct ResourceAttrInfo {
    pub uri_template: String,
    pub mime_type: Option<String>,
    /// Keys every handler marker shares (`description`, `tags`, ...).
    pub common: CommonAttrs,
    /// `ResourceAnnotations` hints.
    pub annotations: ResourceAnnotationAttrs,
    /// Size in bytes.
    pub size: Option<u64>,
}

/// Keys only `#[resource]` accepts, on top of [`CommonAttrs`].
const RESOURCE_KEYS: &[&str] = &["mime_type", "audience", "priority", "last_modified", "size"];

/// Extract the resource URI and its `key = value` metadata.
///
/// Supports:
/// - `#[resource("uri://template")]` - URI only
/// - `#[resource("uri://template", mime_type = "text/plain")]` - URI with MIME type
/// - `#[resource("uri://template", description = "...", tags = ["admin"], version = "1.0")]`
/// - `#[resource("uri://x", audience = ["user"], priority = 0.8,
///   last_modified = "2025-01-12T15:00:58Z", size = 1024)]` - `ResourceAnnotations` and size
fn extract_resource_attrs(attr: &syn::Attribute) -> Result<ResourceAttrInfo, syn::Error> {
    let syn::Meta::List(meta_list) = &attr.meta else {
        return Err(syn::Error::new_spanned(
            attr,
            "Expected #[resource(\"uri://template\")] or #[resource(\"uri://template\", mime_type = \"text/plain\")]",
        ));
    };

    // The URI comes first, followed by an optional `, key = value` list.
    let split = |input: syn::parse::ParseStream<'_>| {
        let uri: syn::LitStr = input.parse().map_err(|e| {
            syn::Error::new(
                e.span(),
                "Expected #[resource(\"uri://template\", ...)] - the first argument must be the URI string",
            )
        })?;
        if !input.is_empty() {
            input.parse::<syn::Token![,]>()?;
        }
        Ok((uri, input.parse::<TokenStream>()?))
    };
    let (uri, rest) = syn::parse::Parser::parse2(split, meta_list.tokens.clone())?;
    let uri_template = uri.value();

    let mut mime_type = None;
    let mut annotations = ResourceAnnotationAttrs::default();
    let mut size = None;
    let common = parse_marker_attrs(rest, "resource", RESOURCE_KEYS, |meta| {
        if meta.path.is_ident("mime_type") {
            mime_type = Some(parse_lit_str(meta)?);
        } else if meta.path.is_ident("audience") {
            let value = meta.value()?;
            let items;
            syn::bracketed!(items in value);
            for role in
                syn::punctuated::Punctuated::<syn::LitStr, syn::Token![,]>::parse_terminated(
                    &items,
                )?
            {
                let variant = match role.value().as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    other => {
                        return Err(syn::Error::new_spanned(
                            &role,
                            format!(
                                "unknown audience `{other}`; expected \"user\" or \"assistant\""
                            ),
                        ));
                    }
                };
                annotations.audience.push(Ident::new(variant, role.span()));
            }
        } else if meta.path.is_ident("priority") {
            let lit: syn::Lit = meta.value()?.parse()?;
            let priority = match &lit {
                syn::Lit::Float(f) => f.base10_parse::<f64>()?,
                syn::Lit::Int(i) => i.base10_parse::<f64>()?,
                _ => return Err(syn::Error::new_spanned(&lit, "priority must be a number")),
            };
            // The schema bounds it: 1 is "most important", 0 "least".
            if !(0.0..=1.0).contains(&priority) {
                return Err(syn::Error::new_spanned(
                    &lit,
                    "priority must be between 0 and 1 inclusive",
                ));
            }
            annotations.priority = Some(priority);
        } else if meta.path.is_ident("last_modified") {
            annotations.last_modified = Some(parse_lit_str(meta)?);
        } else if meta.path.is_ident("size") {
            let lit: syn::LitInt = meta.value()?.parse()?;
            // `ResourceTemplate` has no `size`: a template stands for many
            // resources, which have no one size between them.
            if uri_template.contains('{') {
                return Err(syn::Error::new_spanned(
                    &lit,
                    "`size` applies only to a concrete resource; a URI template has no single size",
                ));
            }
            size = Some(lit.base10_parse::<u64>()?);
        } else {
            return Ok(false);
        }
        Ok(true)
    })?;

    Ok(ResourceAttrInfo {
        uri_template,
        mime_type,
        common,
        annotations,
        size,
    })
}

/// Generate `Resource.annotations` / `ResourceTemplate.annotations` as
/// `Option<ResourceAnnotations>`.
fn generate_resource_annotations_code(
    annotations: &ResourceAnnotationAttrs,
    krate: &TokenStream,
) -> TokenStream {
    if annotations.is_empty() {
        return quote! { None };
    }
    let types = quote! { #krate::__macro_support::turbomcp_types };
    let audience = if annotations.audience.is_empty() {
        quote! { None }
    } else {
        let roles = &annotations.audience;
        quote! { Some(vec![#(#types::Role::#roles),*]) }
    };
    let priority = match annotations.priority {
        Some(p) => quote! { Some(#p) },
        None => quote! { None },
    };
    let last_modified = match &annotations.last_modified {
        Some(t) => quote! { Some(#t.to_string()) },
        None => quote! { None },
    };
    quote! {
        Some(#types::ResourceAnnotations {
            audience: #audience,
            priority: #priority,
            last_modified: #last_modified,
        })
    }
}

/// Does this handler hand back an `McpError` the dispatcher can inspect?
///
/// Matches `-> McpResult<T>` and `-> Result<T, McpError>` (through any path
/// prefix, so `turbomcp::McpResult<T>` and `core::result::Result<T,
/// turbomcp_core::error::McpError>` both count). Anything else — a bare value,
/// or a `Result` over some other error type — is left on the legacy conversion
/// path, where the error can only become display text.
/// Whether a handler's return type is `Result`-shaped at all.
///
/// Deliberately an exact-ident check on the last path segment, so `Result` and
/// `McpResult` match while `-> PromptResult` — which is a success type, not a
/// fallible one — does not.
///
/// [`returns_mcp_error`] is the narrower question: whether the error type is
/// already `McpError` and so needs no conversion.
fn returns_result(sig: &syn::Signature) -> bool {
    let syn::ReturnType::Type(_, ty) = &sig.output else {
        return false;
    };
    let syn::Type::Path(type_path) = ty.as_ref() else {
        return false;
    };
    type_path
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Result" || segment.ident == "McpResult")
}

fn returns_mcp_error(sig: &syn::Signature) -> bool {
    let syn::ReturnType::Type(_, ty) = &sig.output else {
        return false;
    };
    let syn::Type::Path(type_path) = ty.as_ref() else {
        return false;
    };
    let Some(segment) = type_path.path.segments.last() else {
        return false;
    };

    if segment.ident == "McpResult" {
        return true;
    }
    if segment.ident != "Result" {
        return false;
    }

    // `Result<T, E>` — the error type must be McpError.
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    let Some(syn::GenericArgument::Type(syn::Type::Path(err))) = args.args.iter().nth(1) else {
        return false;
    };
    err.path
        .segments
        .last()
        .is_some_and(|seg| seg.ident == "McpError")
}

/// Check if a type is a reference to RequestContext.
///
/// Matches `RequestContext`, `Context`, and any path ending in `::RequestContext`
/// or `::Context` (including reference forms). Uses last-segment ident comparison
/// to avoid false positives on user types whose names contain `RequestContext`
/// as a substring (e.g. `MyRequestContextWrapper`).
fn is_request_context_type(ty: &syn::Type) -> bool {
    // Handle &RequestContext / &Context
    if let syn::Type::Reference(type_ref) = ty {
        return is_request_context_type(&type_ref.elem);
    }

    if let syn::Type::Path(type_path) = ty {
        return type_path
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "RequestContext" || seg.ident == "Context");
    }

    false
}

/// Extract prompt attributes from `#[prompt(...)]`.
///
/// `#[prompt]` has no keys of its own: it takes the shared set, or the
/// `#[prompt("description")]` shorthand.
fn extract_prompt_attrs(attr: &syn::Attribute) -> Result<CommonAttrs, syn::Error> {
    // Handle empty #[prompt]
    let syn::Meta::List(meta_list) = &attr.meta else {
        return Ok(CommonAttrs::default());
    };

    // Handle #[prompt("description")] shorthand
    if let Ok(lit) = syn::parse2::<syn::LitStr>(meta_list.tokens.clone()) {
        return Ok(CommonAttrs {
            description: Some(lit.value()),
            ..CommonAttrs::default()
        });
    }

    parse_marker_attrs(meta_list.tokens.clone(), "prompt", &[], |_| Ok(false))
}

/// Extract prompt arguments from function signature (HIGH-002).
fn extract_prompt_arguments(sig: &syn::Signature) -> Vec<PromptArgumentInfo> {
    let mut args = Vec::new();

    for input in &sig.inputs {
        if let syn::FnArg::Typed(pat_type) = input
            && let syn::Pat::Ident(pat_ident) = &*pat_type.pat
        {
            let name = pat_ident.ident.unraw().to_string();

            // Skip self parameter
            if name == "self" {
                continue;
            }

            // Skip RequestContext parameters (regardless of name: ctx, _ctx, context, etc.)
            if is_request_context_type(&pat_type.ty) {
                continue;
            }

            // Check if type is Option<T> to determine if required. The last
            // segment, as `#[tool]` checks it: the first segment of
            // `std::option::Option<String>` is `std`, which listed the argument
            // as required and bound it as a `String` the handler cannot take.
            let is_option = if let syn::Type::Path(type_path) = &*pat_type.ty {
                type_path
                    .path
                    .segments
                    .last()
                    .is_some_and(|s| s.ident == "Option")
            } else {
                false
            };

            // Pull description from `#[description("...")]` attribute on the
            // parameter, mirroring how #[tool] surfaces param docs to clients.
            // Pre-3.1 prompts always emitted `description: None`, leaving LLM
            // clients without per-argument docs.
            let description = extract_param_description(&pat_type.attrs);
            // SEP-973 `title`: a display label for the argument. Without it a
            // client building the slash-command form the spec illustrates has
            // nothing but the raw Rust identifier to label the field, so the
            // user sees `repo_url` rather than "Repository URL".
            let title = extract_param_str_attr(&pat_type.attrs, "title");

            args.push(PromptArgumentInfo {
                name,
                ident: pat_ident.ident.clone(),
                title,
                description,
                required: !is_option,
            });
        }
    }

    args
}

/// Extract the string from a `#[key("...")]` attribute on a function parameter.
fn extract_param_str_attr(attrs: &[syn::Attribute], key: &str) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident(key)
            && let Ok(s) = attr.parse_args::<syn::LitStr>()
        {
            return Some(s.value());
        }
    }
    None
}

/// Extract the string from `#[description("...")]` on a function parameter.
fn extract_param_description(attrs: &[syn::Attribute]) -> Option<String> {
    extract_param_str_attr(attrs, "description")
}

/// Extract doc comments from attributes.
fn extract_doc_comments(attrs: &[syn::Attribute]) -> Option<String> {
    let doc_lines: Vec<String> = attrs
        .iter()
        .filter_map(|attr| {
            if attr.path().is_ident("doc")
                && let syn::Meta::NameValue(meta) = &attr.meta
                && let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit_str),
                    ..
                }) = &meta.value
            {
                return Some(lit_str.value().trim().to_string());
            }
            None
        })
        .collect();

    if doc_lines.is_empty() {
        None
    } else {
        Some(doc_lines.join(" "))
    }
}

/// Strip the handler marker attributes from impl items.
///
/// Every marker `#[server]` understands must be listed here: the markers are
/// also declared as proc-macro attributes purely so that using one outside a
/// `#[server]` block produces a helpful error, and any that survive into the
/// emitted impl would expand to that error.
fn strip_handler_attributes(impl_block: &ItemImpl) -> ItemImpl {
    let mut stripped = impl_block.clone();
    for item in &mut stripped.items {
        if let syn::ImplItem::Fn(method) = item {
            method.attrs.retain(|attr| {
                !attr.path().is_ident("tool")
                    && !attr.path().is_ident("resource")
                    && !attr.path().is_ident("prompt")
                    && !attr.path().is_ident("completion")
                    && !attr.path().is_ident("subscribe")
                    && !attr.path().is_ident("unsubscribe")
                    && !attr.path().is_ident("set_level")
                    && !attr.path().is_ident("roots_changed")
            });
            // Strip #[description] / #[title] from parameter attributes — the macro
            // has already extracted their values for schema generation, so they must
            // not survive into the compiler output where they'd trigger
            // compile_error!().
            for input in &mut method.sig.inputs {
                if let syn::FnArg::Typed(pat_type) = input {
                    pat_type.attrs.retain(|attr| {
                        !attr.path().is_ident("description") && !attr.path().is_ident("title")
                    });
                }
            }
        }
    }
    stripped
}

/// Generate the `McpHandler` overrides for the marker-attributed methods.
///
/// Only the markers actually present produce an override; the rest keep the
/// trait default, which answers `capability_not_supported`. That pairing is
/// what keeps [`generate_capabilities`] honest.
fn generate_extension_handlers(
    extensions: &ExtensionHandlers,
    turbomcp: &TokenStream,
) -> TokenStream {
    let core = quote! { #turbomcp::__macro_support::turbomcp_core };
    let json = quote! { #turbomcp::__macro_support::serde_json };

    // `resources/subscribe` and `resources/unsubscribe` share a shape: take a
    // URI string, return unit.
    let uri_handler = |handler: &ExtensionHandler, trait_fn: Ident| {
        let fn_name = &handler.fn_name;
        let call = if handler.takes_ctx {
            quote! { self.#fn_name(uri.to_string(), ctx).await }
        } else {
            quote! { self.#fn_name(uri.to_string()).await }
        };
        quote! {
            fn #trait_fn<'a>(
                &'a self,
                uri: &'a str,
                ctx: &'a #core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = #core::error::McpResult<()>>
                + #core::marker::MaybeSend + 'a {
                async move {
                    let _ = ctx;
                    #call
                }
            }
        }
    };

    let subscribe = extensions
        .subscribe
        .as_ref()
        .map(|h| uri_handler(h, syn::parse_quote!(subscribe)));
    let unsubscribe = extensions
        .unsubscribe
        .as_ref()
        .map(|h| uri_handler(h, syn::parse_quote!(unsubscribe)));

    let set_level = extensions.set_level.as_ref().map(|handler| {
        let fn_name = &handler.fn_name;
        let call = if handler.takes_ctx {
            quote! { self.#fn_name(level.to_string(), ctx).await }
        } else {
            quote! { self.#fn_name(level.to_string()).await }
        };
        quote! {
            fn set_log_level<'a>(
                &'a self,
                level: &'a str,
                ctx: &'a #core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = #core::error::McpResult<()>>
                + #core::marker::MaybeSend + 'a {
                async move {
                    let _ = ctx;
                    #call
                }
            }
        }
    });

    let completion = extensions.completion.as_ref().map(|handler| {
        let fn_name = &handler.fn_name;
        let call = if handler.takes_ctx {
            quote! { self.#fn_name(params, ctx).await }
        } else {
            quote! { self.#fn_name(params).await }
        };
        quote! {
            fn complete<'a>(
                &'a self,
                params: #json::Value,
                ctx: &'a #core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = #core::error::McpResult<#json::Value>>
                + #core::marker::MaybeSend + 'a {
                async move {
                    let _ = ctx;
                    #call
                }
            }
        }
    });

    // Takes no value parameter — the notification carries none.
    let roots_changed = extensions.roots_changed.as_ref().map(|handler| {
        let fn_name = &handler.fn_name;
        let call = if handler.takes_ctx {
            quote! { self.#fn_name(ctx).await }
        } else {
            quote! { self.#fn_name().await }
        };
        quote! {
            fn on_roots_list_changed<'a>(
                &'a self,
                ctx: &'a #core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = #core::error::McpResult<()>>
                + #core::marker::MaybeSend + 'a {
                async move {
                    let _ = ctx;
                    #call
                }
            }
        }
    });

    quote! {
        #subscribe
        #unsubscribe
        #set_level
        #completion
        #roots_changed
    }
}

/// Generate `server_capabilities`, inferred from what the server can serve.
///
/// Tools, resources, and prompts are advertised when the impl block declares
/// any; `completions` and `resources.subscribe` are advertised only when the
/// corresponding marker attribute supplied a handler. A server therefore never
/// claims a capability whose method would answer `capability_not_supported`.
///
/// `logging` is always advertised. Every server can emit
/// `notifications/message` through `ctx.log()`, a server that emits log
/// notifications must declare `logging`, and the trait default for
/// `logging/setLevel` accepts the level (the router records it), so the method
/// is served whether or not `#[set_level]` is present.
fn generate_capabilities(info: &ServerInfo, turbomcp: &TokenStream) -> TokenStream {
    let types = quote! { #turbomcp::__macro_support::turbomcp_types };

    let has_tools = !info.tools.is_empty();
    let has_resources = !info.resources.is_empty();
    let has_prompts = !info.prompts.is_empty();
    let subscribe = info.extensions.subscribe.is_some();

    let tools_code = has_tools.then(|| {
        quote! {
            capabilities.tools = Some(#types::ToolsCapabilities {
                list_changed: Some(true),
            });
        }
    });

    // `subscribe` alone is enough to advertise the resources capability: a
    // server can expose only templated or dynamic resources.
    let resources_code = (has_resources || subscribe).then(|| {
        let subscribe_value = if subscribe {
            quote! { Some(true) }
        } else {
            quote! { None }
        };
        quote! {
            capabilities.resources = Some(#types::ResourcesCapabilities {
                subscribe: #subscribe_value,
                list_changed: Some(true),
            });
        }
    });

    let prompts_code = has_prompts.then(|| {
        quote! {
            capabilities.prompts = Some(#types::PromptsCapabilities {
                list_changed: Some(true),
            });
        }
    });

    let completions_code = info.extensions.completion.is_some().then(|| {
        quote! {
            capabilities.completions = Some(#types::CompletionCapabilities::default());
        }
    });

    quote! {
        fn server_capabilities(&self) -> #types::ServerCapabilities {
            let mut capabilities = #types::ServerCapabilities::default();
            #tools_code
            #resources_code
            #prompts_code
            #completions_code
            capabilities.logging = Some(#types::LoggingCapabilities::default());
            capabilities
        }
    }
}

/// Generate code for the meta field (tags and version).
fn generate_meta_code(
    tags: &[String],
    version: &Option<String>,
    krate: &TokenStream,
) -> TokenStream {
    if tags.is_empty() && version.is_none() {
        return quote! { None };
    }

    let tags_code = if tags.is_empty() {
        quote! {}
    } else {
        let tag_strings = tags.iter().map(|t| quote! { #t.to_string() });
        quote! {
            meta.insert(
                "tags".to_string(),
                #krate::__macro_support::serde_json::Value::Array(
                    vec![#(#krate::__macro_support::serde_json::Value::String(#tag_strings)),*]
                )
            );
        }
    };

    let version_code = if let Some(ver) = version {
        quote! {
            meta.insert(
                "version".to_string(),
                #krate::__macro_support::serde_json::Value::String(#ver.to_string())
            );
        }
    } else {
        quote! {}
    };

    quote! {
        {
            let mut meta = ::std::collections::HashMap::new();
            #tags_code
            #version_code
            Some(meta)
        }
    }
}

/// Generate McpHandler implementation.
pub fn generate_mcp_handler(info: &ServerInfo, impl_block: &ItemImpl) -> TokenStream {
    let struct_name = &info.struct_name;
    // Strip handler attributes to prevent them from being processed by their macros
    let stripped_impl_block = strip_handler_attributes(impl_block);
    let name = &info.name;
    let version = &info.version;
    let turbomcp = turbomcp_crate();

    let description_code = match &info.description {
        Some(desc) => quote! { .with_description(#desc) },
        None => quote! {},
    };
    let title_code = match &info.title {
        Some(title) => quote! { .with_title(#title) },
        None => quote! {},
    };
    let website_url_code = match &info.website_url {
        Some(url) => quote! { .with_website_url(#url) },
        None => quote! {},
    };
    let icons_code = info.icons.iter().map(|src| {
        quote! {
            .with_icon(#turbomcp::__macro_support::turbomcp_types::Icon::new(#src))
        }
    });

    // `instructions` is a separate `initialize` field, not part of serverInfo:
    // omit the override entirely when unset so the trait default keeps it off
    // the wire.
    let instructions_code = info.instructions.as_ref().map(|instructions| {
        quote! {
            fn instructions(&self) -> ::std::option::Option<::std::string::String> {
                ::std::option::Option::Some(::std::string::ToString::to_string(&#instructions))
            }
        }
    });

    // Generate tool listing code
    // Uses #turbomcp::__macro_support:: paths so users don't need internal crates
    let tool_list_code = info.tools.iter().map(|tool| {
        let tool_name = &tool.name;
        let schema_code = generate_schema_code(&tool.parameters, &turbomcp);

        // Generate meta field if tags or version present
        let meta_code = generate_meta_code(&tool.tags, &tool.version, &turbomcp);

        // Per MCP spec, omit `description` entirely (i.e. `None`) when no
        // description is available rather than emitting an empty string,
        // which clients otherwise display as "" in tool pickers.
        let description_code = if tool.description.is_empty() {
            quote! { None }
        } else {
            let desc = &tool.description;
            quote! { Some(#desc.to_string()) }
        };

        // SEP-973 / 2025-11-25 surface fields from #[tool(...)] attributes.
        let title_code = match &tool.title {
            Some(t) => quote! { Some(#t.to_string()) },
            None => quote! { None },
        };
        let icons_code = generate_icons_code(&tool.icons, &turbomcp);
        let annotations_code = generate_annotations_code(&tool.annotations, &tool.title, &turbomcp);
        let output_schema_code = generate_output_schema_code(&tool.output_schema, &turbomcp);
        let execution_code = generate_execution_code(&tool.task_support, &turbomcp);

        quote! {
            #turbomcp::__macro_support::turbomcp_types::Tool {
                name: #tool_name.to_string(),
                description: #description_code,
                input_schema: #schema_code,
                title: #title_code,
                icons: #icons_code,
                annotations: #annotations_code,
                execution: #execution_code,
                output_schema: #output_schema_code,
                meta: #meta_code,
            }
        }
    });

    // Generate resource listing code (HIGH-001: includes mimeType)
    let resource_list_code = info
        .resources
        .iter()
        .filter(|resource| !resource.uri_template.contains('{'))
        .map(|resource| {
            let uri = &resource.uri_template;
            let name = &resource.name;
            let meta_code = generate_meta_code(&resource.tags, &resource.version, &turbomcp);
            let mime_type_code = if let Some(mime) = &resource.mime_type {
                quote! { Some(#mime.to_string()) }
            } else {
                quote! { None }
            };
            // Per MCP spec, omit description rather than emit an empty string.
            let description_code = match resource.description.as_deref() {
                Some(desc) if !desc.is_empty() => quote! { Some(#desc.to_string()) },
                _ => quote! { None },
            };
            let title_code = match &resource.title {
                Some(t) => quote! { Some(#t.to_string()) },
                None => quote! { None },
            };
            let icons_code = generate_icons_code(&resource.icons, &turbomcp);
            let annotations_code =
                generate_resource_annotations_code(&resource.annotations, &turbomcp);
            let size_code = match resource.size {
                Some(size) => quote! { Some(#size) },
                None => quote! { None },
            };
            quote! {
                #turbomcp::__macro_support::turbomcp_types::Resource {
                    uri: #uri.to_string(),
                    name: #name.to_string(),
                    description: #description_code,
                    title: #title_code,
                    icons: #icons_code,
                    mime_type: #mime_type_code,
                    annotations: #annotations_code,
                    size: #size_code,
                    meta: #meta_code,
                }
            }
        });

    let resource_template_list_code = info
        .resources
        .iter()
        .filter(|resource| resource.uri_template.contains('{'))
        .map(|resource| {
            let uri_template = &resource.uri_template;
            let name = &resource.name;
            let meta_code = generate_meta_code(&resource.tags, &resource.version, &turbomcp);
            let mime_type_code = if let Some(mime) = &resource.mime_type {
                quote! { Some(#mime.to_string()) }
            } else {
                quote! { None }
            };
            let description_code = match resource.description.as_deref() {
                Some(desc) if !desc.is_empty() => quote! { Some(#desc.to_string()) },
                _ => quote! { None },
            };
            let title_code = match &resource.title {
                Some(t) => quote! { Some(#t.to_string()) },
                None => quote! { None },
            };
            let icons_code = generate_icons_code(&resource.icons, &turbomcp);
            let annotations_code =
                generate_resource_annotations_code(&resource.annotations, &turbomcp);
            quote! {
                #turbomcp::__macro_support::turbomcp_types::ResourceTemplate {
                    uri_template: #uri_template.to_string(),
                    name: #name.to_string(),
                    description: #description_code,
                    title: #title_code,
                    icons: #icons_code,
                    mime_type: #mime_type_code,
                    annotations: #annotations_code,
                    meta: #meta_code,
                }
            }
        });

    // Generate prompt listing code (HIGH-002: includes arguments)
    let prompt_list_code = info.prompts.iter().map(|prompt| {
        let name = &prompt.name;
        let meta_code = generate_meta_code(&prompt.tags, &prompt.version, &turbomcp);

        // Per MCP spec, omit description rather than emit an empty string.
        let description_code = match prompt.description.as_deref() {
            Some(desc) if !desc.is_empty() => quote! { Some(#desc.to_string()) },
            _ => quote! { None },
        };

        // Generate arguments
        let args_code = if prompt.arguments.is_empty() {
            quote! { None }
        } else {
            let arg_structs = prompt.arguments.iter().map(|arg| {
                let arg_name = &arg.name;
                let required = arg.required;
                let arg_desc_code = match arg.description.as_deref() {
                    Some(d) if !d.is_empty() => quote! { Some(#d.to_string()) },
                    _ => quote! { None },
                };
                let arg_title_code = match arg.title.as_deref() {
                    Some(t) if !t.is_empty() => quote! { Some(#t.to_string()) },
                    _ => quote! { None },
                };
                quote! {
                    #turbomcp::__macro_support::turbomcp_types::PromptArgument {
                        name: #arg_name.to_string(),
                        title: #arg_title_code,
                        description: #arg_desc_code,
                        required: Some(#required),
                    }
                }
            });
            quote! { Some(vec![#(#arg_structs),*]) }
        };

        let title_code = match &prompt.title {
            Some(t) => quote! { Some(#t.to_string()) },
            None => quote! { None },
        };
        let icons_code = generate_icons_code(&prompt.icons, &turbomcp);

        quote! {
            #turbomcp::__macro_support::turbomcp_types::Prompt {
                name: #name.to_string(),
                description: #description_code,
                title: #title_code,
                icons: #icons_code,
                arguments: #args_code,
                meta: #meta_code,
            }
        }
    });

    // Generate tool dispatch code.
    //
    // SEP-1303 (MCP 2025-11-25): "Clarify that input validation errors should
    // be returned as Tool Execution Errors rather than Protocol Errors to
    // enable model self-correction." We wrap argument extraction in an async
    // block so a `McpError::invalid_params(...)` from the parser surfaces as
    // `CallToolResult { isError: true, content: [text(...)] }` rather than a
    // JSON-RPC -32602. Other error kinds (internal, transport, etc.) continue
    // to bubble up as protocol errors.
    let tool_dispatch_code = info.tools.iter().map(|tool| {
        let tool_name = &tool.name;
        // The identifier as written, not rebuilt from the wire name: `r#type`
        // is the tool `type`, but only `r#type` names the method.
        let fn_name = &tool.sig.ident;
        let extraction = generate_extraction_code(&tool.parameters, &turbomcp);
        let call_args = generate_call_args(&tool.sig);

        // A handler that returns `McpResult<T>` gets its error *kind* carried
        // into the tool result's `_meta`; the spec's `isError` convention alone
        // would flatten every failure to a message, leaving a client unable to
        // tell bad input from an internal fault. Other return types keep the
        // blanket `Display` conversion, which has no kind to preserve.
        //
        // The one exception is -32042. The spec defines URL elicitation
        // required as a JSON-RPC error whose `data.elicitations` the client
        // acts on; folded into an `isError` result, the client never sees it.
        let ok_conversion = if returns_mcp_error(&tool.sig) {
            quote! {
                match result {
                    Ok(value) => Ok(#turbomcp::__macro_support::turbomcp_types::IntoToolResult::into_tool_result(value)),
                    Err(e) if e.kind == #turbomcp::__macro_support::turbomcp_core::error::ErrorKind::UrlElicitationRequired => Err(e),
                    Err(e) => Ok(e.to_tool_result()),
                }
            }
        } else {
            quote! {
                Ok(#turbomcp::__macro_support::turbomcp_types::IntoToolResult::into_tool_result(result))
            }
        };

        quote! {
            #tool_name => {
                // `Box::pin` keeps the handler body off this function's future.
                // Every arm is inlined into one `call_tool` state machine, so
                // without it the machine is as large as the fattest tool body
                // and every caller pays that size on every call.
                let outcome: ::std::result::Result<_, #turbomcp::__macro_support::turbomcp_core::error::McpError> = ::std::boxed::Box::pin(async {
                    #extraction
                    Ok(self.#fn_name(#call_args).await)
                }).await;

                match outcome {
                    Ok(result) => #ok_conversion,
                    Err(e) if e.kind == #turbomcp::__macro_support::turbomcp_core::error::ErrorKind::InvalidParams => {
                        // SEP-1303: validation failure → tool execution error.
                        Ok(e.to_tool_result())
                    }
                    Err(e) => Err(e),
                }
            }
        }
    });

    // Generate resource dispatch code with proper URI template matching.
    //
    // Concrete URIs are tried before templates regardless of declaration order.
    // A concrete resource appears in `resources/list` as something a client can
    // read by name, so a template declared above it must not be allowed to
    // swallow that URI — the ordering of two `#[resource]` attributes in a file
    // is not something an author should have to reason about.
    let (concrete, templated): (Vec<_>, Vec<_>) = info
        .resources
        .iter()
        .partition(|resource| !resource.uri_template.contains('{'));
    let resource_dispatch_code = concrete.into_iter().chain(templated).map(|resource| {
        let uri_template = &resource.uri_template;
        let fn_name = &resource.fn_name;

        // A declared `mime_type` is advertised in `resources/list`, so the read
        // has to agree with it. The `IntoResourceResult` conversions can only
        // guess from the body (`text/plain`, `application/octet-stream`), which
        // left the catalogue and the content describing the same resource
        // differently.
        //
        // That holds for a single entry, which *is* the listed resource. A read
        // returning several entries — a directory's children, or renditions of
        // one document — carries a type per entry that the handler chose, and
        // overwriting them all with the one declared type mislabelled every
        // entry but the matching ones. There the declared type only fills in
        // entries that set none.
        let apply_mime = match &resource.mime_type {
            Some(mime) => quote! {
                let mut read = read;
                if read.contents.len() == 1 {
                    read = read.with_mime_type(#mime);
                } else {
                    for entry in &mut read.contents {
                        let slot = match entry {
                            #turbomcp::__macro_support::turbomcp_types::ResourceContents::Text(text) => &mut text.mime_type,
                            #turbomcp::__macro_support::turbomcp_types::ResourceContents::Blob(blob) => &mut blob.mime_type,
                        };
                        slot.get_or_insert_with(|| #mime.to_string());
                    }
                }
            },
            None => quote! {},
        };

        // Each body is boxed for the same reason as the tool arms: they all
        // share one `read_resource` state machine.
        let dispatch = quote! {
            let __result: #turbomcp::__macro_support::turbomcp_core::error::McpResult<
                #turbomcp::__macro_support::turbomcp_types::ResourceResult
            > = ::std::boxed::Box::pin(async {
                match self.#fn_name(uri.to_string(), ctx).await {
                    Ok(r) => {
                        let read = #turbomcp::__macro_support::turbomcp_types::IntoResourceResult::into_resource_result(r, &uri);
                        #apply_mime
                        Ok(read)
                    }
                    Err(e) => Err(e),
                }
            }).await;
            return __result;
        };

        if uri_template.contains('{') {
            // Matched against the template's actual RFC 6570 structure. The old
            // matcher took the text before the first `{` as a prefix and after
            // the last `}` as a suffix and ignored everything between, so
            // `db://{table}/rows/{id}.json` also claimed
            // `db://totally/unrelated/path.json`, `db://x.json` and even
            // `db://.json` — and two templates sharing a scheme and extension
            // were indistinguishable, so whichever was declared first took
            // both. The handler still receives the full URI; this only decides
            // which handler gets it.
            quote! {
                if #turbomcp::__macro_support::turbomcp_core::uri_template::matches(#uri_template, &uri) {
                    #dispatch
                }
            }
        } else {
            // Exact match for templates without variables
            quote! {
                if uri == #uri_template {
                    #dispatch
                }
            }
        }
    });

    // Generate prompt dispatch code (HIGH-002: passes arguments to handler)
    // Uses IntoPromptResult to convert the return value, supporting:
    // - String, &str -> PromptResult::user(...)
    // - PromptResult -> passed through
    // - Result<T, E> -> Ok unwrapped, Err converted to message
    let args = args_ident();
    let ctx = ctx_ident();
    let prompt_dispatch_code = info.prompts.iter().map(|prompt| {
        let prompt_name = &prompt.name;
        let fn_name = &prompt.fn_name;

        // Generate argument extraction code
        let arg_extractions = prompt.arguments.iter().map(|arg| {
            let arg_name = &arg.name;
            let arg_ident = &arg.ident;

            if arg.required {
                quote! {
                    let #arg_ident: String = #args
                        .as_ref()
                        .and_then(|a| a.get(#arg_name))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .ok_or_else(|| #turbomcp::__macro_support::turbomcp_core::error::McpError::invalid_params(
                            format!("Missing required argument: {}", #arg_name)
                        ))?;
                }
            } else {
                quote! {
                    let #arg_ident: Option<String> = #args
                        .as_ref()
                        .and_then(|a| a.get(#arg_name))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
            }
        });

        // Generate call arguments (excluding ctx which is passed separately)
        let call_args = prompt.arguments.iter().map(|arg| &arg.ident);

        // A prompt returning `McpResult<T>` propagates its error as a JSON-RPC
        // error. The blanket `IntoPromptResult` conversion renders `Err` as a
        // *user message* reading "Error: …", which makes a failed render
        // indistinguishable from a successful one whose text happens to start
        // that way — the model is then asked to act on the failure.
        let conversion = if prompt.returns_mcp_error {
            quote! {
                match result {
                    Ok(value) => Ok(#turbomcp::__macro_support::turbomcp_types::IntoPromptResult::into_prompt_result(value)),
                    Err(e) => Err(e),
                }
            }
        } else if prompt.returns_result {
            // Fallible, but with some other error type. It still propagates —
            // the failure is converted rather than rendered, so a failed prompt
            // never arrives as a user message the model is asked to act on.
            quote! {
                match result {
                    Ok(value) => Ok(#turbomcp::__macro_support::turbomcp_types::IntoPromptResult::into_prompt_result(value)),
                    Err(e) => Err(
                        #turbomcp::__macro_support::turbomcp_core::error::McpError::internal(
                            ::std::string::ToString::to_string(&e)
                        )
                    ),
                }
            }
        } else {
            quote! {
                Ok(#turbomcp::__macro_support::turbomcp_types::IntoPromptResult::into_prompt_result(result))
            }
        };

        let call = if prompt.arguments.is_empty() {
            quote! { let result = self.#fn_name(#ctx).await; }
        } else {
            quote! {
                #(#arg_extractions)*
                let result = self.#fn_name(#(#call_args,)* #ctx).await;
            }
        };

        quote! {
            #prompt_name => {
                // Boxed for the same reason as the tool arms.
                let __result: #turbomcp::__macro_support::turbomcp_core::error::McpResult<
                    #turbomcp::__macro_support::turbomcp_types::PromptResult
                > = ::std::boxed::Box::pin(async {
                    #call
                    #conversion
                }).await;
                __result
            }
        }
    });

    // Pagination is opt-in: without this the trait default returns `None` and
    // the server hands back its whole catalogue, which is conformant and is the
    // safe default for clients that do not follow cursors.
    let page_size_code = info.page_size.as_ref().map(|size| {
        quote! {
            fn page_size(&self) -> ::std::option::Option<usize> {
                ::std::option::Option::Some(#size)
            }
        }
    });

    let extension_code = generate_extension_handlers(&info.extensions, &turbomcp);
    let capabilities_code = generate_capabilities(info, &turbomcp);

    quote! {
        // Keep the original impl block with handler attributes stripped
        #stripped_impl_block

        // Generate McpHandler implementation (unified v3 architecture)
        // Uses #turbomcp::__macro_support:: paths so users don't need internal crates
        impl #turbomcp::__macro_support::turbomcp_core::handler::McpHandler for #struct_name {
            fn server_info(&self) -> #turbomcp::__macro_support::turbomcp_types::ServerInfo {
                #turbomcp::__macro_support::turbomcp_types::ServerInfo::new(#name, #version)
                    #description_code
                    #title_code
                    #website_url_code
                    #(#icons_code)*
            }

            #instructions_code

            #capabilities_code

            #page_size_code

            #extension_code

            fn list_tools(&self) -> Vec<#turbomcp::__macro_support::turbomcp_types::Tool> {
                vec![#(#tool_list_code),*]
            }

            fn list_resources(&self) -> Vec<#turbomcp::__macro_support::turbomcp_types::Resource> {
                vec![#(#resource_list_code),*]
            }

            fn list_resource_templates(&self) -> Vec<#turbomcp::__macro_support::turbomcp_types::ResourceTemplate> {
                vec![#(#resource_template_list_code),*]
            }

            fn list_prompts(&self) -> Vec<#turbomcp::__macro_support::turbomcp_types::Prompt> {
                vec![#(#prompt_list_code),*]
            }

            fn call_tool<'a>(
                &'a self,
                name: &'a str,
                #args: #turbomcp::__macro_support::serde_json::Value,
                #ctx: &'a #turbomcp::__macro_support::turbomcp_core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = #turbomcp::__macro_support::turbomcp_core::error::McpResult<#turbomcp::__macro_support::turbomcp_types::ToolResult>> + #turbomcp::__macro_support::turbomcp_core::marker::MaybeSend + 'a {
                let name = name.to_string();
                async move {
                    let #args = #args.as_object().cloned().unwrap_or_default();
                    match name.as_str() {
                        #(#tool_dispatch_code)*
                        _ => Err(#turbomcp::__macro_support::turbomcp_core::error::McpError::tool_not_found(&name))
                    }
                }
            }

            fn read_resource<'a>(
                &'a self,
                uri: &'a str,
                ctx: &'a #turbomcp::__macro_support::turbomcp_core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = #turbomcp::__macro_support::turbomcp_core::error::McpResult<#turbomcp::__macro_support::turbomcp_types::ResourceResult>> + #turbomcp::__macro_support::turbomcp_core::marker::MaybeSend + 'a {
                let uri = uri.to_string();
                async move {
                    // Security: Validate URI length to prevent DoS
                    if uri.len() > #turbomcp::__macro_support::turbomcp_core::DEFAULT_MAX_URI_LENGTH {
                        return Err(#turbomcp::__macro_support::turbomcp_core::error::McpError::invalid_params(
                            format!("URI too long: {} bytes (max: {})", uri.len(), #turbomcp::__macro_support::turbomcp_core::DEFAULT_MAX_URI_LENGTH)
                        ));
                    }

                    // Security: reject only schemes on the dangerous denylist
                    // (javascript:, vbscript:). Per MCP spec, custom schemes are allowed.
                    if let Err(e) = #turbomcp::__macro_support::turbomcp_core::check_uri_scheme_safety(&uri) {
                        return Err(#turbomcp::__macro_support::turbomcp_core::error::McpError::security(
                            format!("URI scheme rejected: {}", e)
                        ));
                    }

                    #(#resource_dispatch_code)*
                    Err(#turbomcp::__macro_support::turbomcp_core::error::McpError::resource_not_found(&uri))
                }
            }

            fn get_prompt<'a>(
                &'a self,
                name: &'a str,
                #args: Option<#turbomcp::__macro_support::serde_json::Value>,
                #ctx: &'a #turbomcp::__macro_support::turbomcp_core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = #turbomcp::__macro_support::turbomcp_core::error::McpResult<#turbomcp::__macro_support::turbomcp_types::PromptResult>> + #turbomcp::__macro_support::turbomcp_core::marker::MaybeSend + 'a {
                let name = name.to_string();
                // HIGH-002: Convert args to Map for argument extraction
                let #args = #args.and_then(|v| v.as_object().cloned());
                async move {
                    match name.as_str() {
                        #(#prompt_dispatch_code)*
                        _ => Err(#turbomcp::__macro_support::turbomcp_core::error::McpError::prompt_not_found(&name))
                    }
                }
            }

            fn list_tasks<'a>(
                &'a self,
                _cursor: Option<&'a str>,
                _limit: Option<usize>,
                _ctx: &'a #turbomcp::__macro_support::turbomcp_core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = #turbomcp::__macro_support::turbomcp_core::error::McpResult<#turbomcp::__macro_support::turbomcp_types::ListTasksResult>> + #turbomcp::__macro_support::turbomcp_core::marker::MaybeSend + 'a {
                async { Err(#turbomcp::__macro_support::turbomcp_core::error::McpError::capability_not_supported("tasks/list")) }
            }

            fn get_task<'a>(
                &'a self,
                _task_id: &'a str,
                _ctx: &'a #turbomcp::__macro_support::turbomcp_core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = #turbomcp::__macro_support::turbomcp_core::error::McpResult<#turbomcp::__macro_support::turbomcp_types::Task>> + #turbomcp::__macro_support::turbomcp_core::marker::MaybeSend + 'a {
                async { Err(#turbomcp::__macro_support::turbomcp_core::error::McpError::capability_not_supported("tasks/get")) }
            }

            fn cancel_task<'a>(
                &'a self,
                _task_id: &'a str,
                _ctx: &'a #turbomcp::__macro_support::turbomcp_core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = #turbomcp::__macro_support::turbomcp_core::error::McpResult<#turbomcp::__macro_support::turbomcp_types::Task>> + #turbomcp::__macro_support::turbomcp_core::marker::MaybeSend + 'a {
                async { Err(#turbomcp::__macro_support::turbomcp_core::error::McpError::capability_not_supported("tasks/cancel")) }
            }

            fn get_task_result<'a>(
                &'a self,
                _task_id: &'a str,
                _ctx: &'a #turbomcp::__macro_support::turbomcp_core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = #turbomcp::__macro_support::turbomcp_core::error::McpResult<#turbomcp::__macro_support::serde_json::Value>> + #turbomcp::__macro_support::turbomcp_core::marker::MaybeSend + 'a {
                async { Err(#turbomcp::__macro_support::turbomcp_core::error::McpError::capability_not_supported("tasks/result")) }
            }
        }
    }
}

/// Main entry point for server macro.
pub fn generate_server(
    args: proc_macro::TokenStream,
    input: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let impl_block = match syn::parse::<ItemImpl>(input) {
        Ok(item) => item,
        Err(e) => return e.to_compile_error().into(),
    };

    // Validate the impl block structure
    if let Err(e) = validate_impl_block(&impl_block) {
        return e.to_compile_error().into();
    }

    let attrs = match ServerAttrs::parse(args.into()) {
        Ok(attrs) => attrs,
        Err(e) => return e.to_compile_error().into(),
    };

    let info = match analyze_impl(&impl_block, &attrs) {
        Ok(info) => info,
        Err(e) => return e.to_compile_error().into(),
    };

    // Validate handlers
    if let Err(e) = validate_handlers(&info) {
        return e.to_compile_error().into();
    }

    generate_mcp_handler(&info, &impl_block).into()
}

/// Validate the impl block structure and provide helpful error messages.
fn validate_impl_block(impl_block: &ItemImpl) -> Result<(), syn::Error> {
    // Check for trait impl (not supported)
    if impl_block.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            impl_block,
            "#[server] cannot be used on trait implementations\n\n\
            Hint: Apply #[server] to an inherent impl block:\n\
            \n\
            #[derive(Clone)]\n\
            struct MyServer;\n\
            \n\
            #[server(name = \"my-server\", version = \"1.0.0\")]\n\
            impl MyServer {\n\
                #[tool]\n\
                async fn my_tool(&self, arg: String) -> String { ... }\n\
            }",
        ));
    }

    // Check for methods with potentially misspelled handler attributes
    for item in &impl_block.items {
        if let syn::ImplItem::Fn(method) = item {
            for attr in &method.attrs {
                let path = attr.path();
                if let Some(ident) = path.get_ident() {
                    let ident_str = ident.to_string();

                    // Check for common typos
                    let typo_suggestions = [
                        ("tools", "tool"),
                        ("resources", "resource"),
                        ("prompts", "prompt"),
                        ("Tool", "tool"),
                        ("Resource", "resource"),
                        ("Prompt", "prompt"),
                        ("mcp_tool", "tool"),
                        ("mcp_resource", "resource"),
                        ("mcp_prompt", "prompt"),
                        ("handler", "tool"),
                    ];

                    for (typo, correct) in typo_suggestions {
                        if ident_str == typo {
                            return Err(syn::Error::new_spanned(
                                attr,
                                format!(
                                    "Unknown attribute `#[{}]` - did you mean `#[{}]`?\n\n\
                                    Valid handler attributes: #[tool], #[resource], #[prompt]",
                                    typo, correct
                                ),
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Validate handler definitions and provide helpful error messages.
fn validate_handlers(info: &ServerInfo) -> Result<(), syn::Error> {
    // Check for empty server (no handlers)
    if info.tools.is_empty() && info.resources.is_empty() && info.prompts.is_empty() {
        // This is actually valid - just a server with metadata
        // But we could warn in the future
    }

    validate_extensions(&info.extensions)?;

    // Validate tool signatures
    for tool in &info.tools {
        // Check for async
        if tool.sig.asyncness.is_none() {
            return Err(syn::Error::new_spanned(
                &tool.sig,
                format!(
                    "Tool `{}` must be async\n\n\
                    Hint: Add `async` to the function:\n\
                    \n\
                    #[tool]\n\
                    async fn {}(&self, ...) -> ... {{ ... }}",
                    tool.name, tool.name
                ),
            ));
        }

        // Check for &self receiver
        let has_self = tool
            .sig
            .inputs
            .iter()
            .any(|arg| matches!(arg, syn::FnArg::Receiver(_)));

        if !has_self {
            return Err(syn::Error::new_spanned(
                &tool.sig,
                format!(
                    "Tool `{}` must take &self as the first parameter\n\n\
                    Hint: Add &self to the function:\n\
                    \n\
                    #[tool]\n\
                    async fn {}(&self, arg: String) -> String {{ ... }}",
                    tool.name, tool.name
                ),
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    /// Run the analysis and validation `#[server]` performs, stopping short of
    /// code generation.
    fn check(impl_block: ItemImpl) -> Result<ServerInfo, syn::Error> {
        let info = analyze_impl(&impl_block, &ServerAttrs::default())?;
        validate_handlers(&info)?;
        Ok(info)
    }

    #[test]
    fn subscribe_without_unsubscribe_is_rejected() {
        let err = check(parse_quote! {
            impl S {
                #[subscribe]
                async fn watch(&self, uri: String) -> McpResult<()> { Ok(()) }
            }
        })
        .err()
        .expect("an unpaired #[subscribe] must not compile");
        assert!(err.to_string().contains("#[unsubscribe]"), "{err}");

        check(parse_quote! {
            impl S {
                #[subscribe]
                async fn watch(&self, uri: String) -> McpResult<()> { Ok(()) }
                #[unsubscribe]
                async fn unwatch(&self, uri: String) -> McpResult<()> { Ok(()) }
            }
        })
        .unwrap_or_else(|e| panic!("a paired #[subscribe] must compile: {e}"));
    }

    #[test]
    fn server_unknown_key_lists_every_key() {
        let err = ServerAttrs::parse(quote!(nmae = "x"))
            .err()
            .expect("a misspelled #[server] key must not compile");
        let message = err.to_string();
        for key in [
            "name",
            "version",
            "description",
            "title",
            "instructions",
            "website_url",
            "icons",
            "page_size",
            "logging",
        ] {
            assert!(message.contains(&format!("`{key}`")), "{message}");
        }
    }

    /// Before the shared parser, `#[tool]` discarded the strict parser's error
    /// in favour of a string-scanning fallback, and `#[resource]`/`#[prompt]`
    /// only had the fallback: all three compiled a typo into a handler with
    /// the metadata silently missing.
    #[test]
    fn every_marker_rejects_unknown_keys() {
        for impl_block in [
            parse_quote! {
                impl S {
                    #[tool(descriptio = "typo")]
                    async fn t(&self) -> String { String::new() }
                }
            },
            parse_quote! {
                impl S {
                    #[resource("mem://x", mime = "text/plain")]
                    async fn r(&self, uri: String, ctx: &RequestContext) -> String { uri }
                }
            },
            parse_quote! {
                impl S {
                    #[prompt(descriptio = "typo")]
                    async fn p(&self, ctx: &RequestContext) -> String { String::new() }
                }
            },
        ] {
            let err = check(impl_block)
                .err()
                .expect("an unknown key must not compile");
            assert!(err.to_string().contains("unknown #["), "{err}");
        }
    }

    #[test]
    fn explicit_descriptions_win_over_doc_comments_on_every_marker() {
        let info = check(parse_quote! {
            impl S {
                /// doc
                #[tool(description = "explicit")]
                async fn t(&self) -> String { String::new() }

                /// doc
                #[resource("mem://x", description = "explicit")]
                async fn r(&self, uri: String, ctx: &RequestContext) -> String { uri }

                /// doc
                #[prompt(description = "explicit")]
                async fn p(&self, ctx: &RequestContext) -> String { String::new() }

                /// doc only
                #[prompt]
                async fn q(&self, ctx: &RequestContext) -> String { String::new() }
            }
        })
        .unwrap();
        assert_eq!(info.tools[0].description, "explicit");
        assert_eq!(info.resources[0].description.as_deref(), Some("explicit"));
        assert_eq!(info.prompts[0].description.as_deref(), Some("explicit"));
        assert_eq!(info.prompts[1].description.as_deref(), Some("doc only"));
    }

    #[test]
    fn a_fully_qualified_option_is_an_optional_prompt_argument() {
        let info = check(parse_quote! {
            impl S {
                #[prompt]
                async fn p(
                    &self,
                    a: std::option::Option<String>,
                    b: ::core::option::Option<String>,
                    c: String,
                    ctx: &RequestContext,
                ) -> String { String::new() }
            }
        })
        .unwrap();
        let required: Vec<_> = info.prompts[0]
            .arguments
            .iter()
            .map(|a| a.required)
            .collect();
        assert_eq!(required, [false, false, true]);
    }

    #[test]
    fn metadata_values_are_validated_at_compile_time() {
        for (impl_block, expected) in [
            (
                parse_quote! {
                    impl S {
                        #[resource("mem://x", audience = ["model"])]
                        async fn r(&self, uri: String, ctx: &RequestContext) -> String { uri }
                    }
                },
                "unknown audience",
            ),
            (
                parse_quote! {
                    impl S {
                        #[resource("mem://x", priority = 1.5)]
                        async fn r(&self, uri: String, ctx: &RequestContext) -> String { uri }
                    }
                },
                "between 0 and 1",
            ),
            (
                parse_quote! {
                    impl S {
                        #[resource("mem://{id}", size = 10)]
                        async fn r(&self, uri: String, ctx: &RequestContext) -> String { uri }
                    }
                },
                "concrete resource",
            ),
            (
                parse_quote! {
                    impl S {
                        #[tool(task_support = "sometimes")]
                        async fn t(&self) -> String { String::new() }
                    }
                },
                "unknown task_support",
            ),
        ] {
            let err = check(impl_block)
                .err()
                .expect("an out-of-range value must not compile");
            assert!(err.to_string().contains(expected), "{err}");
        }
    }
}
