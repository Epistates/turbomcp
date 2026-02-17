//! Server macro - generates McpHandler trait implementation.

use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use syn::{Ident, ItemImpl};

use super::tool::{
    ToolAttrs, ToolInfo, generate_call_args, generate_extraction_code, generate_schema_code,
    parse_quoted_value, parse_tags_array,
};

/// Information collected from analyzing the impl block.
pub struct ServerInfo {
    /// Struct name
    pub struct_name: Ident,
    /// Server name
    pub name: String,
    /// Server version
    pub version: String,
    /// Server description
    pub description: Option<String>,
    /// Tool handlers
    pub tools: Vec<ToolInfo>,
    /// Resource handlers
    pub resources: Vec<ResourceInfo>,
    /// Prompt handlers
    pub prompts: Vec<PromptInfo>,
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
    /// Function name
    pub fn_name: Ident,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Version string
    pub version: Option<String>,
}

/// Prompt argument info (HIGH-002).
#[derive(Clone)]
pub struct PromptArgumentInfo {
    /// Argument name
    pub name: String,
    /// Argument description
    pub description: Option<String>,
    /// Whether the argument is required
    pub required: bool,
}

/// Parse server attributes.
pub struct ServerAttrs {
    /// Server name
    pub name: Option<String>,
    /// Server version
    pub version: Option<String>,
    /// Server description
    pub description: Option<String>,
}

impl ServerAttrs {
    /// Parse from attribute token stream.
    pub fn parse(args: proc_macro::TokenStream) -> Result<Self, syn::Error> {
        let mut name = None;
        let mut version = None;
        let mut description = None;

        if args.is_empty() {
            return Ok(Self {
                name,
                version,
                description,
            });
        }

        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("name") {
                let value: syn::LitStr = meta.value()?.parse()?;
                name = Some(value.value());
            } else if meta.path.is_ident("version") {
                let value: syn::LitStr = meta.value()?.parse()?;
                version = Some(value.value());
            } else if meta.path.is_ident("description") {
                let value: syn::LitStr = meta.value()?.parse()?;
                description = Some(value.value());
            } else if meta.path.is_ident("transports") {
                // v3: The `transports` attribute is deprecated.
                meta.value()?;
                let _: syn::ExprArray = meta.input.parse()?;
                return Err(syn::Error::new(
                    meta.path.span(),
                    "`transports` attribute is deprecated. Enable features in Cargo.toml instead:\n\
                    turbomcp = { version = \"3.0\", features = [\"http\", \"tcp\"] }\n\
                    Then call transport methods: server.run_http(\"0.0.0.0:8080\").await",
                ));
            } else if meta.path.is_ident("root") {
                // v3: Ignore `root` attribute for backward compatibility.
                // Roots configuration should be done via builder API.
                let _value: syn::LitStr = meta.value()?.parse()?;
            }
            Ok(())
        });

        syn::parse::Parser::parse(parser, args)?;

        Ok(Self {
            name,
            version,
            description,
        })
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

    let name = attrs
        .name
        .clone()
        .unwrap_or_else(|| struct_name.to_string());
    let version = attrs.version.clone().unwrap_or_else(|| "1.0.0".to_string());
    let description = attrs.description.clone();

    let mut tools = Vec::new();
    let mut resources = Vec::new();
    let mut prompts = Vec::new();

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
                        sig: method.sig.clone(),
                        block: Box::new(syn::parse_quote!({})),
                    };
                    let tool_info = ToolInfo::from_fn(&item_fn, tool_attrs)?;
                    tools.push(tool_info);
                    break;
                } else if attr.path().is_ident("resource") {
                    let resource_attrs = extract_resource_attrs(attr)?;
                    let fn_name = method.sig.ident.clone();
                    let description = extract_doc_comments(&method.attrs);
                    resources.push(ResourceInfo {
                        uri_template: resource_attrs.uri_template,
                        name: fn_name.to_string(),
                        description,
                        mime_type: resource_attrs.mime_type,
                        fn_name,
                        tags: resource_attrs.tags,
                        version: resource_attrs.version,
                    });
                    break;
                } else if attr.path().is_ident("prompt") {
                    let fn_name = method.sig.ident.clone();
                    let prompt_attrs = extract_prompt_attrs(attr);
                    let description =
                        extract_doc_comments(&method.attrs).or(prompt_attrs.description);
                    let arguments = extract_prompt_arguments(&method.sig);
                    prompts.push(PromptInfo {
                        name: fn_name.to_string(),
                        description,
                        arguments,
                        fn_name,
                        tags: prompt_attrs.tags,
                        version: prompt_attrs.version,
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
        tools,
        resources,
        prompts,
    })
}

/// Resource attribute parsed info (HIGH-001).
pub struct ResourceAttrInfo {
    pub uri_template: String,
    pub mime_type: Option<String>,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Version string
    pub version: Option<String>,
}

/// Extract resource URI and optional mime_type, tags, version from attribute.
///
/// Supports:
/// - `#[resource("uri://template")]` - URI only
/// - `#[resource("uri://template", mime_type = "text/plain")]` - URI with MIME type
/// - `#[resource("uri://template", tags = ["admin"], version = "1.0")]` - Full syntax
fn extract_resource_attrs(attr: &syn::Attribute) -> Result<ResourceAttrInfo, syn::Error> {
    if let syn::Meta::List(meta_list) = &attr.meta {
        let tokens = meta_list.tokens.clone();

        // Try to parse as just a string literal first (simple case)
        if let Ok(lit) = syn::parse2::<syn::LitStr>(tokens.clone()) {
            return Ok(ResourceAttrInfo {
                uri_template: lit.value(),
                mime_type: None,
                tags: Vec::new(),
                version: None,
            });
        }

        // Parse comma-separated format
        let token_str = tokens.to_string();
        if token_str.contains(',') || token_str.contains('[') {
            // First part is the URI (with quotes)
            let uri_end = token_str.find(',').unwrap_or(token_str.len());
            let uri_str = token_str[..uri_end].trim().trim_matches('"');

            let mime_type = parse_quoted_value(&token_str, "mime_type");
            let version = parse_quoted_value(&token_str, "version");
            let tags = parse_tags_array(&token_str);

            return Ok(ResourceAttrInfo {
                uri_template: uri_str.to_string(),
                mime_type,
                tags,
                version,
            });
        }
    }
    Err(syn::Error::new_spanned(
        attr,
        "Expected #[resource(\"uri://template\")] or #[resource(\"uri://template\", mime_type = \"text/plain\")]",
    ))
}

/// Check if a type is a reference to RequestContext.
fn is_request_context_type(ty: &syn::Type) -> bool {
    // Handle &RequestContext
    if let syn::Type::Reference(type_ref) = ty {
        return is_request_context_type(&type_ref.elem);
    }

    // Handle RequestContext path
    if let syn::Type::Path(type_path) = ty {
        let path_str = type_path
            .path
            .segments
            .iter()
            .map(|s| s.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");

        // Match various ways RequestContext might be referenced
        return path_str == "RequestContext"
            || path_str.ends_with("::RequestContext")
            || path_str.contains("RequestContext");
    }

    false
}

/// Parsed prompt attributes.
#[derive(Default)]
struct PromptAttrs {
    description: Option<String>,
    tags: Vec<String>,
    version: Option<String>,
}

/// Extract prompt attributes from #[prompt(...)] attribute.
fn extract_prompt_attrs(attr: &syn::Attribute) -> PromptAttrs {
    let mut attrs = PromptAttrs::default();

    // Handle empty #[prompt]
    let syn::Meta::List(meta_list) = &attr.meta else {
        return attrs;
    };

    // Handle #[prompt("description")] shorthand
    if let Ok(lit) = syn::parse2::<syn::LitStr>(meta_list.tokens.clone()) {
        attrs.description = Some(lit.value());
        return attrs;
    }

    // Parse full syntax from token string
    let token_str = meta_list.tokens.to_string();
    attrs.description = parse_quoted_value(&token_str, "description");
    attrs.version = parse_quoted_value(&token_str, "version");
    attrs.tags = parse_tags_array(&token_str);

    attrs
}

/// Extract prompt arguments from function signature (HIGH-002).
fn extract_prompt_arguments(sig: &syn::Signature) -> Vec<PromptArgumentInfo> {
    let mut args = Vec::new();

    for input in &sig.inputs {
        if let syn::FnArg::Typed(pat_type) = input
            && let syn::Pat::Ident(pat_ident) = &*pat_type.pat
        {
            let name = pat_ident.ident.to_string();

            // Skip self parameter
            if name == "self" {
                continue;
            }

            // Skip RequestContext parameters (regardless of name: ctx, _ctx, context, etc.)
            if is_request_context_type(&pat_type.ty) {
                continue;
            }

            // Check if type is Option<T> to determine if required
            let is_option = if let syn::Type::Path(type_path) = &*pat_type.ty {
                type_path
                    .path
                    .segments
                    .first()
                    .map(|s| s.ident == "Option")
                    .unwrap_or(false)
            } else {
                false
            };

            args.push(PromptArgumentInfo {
                name,
                description: None, // Could extract from doc comments if needed
                required: !is_option,
            });
        }
    }

    args
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

/// Strip #[tool], #[resource], and #[prompt] attributes from impl items.
fn strip_handler_attributes(impl_block: &ItemImpl) -> ItemImpl {
    let mut stripped = impl_block.clone();
    for item in &mut stripped.items {
        if let syn::ImplItem::Fn(method) = item {
            method.attrs.retain(|attr| {
                !attr.path().is_ident("tool")
                    && !attr.path().is_ident("resource")
                    && !attr.path().is_ident("prompt")
            });
        }
    }
    stripped
}

/// Generate code for the meta field (tags and version).
fn generate_meta_code(tags: &[String], version: &Option<String>) -> TokenStream {
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
                ::turbomcp::__macro_support::serde_json::Value::Array(
                    vec![#(::turbomcp::__macro_support::serde_json::Value::String(#tag_strings)),*]
                )
            );
        }
    };

    let version_code = if let Some(ver) = version {
        quote! {
            meta.insert(
                "version".to_string(),
                ::turbomcp::__macro_support::serde_json::Value::String(#ver.to_string())
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

    let description_code = if let Some(desc) = &info.description {
        quote! { .with_description(#desc) }
    } else {
        quote! {}
    };

    // Generate tool listing code
    // Uses ::turbomcp::__macro_support:: paths so users don't need internal crates
    let tool_list_code = info.tools.iter().map(|tool| {
        let tool_name = &tool.name;
        let tool_desc = &tool.description;
        let schema_code = generate_schema_code(&tool.parameters);

        // Generate meta field if tags or version present
        let meta_code = generate_meta_code(&tool.tags, &tool.version);

        quote! {
            ::turbomcp::__macro_support::turbomcp_types::Tool {
                name: #tool_name.to_string(),
                description: Some(#tool_desc.to_string()),
                input_schema: #schema_code,
                title: None,
                icon: None,
                annotations: None,
                output_schema: None,
                meta: #meta_code,
            }
        }
    });

    // Generate resource listing code (HIGH-001: includes mimeType)
    let resource_list_code = info.resources.iter().map(|resource| {
        let uri = &resource.uri_template;
        let name = &resource.name;
        let desc = resource.description.as_deref().unwrap_or("");
        let meta_code = generate_meta_code(&resource.tags, &resource.version);
        let mime_type_code = if let Some(mime) = &resource.mime_type {
            quote! { Some(#mime.to_string()) }
        } else {
            quote! { None }
        };
        quote! {
            ::turbomcp::__macro_support::turbomcp_types::Resource {
                uri: #uri.to_string(),
                name: #name.to_string(),
                description: Some(#desc.to_string()),
                title: None,
                icon: None,
                mime_type: #mime_type_code,
                annotations: None,
                size: None,
                meta: #meta_code,
            }
        }
    });

    // Generate prompt listing code (HIGH-002: includes arguments)
    let prompt_list_code = info.prompts.iter().map(|prompt| {
        let name = &prompt.name;
        let desc = prompt.description.as_deref().unwrap_or("");
        let meta_code = generate_meta_code(&prompt.tags, &prompt.version);

        // Generate arguments
        let args_code = if prompt.arguments.is_empty() {
            quote! { None }
        } else {
            let arg_structs = prompt.arguments.iter().map(|arg| {
                let arg_name = &arg.name;
                let arg_desc = arg.description.as_deref().unwrap_or("");
                let required = arg.required;
                quote! {
                    ::turbomcp::__macro_support::turbomcp_types::PromptArgument {
                        name: #arg_name.to_string(),
                        description: Some(#arg_desc.to_string()),
                        required: Some(#required),
                    }
                }
            });
            quote! { Some(vec![#(#arg_structs),*]) }
        };

        quote! {
            ::turbomcp::__macro_support::turbomcp_types::Prompt {
                name: #name.to_string(),
                description: Some(#desc.to_string()),
                title: None,
                icon: None,
                arguments: #args_code,
                meta: #meta_code,
            }
        }
    });

    // Generate tool dispatch code
    let tool_dispatch_code = info.tools.iter().map(|tool| {
        let tool_name = &tool.name;
        let fn_name = syn::Ident::new(&tool.name, proc_macro2::Span::call_site());
        let extraction = generate_extraction_code(&tool.parameters);
        let call_args = generate_call_args(&tool.sig);

        quote! {
            #tool_name => {
                #extraction
                let result = self.#fn_name(#call_args).await;
                Ok(::turbomcp::__macro_support::turbomcp_types::IntoToolResult::into_tool_result(result))
            }
        }
    });

    // Generate resource dispatch code with proper URI template matching
    let resource_dispatch_code = info.resources.iter().map(|resource| {
        let uri_template = &resource.uri_template;
        let fn_name = &resource.fn_name;

        // Check if template has variables (contains '{')
        if uri_template.contains('{') {
            // Extract prefix and suffix for template matching
            // e.g., "file://{path}" -> prefix="file://", suffix=""
            // e.g., "config://{name}/settings" -> prefix="config://", suffix="/settings"
            let prefix = uri_template.split('{').next().unwrap_or("");
            let suffix = uri_template.rsplit('}').next().unwrap_or("");

            // Generate safe template matching code
            quote! {
                if uri.starts_with(#prefix) && uri.ends_with(#suffix) && uri.len() >= #prefix.len() + #suffix.len() {
                    let result = self.#fn_name(uri.to_string(), ctx).await;
                    return match result {
                        Ok(r) => Ok(::turbomcp::__macro_support::turbomcp_types::IntoResourceResult::into_resource_result(r, &uri)),
                        Err(e) => Err(e),
                    };
                }
            }
        } else {
            // Exact match for templates without variables
            quote! {
                if uri == #uri_template {
                    let result = self.#fn_name(uri.to_string(), ctx).await;
                    return match result {
                        Ok(r) => Ok(::turbomcp::__macro_support::turbomcp_types::IntoResourceResult::into_resource_result(r, &uri)),
                        Err(e) => Err(e),
                    };
                }
            }
        }
    });

    // Generate prompt dispatch code (HIGH-002: passes arguments to handler)
    // Uses IntoPromptResult to convert the return value, supporting:
    // - String, &str -> PromptResult::user(...)
    // - PromptResult -> passed through
    // - Result<T, E> -> Ok unwrapped, Err converted to message
    let prompt_dispatch_code = info.prompts.iter().map(|prompt| {
        let prompt_name = &prompt.name;
        let fn_name = &prompt.fn_name;

        // Generate argument extraction code
        let arg_extractions = prompt.arguments.iter().map(|arg| {
            let arg_name = &arg.name;
            let arg_ident = syn::Ident::new(arg_name, proc_macro2::Span::call_site());

            if arg.required {
                quote! {
                    let #arg_ident: String = prompt_args
                        .as_ref()
                        .and_then(|a| a.get(#arg_name))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .ok_or_else(|| ::turbomcp::__macro_support::turbomcp_core::error::McpError::invalid_params(
                            format!("Missing required argument: {}", #arg_name)
                        ))?;
                }
            } else {
                quote! {
                    let #arg_ident: Option<String> = prompt_args
                        .as_ref()
                        .and_then(|a| a.get(#arg_name))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
            }
        });

        // Generate call arguments (excluding ctx which is passed separately)
        let call_args = prompt.arguments.iter().map(|arg| {
            let arg_ident = syn::Ident::new(&arg.name, proc_macro2::Span::call_site());
            quote! { #arg_ident }
        });

        if prompt.arguments.is_empty() {
            quote! {
                #prompt_name => {
                    let result = self.#fn_name(&ctx).await;
                    Ok(::turbomcp::__macro_support::turbomcp_types::IntoPromptResult::into_prompt_result(result))
                }
            }
        } else {
            quote! {
                #prompt_name => {
                    #(#arg_extractions)*
                    let result = self.#fn_name(#(#call_args,)* &ctx).await;
                    Ok(::turbomcp::__macro_support::turbomcp_types::IntoPromptResult::into_prompt_result(result))
                }
            }
        }
    });

    quote! {
        // Keep the original impl block with handler attributes stripped
        #stripped_impl_block

        // Generate McpHandler implementation (unified v3 architecture)
        // Uses ::turbomcp::__macro_support:: paths so users don't need internal crates
        impl ::turbomcp::__macro_support::turbomcp_core::handler::McpHandler for #struct_name {
            fn server_info(&self) -> ::turbomcp::__macro_support::turbomcp_types::ServerInfo {
                ::turbomcp::__macro_support::turbomcp_types::ServerInfo::new(#name, #version)
                    #description_code
            }

            fn list_tools(&self) -> Vec<::turbomcp::__macro_support::turbomcp_types::Tool> {
                vec![#(#tool_list_code),*]
            }

            fn list_resources(&self) -> Vec<::turbomcp::__macro_support::turbomcp_types::Resource> {
                vec![#(#resource_list_code),*]
            }

            fn list_prompts(&self) -> Vec<::turbomcp::__macro_support::turbomcp_types::Prompt> {
                vec![#(#prompt_list_code),*]
            }

            fn call_tool<'a>(
                &'a self,
                name: &'a str,
                args: ::turbomcp::__macro_support::serde_json::Value,
                ctx: &'a ::turbomcp::__macro_support::turbomcp_core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = ::turbomcp::__macro_support::turbomcp_core::error::McpResult<::turbomcp::__macro_support::turbomcp_types::ToolResult>> + ::turbomcp::__macro_support::turbomcp_core::marker::MaybeSend + 'a {
                let name = name.to_string();
                async move {
                    let args = args.as_object().cloned().unwrap_or_default();
                    match name.as_str() {
                        #(#tool_dispatch_code)*
                        _ => Err(::turbomcp::__macro_support::turbomcp_core::error::McpError::tool_not_found(&name))
                    }
                }
            }

            fn read_resource<'a>(
                &'a self,
                uri: &'a str,
                ctx: &'a ::turbomcp::__macro_support::turbomcp_core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = ::turbomcp::__macro_support::turbomcp_core::error::McpResult<::turbomcp::__macro_support::turbomcp_types::ResourceResult>> + ::turbomcp::__macro_support::turbomcp_core::marker::MaybeSend + 'a {
                let uri = uri.to_string();
                async move {
                    // Security: Validate URI length to prevent DoS
                    if uri.len() > ::turbomcp::__macro_support::turbomcp_core::DEFAULT_MAX_URI_LENGTH {
                        return Err(::turbomcp::__macro_support::turbomcp_core::error::McpError::invalid_params(
                            format!("URI too long: {} bytes (max: {})", uri.len(), ::turbomcp::__macro_support::turbomcp_core::DEFAULT_MAX_URI_LENGTH)
                        ));
                    }

                    // Security: Validate URI scheme against allowlist
                    if let Err(e) = ::turbomcp::__macro_support::turbomcp_core::validate_uri_scheme(&uri) {
                        return Err(::turbomcp::__macro_support::turbomcp_core::error::McpError::security(
                            format!("URI scheme validation failed: {}", e)
                        ));
                    }

                    #(#resource_dispatch_code)*
                    Err(::turbomcp::__macro_support::turbomcp_core::error::McpError::resource_not_found(&uri))
                }
            }

            fn get_prompt<'a>(
                &'a self,
                name: &'a str,
                args: Option<::turbomcp::__macro_support::serde_json::Value>,
                ctx: &'a ::turbomcp::__macro_support::turbomcp_core::context::RequestContext,
            ) -> impl ::std::future::Future<Output = ::turbomcp::__macro_support::turbomcp_core::error::McpResult<::turbomcp::__macro_support::turbomcp_types::PromptResult>> + ::turbomcp::__macro_support::turbomcp_core::marker::MaybeSend + 'a {
                let name = name.to_string();
                // HIGH-002: Convert args to Map for argument extraction
                let prompt_args = args.and_then(|v| v.as_object().cloned());
                async move {
                    match name.as_str() {
                        #(#prompt_dispatch_code)*
                        _ => Err(::turbomcp::__macro_support::turbomcp_core::error::McpError::prompt_not_found(&name))
                    }
                }
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

    let attrs = match ServerAttrs::parse(args) {
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
