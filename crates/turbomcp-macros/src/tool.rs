//! v3 tool macro - generates tool metadata with parameter parsing from function signature.
//!
//! # Per-Parameter Documentation
//!
//! The v3 macro system supports per-parameter documentation via the `#[description]` attribute:
//!
//! ```rust,ignore
//! #[tool]
//! async fn greet(
//!     #[description("The name of the person to greet")]
//!     name: String,
//!     #[description("Optional greeting prefix")]
//!     prefix: Option<String>,
//! ) -> String {
//!     // ...
//! }
//! ```
//!
//! This generates JSON Schema with parameter descriptions:
//!
//! ```json
//! {
//!   "type": "object",
//!   "properties": {
//!     "name": { "type": "string", "description": "The name of the person to greet" },
//!     "prefix": { "type": "string", "description": "Optional greeting prefix" }
//!   },
//!   "required": ["name"]
//! }
//! ```
//!
//! # Complex Type Support
//!
//! For complex types that implement `schemars::JsonSchema`, the macro automatically
//! uses the schemars-generated schema. This enables rich nested object schemas:
//!
//! ```rust,ignore
//! use schemars::JsonSchema;
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, JsonSchema)]
//! struct SearchParams {
//!     /// The search query
//!     query: String,
//!     /// Maximum results to return
//!     limit: Option<i32>,
//! }
//!
//! #[tool]
//! async fn search(params: SearchParams) -> Vec<Result> {
//!     // schemars generates the full schema with nested documentation
//! }
//! ```

use proc_macro2::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, Pat, PatType, Signature, Type};

/// Information about a tool handler method.
#[derive(Clone)]
pub struct ToolInfo {
    /// Tool name (from function name)
    pub name: String,
    /// Tool description (from doc comments or attribute)
    pub description: String,
    /// Function signature
    pub sig: Signature,
    /// Parameters extracted from signature
    pub parameters: Vec<ParameterInfo>,
    /// Tags for categorization (e.g., ["admin", "dangerous"])
    pub tags: Vec<String>,
    /// Version string (e.g., "2.0.0")
    pub version: Option<String>,
    /// Human-readable title (SEP-973 / MCP 2025-11-25 BaseMetadata.title).
    pub title: Option<String>,
    /// Icon URIs for the tool (SEP-973). Each entry becomes an `Icon { src, .. }`.
    pub icons: Vec<String>,
    /// Tool annotation hints (MCP `ToolAnnotations`).
    pub annotations: ToolAnnotationFlags,
    /// Optional output-schema source type. The macro emits
    /// `schemars::schema_for!(ty)` and stores the result as `Tool.outputSchema`.
    pub output_schema: Option<Type>,
}

/// Boolean hints copied verbatim into `ToolAnnotations`.
#[derive(Clone, Default)]
pub struct ToolAnnotationFlags {
    pub read_only: Option<bool>,
    pub destructive: Option<bool>,
    pub idempotent: Option<bool>,
    pub open_world: Option<bool>,
}

impl ToolAnnotationFlags {
    pub fn is_empty(&self) -> bool {
        self.read_only.is_none()
            && self.destructive.is_none()
            && self.idempotent.is_none()
            && self.open_world.is_none()
    }
}

/// Information about a function parameter.
#[derive(Clone)]
pub struct ParameterInfo {
    /// Parameter name
    pub name: String,
    /// Parameter type
    pub ty: Type,
    /// Parameter description (from doc comments or #[description] attribute)
    pub description: Option<String>,
    /// Whether this is an optional parameter
    pub is_optional: bool,
}

/// Parsed attributes from the #[tool(...)] macro.
#[derive(Default)]
pub struct ToolAttrs {
    /// Tool description
    pub description: Option<String>,
    /// Tags for categorization
    pub tags: Vec<String>,
    /// Version string
    pub version: Option<String>,
    /// Human-readable title (SEP-973).
    pub title: Option<String>,
    /// Icon URIs (SEP-973). Plain string array; each entry becomes an `Icon`.
    pub icons: Vec<String>,
    /// `ToolAnnotations` boolean hints.
    pub annotations: ToolAnnotationFlags,
    /// Output-schema source type (`output_schema = MyType`).
    pub output_schema: Option<Type>,
}

impl ToolAttrs {
    /// Parse tool attributes from a syn::Attribute.
    ///
    /// Supports multiple formats:
    /// - `#[tool]` - no attributes
    /// - `#[tool("description")]` - just description
    /// - `#[tool(description = "desc", tags = ["a", "b"], version = "1.0")]` - full syntax
    pub fn parse(attr: &syn::Attribute) -> Result<Self, syn::Error> {
        let mut attrs = Self::default();

        // Handle empty #[tool]
        let syn::Meta::List(meta_list) = &attr.meta else {
            return Ok(attrs);
        };

        // Handle #[tool("description")] shorthand
        if let Ok(lit) = syn::parse2::<syn::LitStr>(meta_list.tokens.clone()) {
            attrs.description = Some(lit.value());
            return Ok(attrs);
        }

        // Parse #[tool(description = "...", tags = [...], version = "...", ...)]
        let parser = syn::meta::parser(|meta| {
            if meta.path.is_ident("description") {
                let value: syn::LitStr = meta.value()?.parse()?;
                attrs.description = Some(value.value());
            } else if meta.path.is_ident("tags") {
                // Parse tags = ["a", "b", "c"]
                attrs.tags = parse_lit_str_array(&meta)?;
            } else if meta.path.is_ident("version") {
                let value: syn::LitStr = meta.value()?.parse()?;
                attrs.version = Some(value.value());
            } else if meta.path.is_ident("title") {
                let value: syn::LitStr = meta.value()?.parse()?;
                attrs.title = Some(value.value());
            } else if meta.path.is_ident("icons") {
                // Surface only `src` from the attribute. The MCP `Icon` schema
                // also carries mimeType / sizes / theme; users wanting richer
                // icons can construct them via the runtime builder.
                attrs.icons = parse_lit_str_array(&meta)?;
            } else if meta.path.is_ident("read_only") {
                attrs.annotations.read_only = Some(meta.value()?.parse::<syn::LitBool>()?.value);
            } else if meta.path.is_ident("destructive") {
                attrs.annotations.destructive = Some(meta.value()?.parse::<syn::LitBool>()?.value);
            } else if meta.path.is_ident("idempotent") {
                attrs.annotations.idempotent = Some(meta.value()?.parse::<syn::LitBool>()?.value);
            } else if meta.path.is_ident("open_world") {
                attrs.annotations.open_world = Some(meta.value()?.parse::<syn::LitBool>()?.value);
            } else if meta.path.is_ident("output_schema") {
                // `output_schema = SomeType` — accept any syn::Type so generics
                // and qualified paths work.
                attrs.output_schema = Some(meta.value()?.parse::<Type>()?);
            } else {
                // Unknown key — surface a clear compile-time error instead of
                // silently dropping it. A typo like `descriptio = "..."` would
                // previously parse, leaving the resulting tool with the default
                // description and no diagnostic.
                let key = meta
                    .path
                    .get_ident()
                    .map(|i| i.to_string())
                    .unwrap_or_else(|| "<unknown>".to_string());
                return Err(meta.error(format!(
                    "unknown #[tool] attribute key `{key}`; expected one of `description`, `tags`, `version`, `title`, `icons`, `read_only`, `destructive`, `idempotent`, `open_world`, `output_schema`",
                )));
            }
            Ok(())
        });

        // Try to parse, but if it fails with the nested parser, try an alternative
        if syn::parse::Parser::parse2(parser, meta_list.tokens.clone()).is_err() {
            // Alternative: parse comma-separated items including array literals
            attrs = Self::parse_alternative(&meta_list.tokens)?;
        }

        Ok(attrs)
    }

    /// Alternative parser for complex attribute syntax.
    ///
    /// Used as a fallback when the `syn::meta::parser` path fails. Handles
    /// scalar string keys, the `tags`/`icons` array forms, and the boolean
    /// hints. `output_schema` (a `Type`) is intentionally unsupported here —
    /// stringly extracting a Rust type is brittle, and the primary parser
    /// already covers the realistic syntax.
    fn parse_alternative(tokens: &proc_macro2::TokenStream) -> Result<Self, syn::Error> {
        let mut attrs = Self::default();
        let token_str = tokens.to_string();

        attrs.description = parse_quoted_value(&token_str, "description");
        attrs.version = parse_quoted_value(&token_str, "version");
        attrs.title = parse_quoted_value(&token_str, "title");
        attrs.tags = parse_string_array(&token_str, "tags");
        attrs.icons = parse_string_array(&token_str, "icons");
        attrs.annotations.read_only = parse_bool_value(&token_str, "read_only");
        attrs.annotations.destructive = parse_bool_value(&token_str, "destructive");
        attrs.annotations.idempotent = parse_bool_value(&token_str, "idempotent");
        attrs.annotations.open_world = parse_bool_value(&token_str, "open_world");

        Ok(attrs)
    }
}

/// Parse `["a", "b", ...]` from a `syn::meta::ParseNestedMeta` value position.
///
/// Used by the primary `syn::meta::parser` for array-valued attribute keys
/// (`tags`, `icons`). Without this the meta parser would bail on the bracketed
/// value and the alternative string-based parser would take over — losing any
/// attrs (like `output_schema = Type`) that only the primary parser supports.
fn parse_lit_str_array(meta: &syn::meta::ParseNestedMeta<'_>) -> Result<Vec<String>, syn::Error> {
    let value = meta.value()?;
    let arr;
    syn::bracketed!(arr in value);
    let parsed: syn::punctuated::Punctuated<syn::LitStr, syn::Token![,]> =
        syn::punctuated::Punctuated::parse_terminated(&arr)?;
    Ok(parsed.into_iter().map(|s| s.value()).collect())
}

/// Parse a `key = "value"` pattern from a stringified token stream.
///
/// Fallback for complex attribute syntax when standard parsing fails. Walks
/// the token stream looking for the bare ident `key`, an `=` punct, and a
/// string literal — this avoids substring matches inside other identifiers
/// or string values (e.g. a description containing the word `version` would
/// previously poison the lookup).
pub fn parse_quoted_value(token_str: &str, key: &str) -> Option<String> {
    let tokens = syn::parse_str::<proc_macro2::TokenStream>(token_str).ok()?;
    let mut iter = tokens.into_iter().peekable();

    while let Some(token) = iter.next() {
        let proc_macro2::TokenTree::Ident(ident) = &token else {
            continue;
        };
        if ident != key {
            continue;
        }
        // Expect `=` punct next.
        let Some(proc_macro2::TokenTree::Punct(p)) = iter.next() else {
            continue;
        };
        if p.as_char() != '=' {
            continue;
        }
        // Expect a string literal next.
        let Some(proc_macro2::TokenTree::Literal(lit)) = iter.next() else {
            continue;
        };
        // syn parses `Literal` -> `LitStr` to safely unquote and unescape.
        if let Ok(s) = syn::parse_str::<syn::LitStr>(&lit.to_string()) {
            return Some(s.value());
        }
    }

    None
}

/// Parse `key = ["a", "b", "c"]` pattern from a stringified token stream.
///
/// Fallback for complex attribute syntax when standard parsing fails. Walks
/// tokens to find the `key` ident, an `=` punct, and a bracketed group, then
/// extracts the string literals inside. This avoids substring collisions —
/// for example, a description containing the literal `key` text or `[`
/// would previously break the parser.
pub fn parse_string_array(token_str: &str, key: &str) -> Vec<String> {
    let Ok(tokens) = syn::parse_str::<proc_macro2::TokenStream>(token_str) else {
        return Vec::new();
    };
    let mut iter = tokens.into_iter();

    while let Some(token) = iter.next() {
        let proc_macro2::TokenTree::Ident(ident) = &token else {
            continue;
        };
        if ident != key {
            continue;
        }
        let Some(proc_macro2::TokenTree::Punct(p)) = iter.next() else {
            continue;
        };
        if p.as_char() != '=' {
            continue;
        }
        let Some(proc_macro2::TokenTree::Group(group)) = iter.next() else {
            continue;
        };
        if group.delimiter() != proc_macro2::Delimiter::Bracket {
            continue;
        }

        return group
            .stream()
            .into_iter()
            .filter_map(|tt| {
                if let proc_macro2::TokenTree::Literal(lit) = tt {
                    syn::parse_str::<syn::LitStr>(&lit.to_string())
                        .ok()
                        .map(|s| s.value())
                } else {
                    None
                }
            })
            .collect();
    }

    Vec::new()
}

/// Back-compat alias: `tags = [...]`.
pub fn parse_tags_array(token_str: &str) -> Vec<String> {
    parse_string_array(token_str, "tags")
}

/// Parse `key = true|false` from a stringified token stream.
///
/// Used by the alternative attribute parser. Returns `None` if the key is
/// absent, the value is malformed, or the literal isn't a boolean.
pub fn parse_bool_value(token_str: &str, key: &str) -> Option<bool> {
    let tokens = syn::parse_str::<proc_macro2::TokenStream>(token_str).ok()?;
    let mut iter = tokens.into_iter();

    while let Some(token) = iter.next() {
        let proc_macro2::TokenTree::Ident(ident) = &token else {
            continue;
        };
        if ident != key {
            continue;
        }
        let Some(proc_macro2::TokenTree::Punct(p)) = iter.next() else {
            continue;
        };
        if p.as_char() != '=' {
            continue;
        }
        let Some(next) = iter.next() else {
            continue;
        };
        // `true` / `false` arrive as `Ident`s, not `Literal`s.
        return match next {
            proc_macro2::TokenTree::Ident(b) if b == "true" => Some(true),
            proc_macro2::TokenTree::Ident(b) if b == "false" => Some(false),
            _ => None,
        };
    }

    None
}

impl ToolInfo {
    /// Extract tool info from a function.
    pub fn from_fn(item: &ItemFn, attrs: ToolAttrs) -> Result<Self, syn::Error> {
        let name = item.sig.ident.to_string();

        // Get description from doc comments or attribute
        let doc_description = extract_doc_comments(&item.attrs);
        let description = attrs.description.or(doc_description).unwrap_or_default();

        // Analyze parameters
        let parameters = analyze_parameters(&item.sig)?;

        Ok(Self {
            name,
            description,
            sig: item.sig.clone(),
            parameters,
            tags: attrs.tags,
            version: attrs.version,
            title: attrs.title,
            icons: attrs.icons,
            annotations: attrs.annotations,
            output_schema: attrs.output_schema,
        })
    }
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

/// Analyze function parameters.
fn analyze_parameters(sig: &Signature) -> Result<Vec<ParameterInfo>, syn::Error> {
    let mut parameters = Vec::new();

    for input in &sig.inputs {
        match input {
            FnArg::Receiver(_) => {
                // Skip self parameter
                continue;
            }
            FnArg::Typed(PatType { pat, ty, attrs, .. }) => {
                if let Pat::Ident(pat_ident) = pat.as_ref() {
                    let param_name = pat_ident.ident.to_string();

                    // Skip context parameters
                    if is_context_type(ty) {
                        continue;
                    }

                    // Check for #[description("...")] attribute first, then fall back to doc comments
                    let description =
                        extract_description_attr(attrs).or_else(|| extract_doc_comments(attrs));
                    let is_optional = is_option_type(ty);

                    parameters.push(ParameterInfo {
                        name: param_name,
                        ty: (**ty).clone(),
                        description,
                        is_optional,
                    });
                }
            }
        }
    }

    Ok(parameters)
}

/// Extract description from #[description("...")] attribute.
fn extract_description_attr(attrs: &[syn::Attribute]) -> Option<String> {
    for attr in attrs {
        if attr.path().is_ident("description") {
            // Handle #[description("text")] - List style
            if let syn::Meta::List(meta_list) = &attr.meta
                && let Ok(lit) = syn::parse2::<syn::LitStr>(meta_list.tokens.clone())
            {
                return Some(lit.value());
            }
            // Handle #[description = "text"] - NameValue style
            if let syn::Meta::NameValue(meta_nv) = &attr.meta
                && let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(lit_str),
                    ..
                }) = &meta_nv.value
            {
                return Some(lit_str.value());
            }
        }
    }
    None
}

/// Check if a type is a context type (supports both owned and reference forms).
fn is_context_type(ty: &Type) -> bool {
    match ty {
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "Context" || seg.ident == "RequestContext"),
        Type::Reference(type_ref) => is_context_type(&type_ref.elem),
        _ => false,
    }
}

/// Check if a type is Option<T>.
fn is_option_type(ty: &Type) -> bool {
    if let Type::Path(type_path) = ty {
        type_path
            .path
            .segments
            .last()
            .is_some_and(|seg| seg.ident == "Option")
    } else {
        false
    }
}

/// Generate JSON schema code for tool parameters.
///
/// This function generates code that produces a `ToolInputSchema` at runtime.
/// All types use schemars for consistent, accurate schema generation.
///
/// Every parameter is rendered through one shared `SchemaGenerator`, so the
/// tool schema is a single JSON Schema document: each property is the
/// parameter type's own (inlined) schema, and every definition those types
/// pull in lands in one root `$defs` that the `#/$defs/...` pointers actually
/// resolve against. Running `schema_for!` per parameter instead produces a
/// *root* schema per property — `$schema`, a `title` holding the Rust type
/// name, and a private `$defs` nested one level too deep — which validating
/// clients reject. Sharing the generator also lets schemars disambiguate two
/// parameter types that share a name, and turns a self-reference into
/// `#/$defs/T` rather than `"$ref": "#"` (the tool schema, not the type).
///
/// The `krate` parameter is the resolved path to the turbomcp crate
/// (e.g., `::turbomcp` or `::turbomcp_server`).
pub fn generate_schema_code(parameters: &[ParameterInfo], krate: &TokenStream) -> TokenStream {
    if parameters.is_empty() {
        return quote! {
            #krate::__macro_support::turbomcp_types::ToolInputSchema::empty()
        };
    }

    let mut prop_code = Vec::new();
    let mut required_names = Vec::new();

    for param in parameters {
        let name = &param.name;
        let ty = &param.ty;

        // Generate the parameter's JSON Schema fragment via schemars.
        //
        // `JsonSchema::json_schema` (rather than `subschema_for`) keeps the
        // parameter's own type inlined — a bare struct or enum parameter stays
        // `{"type":"object", ...}` / `{"enum": [...]}` instead of collapsing to
        // a `$ref`, which naive clients don't follow. Nested types are still
        // registered on the shared generator and referenced by `$ref`.
        //
        // schemars sometimes emits a non-object schema (`true`/`false` for
        // types like `serde_json::Value`). The previous fallback collapsed
        // those to `{"type":"object"}`, which erased the parameter's actual
        // type from the tool input schema and made LLM clients send
        // wrong-typed values. Now we wrap a non-object schema as a single-key
        // object so the schema correctly describes the property.
        let schema_code = quote! {
            {
                let schema = <#ty as #krate::__macro_support::schemars::JsonSchema>::json_schema(&mut generator);
                match schema.to_value() {
                    #krate::__macro_support::serde_json::Value::Object(map) => map,
                    other => {
                        // Non-object schema (boolean schema). Treat it as an
                        // inline schema fragment by wrapping in an object whose
                        // only entry is the actual schema. JSON Schema permits a
                        // sub-schema to be any JSON value; placing it under
                        // `allOf` keeps validators happy and preserves the type
                        // information that would otherwise be lost.
                        let mut m = #krate::__macro_support::serde_json::Map::new();
                        m.insert(
                            "allOf".to_string(),
                            #krate::__macro_support::serde_json::Value::Array(vec![other]),
                        );
                        m
                    }
                }
            }
        };

        let description_code = if let Some(desc) = &param.description {
            quote! {
                prop.insert("description".to_string(), #krate::__macro_support::serde_json::Value::String(#desc.to_string()));
            }
        } else {
            quote! {}
        };

        prop_code.push(quote! {
            {
                let mut prop = #schema_code;
                #description_code
                properties.insert(#name.to_string(), #krate::__macro_support::serde_json::Value::Object(prop));
            }
        });

        if !param.is_optional {
            required_names.push(name.clone());
        }
    }

    quote! {
        {
            // Pinned to the dialect the root `$schema` below declares, with
            // definitions at `#/$defs`. (`schema_for!` uses the crate default,
            // which schemars documents as liable to change.)
            let mut generator = #krate::__macro_support::schemars::SchemaGenerator::new(
                #krate::__macro_support::schemars::generate::SchemaSettings::draft2020_12(),
            );
            let mut properties = #krate::__macro_support::serde_json::Map::new();
            #(#prop_code)*

            let required: Vec<String> = vec![#(#required_names.to_string()),*];

            // SEP-1613: declare JSON Schema 2020-12 dialect on every macro-built
            // tool schema. Without this, generated `inputSchema` JSON omits
            // `$schema` and clients have to guess the dialect.
            let mut extras = ::std::collections::HashMap::new();
            extras.insert(
                "$schema".to_string(),
                #krate::__macro_support::serde_json::Value::String(
                    #krate::__macro_support::turbomcp_types::JSON_SCHEMA_DIALECT_2020_12.to_string(),
                ),
            );

            // Every definition the parameter types pulled in, at the root
            // where their `#/$defs/...` pointers resolve. Omitted entirely
            // when nothing was registered.
            let definitions = generator.take_definitions(true);
            if !definitions.is_empty() {
                extras.insert(
                    "$defs".to_string(),
                    #krate::__macro_support::serde_json::Value::Object(definitions),
                );
            }

            #krate::__macro_support::turbomcp_types::ToolInputSchema {
                schema_type: Some("object".into()),
                properties: Some(#krate::__macro_support::serde_json::Value::Object(properties)),
                required: if required.is_empty() { None } else { Some(required) },
                additional_properties: Some(false.into()),
                extra_keywords: extras,
            }
        }
    }
}

/// Maximum size for a single parameter value (1MB)
const MAX_PARAM_VALUE_SIZE: usize = 1024 * 1024;

/// Generate parameter extraction code with size validation.
///
/// This includes security checks to prevent DoS attacks via oversized parameters.
/// The `krate` parameter is the resolved path to the turbomcp crate.
pub fn generate_extraction_code(parameters: &[ParameterInfo], krate: &TokenStream) -> TokenStream {
    if parameters.is_empty() {
        return quote! {};
    }

    // Add parameter count validation at the start
    let param_count = parameters.len();
    let mut extraction = quote! {
        // Validate parameter count (defense against parameter pollution)
        if args.len() > #param_count + 10 {
            return Err(#krate::__macro_support::turbomcp_core::error::McpError::invalid_params(
                format!("Too many parameters: got {}, expected at most {}", args.len(), #param_count)
            ));
        }
    };

    for param in parameters {
        let name_str = &param.name;
        let name_ident = syn::Ident::new(&param.name, proc_macro2::Span::call_site());
        let ty = &param.ty;

        // Generate size check code
        let size_check = quote! {
            // Security: Validate parameter size before deserialization
            if let Some(v) = args.get(#name_str) {
                let size_estimate = v.to_string().len();
                if size_estimate > #MAX_PARAM_VALUE_SIZE {
                    return Err(#krate::__macro_support::turbomcp_core::error::McpError::invalid_params(
                        format!("Parameter '{}' exceeds maximum size ({} bytes)", #name_str, size_estimate)
                    ));
                }
            }
        };

        if param.is_optional {
            // For Option<T> parameters: distinguish "key absent" (legitimate None) from
            // "key present but malformed" (must surface as an invalid_params error).
            // The previous `.transpose().map_err(...)?.flatten()` chain quietly turned
            // a present-but-null value into None — but if the inner type was non-null
            // and deserialization failed, the error path actually fired correctly.
            // The subtle bug was different: `.flatten()` on `Option<Option<T>>` collapses
            // a parsed `Some(None)` into None, hiding cases where the user explicitly
            // sent JSON `null` to indicate "use default". The new pattern preserves the
            // distinction by parsing the value as `Option<T>` directly.
            extraction.extend(quote! {
                #size_check
                let #name_ident: #ty = match args.get(#name_str) {
                    None => None,
                    Some(v) => {
                        #krate::__macro_support::serde_json::from_value::<#ty>(v.clone())
                            .map_err(|e| #krate::__macro_support::turbomcp_core::error::McpError::invalid_params(
                                format!("Invalid parameter '{}': {}", #name_str, e)
                            ))?
                    }
                };
            });
        } else {
            extraction.extend(quote! {
                #size_check
                let #name_ident: #ty = args
                    .get(#name_str)
                    .ok_or_else(|| #krate::__macro_support::turbomcp_core::error::McpError::invalid_params(
                        format!("Missing required parameter: {}", #name_str)
                    ))
                    .and_then(|v| #krate::__macro_support::serde_json::from_value(v.clone())
                        .map_err(|e| #krate::__macro_support::turbomcp_core::error::McpError::invalid_params(
                            format!("Invalid parameter '{}': {}", #name_str, e)
                        )))?;
            });
        }
    }

    extraction
}

/// Generate `Tool.icons` as `Option<Vec<Icon>>` from a list of source URIs.
///
/// Each entry becomes `Icon { src, .. Default::default() }`. Richer fields
/// (mimeType, sizes, theme) are reachable via the runtime builder if a user
/// needs them; the macro covers the 80% case.
pub fn generate_icons_code(icons: &[String], krate: &TokenStream) -> TokenStream {
    if icons.is_empty() {
        return quote! { None };
    }
    let icon_exprs = icons.iter().map(|src| {
        quote! {
            #krate::__macro_support::turbomcp_types::Icon {
                src: #src.to_string(),
                mime_type: None,
                sizes: None,
                theme: None,
            }
        }
    });
    quote! {
        Some(vec![#(#icon_exprs),*])
    }
}

/// Generate `Tool.annotations` as `Option<ToolAnnotations>`.
pub fn generate_annotations_code(
    annotations: &ToolAnnotationFlags,
    title: &Option<String>,
    krate: &TokenStream,
) -> TokenStream {
    if annotations.is_empty() && title.is_none() {
        return quote! { None };
    }
    let read_only = match annotations.read_only {
        Some(v) => quote! { Some(#v) },
        None => quote! { None },
    };
    let destructive = match annotations.destructive {
        Some(v) => quote! { Some(#v) },
        None => quote! { None },
    };
    let idempotent = match annotations.idempotent {
        Some(v) => quote! { Some(#v) },
        None => quote! { None },
    };
    let open_world = match annotations.open_world {
        Some(v) => quote! { Some(#v) },
        None => quote! { None },
    };
    let title_code = match title {
        Some(t) => quote! { Some(#t.to_string()) },
        None => quote! { None },
    };
    quote! {
        Some(#krate::__macro_support::turbomcp_types::ToolAnnotations {
            read_only_hint: #read_only,
            destructive_hint: #destructive,
            idempotent_hint: #idempotent,
            open_world_hint: #open_world,
            title: #title_code,
        })
    }
}

/// Generate `Tool.outputSchema` as `Option<ToolOutputSchema>`.
///
/// When `output_schema = MyType` is supplied, runs `schemars::schema_for!(MyType)`
/// at runtime and converts the result via `ToolOutputSchema::from_value`. When
/// the conversion can't produce an object schema, falls back to an empty
/// schema so the field stays well-typed without lying about the structure.
pub fn generate_output_schema_code(ty: &Option<Type>, krate: &TokenStream) -> TokenStream {
    let Some(ty) = ty else {
        return quote! { None };
    };
    quote! {
        {
            let schema = #krate::__macro_support::schemars::schema_for!(#ty);
            let value = #krate::__macro_support::serde_json::to_value(&schema)
                .unwrap_or(#krate::__macro_support::serde_json::Value::Null);
            Some(#krate::__macro_support::turbomcp_types::ToolOutputSchema::from_value(value))
        }
    }
}

/// Generate call arguments.
pub fn generate_call_args(sig: &Signature) -> TokenStream {
    let mut args = Vec::new();

    for input in &sig.inputs {
        match input {
            FnArg::Receiver(_) => continue,
            FnArg::Typed(PatType { pat, ty, .. }) => {
                if let Pat::Ident(pat_ident) = pat.as_ref() {
                    if is_context_type(ty) {
                        args.push(quote! { ctx });
                    } else {
                        let name = &pat_ident.ident;
                        args.push(quote! { #name });
                    }
                }
            }
        }
    }

    quote! { #(#args),* }
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn test_extract_doc_comments() {
        let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[doc = " This is a test"])];
        let doc = extract_doc_comments(&attrs);
        assert_eq!(doc, Some("This is a test".to_string()));
    }

    #[test]
    fn test_extract_description_attr_list_style() {
        // Test #[description("text")]
        let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[description("The name to greet")])];
        let desc = extract_description_attr(&attrs);
        assert_eq!(desc, Some("The name to greet".to_string()));
    }

    #[test]
    fn test_extract_description_attr_name_value_style() {
        // Test #[description = "text"]
        let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[description = "A value"])];
        let desc = extract_description_attr(&attrs);
        assert_eq!(desc, Some("A value".to_string()));
    }

    #[test]
    fn test_is_option_type() {
        let ty: Type = parse_quote!(Option<String>);
        assert!(is_option_type(&ty));

        let ty: Type = parse_quote!(String);
        assert!(!is_option_type(&ty));
    }

    #[test]
    fn test_is_context_type() {
        let ty: Type = parse_quote!(Context);
        assert!(is_context_type(&ty));

        let ty: Type = parse_quote!(RequestContext);
        assert!(is_context_type(&ty));

        let ty: Type = parse_quote!(&RequestContext);
        assert!(is_context_type(&ty));

        let ty: Type = parse_quote!(String);
        assert!(!is_context_type(&ty));
    }
}
