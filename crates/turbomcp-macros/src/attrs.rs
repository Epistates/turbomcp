//! Attribute parsing for TurboMCP macros
//!
//! This module provides robust, syn-based parsing for macro attributes,
//! following patterns from Serde, Clap, and other established Rust libraries.

use quote::quote;
use syn::{
    Expr, Ident, Lit, Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

/// A single root declaration in the server macro
#[derive(Debug, Clone)]
pub struct Root {
    pub uri: String,
    pub name: Option<String>,
}

impl Root {
    /// Parse from "uri:name" or just "uri" format
    /// Handles file:// URIs correctly by finding the last colon for the name separator
    pub fn from_str(s: &str) -> Self {
        // For file URIs, we need to be careful not to split on the protocol colon
        // Look for the last colon that could be a separator
        if s.starts_with("file://") || s.starts_with("http://") || s.starts_with("https://") {
            // Find the last colon in the string
            if let Some(last_colon) = s.rfind(':') {
                // Check if this colon is part of the protocol
                let before_colon = &s[..last_colon];
                if before_colon == "file"
                    || before_colon == "http"
                    || before_colon == "https"
                    || before_colon.ends_with("//")
                {
                    // This is the protocol colon, no name specified
                    Root {
                        uri: s.to_string(),
                        name: None,
                    }
                } else {
                    // This colon separates the URI from the name
                    Root {
                        uri: before_colon.to_string(),
                        name: Some(s[last_colon + 1..].to_string()),
                    }
                }
            } else {
                Root {
                    uri: s.to_string(),
                    name: None,
                }
            }
        } else {
            // For non-URI strings, use simple colon splitting
            if let Some(colon_pos) = s.find(':') {
                Root {
                    uri: s[..colon_pos].to_string(),
                    name: Some(s[colon_pos + 1..].to_string()),
                }
            } else {
                Root {
                    uri: s.to_string(),
                    name: None,
                }
            }
        }
    }
}

/// Protocol version preset for the server macro
#[derive(Debug, Clone, Default)]
pub enum ProtocolVersionPreset {
    /// Use latest spec (2025-11-25) with fallback enabled
    #[default]
    Latest,
    /// Use Claude Code compatible settings (prefer 2025-06-18)
    Compatible,
    /// Strict mode - only accept the specified version
    Strict(String),
    /// Custom version - prefer this version with fallback enabled
    Custom(String),
}

impl ProtocolVersionPreset {
    /// Parse from string value
    /// - "latest" -> Latest
    /// - "compatible" -> Compatible
    /// - "strict:2025-11-25" -> Strict("2025-11-25")
    /// - "2025-06-18" -> Custom("2025-06-18")
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "latest" => Self::Latest,
            "compatible" => Self::Compatible,
            s if s.starts_with("strict:") => Self::Strict(
                s.strip_prefix("strict:")
                    .unwrap_or("2025-11-25")
                    .to_string(),
            ),
            version => Self::Custom(version.to_string()),
        }
    }
}

/// Server macro attributes with syn-based parsing
#[derive(Debug, Default)]
pub struct ServerAttrs {
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub roots: Vec<Root>,
    /// Optional: explicitly specify which transports to generate code for
    /// If not specified, generates code for all enabled features
    /// Example: transports = ["http", "tcp"]
    pub transports: Option<Vec<String>>,
    /// Protocol version configuration preset
    /// - "latest" (default): 2025-11-25 with fallback
    /// - "compatible": 2025-06-18 for Claude Code compatibility
    /// - "strict:VERSION": Only accept specified version
    /// - "VERSION": Prefer specified version with fallback
    pub protocol_version: ProtocolVersionPreset,
}

impl ServerAttrs {
    /// Valid transport names that can be used in the transports attribute
    const VALID_TRANSPORTS: &'static [&'static str] =
        &["stdio", "http", "websocket", "tcp", "unix"];

    /// Parse from the macro attribute arguments
    /// Supports multiple syntaxes for maximum ergonomics:
    /// - name = "server-name"
    /// - version = "1.0.0"
    /// - description = "Server description"
    /// - root = "/path:Name"
    /// - root = "/another/path"
    /// - transports = ["http", "tcp"]
    pub fn from_args(args: proc_macro::TokenStream) -> syn::Result<Self> {
        let mut attrs = ServerAttrs::default();

        if args.is_empty() {
            return Ok(attrs);
        }

        // Parse as attribute arguments
        let parsed = syn::parse::<ServerAttrArgs>(args)?;

        for item in parsed.items {
            match item.name.to_string().as_str() {
                "name" => {
                    if let Some(value) = item.get_string_value() {
                        attrs.name = Some(value);
                    }
                }
                "version" => {
                    if let Some(value) = item.get_string_value() {
                        attrs.version = Some(value);
                    }
                }
                "description" => {
                    if let Some(value) = item.get_string_value() {
                        attrs.description = Some(value);
                    }
                }
                "root" => {
                    if let Some(value) = item.get_string_value() {
                        attrs.roots.push(Root::from_str(&value));
                    }
                }
                "transports" => {
                    if let Some(transports) = item.get_string_array_value() {
                        // Validate all transports
                        for transport in &transports {
                            if !Self::VALID_TRANSPORTS.contains(&transport.as_str()) {
                                return Err(syn::Error::new_spanned(
                                    &item.value,
                                    format!(
                                        "Invalid transport '{}'. Valid transports are: {}",
                                        transport,
                                        Self::VALID_TRANSPORTS.join(", ")
                                    ),
                                ));
                            }
                        }
                        attrs.transports = Some(transports);
                    }
                }
                "protocol_version" => {
                    if let Some(value) = item.get_string_value() {
                        attrs.protocol_version = ProtocolVersionPreset::from_str(&value);
                    }
                }
                _ => {
                    // Ignore unknown attributes for forward compatibility
                }
            }
        }

        Ok(attrs)
    }

    /// Generate the roots configuration code for the server builder
    pub fn generate_roots_config(&self) -> proc_macro2::TokenStream {
        if self.roots.is_empty() {
            return quote! {};
        }

        let root_configs: Vec<_> = self
            .roots
            .iter()
            .map(|root| {
                let uri = &root.uri;
                match &root.name {
                    Some(name) => quote! {
                        builder = builder.root(#uri, Some(#name.to_string()));
                    },
                    None => quote! {
                        builder = builder.root(#uri, None);
                    },
                }
            })
            .collect();

        quote! {
            #(#root_configs)*
        }
    }

    /// Generate the protocol version configuration code for the server builder
    pub fn generate_protocol_version_config(&self) -> proc_macro2::TokenStream {
        match &self.protocol_version {
            ProtocolVersionPreset::Latest => {
                quote! {
                    builder = builder.protocol_version_config(
                        ::turbomcp::ProtocolVersionConfig::latest()
                    );
                }
            }
            ProtocolVersionPreset::Compatible => {
                quote! {
                    builder = builder.protocol_version_config(
                        ::turbomcp::ProtocolVersionConfig::compatible()
                    );
                }
            }
            ProtocolVersionPreset::Strict(version) => {
                quote! {
                    builder = builder.protocol_version_config(
                        ::turbomcp::ProtocolVersionConfig::strict(#version)
                    );
                }
            }
            ProtocolVersionPreset::Custom(version) => {
                quote! {
                    builder = builder.protocol_version_config(
                        ::turbomcp::ProtocolVersionConfig::custom(
                            #version,
                            vec!["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"]
                        )
                    );
                }
            }
        }
    }
}

/// A single attribute item (name = value)
struct AttrItem {
    name: Ident,
    _eq: Token![=],
    value: Expr,
}

impl AttrItem {
    /// Get the string value if this is a string literal
    fn get_string_value(&self) -> Option<String> {
        match &self.value {
            Expr::Lit(lit) => match &lit.lit {
                Lit::Str(s) => Some(s.value()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Get array of string values if this is an array of string literals
    /// Example: ["http", "tcp"] → Some(vec!["http".to_string(), "tcp".to_string()])
    fn get_string_array_value(&self) -> Option<Vec<String>> {
        match &self.value {
            Expr::Array(arr) => {
                let mut values = Vec::new();
                for elem in &arr.elems {
                    match elem {
                        Expr::Lit(lit_expr) => match &lit_expr.lit {
                            Lit::Str(s) => values.push(s.value()),
                            _ => return None, // Non-string element in array
                        },
                        _ => return None, // Non-literal element in array
                    }
                }
                Some(values)
            }
            _ => None,
        }
    }
}

impl Parse for AttrItem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(AttrItem {
            name: input.parse()?,
            _eq: input.parse()?,
            value: input.parse()?,
        })
    }
}

/// Collection of attribute items
struct ServerAttrArgs {
    items: Vec<AttrItem>,
}

impl Parse for ServerAttrArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let items = Punctuated::<AttrItem, Token![,]>::parse_terminated(input)?
            .into_iter()
            .collect();
        Ok(ServerAttrArgs { items })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_root_parsing() {
        let root1 = Root::from_str("/path/to/dir:My Directory");
        assert_eq!(root1.uri, "/path/to/dir");
        assert_eq!(root1.name, Some("My Directory".to_string()));

        let root2 = Root::from_str("/tmp");
        assert_eq!(root2.uri, "/tmp");
        assert_eq!(root2.name, None);
    }
}

/// Tool macro attributes with syn-based parsing
///
/// Supports multiple rich metadata fields for improved LLM integration:
/// - description: Primary tool description
/// - usage: When/why to use this tool
/// - performance: Expected performance characteristics
/// - related: List of related/complementary tools
/// - examples: Common usage examples
///
/// All fields combined into single pipe-delimited description for MCP compliance.
#[derive(Debug, Default, Clone)]
pub struct ToolAttrs {
    pub description: Option<String>,
    pub usage: Option<String>,
    pub performance: Option<String>,
    pub related: Vec<String>,
    pub examples: Vec<String>,
}

impl ToolAttrs {
    /// Parse from macro attribute arguments
    ///
    /// Supports multiple syntaxes:
    /// - Backward compatible simple string: `#[tool("Say hello")]`
    /// - Keyword format: `#[tool(description = "Say hello")]`
    /// - Multiple fields with `description`, `usage`, `performance`, `related`, `examples`
    /// - Array fields for related tools and examples
    ///
    /// Examples:
    /// ```ignore
    /// #[tool(description = "Query notes")]
    /// #[tool(
    ///    description = "Query",
    ///    usage = "Find targets",
    ///    performance = "<100ms",
    ///    related = ["batch_execute"],
    ///    examples = ["status: done"]
    /// )]
    /// ```
    pub fn from_args(args: proc_macro::TokenStream) -> syn::Result<Self> {
        let mut attrs = ToolAttrs::default();

        if args.is_empty() {
            return Ok(attrs);
        }

        // Try structured parsing first (handles multiple fields)
        if let Ok(parsed) = syn::parse::<ToolAttrArgs>(args.clone()) {
            for item in parsed.items {
                match item.name.to_string().as_str() {
                    "description" => {
                        if let Some(value) = item.get_string_value() {
                            attrs.description = Some(value);
                        }
                    }
                    "usage" => {
                        if let Some(value) = item.get_string_value() {
                            attrs.usage = Some(value);
                        }
                    }
                    "performance" => {
                        if let Some(value) = item.get_string_value() {
                            attrs.performance = Some(value);
                        }
                    }
                    "related" => {
                        if let Ok(values) = item.get_string_array() {
                            attrs.related.extend(values);
                        }
                    }
                    "examples" => {
                        if let Ok(values) = item.get_string_array() {
                            attrs.examples.extend(values);
                        }
                    }
                    _ => {
                        // Ignore unknown attributes for forward compatibility
                        // This allows future extensions without breaking existing code
                    }
                }
            }
            return Ok(attrs);
        }

        // Fallback: single string parsing (backward compatibility)
        // Handles: #[tool("description")]
        if let Ok(lit_str) = syn::parse::<syn::LitStr>(args) {
            attrs.description = Some(lit_str.value());
        }

        Ok(attrs)
    }

    /// Combine all fields into single pipe-delimited description string
    ///
    /// Format: "primary | Field: value | Field: value"
    /// This keeps all metadata in the single `description` field required by MCP spec,
    /// while providing rich context to LLMs.
    ///
    /// Example output:
    /// "Query notes by metadata | Usage: Identify targets before batch ops | Performance: <100ms | Related: batch_execute, read_note | Examples: status: done, priority > 3"
    #[allow(clippy::collapsible_if)]
    pub fn combine_description(&self) -> String {
        let mut parts = Vec::new();

        // Primary description always first
        if let Some(desc) = &self.description {
            if !desc.is_empty() {
                parts.push(desc.clone());
            }
        }

        // Add optional fields in order
        if let Some(usage) = &self.usage {
            if !usage.is_empty() {
                parts.push(format!("Usage: {}", usage));
            }
        }

        if let Some(perf) = &self.performance {
            if !perf.is_empty() {
                parts.push(format!("Performance: {}", perf));
            }
        }

        let related_str = self
            .related
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if !related_str.is_empty() {
            parts.push(format!("Related: {}", related_str));
        }

        let examples_str = self
            .examples
            .iter()
            .filter(|s| !s.is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if !examples_str.is_empty() {
            parts.push(format!("Examples: {}", examples_str));
        }

        // Join with " | " delimiter (chosen for clarity in descriptions)
        if parts.is_empty() {
            "Tool".to_string()
        } else {
            parts.join(" | ")
        }
    }
}

/// A single tool attribute item (name = value)
/// Extended from AttrItem to support array values
#[derive(Clone)]
struct ToolAttrItem {
    name: Ident,
    _eq: Token![=],
    value: Expr,
}

impl ToolAttrItem {
    /// Get the string value if this is a string literal
    fn get_string_value(&self) -> Option<String> {
        match &self.value {
            Expr::Lit(lit) => match &lit.lit {
                Lit::Str(s) => Some(s.value()),
                _ => None,
            },
            _ => None,
        }
    }

    /// Get array of strings from expression
    /// Supports: ["a", "b", "c"] syntax
    fn get_string_array(&self) -> syn::Result<Vec<String>> {
        match &self.value {
            Expr::Array(array) => {
                let mut strings = Vec::new();
                for elem in &array.elems {
                    if let Expr::Lit(lit) = elem {
                        if let Lit::Str(s) = &lit.lit {
                            strings.push(s.value());
                        } else {
                            return Err(syn::Error::new_spanned(
                                elem,
                                "Expected string literal in array",
                            ));
                        }
                    } else {
                        return Err(syn::Error::new_spanned(
                            elem,
                            "Expected string literal in array",
                        ));
                    }
                }
                Ok(strings)
            }
            _ => Err(syn::Error::new_spanned(
                &self.value,
                "Expected array of strings, like [\"a\", \"b\"]",
            )),
        }
    }
}

impl Parse for ToolAttrItem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(ToolAttrItem {
            name: input.parse()?,
            _eq: input.parse()?,
            value: input.parse()?,
        })
    }
}

/// Collection of tool attribute items
struct ToolAttrArgs {
    items: Vec<ToolAttrItem>,
}

impl Parse for ToolAttrArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let items = Punctuated::<ToolAttrItem, Token![,]>::parse_terminated(input)?
            .into_iter()
            .collect();
        Ok(ToolAttrArgs { items })
    }
}

#[cfg(test)]
mod tool_attr_tests {
    use super::*;

    // Tests for combine_description() logic (no proc_macro TokenStream needed)

    #[test]
    fn test_tool_combine_description_empty() {
        let attrs = ToolAttrs::default();
        let combined = attrs.combine_description();
        assert_eq!(combined, "Tool");
    }

    #[test]
    fn test_tool_combine_description_single_field() {
        let attrs = ToolAttrs {
            description: Some("Query metadata".to_string()),
            ..Default::default()
        };
        let combined = attrs.combine_description();
        assert_eq!(combined, "Query metadata");
    }

    #[test]
    fn test_tool_combine_description_multiple_fields() {
        let attrs = ToolAttrs {
            description: Some("Query metadata".to_string()),
            usage: Some("Identify targets".to_string()),
            performance: Some("<100ms".to_string()),
            related: vec!["batch_execute".to_string()],
            examples: vec!["status: done".to_string()],
        };

        let combined = attrs.combine_description();

        // Verify all components are present
        assert!(combined.contains("Query metadata"));
        assert!(combined.contains("Usage: Identify targets"));
        assert!(combined.contains("Performance: <100ms"));
        assert!(combined.contains("Related: batch_execute"));
        assert!(combined.contains("Examples: status: done"));

        // Verify pipe delimiters are used
        assert!(combined.contains(" | "));

        // Verify order: description first
        assert!(combined.starts_with("Query metadata"));
    }

    #[test]
    fn test_tool_combine_description_multiple_related_and_examples() {
        let attrs = ToolAttrs {
            description: Some("Query".to_string()),
            usage: None,
            performance: None,
            related: vec![
                "tool1".to_string(),
                "tool2".to_string(),
                "tool3".to_string(),
            ],
            examples: vec!["example1".to_string(), "example2".to_string()],
        };

        let combined = attrs.combine_description();
        assert!(combined.contains("Related: tool1, tool2, tool3"));
        assert!(combined.contains("Examples: example1, example2"));
    }

    #[test]
    fn test_tool_empty_fields_not_added() {
        let attrs = ToolAttrs {
            description: Some("Test".to_string()),
            usage: Some("".to_string()), // Empty - should not be added
            performance: None,
            related: vec!["".to_string()], // Empty - should be filtered
            examples: vec![],
        };

        let combined = attrs.combine_description();
        assert_eq!(combined, "Test"); // Only description, no empty fields
    }

    #[test]
    fn test_tool_combine_description_with_all_fields() {
        let attrs = ToolAttrs {
            description: Some("Query by metadata pattern".to_string()),
            usage: Some("Identify targets before batch operations".to_string()),
            performance: Some("<100ms typical on 10k notes".to_string()),
            related: vec!["batch_execute".to_string(), "read_note".to_string()],
            examples: vec!["status: \"draft\"".to_string(), "priority > 3".to_string()],
        };

        let combined = attrs.combine_description();

        // Verify structure
        assert!(combined.contains("Query by metadata pattern"));
        assert!(combined.contains("Usage: Identify targets before batch operations"));
        assert!(combined.contains("Performance: <100ms typical on 10k notes"));
        assert!(combined.contains("Related: batch_execute, read_note"));
        assert!(combined.contains("Examples: status: \"draft\", priority > 3"));

        let expected_parts_count = 5; // description, usage, performance, related, examples
        let pipe_count = combined.matches(" | ").count();
        assert_eq!(pipe_count, expected_parts_count - 1); // N parts means N-1 pipes
    }
}
