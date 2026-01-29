//! WASM server macro implementation
//!
//! Generates code that uses the `turbomcp_wasm::wasm_server::McpServer` builder.
//!
//! # Attribute Syntax
//!
//! The macro supports rich attribute syntax for tools, resources, and prompts:
//!
//! ```rust,ignore
//! #[server(name = "my-server", version = "1.0.0")]
//! impl MyServer {
//!     // Simple description
//!     #[tool("Say hello")]
//!     async fn hello(&self, args: HelloArgs) -> String { ... }
//!
//!     // Full syntax with tags and version
//!     #[tool(description = "Admin tool", tags = ["admin", "dangerous"], version = "2.0")]
//!     async fn admin_op(&self, ctx: Arc<RequestContext>, args: AdminArgs) -> Result<String, ToolError> { ... }
//!
//!     // Context injection (automatically uses _with_ctx variant)
//!     #[tool("Auth required")]
//!     async fn auth_tool(&self, ctx: Arc<RequestContext>, args: AuthArgs) -> Result<String, ToolError> { ... }
//! }
//! ```

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{ToTokens, quote};
use syn::{
    Attribute, Expr, ExprLit, FnArg, Ident, ImplItem, ItemImpl, Lit, LitStr, Meta, Pat, PatType,
    Result, Token, Type,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

/// Parsed server attributes
pub struct ServerArgs {
    pub name: String,
    pub version: String,
    pub description: Option<String>,
}

impl Parse for ServerArgs {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut name = None;
        let mut version = None;
        let mut description = None;

        let args = Punctuated::<Meta, Token![,]>::parse_terminated(input)?;

        for meta in args {
            if let Meta::NameValue(nv) = meta
                && let Some(key) = nv.path.get_ident().map(|i| i.to_string())
                && let Expr::Lit(ExprLit {
                    lit: Lit::Str(lit), ..
                }) = &nv.value
            {
                match key.as_str() {
                    "name" => name = Some(lit.value()),
                    "version" => version = Some(lit.value()),
                    "description" => description = Some(lit.value()),
                    _ => {}
                }
            }
        }

        Ok(ServerArgs {
            name: name.unwrap_or_else(|| "mcp-server".to_string()),
            version: version.unwrap_or_else(|| "1.0.0".to_string()),
            description,
        })
    }
}

/// Parsed component attributes (shared by tool, resource, prompt)
#[derive(Default, Clone)]
struct ComponentAttrs {
    /// Description
    description: Option<String>,
    /// Tags for categorization
    tags: Vec<String>,
    /// Version string
    version: Option<String>,
}

impl ComponentAttrs {
    /// Parse component attributes from an attribute.
    ///
    /// Supports multiple formats:
    /// - `#[tool("description")]` - just description
    /// - `#[tool(description = "desc", tags = ["a", "b"], version = "1.0")]` - full syntax
    fn parse(attr: &Attribute) -> Self {
        let mut attrs = Self::default();

        // Try parsing as #[attr("value")]
        if let Ok(lit) = attr.parse_args::<LitStr>() {
            attrs.description = Some(lit.value());
            return attrs;
        }

        // Try parsing as #[attr(description = "value", tags = [...], version = "...")]
        if let Ok(args) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) {
            for meta in args {
                if let Meta::NameValue(nv) = &meta
                    && let Some(key) = nv.path.get_ident().map(|i| i.to_string())
                    && let Expr::Lit(ExprLit {
                        lit: Lit::Str(s), ..
                    }) = &nv.value
                {
                    match key.as_str() {
                        "description" => attrs.description = Some(s.value()),
                        "version" => attrs.version = Some(s.value()),
                        _ => {}
                    }
                }
            }

            // Parse tags using alternative method (handles array syntax)
            let token_str = attr.meta.to_token_stream().to_string();
            attrs.tags = parse_tags_array(&token_str);
        }

        attrs
    }
}

/// Parse `tags = ["a", "b", "c"]` pattern from token string.
fn parse_tags_array(token_str: &str) -> Vec<String> {
    let Some(tags_start) = token_str.find("tags") else {
        return Vec::new();
    };
    let Some(bracket_start) = token_str[tags_start..].find('[') else {
        return Vec::new();
    };
    let after_bracket = &token_str[tags_start + bracket_start + 1..];
    let Some(bracket_end) = after_bracket.find(']') else {
        return Vec::new();
    };

    let tags_content = &after_bracket[..bracket_end];
    tags_content
        .split(',')
        .filter_map(|part| {
            let part = part.trim();
            if part.starts_with('"') && part.ends_with('"') && part.len() >= 2 {
                Some(part[1..part.len() - 1].to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Information about a tool method
struct ToolMethod {
    name: Ident,
    description: String,
    arg_type: Option<Type>,
    /// Whether handler takes context as first param
    has_context: bool,
    /// Tags for visibility filtering
    tags: Vec<String>,
    /// Version string
    version: Option<String>,
}

/// Information about a resource method
struct ResourceMethod {
    name: Ident,
    uri_template: String,
    /// Whether handler takes context as first param
    has_context: bool,
    /// Tags for visibility filtering
    tags: Vec<String>,
    /// Version string
    version: Option<String>,
}

/// Information about a prompt method
struct PromptMethod {
    name: Ident,
    description: String,
    has_args: bool,
    arg_type: Option<Type>,
    /// Whether handler takes context as first param
    has_context: bool,
    /// Tags for visibility filtering
    tags: Vec<String>,
    /// Version string
    version: Option<String>,
}

/// Generate the WASM server implementation
pub fn generate_server(args: ServerArgs, mut impl_block: ItemImpl) -> Result<TokenStream2> {
    // Extract struct name
    let struct_name = extract_struct_name(&impl_block)?;

    // Extract methods with MCP attributes
    let tools = extract_tool_methods(&impl_block);
    let resources = extract_resource_methods(&impl_block);
    let prompts = extract_prompt_methods(&impl_block);

    // Strip MCP attributes from methods
    strip_mcp_attributes(&mut impl_block);

    // Generate builder code
    let tool_registrations = generate_tool_registrations(&tools);
    let resource_registrations = generate_resource_registrations(&resources);
    let prompt_registrations = generate_prompt_registrations(&prompts);

    // Generate metadata
    let tool_metadata: Vec<_> = tools
        .iter()
        .map(|t| {
            let name = t.name.to_string();
            let desc = &t.description;
            let tags = &t.tags;
            let version = t.version.as_deref().unwrap_or("");
            quote! { (#name, #desc, &[#(#tags),*], #version) }
        })
        .collect();

    let resource_metadata: Vec<_> = resources
        .iter()
        .map(|r| {
            let uri = &r.uri_template;
            let name = r.name.to_string();
            let tags = &r.tags;
            let version = r.version.as_deref().unwrap_or("");
            quote! { (#uri, #name, &[#(#tags),*], #version) }
        })
        .collect();

    let prompt_metadata: Vec<_> = prompts
        .iter()
        .map(|p| {
            let name = p.name.to_string();
            let desc = &p.description;
            let tags = &p.tags;
            let version = p.version.as_deref().unwrap_or("");
            quote! { (#name, #desc, &[#(#tags),*], #version) }
        })
        .collect();

    // Generate tool tags for VisibilityLayer integration
    let tool_tags: Vec<_> = tools
        .iter()
        .filter(|t| !t.tags.is_empty())
        .map(|t| {
            let name = t.name.to_string();
            let tags = &t.tags;
            quote! { (#name, vec![#(#tags.to_string()),*]) }
        })
        .collect();

    let resource_tags: Vec<_> = resources
        .iter()
        .filter(|r| !r.tags.is_empty())
        .map(|r| {
            let uri = &r.uri_template;
            let tags = &r.tags;
            quote! { (#uri, vec![#(#tags.to_string()),*]) }
        })
        .collect();

    let prompt_tags: Vec<_> = prompts
        .iter()
        .filter(|p| !p.tags.is_empty())
        .map(|p| {
            let name = p.name.to_string();
            let tags = &p.tags;
            quote! { (#name, vec![#(#tags.to_string()),*]) }
        })
        .collect();

    let server_name = &args.name;
    let server_version = &args.version;

    let description_call = if let Some(desc) = &args.description {
        quote! { .description(#desc) }
    } else {
        quote! {}
    };

    let expanded = quote! {
        #impl_block

        impl #struct_name {
            /// Create a WASM MCP server from this implementation.
            ///
            /// This method builds a fully-configured `McpServer` with all registered
            /// tools, resources, and prompts. Use this for Cloudflare Workers,
            /// Deno Deploy, and other WASM edge environments.
            ///
            /// # Note
            ///
            /// For native server support (stdio, HTTP, TCP, WebSocket, Unix),
            /// use the `#[turbomcp::server]` macro from `turbomcp-macros` instead.
            /// That macro generates `impl McpHandler` which works with all native
            /// transports via `McpHandlerExt`.
            pub fn into_mcp_server(self) -> ::turbomcp_wasm::wasm_server::McpServer {
                ::turbomcp_wasm::wasm_server::McpServer::builder(#server_name, #server_version)
                    #description_call
                    #tool_registrations
                    #resource_registrations
                    #prompt_registrations
                    .build()
            }

            /// Get metadata for all registered tools.
            ///
            /// Returns a vector of (name, description, tags, version) tuples.
            pub fn get_tools_metadata() -> Vec<(&'static str, &'static str, &'static [&'static str], &'static str)> {
                vec![#(#tool_metadata),*]
            }

            /// Get metadata for all registered resources.
            ///
            /// Returns a vector of (uri_template, name, tags, version) tuples.
            pub fn get_resources_metadata() -> Vec<(&'static str, &'static str, &'static [&'static str], &'static str)> {
                vec![#(#resource_metadata),*]
            }

            /// Get metadata for all registered prompts.
            ///
            /// Returns a vector of (name, description, tags, version) tuples.
            pub fn get_prompts_metadata() -> Vec<(&'static str, &'static str, &'static [&'static str], &'static str)> {
                vec![#(#prompt_metadata),*]
            }

            /// Get tool tags mapping for VisibilityLayer integration.
            ///
            /// Returns a vector of (tool_name, tags) tuples for tools that have tags.
            pub fn get_tool_tags() -> Vec<(&'static str, Vec<String>)> {
                vec![#(#tool_tags),*]
            }

            /// Get resource tags mapping for VisibilityLayer integration.
            ///
            /// Returns a vector of (uri_template, tags) tuples for resources that have tags.
            pub fn get_resource_tags() -> Vec<(&'static str, Vec<String>)> {
                vec![#(#resource_tags),*]
            }

            /// Get prompt tags mapping for VisibilityLayer integration.
            ///
            /// Returns a vector of (prompt_name, tags) tuples for prompts that have tags.
            pub fn get_prompt_tags() -> Vec<(&'static str, Vec<String>)> {
                vec![#(#prompt_tags),*]
            }

            /// Get server info.
            ///
            /// Returns (name, version) tuple.
            pub fn server_info() -> (&'static str, &'static str) {
                (#server_name, #server_version)
            }
        }
    };

    Ok(expanded)
}

/// Extract struct name from impl block
fn extract_struct_name(impl_block: &ItemImpl) -> Result<Ident> {
    match &*impl_block.self_ty {
        Type::Path(type_path) => {
            if let Some(segment) = type_path.path.segments.last() {
                Ok(segment.ident.clone())
            } else {
                Err(syn::Error::new_spanned(
                    &type_path.path,
                    "Expected a valid type path",
                ))
            }
        }
        _ => Err(syn::Error::new(
            Span::call_site(),
            "The #[wasm_server] attribute only supports named types",
        )),
    }
}

/// Extract tool methods from impl block
fn extract_tool_methods(impl_block: &ItemImpl) -> Vec<ToolMethod> {
    let mut tools = Vec::new();

    for item in &impl_block.items {
        if let ImplItem::Fn(method) = item {
            for attr in &method.attrs {
                if attr.path().is_ident("tool") {
                    let attrs = ComponentAttrs::parse(attr);
                    let description = attrs.description.unwrap_or_else(|| "Tool".to_string());
                    let (has_context, arg_type) = extract_tool_arg_type_with_ctx(&method.sig);

                    tools.push(ToolMethod {
                        name: method.sig.ident.clone(),
                        description,
                        arg_type,
                        has_context,
                        tags: attrs.tags,
                        version: attrs.version,
                    });
                    break;
                }
            }
        }
    }

    tools
}

/// Extract resource methods from impl block
fn extract_resource_methods(impl_block: &ItemImpl) -> Vec<ResourceMethod> {
    let mut resources = Vec::new();

    for item in &impl_block.items {
        if let ImplItem::Fn(method) = item {
            for attr in &method.attrs {
                if attr.path().is_ident("resource") {
                    let attrs = ComponentAttrs::parse(attr);
                    let uri_template = attrs
                        .description
                        .unwrap_or_else(|| "resource://".to_string());
                    let has_context = method_has_context(&method.sig);

                    resources.push(ResourceMethod {
                        name: method.sig.ident.clone(),
                        uri_template,
                        has_context,
                        tags: attrs.tags,
                        version: attrs.version,
                    });
                    break;
                }
            }
        }
    }

    resources
}

/// Extract prompt methods from impl block
fn extract_prompt_methods(impl_block: &ItemImpl) -> Vec<PromptMethod> {
    let mut prompts = Vec::new();

    for item in &impl_block.items {
        if let ImplItem::Fn(method) = item {
            for attr in &method.attrs {
                if attr.path().is_ident("prompt") {
                    let attrs = ComponentAttrs::parse(attr);
                    let description = attrs.description.unwrap_or_else(|| "Prompt".to_string());
                    let (has_context, has_args, arg_type) =
                        extract_prompt_arg_info_with_ctx(&method.sig);

                    prompts.push(PromptMethod {
                        name: method.sig.ident.clone(),
                        description,
                        has_args,
                        arg_type,
                        has_context,
                        tags: attrs.tags,
                        version: attrs.version,
                    });
                    break;
                }
            }
        }
    }

    prompts
}

/// Extract the argument type from a tool method signature, along with context detection.
/// Returns (has_context, arg_type).
fn extract_tool_arg_type_with_ctx(sig: &syn::Signature) -> (bool, Option<Type>) {
    let mut has_context = false;
    let mut arg_type = None;

    for input in &sig.inputs {
        if let FnArg::Typed(PatType { ty, .. }) = input {
            if is_context_type(ty) {
                has_context = true;
            } else if !is_self_type(ty) && arg_type.is_none() {
                arg_type = Some((**ty).clone());
            }
        }
    }

    (has_context, arg_type)
}

/// Check if method has a context parameter
fn method_has_context(sig: &syn::Signature) -> bool {
    for input in &sig.inputs {
        if let FnArg::Typed(PatType { ty, .. }) = input
            && is_context_type(ty)
        {
            return true;
        }
    }
    false
}

/// Extract argument info from a prompt method signature, including context detection.
/// Returns (has_context, has_args, arg_type).
fn extract_prompt_arg_info_with_ctx(sig: &syn::Signature) -> (bool, bool, Option<Type>) {
    let mut has_context = false;
    let mut arg_type = None;

    for input in &sig.inputs {
        if let FnArg::Typed(PatType { ty, pat, .. }) = input {
            if is_context_type(ty) {
                has_context = true;
            } else if !is_self_type(ty)
                && let Pat::Ident(pat_ident) = pat.as_ref()
                && pat_ident.ident != "self"
            {
                arg_type = Some((**ty).clone());
            }
        }
    }

    let has_args = arg_type.is_some();
    (has_context, has_args, arg_type)
}

/// Check if type is a context type (Context, RequestContext, or Arc<RequestContext>)
fn is_context_type(ty: &Type) -> bool {
    // Check for direct Context or RequestContext
    if let Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        let name = segment.ident.to_string();
        if name == "Context" || name == "RequestContext" {
            return true;
        }
        // Check for Arc<RequestContext>
        if name == "Arc"
            && let syn::PathArguments::AngleBracketed(args) = &segment.arguments
        {
            for arg in &args.args {
                if let syn::GenericArgument::Type(Type::Path(inner_path)) = arg
                    && let Some(inner_seg) = inner_path.path.segments.last()
                    && inner_seg.ident == "RequestContext"
                {
                    return true;
                }
            }
        }
    }
    // Check for references to Context/RequestContext
    if let Type::Reference(type_ref) = ty
        && let Type::Path(type_path) = &*type_ref.elem
        && let Some(segment) = type_path.path.segments.last()
    {
        let name = segment.ident.to_string();
        return name == "Context" || name == "RequestContext";
    }
    false
}

/// Check if type is self
fn is_self_type(ty: &Type) -> bool {
    if let Type::Reference(type_ref) = ty
        && let Type::Path(type_path) = &*type_ref.elem
        && let Some(segment) = type_path.path.segments.last()
    {
        return segment.ident == "Self";
    }
    false
}

/// Strip MCP attributes from impl block methods
fn strip_mcp_attributes(impl_block: &mut ItemImpl) {
    for item in &mut impl_block.items {
        if let ImplItem::Fn(method) = item {
            method.attrs.retain(|attr| {
                !attr.path().is_ident("tool")
                    && !attr.path().is_ident("resource")
                    && !attr.path().is_ident("prompt")
            });
        }
    }
}

/// Generate tool registration code
fn generate_tool_registrations(tools: &[ToolMethod]) -> TokenStream2 {
    let registrations: Vec<_> = tools
        .iter()
        .map(|tool| {
            let method_name = &tool.name;
            let tool_name = method_name.to_string();
            let description = &tool.description;

            match (tool.has_context, &tool.arg_type) {
                // With context and arguments
                (true, Some(arg_type)) => {
                    quote! {
                        .tool_with_ctx(#tool_name, #description, {
                            let server = self.clone();
                            move |ctx: ::std::sync::Arc<::turbomcp_wasm::wasm_server::RequestContext>, args: #arg_type| {
                                let server = server.clone();
                                async move {
                                    server.#method_name(ctx, args).await
                                }
                            }
                        })
                    }
                }
                // With context, no arguments
                (true, None) => {
                    quote! {
                        .tool_with_ctx_no_args(#tool_name, #description, {
                            let server = self.clone();
                            move |ctx: ::std::sync::Arc<::turbomcp_wasm::wasm_server::RequestContext>| {
                                let server = server.clone();
                                async move {
                                    server.#method_name(ctx).await
                                }
                            }
                        })
                    }
                }
                // No context, with arguments
                (false, Some(arg_type)) => {
                    quote! {
                        .tool(#tool_name, #description, {
                            let server = self.clone();
                            move |args: #arg_type| {
                                let server = server.clone();
                                async move {
                                    server.#method_name(args).await
                                }
                            }
                        })
                    }
                }
                // No context, no arguments
                (false, None) => {
                    quote! {
                        .tool_no_args(#tool_name, #description, {
                            let server = self.clone();
                            move || {
                                let server = server.clone();
                                async move {
                                    server.#method_name().await
                                }
                            }
                        })
                    }
                }
            }
        })
        .collect();

    quote! { #(#registrations)* }
}

/// Generate resource registration code
fn generate_resource_registrations(resources: &[ResourceMethod]) -> TokenStream2 {
    let registrations: Vec<_> = resources
        .iter()
        .map(|resource| {
            let method_name = &resource.name;
            let uri_template = &resource.uri_template;
            let name = method_name.to_string();
            let description = format!("Resource at {}", uri_template);

            if resource.has_context {
                quote! {
                    .resource_with_ctx(#uri_template, #name, #description, {
                        let server = self.clone();
                        move |ctx: ::std::sync::Arc<::turbomcp_wasm::wasm_server::RequestContext>, uri: String| {
                            let server = server.clone();
                            async move {
                                server.#method_name(ctx, uri).await
                            }
                        }
                    })
                }
            } else {
                quote! {
                    .resource(#uri_template, #name, #description, {
                        let server = self.clone();
                        move |uri: String| {
                            let server = server.clone();
                            async move {
                                server.#method_name(uri).await
                            }
                        }
                    })
                }
            }
        })
        .collect();

    quote! { #(#registrations)* }
}

/// Generate prompt registration code
fn generate_prompt_registrations(prompts: &[PromptMethod]) -> TokenStream2 {
    let registrations: Vec<_> = prompts
        .iter()
        .map(|prompt| {
            let method_name = &prompt.name;
            let prompt_name = method_name.to_string();
            let description = &prompt.description;

            match (prompt.has_context, prompt.has_args, &prompt.arg_type) {
                // With context and arguments
                (true, true, Some(arg_type)) => {
                    quote! {
                        .prompt_with_ctx(#prompt_name, #description, {
                            let server = self.clone();
                            move |ctx: ::std::sync::Arc<::turbomcp_wasm::wasm_server::RequestContext>, args: Option<#arg_type>| {
                                let server = server.clone();
                                async move {
                                    server.#method_name(ctx, args).await
                                }
                            }
                        })
                    }
                }
                // With context, no arguments
                (true, false, _) => {
                    quote! {
                        .prompt_with_ctx_no_args(#prompt_name, #description, {
                            let server = self.clone();
                            move |ctx: ::std::sync::Arc<::turbomcp_wasm::wasm_server::RequestContext>| {
                                let server = server.clone();
                                async move {
                                    server.#method_name(ctx).await
                                }
                            }
                        })
                    }
                }
                // No context, with arguments
                (false, true, Some(arg_type)) => {
                    quote! {
                        .prompt(#prompt_name, #description, {
                            let server = self.clone();
                            move |args: Option<#arg_type>| {
                                let server = server.clone();
                                async move {
                                    server.#method_name(args).await
                                }
                            }
                        })
                    }
                }
                // No context, no arguments
                (false, false, _) => {
                    quote! {
                        .prompt_no_args(#prompt_name, #description, {
                            let server = self.clone();
                            move || {
                                let server = server.clone();
                                async move {
                                    server.#method_name().await
                                }
                            }
                        })
                    }
                }
                // has_args but no arg_type - shouldn't happen, return empty
                (_, true, None) => {
                    quote! {}
                }
            }
        })
        .collect();

    quote! { #(#registrations)* }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_args_parsing() {
        // Basic test that the struct exists
        let args = ServerArgs {
            name: "test".to_string(),
            version: "1.0.0".to_string(),
            description: Some("A test server".to_string()),
        };
        assert_eq!(args.name, "test");
        assert_eq!(args.version, "1.0.0");
    }

    #[test]
    fn test_parse_tags_array() {
        // Test parsing tags from attribute string
        let token_str =
            r#"tool(description = "test", tags = ["admin", "dangerous"], version = "1.0")"#;
        let tags = parse_tags_array(token_str);
        assert_eq!(tags, vec!["admin", "dangerous"]);
    }

    #[test]
    fn test_parse_tags_array_empty() {
        let token_str = r#"tool(description = "test")"#;
        let tags = parse_tags_array(token_str);
        assert!(tags.is_empty());
    }

    #[test]
    fn test_component_attrs_default() {
        let attrs = ComponentAttrs::default();
        assert!(attrs.description.is_none());
        assert!(attrs.tags.is_empty());
        assert!(attrs.version.is_none());
    }

    #[test]
    fn test_is_context_type_arc_request_context() {
        // Test Arc<RequestContext> detection
        let ty: Type = syn::parse_quote!(Arc<RequestContext>);
        assert!(is_context_type(&ty));
    }

    #[test]
    fn test_is_context_type_request_context() {
        let ty: Type = syn::parse_quote!(RequestContext);
        assert!(is_context_type(&ty));
    }

    #[test]
    fn test_is_context_type_other() {
        let ty: Type = syn::parse_quote!(String);
        assert!(!is_context_type(&ty));
    }
}
