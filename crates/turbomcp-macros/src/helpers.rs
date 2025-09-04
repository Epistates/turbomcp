//! Helper macro implementations for TurboMCP content creation

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    Expr, Token,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

/// Parse format macro arguments (format string + arguments)
struct FormatArgs {
    format_string: Expr,
    args: Vec<Expr>,
}

impl Parse for FormatArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let format_string: Expr = input.parse()?;
        let mut args = Vec::new();

        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            if !input.is_empty() {
                args.push(input.parse()?);
            }
        }

        Ok(FormatArgs {
            format_string,
            args,
        })
    }
}

/// Generate text content helper with format string support
pub fn generate_text_content(input: TokenStream) -> TokenStream {
    let format_args = parse_macro_input!(input as FormatArgs);
    let format_string = &format_args.format_string;
    let args = &format_args.args;

    let expanded = quote! {
        ::turbomcp_protocol::types::ContentBlock::Text(::turbomcp_protocol::types::TextContent {
            text: format!(#format_string, #(#args),*),
            annotations: None,
            meta: None,
        })
    };

    TokenStream::from(expanded)
}

/// Generate MCP error helper with format string support
pub fn generate_error(input: TokenStream) -> TokenStream {
    let format_args = parse_macro_input!(input as FormatArgs);
    let format_string = &format_args.format_string;
    let args = &format_args.args;

    let expanded = quote! {
        ::turbomcp_core::Error::handler(format!(#format_string, #(#args),*))
    };

    TokenStream::from(expanded)
}

/// Tool result macro input parser
struct ToolResultInput {
    content: Vec<Expr>,
    is_error: Option<bool>,
}

impl Parse for ToolResultInput {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut content = Vec::new();
        let mut is_error = None;

        // Parse structured input: tool_result!(content = [expr1, expr2], is_error = false)
        // Or simple input: tool_result!(expr)

        if input.is_empty() {
            return Ok(ToolResultInput { content, is_error });
        }

        // Try to parse as structured assignment syntax first
        let lookahead = input.lookahead1();
        if lookahead.peek(syn::Ident) {
            // Check if this is parameter assignment (ident = ...)
            let fork = input.fork();
            let _ident: syn::Ident = fork.parse()?;

            if fork.peek(syn::Token![=]) {
                // This is parameter assignment syntax
                let ident: syn::Ident = input.parse()?;
                match ident.to_string().as_str() {
                    "content" => {
                        input.parse::<syn::Token![=]>()?;

                        if input.peek(syn::token::Bracket) {
                            // Parse array: [expr1, expr2, ...]
                            let content_array;
                            syn::bracketed!(content_array in input);

                            while !content_array.is_empty() {
                                let expr: Expr = content_array.parse()?;
                                content.push(expr);

                                if !content_array.is_empty() {
                                    content_array.parse::<syn::Token![,]>()?;
                                }
                            }
                        } else {
                            // Single expression
                            let expr: Expr = input.parse()?;
                            content.push(expr);
                        }

                        // Check for additional parameters
                        if input.peek(syn::Token![,]) {
                            input.parse::<syn::Token![,]>()?;

                            if input.peek(syn::Ident) {
                                let next_ident: syn::Ident = input.parse()?;
                                if next_ident == "is_error" {
                                    input.parse::<syn::Token![=]>()?;
                                    let error_expr: syn::LitBool = input.parse()?;
                                    is_error = Some(error_expr.value);
                                }
                            }
                        }
                    }
                    "is_error" => {
                        input.parse::<syn::Token![=]>()?;
                        let error_expr: syn::LitBool = input.parse()?;
                        is_error = Some(error_expr.value);

                        if input.peek(syn::Token![,]) {
                            input.parse::<syn::Token![,]>()?;
                            // Parse content after is_error
                            if input.peek(syn::Ident) {
                                let next_ident: syn::Ident = input.parse()?;
                                if next_ident == "content" {
                                    input.parse::<syn::Token![=]>()?;
                                    let expr: Expr = input.parse()?;
                                    content.push(expr);
                                }
                            }
                        }
                    }
                    _ => {
                        // Not a known parameter, treat as simple expression
                        // Reconstruct the ident as part of the expression
                        return Err(syn::Error::new(
                            ident.span(),
                            "Unknown parameter. Use 'content' or 'is_error'",
                        ));
                    }
                }
            } else {
                // This is just a variable/expression, parse as simple expression
                let expr: Expr = input.parse()?;
                content.push(expr);
            }
        } else {
            // Parse as simple expression: tool_result!(expr)
            let expr: Expr = input.parse()?;
            content.push(expr);
        }

        Ok(ToolResultInput { content, is_error })
    }
}

/// Generate tool result helper with content and error flag support
pub fn generate_tool_result(input: TokenStream) -> TokenStream {
    let input_parsed = if input.is_empty() {
        ToolResultInput {
            content: Vec::new(),
            is_error: Some(false),
        }
    } else {
        parse_macro_input!(input as ToolResultInput)
    };

    let content_items: Vec<TokenStream2> = input_parsed
        .content
        .iter()
        .map(|expr| {
            quote! { #expr }
        })
        .collect();

    let is_error = input_parsed.is_error.unwrap_or(false);

    let expanded = quote! {
        ::turbomcp_protocol::types::CallToolResult {
            content: vec![#(#content_items),*],
            is_error: Some(#is_error),
        }
    };

    TokenStream::from(expanded)
}

/// Parse elicit! macro arguments (context, message, schema)
struct ElicitArgs {
    context: Expr,
    message: Expr,
    schema: Expr,
}

impl Parse for ElicitArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let context: Expr = input.parse()?;
        input.parse::<Token![,]>()?;
        let message: Expr = input.parse()?;
        input.parse::<Token![,]>()?;
        let schema: Expr = input.parse()?;

        Ok(ElicitArgs {
            context,
            message,
            schema,
        })
    }
}

/// Generate elicitation helper with automatic request handling
pub fn generate_elicitation(input: TokenStream) -> TokenStream {
    let elicit_args = parse_macro_input!(input as ElicitArgs);
    let context = &elicit_args.context;
    let message = &elicit_args.message;
    let schema = &elicit_args.schema;

    let expanded = quote! {
        {
            use ::turbomcp_protocol::elicitation::{ElicitationCreateRequest};

            // Create the elicitation request
            let elicitation_request = ElicitationCreateRequest {
                message: #message.to_string(),
                requested_schema: #schema,
            };

            // Use server capabilities to send elicitation
            if let Some(capabilities) = #context.request.server_capabilities() {
                capabilities.send_elicitation(elicitation_request).await
            } else {
                Err(::turbomcp_core::Error::handler("Server capabilities not available for elicitation".to_string()).into())
            }
        }
    };

    TokenStream::from(expanded)
}
