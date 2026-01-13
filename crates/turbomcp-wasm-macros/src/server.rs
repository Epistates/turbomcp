//! WASM server macro implementation
//!
//! Generates code that uses the `turbomcp_wasm::wasm_server::McpServer` builder.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
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

/// Information about a tool method
struct ToolMethod {
    name: Ident,
    description: String,
    arg_type: Option<Type>,
}

/// Information about a resource method
struct ResourceMethod {
    name: Ident,
    uri_template: String,
}

/// Information about a prompt method
struct PromptMethod {
    name: Ident,
    description: String,
    has_args: bool,
    arg_type: Option<Type>,
}

/// Generate the WASM server implementation
pub fn generate_wasm_server(args: ServerArgs, mut impl_block: ItemImpl) -> Result<TokenStream2> {
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
            quote! { (#name, #desc) }
        })
        .collect();

    let resource_metadata: Vec<_> = resources
        .iter()
        .map(|r| {
            let uri = &r.uri_template;
            let name = r.name.to_string();
            quote! { (#uri, #name) }
        })
        .collect();

    let prompt_metadata: Vec<_> = prompts
        .iter()
        .map(|p| {
            let name = p.name.to_string();
            let desc = &p.description;
            quote! { (#name, #desc) }
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
            /// Create an MCP server from this implementation.
            ///
            /// This method builds a fully-configured `McpServer` with all registered
            /// tools, resources, and prompts.
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
            /// Returns a vector of (name, description) tuples.
            pub fn get_tools_metadata() -> Vec<(&'static str, &'static str)> {
                vec![#(#tool_metadata),*]
            }

            /// Get metadata for all registered resources.
            ///
            /// Returns a vector of (uri_template, name) tuples.
            pub fn get_resources_metadata() -> Vec<(&'static str, &'static str)> {
                vec![#(#resource_metadata),*]
            }

            /// Get metadata for all registered prompts.
            ///
            /// Returns a vector of (name, description) tuples.
            pub fn get_prompts_metadata() -> Vec<(&'static str, &'static str)> {
                vec![#(#prompt_metadata),*]
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
                    let description = parse_string_attr(attr).unwrap_or_else(|| "Tool".to_string());
                    let arg_type = extract_tool_arg_type(&method.sig);

                    tools.push(ToolMethod {
                        name: method.sig.ident.clone(),
                        description,
                        arg_type,
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
                    let uri_template =
                        parse_string_attr(attr).unwrap_or_else(|| "resource://".to_string());

                    resources.push(ResourceMethod {
                        name: method.sig.ident.clone(),
                        uri_template,
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
                    let description =
                        parse_string_attr(attr).unwrap_or_else(|| "Prompt".to_string());
                    let (has_args, arg_type) = extract_prompt_arg_info(&method.sig);

                    prompts.push(PromptMethod {
                        name: method.sig.ident.clone(),
                        description,
                        has_args,
                        arg_type,
                    });
                    break;
                }
            }
        }
    }

    prompts
}

/// Parse a string attribute value like #[tool("description")]
fn parse_string_attr(attr: &Attribute) -> Option<String> {
    // Try parsing as #[attr("value")]
    if let Ok(lit) = attr.parse_args::<LitStr>() {
        return Some(lit.value());
    }

    // Try parsing as #[attr(description = "value")]
    if let Ok(args) = attr.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) {
        for meta in args {
            if let Meta::NameValue(nv) = meta
                && nv.path.is_ident("description")
                && let Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) = &nv.value
            {
                return Some(s.value());
            }
        }
    }

    None
}

/// Extract the argument type from a tool method signature
fn extract_tool_arg_type(sig: &syn::Signature) -> Option<Type> {
    for input in &sig.inputs {
        if let FnArg::Typed(PatType { ty, .. }) = input {
            // Skip &self and Context types
            if !is_self_or_context(ty) {
                return Some((**ty).clone());
            }
        }
    }
    None
}

/// Extract argument info from a prompt method signature
fn extract_prompt_arg_info(sig: &syn::Signature) -> (bool, Option<Type>) {
    for input in &sig.inputs {
        if let FnArg::Typed(PatType { ty, pat, .. }) = input
            && !is_self_or_context(ty)
            && let Pat::Ident(pat_ident) = pat.as_ref()
            && pat_ident.ident != "self"
        {
            return (true, Some((**ty).clone()));
        }
    }
    (false, None)
}

/// Check if type is self or Context
fn is_self_or_context(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty
        && let Some(segment) = type_path.path.segments.last()
    {
        let name = segment.ident.to_string();
        return name == "Context" || name == "RequestContext";
    }
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

            if let Some(arg_type) = &tool.arg_type {
                // Tool with typed arguments
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
            } else {
                // Tool with no arguments
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

            if prompt.has_args {
                if let Some(arg_type) = &prompt.arg_type {
                    // Prompt with typed arguments
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
                } else {
                    quote! {}
                }
            } else {
                // Prompt with no arguments
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
}
