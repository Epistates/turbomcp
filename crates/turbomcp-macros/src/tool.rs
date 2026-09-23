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
use syn::ext::IdentExt;
use syn::{FnArg, ItemFn, Pat, PatType, Signature, Type};

use crate::attrs::{CommonAttrs, parse_lit_bool, parse_marker_attrs};

/// Information about a tool handler method.
#[derive(Clone)]
pub struct ToolInfo {
    /// Tool name on the wire: the function name with any `r#` prefix removed.
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
    /// `TaskSupportLevel` variant for `execution.taskSupport`, if declared.
    pub task_support: Option<syn::Ident>,
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
    /// Parameter name on the wire: the identifier with any `r#` prefix removed,
    /// so `r#type` is the `type` argument a client actually sends.
    pub name: String,
    /// The identifier as written, which generated code must bind and pass.
    /// `Ident::new` rejects `r#type`, so it cannot be rebuilt from `name`.
    pub ident: syn::Ident,
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
    /// Keys every handler marker shares (`description`, `tags`, ...).
    pub common: CommonAttrs,
    /// `ToolAnnotations` boolean hints.
    pub annotations: ToolAnnotationFlags,
    /// Output-schema source type (`output_schema = MyType`).
    pub output_schema: Option<Type>,
    /// `task_support = "..."`: the `TaskSupportLevel` variant to advertise
    /// as `execution.taskSupport`.
    pub task_support: Option<syn::Ident>,
}

/// Keys only `#[tool]` accepts, on top of [`CommonAttrs`].
const TOOL_KEYS: &[&str] = &[
    "read_only",
    "destructive",
    "idempotent",
    "open_world",
    "output_schema",
    "task_support",
];

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
            attrs.common.description = Some(lit.value());
            return Ok(attrs);
        }

        let Self {
            common,
            annotations,
            output_schema,
            task_support,
        } = &mut attrs;
        *common = parse_marker_attrs(meta_list.tokens.clone(), "tool", TOOL_KEYS, |meta| {
            if meta.path.is_ident("read_only") {
                annotations.read_only = Some(parse_lit_bool(meta)?);
            } else if meta.path.is_ident("destructive") {
                annotations.destructive = Some(parse_lit_bool(meta)?);
            } else if meta.path.is_ident("idempotent") {
                annotations.idempotent = Some(parse_lit_bool(meta)?);
            } else if meta.path.is_ident("open_world") {
                annotations.open_world = Some(parse_lit_bool(meta)?);
            } else if meta.path.is_ident("output_schema") {
                // `output_schema = SomeType` — accept any syn::Type so generics
                // and qualified paths work.
                *output_schema = Some(meta.value()?.parse::<Type>()?);
            } else if meta.path.is_ident("task_support") {
                let value: syn::LitStr = meta.value()?.parse()?;
                let variant = match value.value().as_str() {
                    "forbidden" => "Forbidden",
                    "optional" => "Optional",
                    "required" => "Required",
                    other => {
                        return Err(syn::Error::new_spanned(
                            &value,
                            format!(
                                "unknown task_support `{other}`; expected \"forbidden\", \
                                 \"optional\", or \"required\""
                            ),
                        ));
                    }
                };
                *task_support = Some(syn::Ident::new(variant, value.span()));
            } else {
                return Ok(false);
            }
            Ok(true)
        })?;

        Ok(attrs)
    }
}

impl ToolInfo {
    /// Extract tool info from a function.
    pub fn from_fn(item: &ItemFn, attrs: ToolAttrs) -> Result<Self, syn::Error> {
        let name = item.sig.ident.unraw().to_string();

        // An explicit description wins over the doc comment.
        let doc_description = extract_doc_comments(&item.attrs);
        let description = attrs
            .common
            .description
            .or(doc_description)
            .unwrap_or_default();

        // Analyze parameters
        let parameters = analyze_parameters(&item.sig)?;

        Ok(Self {
            name,
            description,
            sig: item.sig.clone(),
            parameters,
            tags: attrs.common.tags,
            version: attrs.common.version,
            title: attrs.common.title,
            icons: attrs.common.icons,
            annotations: attrs.annotations,
            // An explicit `output_schema = T` always wins; otherwise infer it
            // from a `Json<T>` return, which is the wrapper whose whole purpose
            // is typed output.
            output_schema: attrs
                .output_schema
                .or_else(|| infer_output_schema_type(&item.sig)),
            task_support: attrs.task_support,
        })
    }
}

/// Infer the output-schema source type from a handler that returns `Json<T>`.
///
/// Recognises `Json<T>`, `McpResult<Json<T>>`, and `Result<Json<T>, E>`, which
/// covers how the wrapper is actually written. Any other return type yields
/// `None`: a tool that returns a bare `String` has no schema to advertise, and
/// guessing one would be worse than staying silent.
///
/// The declaration is still gated at runtime on the schema describing an object
/// (see `generate_output_schema_code`), because the spec requires a tool that
/// declares `outputSchema` to return conforming `structuredContent`, and
/// `structuredContent` is typed `{ [key: string]: unknown }` in every wire this
/// SDK speaks. Declaring a schema for a `Json<Vec<_>>` would promise something
/// the result is forbidden to deliver.
fn infer_output_schema_type(sig: &Signature) -> Option<Type> {
    let syn::ReturnType::Type(_, ty) = &sig.output else {
        return None;
    };
    json_payload_type(ty)
}

/// Unwrap one `Result`/`McpResult` layer, then match `Json<T>` and return `T`.
fn json_payload_type(ty: &Type) -> Option<Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    let segment = type_path.path.segments.last()?;

    let first_generic = |seg: &syn::PathSegment| -> Option<Type> {
        let syn::PathArguments::AngleBracketed(args) = &seg.arguments else {
            return None;
        };
        args.args.iter().find_map(|arg| match arg {
            syn::GenericArgument::Type(t) => Some(t.clone()),
            _ => None,
        })
    };

    if segment.ident == "Json" {
        return first_generic(segment);
    }

    // `McpResult<Json<T>>` / `Result<Json<T>, E>` — recurse into the Ok type.
    if segment.ident == "McpResult" || segment.ident == "Result" {
        let ok_ty = first_generic(segment)?;
        return json_payload_type(&ok_ty);
    }

    None
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
                    let param_name = pat_ident.ident.unraw().to_string();

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
                        ident: pat_ident.ident.clone(),
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

/// The arguments map as the generated dispatch code binds it.
///
/// Every handler parameter is bound under the user's own name in the same
/// scope as this map, so a plain `args` would be shadowed by a parameter called
/// `args` — and every parameter extracted after it would then read from the
/// wrong value. A reserved name keeps the two apart.
pub fn args_ident() -> syn::Ident {
    syn::Ident::new("__turbomcp_args", proc_macro2::Span::call_site())
}

/// The `&RequestContext` as the generated dispatch code binds it, reserved for
/// the same reason as [`args_ident`]: a tool parameter called `ctx` would
/// otherwise shadow the context that a `&RequestContext` parameter is handed.
pub fn ctx_ident() -> syn::Ident {
    syn::Ident::new("__turbomcp_ctx", proc_macro2::Span::call_site())
}

/// Generate parameter extraction code with size validation.
///
/// This includes security checks to prevent DoS attacks via oversized parameters.
/// The `krate` parameter is the resolved path to the turbomcp crate.
///
/// Reads the arguments map bound as [`args_ident`].
pub fn generate_extraction_code(parameters: &[ParameterInfo], krate: &TokenStream) -> TokenStream {
    if parameters.is_empty() {
        return quote! {};
    }

    let args = args_ident();

    // Add parameter count validation at the start
    let param_count = parameters.len();
    let mut extraction = quote! {
        // Validate parameter count (defense against parameter pollution)
        if #args.len() > #param_count + 10 {
            return Err(#krate::__macro_support::turbomcp_core::error::McpError::invalid_params(
                format!("Too many parameters: got {}, expected at most {}", #args.len(), #param_count)
            ));
        }
    };

    for param in parameters {
        let name_str = &param.name;
        let name_ident = &param.ident;
        let ty = &param.ty;

        // Generate size check code
        let size_check = quote! {
            // Security: Validate parameter size before deserialization
            if let Some(v) = #args.get(#name_str) {
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
                let #name_ident: #ty = match #args.get(#name_str) {
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
                let #name_ident: #ty = #args
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

/// Generate `Tool.execution` as `Option<ToolExecution>` from `task_support`.
///
/// This is declaration only. Clients may attempt task augmentation only when
/// the server also advertises `tasks.requests.tools.call`, which a
/// `#[server]` does not; the key exists so the catalogue can say what the spec
/// lets a tool say about itself.
pub fn generate_execution_code(
    task_support: &Option<syn::Ident>,
    krate: &TokenStream,
) -> TokenStream {
    let Some(level) = task_support else {
        return quote! { None };
    };
    quote! {
        Some(#krate::__macro_support::turbomcp_types::ToolExecution {
            task_support: Some(#krate::__macro_support::turbomcp_types::TaskSupportLevel::#level),
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
            // Declaring `outputSchema` obliges the tool to return conforming
            // `structuredContent`, and that field is typed
            // `{ [key: string]: unknown }` in every wire this SDK speaks. A
            // schema for a non-object payload could therefore never be
            // satisfied, so it is not advertised — matching the same
            // object test that decides whether `structuredContent` is
            // populated at all, so the promise and the payload cannot diverge.
            if value.get("type").and_then(|t| t.as_str()) == Some("object") {
                Some(#krate::__macro_support::turbomcp_types::ToolOutputSchema::from_value(value))
            } else {
                None
            }
        }
    }
}

/// Generate call arguments.
pub fn generate_call_args(sig: &Signature) -> TokenStream {
    let mut args = Vec::new();
    let ctx = ctx_ident();

    for input in &sig.inputs {
        match input {
            FnArg::Receiver(_) => continue,
            FnArg::Typed(PatType { pat, ty, .. }) => {
                if let Pat::Ident(pat_ident) = pat.as_ref() {
                    if is_context_type(ty) {
                        args.push(quote! { #ctx });
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
