//! Type conversion utilities between MCP and proto types
//!
//! This module provides bidirectional conversion between native MCP types
//! and the generated protobuf types.

use crate::error::{GrpcError, GrpcResult};
use crate::proto;
use turbomcp_core::types::{
    capabilities::{ClientCapabilities, ServerCapabilities},
    content::{Content, PromptMessage, ResourceContent},
    core::{Annotations, Icon, Implementation, Role},
    initialization::{InitializeRequest, InitializeResult},
    prompts::{GetPromptResult, Prompt, PromptArgument},
    resources::{Resource, ResourceTemplate},
    tools::{CallToolResult, Tool, ToolInputSchema},
};

fn icons_to_proto(icons: Option<Vec<Icon>>) -> Vec<proto::Icon> {
    icons
        .unwrap_or_default()
        .into_iter()
        .map(Into::into)
        .collect()
}

fn proto_icons_to_option(icons: Vec<proto::Icon>) -> Option<Vec<Icon>> {
    let icons: Vec<_> = icons
        .into_iter()
        .filter_map(|icon| Icon::try_from(icon).ok())
        .collect();
    if icons.is_empty() { None } else { Some(icons) }
}

// =============================================================================
// Implementation
// =============================================================================

impl From<Implementation> for proto::Implementation {
    fn from(impl_: Implementation) -> Self {
        Self {
            name: impl_.name,
            version: impl_.version,
            title: impl_.title,
            description: impl_.description,
            icons: icons_to_proto(impl_.icons),
            website_url: impl_.website_url,
        }
    }
}

impl From<proto::Implementation> for Implementation {
    fn from(impl_: proto::Implementation) -> Self {
        Self {
            name: impl_.name,
            title: impl_.title,
            description: impl_.description,
            version: impl_.version,
            icons: proto_icons_to_option(impl_.icons),
            website_url: impl_.website_url,
        }
    }
}

// =============================================================================
// Role
// =============================================================================

impl From<Role> for proto::Role {
    fn from(role: Role) -> Self {
        match role {
            Role::User => proto::Role::User,
            Role::Assistant => proto::Role::Assistant,
        }
    }
}

impl From<proto::Role> for Role {
    fn from(role: proto::Role) -> Self {
        match role {
            proto::Role::User | proto::Role::Unspecified => Role::User,
            proto::Role::Assistant => Role::Assistant,
        }
    }
}

// =============================================================================
// Initialize
// =============================================================================

impl TryFrom<InitializeRequest> for proto::InitializeRequest {
    type Error = GrpcError;

    fn try_from(req: InitializeRequest) -> GrpcResult<Self> {
        Ok(Self {
            protocol_version: req.protocol_version.to_string(),
            capabilities: Some(req.capabilities.into()),
            client_info: Some(req.client_info.into()),
        })
    }
}

impl TryFrom<proto::InitializeRequest> for InitializeRequest {
    type Error = GrpcError;

    fn try_from(req: proto::InitializeRequest) -> GrpcResult<Self> {
        Ok(Self {
            protocol_version: req.protocol_version.into(),
            capabilities: req.capabilities.map(Into::into).unwrap_or_default(),
            client_info: req
                .client_info
                .map(Into::into)
                .ok_or_else(|| GrpcError::invalid_request("Missing client_info"))?,
            _meta: None,
        })
    }
}

impl From<InitializeResult> for proto::InitializeResult {
    fn from(res: InitializeResult) -> Self {
        Self {
            protocol_version: res.protocol_version.to_string(),
            capabilities: Some(res.capabilities.into()),
            server_info: Some(res.server_info.into()),
            instructions: res.instructions,
        }
    }
}

impl TryFrom<proto::InitializeResult> for InitializeResult {
    type Error = GrpcError;

    fn try_from(res: proto::InitializeResult) -> GrpcResult<Self> {
        Ok(Self {
            protocol_version: res.protocol_version.into(),
            capabilities: res.capabilities.map(Into::into).unwrap_or_default(),
            server_info: res
                .server_info
                .map(Into::into)
                .ok_or_else(|| GrpcError::invalid_request("Missing server_info"))?,
            instructions: res.instructions,
            _meta: None,
        })
    }
}

// =============================================================================
// Capabilities
// =============================================================================

impl From<ClientCapabilities> for proto::ClientCapabilities {
    fn from(caps: ClientCapabilities) -> Self {
        Self {
            roots: caps.roots.map(|r| proto::RootsCapability {
                list_changed: r.list_changed.unwrap_or(false),
            }),
            sampling: caps.sampling.map(|_| proto::SamplingCapability {}),
            experimental: None,
        }
    }
}

impl From<proto::ClientCapabilities> for ClientCapabilities {
    fn from(caps: proto::ClientCapabilities) -> Self {
        Self {
            roots: caps
                .roots
                .map(|r| turbomcp_core::types::capabilities::RootsCapability {
                    list_changed: Some(r.list_changed),
                }),
            sampling: caps
                .sampling
                .map(|_| turbomcp_core::types::capabilities::SamplingCapability {}),
            elicitation: None,
            tasks: None,
            experimental: None,
        }
    }
}

impl From<ServerCapabilities> for proto::ServerCapabilities {
    fn from(caps: ServerCapabilities) -> Self {
        Self {
            prompts: caps.prompts.map(|p| proto::PromptsCapability {
                list_changed: p.list_changed.unwrap_or(false),
            }),
            resources: caps.resources.map(|r| proto::ResourcesCapability {
                subscribe: r.subscribe.unwrap_or(false),
                list_changed: r.list_changed.unwrap_or(false),
            }),
            tools: caps.tools.map(|t| proto::ToolsCapability {
                list_changed: t.list_changed.unwrap_or(false),
            }),
            logging: caps.logging.map(|_| proto::LoggingCapability {}),
            experimental: None,
        }
    }
}

impl From<proto::ServerCapabilities> for ServerCapabilities {
    fn from(caps: proto::ServerCapabilities) -> Self {
        Self {
            prompts: caps
                .prompts
                .map(|p| turbomcp_core::types::capabilities::PromptsCapability {
                    list_changed: Some(p.list_changed),
                }),
            resources: caps.resources.map(|r| {
                turbomcp_core::types::capabilities::ResourcesCapability {
                    subscribe: Some(r.subscribe),
                    list_changed: Some(r.list_changed),
                }
            }),
            tools: caps
                .tools
                .map(|t| turbomcp_core::types::capabilities::ToolsCapability {
                    list_changed: Some(t.list_changed),
                }),
            logging: caps
                .logging
                .map(|_| turbomcp_core::types::capabilities::LoggingCapability {}),
            tasks: None,
            experimental: None,
        }
    }
}

// =============================================================================
// Annotations (base type - for Resource, ResourceTemplate, Content)
// =============================================================================
//
// Note: proto::Annotations only has audience and priority. The MCP Annotations
// type also has last_modified and custom fields which are lost in conversion.
// ToolAnnotations (destructive_hint, read_only_hint, etc.) is a separate type
// that doesn't have a direct proto representation - tool hints are not preserved
// in gRPC transport.

impl From<Annotations> for proto::Annotations {
    fn from(annotations: Annotations) -> Self {
        Self {
            audience: annotations.audience.unwrap_or_default(),
            priority: annotations.priority.unwrap_or(0.0),
        }
    }
}

#[allow(clippy::default_trait_access)]
impl From<proto::Annotations> for Annotations {
    fn from(annotations: proto::Annotations) -> Self {
        Self {
            audience: if annotations.audience.is_empty() {
                None
            } else {
                Some(annotations.audience)
            },
            priority: if annotations.priority == 0.0 {
                None
            } else {
                Some(annotations.priority)
            },
            last_modified: None,
            custom: Default::default(),
        }
    }
}

// =============================================================================
// Icon
// =============================================================================

impl From<Icon> for proto::Icon {
    fn from(icon: Icon) -> Self {
        match icon {
            Icon::DataUri(data_uri) => Self {
                icon: Some(proto::icon::Icon::DataUri(data_uri)),
            },
            Icon::Url(url) => Self {
                icon: Some(proto::icon::Icon::Uri(url)),
            },
        }
    }
}

impl TryFrom<proto::Icon> for Icon {
    type Error = GrpcError;

    fn try_from(icon: proto::Icon) -> GrpcResult<Self> {
        match icon.icon {
            Some(proto::icon::Icon::Uri(uri)) => Ok(Icon::Url(uri)),
            Some(proto::icon::Icon::DataUri(data_uri)) => Ok(Icon::DataUri(data_uri)),
            None => Err(GrpcError::invalid_request("Icon missing URI")),
        }
    }
}

// =============================================================================
// Tool
// =============================================================================
//
// Note: ToolAnnotations (destructive_hint, read_only_hint, etc.) doesn't map to
// proto::Annotations (which only has audience, priority). Tool hints are not
// preserved in gRPC transport - they would need a dedicated proto message to
// support them properly.

impl TryFrom<Tool> for proto::Tool {
    type Error = GrpcError;

    fn try_from(tool: Tool) -> GrpcResult<Self> {
        let input_schema = serde_json::to_vec(&tool.input_schema)?;
        // Note: tool.annotations is ToolAnnotations which doesn't have audience/priority
        // proto::Annotations has audience/priority, so we can't directly convert.
        // Tool hints (destructive_hint, etc.) are lost in gRPC transport.
        Ok(Self {
            name: tool.name,
            description: tool.description,
            input_schema,
            annotations: None, // ToolAnnotations doesn't map to proto::Annotations
            icons: icons_to_proto(tool.icons),
            title: tool.title,
        })
    }
}

impl TryFrom<proto::Tool> for Tool {
    type Error = GrpcError;

    fn try_from(tool: proto::Tool) -> GrpcResult<Self> {
        let input_schema: ToolInputSchema = if tool.input_schema.is_empty() {
            ToolInputSchema::default()
        } else {
            serde_json::from_slice(&tool.input_schema)?
        };

        // Note: proto::Annotations has audience/priority which are base Annotations fields,
        // not ToolAnnotations fields. The MCP Tool type expects ToolAnnotations, so we
        // would need a separate proto message to properly support tool hints.
        Ok(Self {
            name: tool.name,
            description: tool.description,
            input_schema,
            title: tool.title,
            icons: proto_icons_to_option(tool.icons),
            annotations: None, // proto::Annotations doesn't map to ToolAnnotations
        })
    }
}

// =============================================================================
// Resource
// =============================================================================

impl From<Resource> for proto::Resource {
    fn from(resource: Resource) -> Self {
        Self {
            uri: resource.uri.to_string(),
            name: resource.name,
            description: resource.description,
            title: resource.title,
            mime_type: resource.mime_type.map(Into::into),
            annotations: resource.annotations.map(Into::into),
            icons: icons_to_proto(resource.icons),
            size: resource.size,
        }
    }
}

impl From<proto::Resource> for Resource {
    fn from(resource: proto::Resource) -> Self {
        Self {
            uri: resource.uri.into(),
            name: resource.name,
            description: resource.description,
            title: resource.title,
            icons: proto_icons_to_option(resource.icons),
            mime_type: resource.mime_type.map(Into::into),
            size: resource.size,
            annotations: resource.annotations.map(Into::into),
        }
    }
}

impl From<ResourceTemplate> for proto::ResourceTemplate {
    fn from(template: ResourceTemplate) -> Self {
        Self {
            uri_template: template.uri_template,
            name: template.name,
            description: template.description,
            title: template.title,
            mime_type: template.mime_type.map(Into::into),
            annotations: template.annotations.map(Into::into),
            icons: icons_to_proto(template.icons),
        }
    }
}

impl From<proto::ResourceTemplate> for ResourceTemplate {
    fn from(template: proto::ResourceTemplate) -> Self {
        Self {
            uri_template: template.uri_template,
            name: template.name,
            description: template.description,
            title: template.title,
            icons: proto_icons_to_option(template.icons),
            mime_type: template.mime_type.map(Into::into),
            annotations: template.annotations.map(Into::into),
        }
    }
}

// =============================================================================
// ResourceContent
// =============================================================================

impl TryFrom<ResourceContent> for proto::ResourceContent {
    type Error = GrpcError;

    fn try_from(content: ResourceContent) -> GrpcResult<Self> {
        let (text, blob) = match (content.text, content.blob) {
            (Some(text), _) => (Some(text), None),
            (_, Some(blob)) => (None, Some(blob.into_bytes())),
            (None, None) => (None, None),
        };

        Ok(Self {
            uri: content.uri.to_string(),
            mime_type: content.mime_type.map(Into::into),
            content: text
                .map(proto::resource_content::Content::Text)
                .or_else(|| blob.map(proto::resource_content::Content::Blob)),
        })
    }
}

impl From<proto::ResourceContent> for ResourceContent {
    fn from(content: proto::ResourceContent) -> Self {
        let (text, blob) = match content.content {
            Some(proto::resource_content::Content::Text(t)) => (Some(t), None),
            Some(proto::resource_content::Content::Blob(b)) => {
                (None, Some(String::from_utf8_lossy(&b).to_string()))
            }
            None => (None, None),
        };

        Self {
            uri: content.uri.into(),
            mime_type: content.mime_type.map(Into::into),
            text,
            blob,
        }
    }
}

// =============================================================================
// Prompt
// =============================================================================

impl From<Prompt> for proto::Prompt {
    fn from(prompt: Prompt) -> Self {
        Self {
            name: prompt.name,
            description: prompt.description,
            title: prompt.title,
            arguments: prompt
                .arguments
                .unwrap_or_default()
                .into_iter()
                .map(Into::into)
                .collect(),
            icons: icons_to_proto(prompt.icons),
        }
    }
}

impl From<proto::Prompt> for Prompt {
    fn from(prompt: proto::Prompt) -> Self {
        Self {
            name: prompt.name,
            description: prompt.description,
            title: prompt.title,
            icons: proto_icons_to_option(prompt.icons),
            arguments: if prompt.arguments.is_empty() {
                None
            } else {
                Some(prompt.arguments.into_iter().map(Into::into).collect())
            },
        }
    }
}

impl From<PromptArgument> for proto::PromptArgument {
    fn from(arg: PromptArgument) -> Self {
        Self {
            name: arg.name,
            description: arg.description,
            required: arg.required,
        }
    }
}

impl From<proto::PromptArgument> for PromptArgument {
    fn from(arg: proto::PromptArgument) -> Self {
        Self {
            name: arg.name,
            description: arg.description,
            required: arg.required,
        }
    }
}

impl TryFrom<GetPromptResult> for proto::GetPromptResult {
    type Error = GrpcError;

    fn try_from(result: GetPromptResult) -> GrpcResult<Self> {
        let messages: Result<Vec<_>, _> =
            result.messages.into_iter().map(TryInto::try_into).collect();

        Ok(Self {
            description: result.description,
            messages: messages?,
        })
    }
}

impl TryFrom<proto::GetPromptResult> for GetPromptResult {
    type Error = GrpcError;

    fn try_from(result: proto::GetPromptResult) -> GrpcResult<Self> {
        let messages: Result<Vec<_>, _> =
            result.messages.into_iter().map(TryInto::try_into).collect();

        Ok(Self {
            description: result.description,
            messages: messages?,
            _meta: None,
        })
    }
}

impl TryFrom<PromptMessage> for proto::PromptMessage {
    type Error = GrpcError;

    fn try_from(msg: PromptMessage) -> GrpcResult<Self> {
        Ok(Self {
            role: proto::Role::from(msg.role).into(),
            content: Some(msg.content.try_into()?),
        })
    }
}

impl TryFrom<proto::PromptMessage> for PromptMessage {
    type Error = GrpcError;

    fn try_from(msg: proto::PromptMessage) -> GrpcResult<Self> {
        Ok(Self {
            role: proto::Role::try_from(msg.role)
                .unwrap_or(proto::Role::User)
                .into(),
            content: msg
                .content
                .ok_or_else(|| GrpcError::invalid_request("Missing content"))?
                .try_into()?,
        })
    }
}

// =============================================================================
// Content
// =============================================================================

impl TryFrom<Content> for proto::Content {
    type Error = GrpcError;

    fn try_from(content: Content) -> GrpcResult<Self> {
        let (content_type, annotations) = match content {
            Content::Text { text, annotations } => (
                proto::content::Content::Text(proto::TextContent { text }),
                annotations,
            ),
            Content::Image {
                data,
                mime_type,
                annotations,
            } => (
                proto::content::Content::Image(proto::ImageContent {
                    data,
                    mime_type: mime_type.to_string(),
                }),
                annotations,
            ),
            Content::Audio {
                data,
                mime_type,
                annotations,
            } => (
                proto::content::Content::Audio(proto::AudioContent {
                    data,
                    mime_type: mime_type.to_string(),
                }),
                annotations,
            ),
            Content::Resource {
                resource,
                annotations,
            } => (
                proto::content::Content::Resource(resource.try_into()?),
                annotations,
            ),
        };

        Ok(Self {
            content: Some(content_type),
            annotations: annotations.map(Into::into),
        })
    }
}

impl TryFrom<proto::Content> for Content {
    type Error = GrpcError;

    fn try_from(content: proto::Content) -> GrpcResult<Self> {
        let annotations = content.annotations.map(Into::into);

        match content.content {
            Some(proto::content::Content::Text(t)) => Ok(Content::Text {
                text: t.text,
                annotations,
            }),
            Some(proto::content::Content::Image(i)) => Ok(Content::Image {
                data: i.data,
                mime_type: i.mime_type.into(),
                annotations,
            }),
            Some(proto::content::Content::Audio(a)) => Ok(Content::Audio {
                data: a.data,
                mime_type: a.mime_type.into(),
                annotations,
            }),
            Some(proto::content::Content::Resource(r)) => Ok(Content::Resource {
                resource: r.into(),
                annotations,
            }),
            None => Err(GrpcError::invalid_request("Missing content")),
        }
    }
}

// =============================================================================
// CallToolResult
// =============================================================================

impl TryFrom<CallToolResult> for proto::CallToolResult {
    type Error = GrpcError;

    fn try_from(result: CallToolResult) -> GrpcResult<Self> {
        let content: Result<Vec<_>, _> =
            result.content.into_iter().map(TryInto::try_into).collect();

        Ok(Self {
            content: content?,
            is_error: result.is_error,
        })
    }
}

impl TryFrom<proto::CallToolResult> for CallToolResult {
    type Error = GrpcError;

    fn try_from(result: proto::CallToolResult) -> GrpcResult<Self> {
        let content: Result<Vec<_>, _> =
            result.content.into_iter().map(TryInto::try_into).collect();

        Ok(Self {
            content: content?,
            is_error: result.is_error,
            _meta: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_implementation_conversion() {
        let impl_ = Implementation {
            name: "test".to_string(),
            title: Some("Test".to_string()),
            description: Some("grpc metadata".to_string()),
            version: "1.0.0".to_string(),
            icons: Some(vec![Icon::url("https://example.com/icon.svg")]),
            website_url: Some("https://example.com".to_string()),
        };

        let proto_impl: proto::Implementation = impl_.clone().into();
        assert_eq!(proto_impl.name, "test");
        assert_eq!(proto_impl.version, "1.0.0");
        assert_eq!(proto_impl.title.as_deref(), Some("Test"));
        assert_eq!(proto_impl.description.as_deref(), Some("grpc metadata"));
        assert_eq!(proto_impl.icons.len(), 1);
        assert_eq!(
            proto_impl.website_url.as_deref(),
            Some("https://example.com")
        );

        let back: Implementation = proto_impl.into();
        assert_eq!(back.name, impl_.name);
        assert_eq!(back.version, impl_.version);
        assert_eq!(back.title, impl_.title);
        assert_eq!(back.description, impl_.description);
        assert_eq!(back.website_url, impl_.website_url);
        assert_eq!(back.icons.as_ref().map(Vec::len), Some(1));
    }

    #[test]
    fn test_role_conversion() {
        assert_eq!(proto::Role::from(Role::User), proto::Role::User);
        assert_eq!(proto::Role::from(Role::Assistant), proto::Role::Assistant);
        assert_eq!(Role::from(proto::Role::User), Role::User);
        assert_eq!(Role::from(proto::Role::Assistant), Role::Assistant);
    }

    #[test]
    fn test_tool_conversion() {
        let tool = Tool {
            name: "calculator".to_string(),
            description: Some("Does math".to_string()),
            input_schema: ToolInputSchema::default(),
            title: None,
            icons: None,
            annotations: None,
        };

        let proto_tool: proto::Tool = tool.try_into().unwrap();
        assert_eq!(proto_tool.name, "calculator");
        assert_eq!(proto_tool.description, Some("Does math".to_string()));

        let back: Tool = proto_tool.try_into().unwrap();
        assert_eq!(back.name, "calculator");
    }

    #[test]
    fn test_resource_conversion() {
        let resource = Resource {
            uri: "file:///test.txt".into(),
            name: "Test File".to_string(),
            description: Some("A test file".to_string()),
            title: None,
            icons: None,
            mime_type: Some("text/plain".into()),
            size: None,
            annotations: None,
        };

        let proto_resource: proto::Resource = resource.clone().into();
        assert_eq!(proto_resource.uri, "file:///test.txt");

        let back: Resource = proto_resource.into();
        assert_eq!(back.uri, resource.uri);
        assert_eq!(back.name, resource.name);
    }

    #[test]
    fn test_prompt_conversion() {
        let prompt = Prompt {
            name: "greeting".to_string(),
            description: Some("A greeting prompt".to_string()),
            title: None,
            icons: None,
            arguments: Some(vec![PromptArgument {
                name: "name".to_string(),
                description: Some("The name to greet".to_string()),
                required: Some(true),
            }]),
        };

        let proto_prompt: proto::Prompt = prompt.clone().into();
        assert_eq!(proto_prompt.name, "greeting");
        assert_eq!(proto_prompt.arguments.len(), 1);

        let back: Prompt = proto_prompt.into();
        assert_eq!(back.name, prompt.name);
    }
}
