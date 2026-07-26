//! Bridges between the `2025-06-18` and `2025-11-25` wire types.
//!
//! # Why these go through `2025-11-25` rather than straight from `neutral`
//!
//! `2025-06-18` is `2025-11-25` minus a short, closed list of additions:
//! `icons`, `Tool.execution`, `ServerCapabilities.tasks`, and
//! `Implementation.description`/`websiteUrl`. Twenty-odd of the types are
//! field-for-field identical. Writing a second full set of `From<neutral::_>`
//! impls would duplicate that, and — worse — a field added to a neutral type
//! later would compile fine while silently never reaching the `2025-06-18`
//! wire.
//!
//! So the neutral conversion happens **once**, into `2025-11-25`, and each
//! type here is a narrow step down from it. Every conversion destructures its
//! source exhaustively — no `..` patterns — so adding a field to either wire
//! is a *compile error* here until someone decides what the other revision
//! does with it. That is the property the whole arrangement exists for.
//!
//! This is not the shape v3 used. v3's `VersionAdapter::filter_result` deleted
//! JSON keys by string name, per method, with a `_ => result` fallthrough that
//! passed anything unrecognized through untouched. These are typed, total, and
//! compiler-checked; nothing passes through unconsidered.
//!
//! Both revisions are frozen published specs, so this couples two fixed points.
//! The draft — which still moves — has its own direct conversions from the
//! neutral types and does not participate here.

use alloc::string::String;
use alloc::vec::Vec;

use crate::v2025_06_18::types as v06;
use crate::v2025_11_25::types as v11;

/// `$schema` has no declared home on the `2025-06-18` `inputSchema`, but the
/// node is an open JSON Schema, so the keyword rides in the catch-all rather
/// than being dropped.
const SCHEMA_KEYWORD: &str = "$schema";

// ---- leaves ------------------------------------------------------------------

impl From<v11::Role> for v06::Role {
    fn from(role: v11::Role) -> Self {
        match role {
            v11::Role::Assistant => Self::Assistant,
            v11::Role::User => Self::User,
        }
    }
}

impl From<v06::Role> for v11::Role {
    fn from(role: v06::Role) -> Self {
        match role {
            v06::Role::Assistant => Self::Assistant,
            v06::Role::User => Self::User,
        }
    }
}

impl From<v11::Annotations> for v06::Annotations {
    fn from(a: v11::Annotations) -> Self {
        let v11::Annotations {
            audience,
            last_modified,
            priority,
        } = a;
        Self {
            audience: audience.into_iter().map(Into::into).collect(),
            last_modified,
            priority,
        }
    }
}

impl From<v06::Annotations> for v11::Annotations {
    fn from(a: v06::Annotations) -> Self {
        let v06::Annotations {
            audience,
            last_modified,
            priority,
        } = a;
        Self {
            audience: audience.into_iter().map(Into::into).collect(),
            last_modified,
            priority,
        }
    }
}

impl From<v11::ToolAnnotations> for v06::ToolAnnotations {
    fn from(a: v11::ToolAnnotations) -> Self {
        let v11::ToolAnnotations {
            destructive_hint,
            idempotent_hint,
            open_world_hint,
            read_only_hint,
            title,
        } = a;
        Self {
            destructive_hint,
            idempotent_hint,
            open_world_hint,
            read_only_hint,
            title,
        }
    }
}

impl From<v06::ToolAnnotations> for v11::ToolAnnotations {
    fn from(a: v06::ToolAnnotations) -> Self {
        let v06::ToolAnnotations {
            destructive_hint,
            idempotent_hint,
            open_world_hint,
            read_only_hint,
            title,
        } = a;
        Self {
            destructive_hint,
            idempotent_hint,
            open_world_hint,
            read_only_hint,
            title,
        }
    }
}

// ---- resource contents ---------------------------------------------------------

impl From<v11::TextResourceContents> for v06::TextResourceContents {
    fn from(c: v11::TextResourceContents) -> Self {
        let v11::TextResourceContents {
            meta,
            mime_type,
            text,
            uri,
        } = c;
        Self {
            meta,
            mime_type,
            text,
            uri,
        }
    }
}

impl From<v06::TextResourceContents> for v11::TextResourceContents {
    fn from(c: v06::TextResourceContents) -> Self {
        let v06::TextResourceContents {
            meta,
            mime_type,
            text,
            uri,
        } = c;
        Self {
            meta,
            mime_type,
            text,
            uri,
        }
    }
}

impl From<v11::BlobResourceContents> for v06::BlobResourceContents {
    fn from(c: v11::BlobResourceContents) -> Self {
        let v11::BlobResourceContents {
            blob,
            meta,
            mime_type,
            uri,
        } = c;
        Self {
            blob,
            meta,
            mime_type,
            uri,
        }
    }
}

impl From<v06::BlobResourceContents> for v11::BlobResourceContents {
    fn from(c: v06::BlobResourceContents) -> Self {
        let v06::BlobResourceContents {
            blob,
            meta,
            mime_type,
            uri,
        } = c;
        Self {
            blob,
            meta,
            mime_type,
            uri,
        }
    }
}

impl From<v11::ReadResourceResultContentsItem> for v06::ReadResourceResultContentsItem {
    fn from(c: v11::ReadResourceResultContentsItem) -> Self {
        match c {
            v11::ReadResourceResultContentsItem::TextResourceContents(t) => {
                Self::TextResourceContents(t.into())
            }
            v11::ReadResourceResultContentsItem::BlobResourceContents(b) => {
                Self::BlobResourceContents(b.into())
            }
        }
    }
}

impl From<v06::ReadResourceResultContentsItem> for v11::ReadResourceResultContentsItem {
    fn from(c: v06::ReadResourceResultContentsItem) -> Self {
        match c {
            v06::ReadResourceResultContentsItem::TextResourceContents(t) => {
                Self::TextResourceContents(t.into())
            }
            v06::ReadResourceResultContentsItem::BlobResourceContents(b) => {
                Self::BlobResourceContents(b.into())
            }
        }
    }
}

impl From<v11::EmbeddedResourceResource> for v06::EmbeddedResourceResource {
    fn from(r: v11::EmbeddedResourceResource) -> Self {
        match r {
            v11::EmbeddedResourceResource::TextResourceContents(t) => {
                Self::TextResourceContents(t.into())
            }
            v11::EmbeddedResourceResource::BlobResourceContents(b) => {
                Self::BlobResourceContents(b.into())
            }
        }
    }
}

impl From<v06::EmbeddedResourceResource> for v11::EmbeddedResourceResource {
    fn from(r: v06::EmbeddedResourceResource) -> Self {
        match r {
            v06::EmbeddedResourceResource::TextResourceContents(t) => {
                Self::TextResourceContents(t.into())
            }
            v06::EmbeddedResourceResource::BlobResourceContents(b) => {
                Self::BlobResourceContents(b.into())
            }
        }
    }
}

// ---- content blocks ------------------------------------------------------------

impl From<v11::TextContent> for v06::TextContent {
    fn from(c: v11::TextContent) -> Self {
        let v11::TextContent {
            annotations,
            meta,
            text,
            type_,
        } = c;
        Self {
            annotations: annotations.map(Into::into),
            meta,
            text,
            type_,
        }
    }
}

impl From<v06::TextContent> for v11::TextContent {
    fn from(c: v06::TextContent) -> Self {
        let v06::TextContent {
            annotations,
            meta,
            text,
            type_,
        } = c;
        Self {
            annotations: annotations.map(Into::into),
            meta,
            text,
            type_,
        }
    }
}

impl From<v11::ImageContent> for v06::ImageContent {
    fn from(c: v11::ImageContent) -> Self {
        let v11::ImageContent {
            annotations,
            data,
            meta,
            mime_type,
            type_,
        } = c;
        Self {
            annotations: annotations.map(Into::into),
            data,
            meta,
            mime_type,
            type_,
        }
    }
}

impl From<v06::ImageContent> for v11::ImageContent {
    fn from(c: v06::ImageContent) -> Self {
        let v06::ImageContent {
            annotations,
            data,
            meta,
            mime_type,
            type_,
        } = c;
        Self {
            annotations: annotations.map(Into::into),
            data,
            meta,
            mime_type,
            type_,
        }
    }
}

impl From<v11::AudioContent> for v06::AudioContent {
    fn from(c: v11::AudioContent) -> Self {
        let v11::AudioContent {
            annotations,
            data,
            meta,
            mime_type,
            type_,
        } = c;
        Self {
            annotations: annotations.map(Into::into),
            data,
            meta,
            mime_type,
            type_,
        }
    }
}

impl From<v06::AudioContent> for v11::AudioContent {
    fn from(c: v06::AudioContent) -> Self {
        let v06::AudioContent {
            annotations,
            data,
            meta,
            mime_type,
            type_,
        } = c;
        Self {
            annotations: annotations.map(Into::into),
            data,
            meta,
            mime_type,
            type_,
        }
    }
}

impl From<v11::ResourceLink> for v06::ResourceLink {
    /// Drops `icons` — `2025-06-18` has no field for them.
    fn from(l: v11::ResourceLink) -> Self {
        let v11::ResourceLink {
            annotations,
            description,
            icons: _,
            meta,
            mime_type,
            name,
            size,
            title,
            type_,
            uri,
        } = l;
        Self {
            annotations: annotations.map(Into::into),
            description,
            meta,
            mime_type,
            name,
            size,
            title,
            type_,
            uri,
        }
    }
}

impl From<v06::ResourceLink> for v11::ResourceLink {
    fn from(l: v06::ResourceLink) -> Self {
        let v06::ResourceLink {
            annotations,
            description,
            meta,
            mime_type,
            name,
            size,
            title,
            type_,
            uri,
        } = l;
        Self {
            annotations: annotations.map(Into::into),
            description,
            icons: Vec::new(),
            meta,
            mime_type,
            name,
            size,
            title,
            type_,
            uri,
        }
    }
}

impl From<v11::EmbeddedResource> for v06::EmbeddedResource {
    fn from(e: v11::EmbeddedResource) -> Self {
        let v11::EmbeddedResource {
            annotations,
            meta,
            resource,
            type_,
        } = e;
        Self {
            annotations: annotations.map(Into::into),
            meta,
            resource: resource.into(),
            type_,
        }
    }
}

impl From<v06::EmbeddedResource> for v11::EmbeddedResource {
    fn from(e: v06::EmbeddedResource) -> Self {
        let v06::EmbeddedResource {
            annotations,
            meta,
            resource,
            type_,
        } = e;
        Self {
            annotations: annotations.map(Into::into),
            meta,
            resource: resource.into(),
            type_,
        }
    }
}

impl From<v11::ContentBlock> for v06::ContentBlock {
    fn from(b: v11::ContentBlock) -> Self {
        match b {
            v11::ContentBlock::TextContent(c) => Self::TextContent(c.into()),
            v11::ContentBlock::ImageContent(c) => Self::ImageContent(c.into()),
            v11::ContentBlock::AudioContent(c) => Self::AudioContent(c.into()),
            v11::ContentBlock::ResourceLink(c) => Self::ResourceLink(c.into()),
            v11::ContentBlock::EmbeddedResource(c) => Self::EmbeddedResource(c.into()),
        }
    }
}

impl From<v06::ContentBlock> for v11::ContentBlock {
    fn from(b: v06::ContentBlock) -> Self {
        match b {
            v06::ContentBlock::TextContent(c) => Self::TextContent(c.into()),
            v06::ContentBlock::ImageContent(c) => Self::ImageContent(c.into()),
            v06::ContentBlock::AudioContent(c) => Self::AudioContent(c.into()),
            v06::ContentBlock::ResourceLink(c) => Self::ResourceLink(c.into()),
            v06::ContentBlock::EmbeddedResource(c) => Self::EmbeddedResource(c.into()),
        }
    }
}

// ---- tools ---------------------------------------------------------------------

impl From<v11::ToolInputSchema> for v06::ToolInputSchema {
    /// `$schema` moves into the catch-all rather than being dropped: the node
    /// is an open JSON Schema on both revisions, `2025-06-18` simply doesn't
    /// name the keyword.
    fn from(s: v11::ToolInputSchema) -> Self {
        let v11::ToolInputSchema {
            properties,
            required,
            schema,
            type_,
            mut extra,
        } = s;
        if let Some(schema) = schema {
            extra.insert(SCHEMA_KEYWORD.into(), schema.into());
        }
        Self {
            properties,
            required,
            type_,
            extra,
        }
    }
}

impl From<v06::ToolInputSchema> for v11::ToolInputSchema {
    fn from(s: v06::ToolInputSchema) -> Self {
        let v06::ToolInputSchema {
            properties,
            required,
            type_,
            mut extra,
        } = s;
        let schema = extra
            .remove(SCHEMA_KEYWORD)
            .and_then(|v| v.as_str().map(String::from));
        Self {
            properties,
            required,
            schema,
            type_,
            extra,
        }
    }
}

impl From<v11::ToolOutputSchema> for v06::ToolOutputSchema {
    fn from(s: v11::ToolOutputSchema) -> Self {
        let v11::ToolOutputSchema {
            properties,
            required,
            schema,
            type_,
            mut extra,
        } = s;
        if let Some(schema) = schema {
            extra.insert(SCHEMA_KEYWORD.into(), schema.into());
        }
        Self {
            properties,
            required,
            type_,
            extra,
        }
    }
}

impl From<v06::ToolOutputSchema> for v11::ToolOutputSchema {
    fn from(s: v06::ToolOutputSchema) -> Self {
        let v06::ToolOutputSchema {
            properties,
            required,
            type_,
            mut extra,
        } = s;
        let schema = extra
            .remove(SCHEMA_KEYWORD)
            .and_then(|v| v.as_str().map(String::from));
        Self {
            properties,
            required,
            schema,
            type_,
            extra,
        }
    }
}

impl From<v11::Tool> for v06::Tool {
    /// Drops `icons` and `execution`. `execution` carries a tool's task
    /// support, and `2025-06-18` has no Tasks at all — a client on it could not
    /// act on the field even if it were sent.
    fn from(t: v11::Tool) -> Self {
        let v11::Tool {
            annotations,
            description,
            execution: _,
            icons: _,
            input_schema,
            meta,
            name,
            output_schema,
            title,
        } = t;
        Self {
            annotations: annotations.map(Into::into),
            description,
            input_schema: input_schema.into(),
            meta,
            name,
            output_schema: output_schema.map(Into::into),
            title,
        }
    }
}

impl From<v06::Tool> for v11::Tool {
    fn from(t: v06::Tool) -> Self {
        let v06::Tool {
            annotations,
            description,
            input_schema,
            meta,
            name,
            output_schema,
            title,
        } = t;
        Self {
            annotations: annotations.map(Into::into),
            description,
            execution: None,
            icons: Vec::new(),
            input_schema: input_schema.into(),
            meta,
            name,
            output_schema: output_schema.map(Into::into),
            title,
        }
    }
}

// ---- resources -----------------------------------------------------------------

impl From<v11::Resource> for v06::Resource {
    /// Drops `icons`.
    fn from(r: v11::Resource) -> Self {
        let v11::Resource {
            annotations,
            description,
            icons: _,
            meta,
            mime_type,
            name,
            size,
            title,
            uri,
        } = r;
        Self {
            annotations: annotations.map(Into::into),
            description,
            meta,
            mime_type,
            name,
            size,
            title,
            uri,
        }
    }
}

impl From<v06::Resource> for v11::Resource {
    fn from(r: v06::Resource) -> Self {
        let v06::Resource {
            annotations,
            description,
            meta,
            mime_type,
            name,
            size,
            title,
            uri,
        } = r;
        Self {
            annotations: annotations.map(Into::into),
            description,
            icons: Vec::new(),
            meta,
            mime_type,
            name,
            size,
            title,
            uri,
        }
    }
}

impl From<v11::ResourceTemplate> for v06::ResourceTemplate {
    /// Drops `icons`.
    fn from(t: v11::ResourceTemplate) -> Self {
        let v11::ResourceTemplate {
            annotations,
            description,
            icons: _,
            meta,
            mime_type,
            name,
            title,
            uri_template,
        } = t;
        Self {
            annotations: annotations.map(Into::into),
            description,
            meta,
            mime_type,
            name,
            title,
            uri_template,
        }
    }
}

impl From<v06::ResourceTemplate> for v11::ResourceTemplate {
    fn from(t: v06::ResourceTemplate) -> Self {
        let v06::ResourceTemplate {
            annotations,
            description,
            meta,
            mime_type,
            name,
            title,
            uri_template,
        } = t;
        Self {
            annotations: annotations.map(Into::into),
            description,
            icons: Vec::new(),
            meta,
            mime_type,
            name,
            title,
            uri_template,
        }
    }
}

// ---- prompts -------------------------------------------------------------------

impl From<v11::PromptArgument> for v06::PromptArgument {
    fn from(a: v11::PromptArgument) -> Self {
        let v11::PromptArgument {
            description,
            name,
            required,
            title,
        } = a;
        Self {
            description,
            name,
            required,
            title,
        }
    }
}

impl From<v06::PromptArgument> for v11::PromptArgument {
    fn from(a: v06::PromptArgument) -> Self {
        let v06::PromptArgument {
            description,
            name,
            required,
            title,
        } = a;
        Self {
            description,
            name,
            required,
            title,
        }
    }
}

impl From<v11::Prompt> for v06::Prompt {
    /// Drops `icons`.
    fn from(p: v11::Prompt) -> Self {
        let v11::Prompt {
            arguments,
            description,
            icons: _,
            meta,
            name,
            title,
        } = p;
        Self {
            arguments: arguments.into_iter().map(Into::into).collect(),
            description,
            meta,
            name,
            title,
        }
    }
}

impl From<v06::Prompt> for v11::Prompt {
    fn from(p: v06::Prompt) -> Self {
        let v06::Prompt {
            arguments,
            description,
            meta,
            name,
            title,
        } = p;
        Self {
            arguments: arguments.into_iter().map(Into::into).collect(),
            description,
            icons: Vec::new(),
            meta,
            name,
            title,
        }
    }
}

impl From<v11::PromptMessage> for v06::PromptMessage {
    fn from(m: v11::PromptMessage) -> Self {
        let v11::PromptMessage { content, role } = m;
        Self {
            content: content.into(),
            role: role.into(),
        }
    }
}

impl From<v06::PromptMessage> for v11::PromptMessage {
    fn from(m: v06::PromptMessage) -> Self {
        let v06::PromptMessage { content, role } = m;
        Self {
            content: content.into(),
            role: role.into(),
        }
    }
}

// ---- results -------------------------------------------------------------------

impl From<v11::ListToolsResult> for v06::ListToolsResult {
    fn from(r: v11::ListToolsResult) -> Self {
        let v11::ListToolsResult {
            meta,
            next_cursor,
            tools,
        } = r;
        Self {
            meta,
            next_cursor,
            tools: tools.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v06::ListToolsResult> for v11::ListToolsResult {
    fn from(r: v06::ListToolsResult) -> Self {
        let v06::ListToolsResult {
            meta,
            next_cursor,
            tools,
        } = r;
        Self {
            meta,
            next_cursor,
            tools: tools.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v11::CallToolResult> for v06::CallToolResult {
    fn from(r: v11::CallToolResult) -> Self {
        let v11::CallToolResult {
            content,
            is_error,
            meta,
            structured_content,
        } = r;
        Self {
            content: content.into_iter().map(Into::into).collect(),
            is_error,
            meta,
            structured_content,
        }
    }
}

impl From<v06::CallToolResult> for v11::CallToolResult {
    fn from(r: v06::CallToolResult) -> Self {
        let v06::CallToolResult {
            content,
            is_error,
            meta,
            structured_content,
        } = r;
        Self {
            content: content.into_iter().map(Into::into).collect(),
            is_error,
            meta,
            structured_content,
        }
    }
}

impl From<v11::ListResourcesResult> for v06::ListResourcesResult {
    fn from(r: v11::ListResourcesResult) -> Self {
        let v11::ListResourcesResult {
            meta,
            next_cursor,
            resources,
        } = r;
        Self {
            meta,
            next_cursor,
            resources: resources.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v06::ListResourcesResult> for v11::ListResourcesResult {
    fn from(r: v06::ListResourcesResult) -> Self {
        let v06::ListResourcesResult {
            meta,
            next_cursor,
            resources,
        } = r;
        Self {
            meta,
            next_cursor,
            resources: resources.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v11::ListResourceTemplatesResult> for v06::ListResourceTemplatesResult {
    fn from(r: v11::ListResourceTemplatesResult) -> Self {
        let v11::ListResourceTemplatesResult {
            meta,
            next_cursor,
            resource_templates,
        } = r;
        Self {
            meta,
            next_cursor,
            resource_templates: resource_templates.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v06::ListResourceTemplatesResult> for v11::ListResourceTemplatesResult {
    fn from(r: v06::ListResourceTemplatesResult) -> Self {
        let v06::ListResourceTemplatesResult {
            meta,
            next_cursor,
            resource_templates,
        } = r;
        Self {
            meta,
            next_cursor,
            resource_templates: resource_templates.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v11::ReadResourceResult> for v06::ReadResourceResult {
    fn from(r: v11::ReadResourceResult) -> Self {
        let v11::ReadResourceResult { contents, meta } = r;
        Self {
            contents: contents.into_iter().map(Into::into).collect(),
            meta,
        }
    }
}

impl From<v06::ReadResourceResult> for v11::ReadResourceResult {
    fn from(r: v06::ReadResourceResult) -> Self {
        let v06::ReadResourceResult { contents, meta } = r;
        Self {
            contents: contents.into_iter().map(Into::into).collect(),
            meta,
        }
    }
}

impl From<v11::ListPromptsResult> for v06::ListPromptsResult {
    fn from(r: v11::ListPromptsResult) -> Self {
        let v11::ListPromptsResult {
            meta,
            next_cursor,
            prompts,
        } = r;
        Self {
            meta,
            next_cursor,
            prompts: prompts.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v06::ListPromptsResult> for v11::ListPromptsResult {
    fn from(r: v06::ListPromptsResult) -> Self {
        let v06::ListPromptsResult {
            meta,
            next_cursor,
            prompts,
        } = r;
        Self {
            meta,
            next_cursor,
            prompts: prompts.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<v11::GetPromptResult> for v06::GetPromptResult {
    fn from(r: v11::GetPromptResult) -> Self {
        let v11::GetPromptResult {
            description,
            messages,
            meta,
        } = r;
        Self {
            description,
            messages: messages.into_iter().map(Into::into).collect(),
            meta,
        }
    }
}

impl From<v06::GetPromptResult> for v11::GetPromptResult {
    fn from(r: v06::GetPromptResult) -> Self {
        let v06::GetPromptResult {
            description,
            messages,
            meta,
        } = r;
        Self {
            description,
            messages: messages.into_iter().map(Into::into).collect(),
            meta,
        }
    }
}

impl From<v11::CompleteResultCompletion> for v06::CompleteResultCompletion {
    fn from(c: v11::CompleteResultCompletion) -> Self {
        let v11::CompleteResultCompletion {
            has_more,
            total,
            values,
        } = c;
        Self {
            has_more,
            total,
            values,
        }
    }
}

impl From<v06::CompleteResultCompletion> for v11::CompleteResultCompletion {
    fn from(c: v06::CompleteResultCompletion) -> Self {
        let v06::CompleteResultCompletion {
            has_more,
            total,
            values,
        } = c;
        Self {
            has_more,
            total,
            values,
        }
    }
}

impl From<v11::CompleteResult> for v06::CompleteResult {
    fn from(r: v11::CompleteResult) -> Self {
        let v11::CompleteResult { completion, meta } = r;
        Self {
            completion: completion.into(),
            meta,
        }
    }
}

impl From<v06::CompleteResult> for v11::CompleteResult {
    fn from(r: v06::CompleteResult) -> Self {
        let v06::CompleteResult { completion, meta } = r;
        Self {
            completion: completion.into(),
            meta,
        }
    }
}

// ---- handshake -----------------------------------------------------------------

impl From<v11::Implementation> for v06::Implementation {
    /// Drops `description`, `icons`, and `websiteUrl` — all `2025-11-25`
    /// additions to `Implementation`.
    fn from(i: v11::Implementation) -> Self {
        let v11::Implementation {
            description: _,
            icons: _,
            name,
            title,
            version,
            website_url: _,
        } = i;
        Self {
            name,
            title,
            version,
        }
    }
}

impl From<v06::Implementation> for v11::Implementation {
    fn from(i: v06::Implementation) -> Self {
        let v06::Implementation {
            name,
            title,
            version,
        } = i;
        Self {
            description: None,
            icons: Vec::new(),
            name,
            title,
            version,
            website_url: None,
        }
    }
}

impl From<v11::ServerCapabilitiesPrompts> for v06::ServerCapabilitiesPrompts {
    fn from(c: v11::ServerCapabilitiesPrompts) -> Self {
        let v11::ServerCapabilitiesPrompts { list_changed } = c;
        Self { list_changed }
    }
}

impl From<v06::ServerCapabilitiesPrompts> for v11::ServerCapabilitiesPrompts {
    fn from(c: v06::ServerCapabilitiesPrompts) -> Self {
        let v06::ServerCapabilitiesPrompts { list_changed } = c;
        Self { list_changed }
    }
}

impl From<v11::ServerCapabilitiesResources> for v06::ServerCapabilitiesResources {
    fn from(c: v11::ServerCapabilitiesResources) -> Self {
        let v11::ServerCapabilitiesResources {
            list_changed,
            subscribe,
        } = c;
        Self {
            list_changed,
            subscribe,
        }
    }
}

impl From<v06::ServerCapabilitiesResources> for v11::ServerCapabilitiesResources {
    fn from(c: v06::ServerCapabilitiesResources) -> Self {
        let v06::ServerCapabilitiesResources {
            list_changed,
            subscribe,
        } = c;
        Self {
            list_changed,
            subscribe,
        }
    }
}

impl From<v11::ServerCapabilitiesTools> for v06::ServerCapabilitiesTools {
    fn from(c: v11::ServerCapabilitiesTools) -> Self {
        let v11::ServerCapabilitiesTools { list_changed } = c;
        Self { list_changed }
    }
}

impl From<v06::ServerCapabilitiesTools> for v11::ServerCapabilitiesTools {
    fn from(c: v06::ServerCapabilitiesTools) -> Self {
        let v06::ServerCapabilitiesTools { list_changed } = c;
        Self { list_changed }
    }
}

impl From<v11::ServerCapabilities> for v06::ServerCapabilities {
    /// Drops `tasks`: `2025-06-18` has no Tasks methods, so advertising the
    /// capability would invite calls the server can only answer `-32601`.
    fn from(c: v11::ServerCapabilities) -> Self {
        let v11::ServerCapabilities {
            completions,
            experimental,
            logging,
            prompts,
            resources,
            tasks: _,
            tools,
        } = c;
        Self {
            completions,
            experimental,
            logging,
            prompts: prompts.map(Into::into),
            resources: resources.map(Into::into),
            tools: tools.map(Into::into),
        }
    }
}

impl From<v06::ServerCapabilities> for v11::ServerCapabilities {
    fn from(c: v06::ServerCapabilities) -> Self {
        let v06::ServerCapabilities {
            completions,
            experimental,
            logging,
            prompts,
            resources,
            tools,
        } = c;
        Self {
            completions,
            experimental,
            logging,
            prompts: prompts.map(Into::into),
            resources: resources.map(Into::into),
            tasks: None,
            tools: tools.map(Into::into),
        }
    }
}

impl From<v11::InitializeResult> for v06::InitializeResult {
    fn from(r: v11::InitializeResult) -> Self {
        let v11::InitializeResult {
            capabilities,
            instructions,
            meta,
            protocol_version,
            server_info,
        } = r;
        Self {
            capabilities: capabilities.into(),
            instructions,
            meta,
            protocol_version,
            server_info: server_info.into(),
        }
    }
}

impl From<v06::InitializeResult> for v11::InitializeResult {
    fn from(r: v06::InitializeResult) -> Self {
        let v06::InitializeResult {
            capabilities,
            instructions,
            meta,
            protocol_version,
            server_info,
        } = r;
        Self {
            capabilities: capabilities.into(),
            instructions,
            meta,
            protocol_version,
            server_info: server_info.into(),
        }
    }
}
