//! The user-facing server traits: one required core trait plus one trait per
//! capability. A server *is* what it implements — the [`MethodRouter`] derives
//! advertised capabilities from which of these are present, so there is no way
//! for declared capabilities to drift from actual handlers.
//!
//! Capability methods use native async-fn-in-trait (RPITIT) and speak
//! [`turbomcp_protocol::neutral`] types, never wire types. That is what lets a
//! second wire version (Phase 5) be added without touching a single handler.
//!
//! [`MethodRouter`]: crate::MethodRouter

use core::future::Future;

use turbomcp_core::{Implementation, McpResult, ProtocolVersion};
use turbomcp_protocol::neutral;

use crate::context::{
    CallToolContext, CompleteContext, GetPromptContext, ListPromptsContext,
    ListResourceTemplatesContext, ListResourcesContext, ListToolsContext, ReadResourceContext,
};

/// The required server trait. Every server implements this; capability traits
/// (`WithTools`, …) are added à la carte.
///
/// `Clone + Send + Sync + 'static`: the dispatcher clones the server per request
/// and the handler runs on a spawned task. Keep the server cheap to clone (wrap
/// shared state in `Arc`).
pub trait McpServerCore: Clone + Send + Sync + 'static {
    /// Identity of this server implementation (name, version, optional title).
    fn server_info(&self) -> Implementation;

    /// Protocol versions this server accepts. Defaults to
    /// [`ProtocolVersion::SUPPORTED`] (the current stable set).
    fn supported_versions(&self) -> &'static [ProtocolVersion] {
        ProtocolVersion::SUPPORTED
    }

    /// Natural-language guidance returned to clients in discovery, to help an
    /// LLM use the server effectively. `None` by default.
    fn instructions(&self) -> Option<String> {
        None
    }
}

/// Implement to serve tools (`tools/list`, `tools/call`).
pub trait WithTools: McpServerCore {
    /// Look up an executable component independently of list pagination.
    /// Override for an indexed/dynamic catalog. The default follows bounded,
    /// advancing pages and propagates lookup errors; absence is explicit.
    fn lookup_tool(
        &self,
        ctx: &ListToolsContext,
        name: String,
    ) -> impl Future<Output = McpResult<Option<neutral::Tool>>> + Send {
        async move {
            crate::catalog::find(
                |params| async move {
                    let page = self.list_tools(ctx, params).await?;
                    Ok((page.tools, page.next_cursor))
                },
                |tool: &neutral::Tool| tool.name == name,
            )
            .await
        }
    }

    /// Enumerate available tools. `params.cursor` continues a prior page.
    ///
    /// Return tools in a **deterministic order** across calls (the spec
    /// SHOULD: stable ordering enables client-side caching and improves LLM
    /// prompt-cache hit rates). `#[server]`-generated impls list tools in
    /// declaration order, which satisfies this.
    fn list_tools(
        &self,
        ctx: &ListToolsContext,
        params: neutral::ListParams,
    ) -> impl Future<Output = McpResult<neutral::ListToolsResult>> + Send;

    /// Invoke a tool. Per spec, tool-level failure is reported via
    /// [`neutral::CallToolResult::error`] (so the model can self-correct), not
    /// as a JSON-RPC error.
    fn call_tool(
        &self,
        ctx: &CallToolContext,
        params: neutral::CallToolParams,
    ) -> impl Future<Output = McpResult<neutral::CallToolResult>> + Send;
}

/// Implement to serve resources (`resources/list`, `resources/read`, and
/// optionally `resources/templates/list`).
pub trait WithResources: McpServerCore {
    /// Look up an executable component independently of list pagination.
    /// Override for an indexed/dynamic catalog. The default follows bounded,
    /// advancing pages and propagates lookup errors; absence is explicit.
    fn lookup_resource_template(
        &self,
        ctx: &ListResourceTemplatesContext,
        name: String,
    ) -> impl Future<Output = McpResult<Option<neutral::ResourceTemplate>>> + Send {
        async move {
            crate::catalog::find(
                |params| async move {
                    let page = self.list_resource_templates(ctx, params).await?;
                    Ok((page.resource_templates, page.next_cursor))
                },
                |tool: &neutral::ResourceTemplate| {
                    crate::__macro_support::match_uri_template(&tool.uri_template, &name).is_some()
                },
            )
            .await
        }
    }

    /// Look up an executable component independently of list pagination.
    /// Override for an indexed/dynamic catalog. The default follows bounded,
    /// advancing pages and propagates lookup errors; absence is explicit.
    fn lookup_resource(
        &self,
        ctx: &ListResourcesContext,
        name: String,
    ) -> impl Future<Output = McpResult<Option<neutral::Resource>>> + Send {
        async move {
            crate::catalog::find(
                |params| async move {
                    let page = self.list_resources(ctx, params).await?;
                    Ok((page.resources, page.next_cursor))
                },
                |tool: &neutral::Resource| tool.uri == name,
            )
            .await
        }
    }

    /// Enumerate concrete resources. `params.cursor` continues a prior page.
    fn list_resources(
        &self,
        ctx: &ListResourcesContext,
        params: neutral::ListParams,
    ) -> impl Future<Output = McpResult<neutral::ListResourcesResult>> + Send;

    /// Read a resource by URI.
    fn read_resource(
        &self,
        ctx: &ReadResourceContext,
        params: neutral::ReadResourceParams,
    ) -> impl Future<Output = McpResult<neutral::ReadResourceResult>> + Send;

    /// Enumerate resource templates (RFC 6570 URI Templates). Defaults to an
    /// empty list, so servers exposing only concrete resources need not override
    /// it; `resources/templates/list` is still answered (with no templates).
    fn list_resource_templates(
        &self,
        _ctx: &ListResourceTemplatesContext,
        _params: neutral::ListParams,
    ) -> impl Future<Output = McpResult<neutral::ListResourceTemplatesResult>> + Send {
        core::future::ready(Ok(neutral::ListResourceTemplatesResult::default()))
    }
}

/// Implement to serve prompts (`prompts/list`, `prompts/get`).
pub trait WithPrompts: McpServerCore {
    /// Look up an executable component independently of list pagination.
    /// Override for an indexed/dynamic catalog. The default follows bounded,
    /// advancing pages and propagates lookup errors; absence is explicit.
    fn lookup_prompt(
        &self,
        ctx: &ListPromptsContext,
        name: String,
    ) -> impl Future<Output = McpResult<Option<neutral::Prompt>>> + Send {
        async move {
            crate::catalog::find(
                |params| async move {
                    let page = self.list_prompts(ctx, params).await?;
                    Ok((page.prompts, page.next_cursor))
                },
                |tool: &neutral::Prompt| tool.name == name,
            )
            .await
        }
    }

    /// Enumerate available prompts. `params.cursor` continues a prior page.
    fn list_prompts(
        &self,
        ctx: &ListPromptsContext,
        params: neutral::ListParams,
    ) -> impl Future<Output = McpResult<neutral::ListPromptsResult>> + Send;

    /// Render a prompt to a message sequence, applying `params.arguments`.
    fn get_prompt(
        &self,
        ctx: &GetPromptContext,
        params: neutral::GetPromptParams,
    ) -> impl Future<Output = McpResult<neutral::GetPromptResult>> + Send;
}

/// Implement to serve argument autocompletion (`completion/complete`).
pub trait WithCompletions: McpServerCore {
    /// Suggest completions for the partially-typed argument in `params`.
    fn complete(
        &self,
        ctx: &CompleteContext,
        params: neutral::CompleteParams,
    ) -> impl Future<Output = McpResult<neutral::CompleteResult>> + Send;
}
