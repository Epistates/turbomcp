//! [`MethodRouter`]: the capability registry that backs a [`VersionDispatcher`].
//!
//! The router stores one type-erased handler closure per supported method.
//! Registration methods (`with_tools`, `with_resources`, …) are bounded on the
//! corresponding capability trait, so a server can only register `tools/*` if it
//! actually implements [`WithTools`] — yet the *dispatcher* needs only
//! [`McpServerCore`], because the trait bound is erased into the stored closure
//! (axum's pattern).
//!
//! Two consequences fall out for free:
//! - Advertised capabilities are derived from what's registered ([`has_tools`]
//!   et al.), so they cannot drift from the handlers.
//! - A future macro (`#[server]`, Phase 3) registers exactly the capabilities
//!   the user implemented, with no per-server boilerplate here.
//!
//! Every handler closure has the same shape — `Fn(S, Ctx, Params) -> BoxFuture`
//! — so the slot type aliases and dispatch methods are generated from two small
//! macros to keep them in lockstep.
//!
//! [`VersionDispatcher`]: crate::VersionDispatcher
//! [`has_tools`]: MethodRouter::has_tools

use futures::future::BoxFuture;

use turbomcp_core::McpResult;
use turbomcp_protocol::neutral;

use crate::context::{
    CallToolContext, CompleteContext, GetPromptContext, ListPromptsContext,
    ListResourceTemplatesContext, ListResourcesContext, ListToolsContext, ReadResourceContext,
};
use crate::traits::{McpServerCore, WithCompletions, WithPrompts, WithResources, WithTools};

/// Type alias for a type-erased `Fn(S, Ctx, Params) -> BoxFuture<Result>` slot.
macro_rules! handler_slot {
    ($alias:ident<$s:ident>, $ctx:ty, $params:ty, $result:ty) => {
        type $alias<$s> =
            Box<dyn Fn($s, $ctx, $params) -> BoxFuture<'static, McpResult<$result>> + Send + Sync>;
    };
}

handler_slot!(
    ListToolsHandler<S>,
    ListToolsContext,
    neutral::ListParams,
    neutral::ListToolsResult
);
handler_slot!(
    CallToolHandler<S>,
    CallToolContext,
    neutral::CallToolParams,
    neutral::CallToolResult
);
handler_slot!(
    ListResourcesHandler<S>,
    ListResourcesContext,
    neutral::ListParams,
    neutral::ListResourcesResult
);
handler_slot!(
    ReadResourceHandler<S>,
    ReadResourceContext,
    neutral::ReadResourceParams,
    neutral::ReadResourceResult
);
handler_slot!(
    ListResourceTemplatesHandler<S>,
    ListResourceTemplatesContext,
    neutral::ListParams,
    neutral::ListResourceTemplatesResult
);
handler_slot!(
    ListPromptsHandler<S>,
    ListPromptsContext,
    neutral::ListParams,
    neutral::ListPromptsResult
);
handler_slot!(
    GetPromptHandler<S>,
    GetPromptContext,
    neutral::GetPromptParams,
    neutral::GetPromptResult
);
handler_slot!(
    CompleteHandler<S>,
    CompleteContext,
    neutral::CompleteParams,
    neutral::CompleteResult
);

/// Lists the capabilities actually registered, which is the question worth
/// asking of a router. Unbounded in `S` on purpose: requiring `Debug` of the
/// user's server type would stop anything holding a router from deriving its
/// own.
impl<S> core::fmt::Debug for MethodRouter<S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut registered: Vec<&str> = Vec::new();
        for (name, present) in [
            ("list_tools", self.list_tools.is_some()),
            ("call_tool", self.call_tool.is_some()),
            ("list_resources", self.list_resources.is_some()),
            ("read_resource", self.read_resource.is_some()),
            (
                "list_resource_templates",
                self.list_resource_templates.is_some(),
            ),
            ("list_prompts", self.list_prompts.is_some()),
            ("get_prompt", self.get_prompt.is_some()),
            ("complete", self.complete.is_some()),
        ] {
            if present {
                registered.push(name);
            }
        }
        f.debug_struct("MethodRouter")
            .field("registered", &registered)
            .finish_non_exhaustive()
    }
}

/// Per-method handler table, generic over the server type `S`.
pub struct MethodRouter<S> {
    list_tools: Option<ListToolsHandler<S>>,
    call_tool: Option<CallToolHandler<S>>,
    list_resources: Option<ListResourcesHandler<S>>,
    read_resource: Option<ReadResourceHandler<S>>,
    list_resource_templates: Option<ListResourceTemplatesHandler<S>>,
    list_prompts: Option<ListPromptsHandler<S>>,
    get_prompt: Option<GetPromptHandler<S>>,
    complete: Option<CompleteHandler<S>>,
    logging: bool,
}

impl<S> Default for MethodRouter<S> {
    fn default() -> Self {
        Self {
            list_tools: None,
            call_tool: None,
            list_resources: None,
            read_resource: None,
            list_resource_templates: None,
            list_prompts: None,
            get_prompt: None,
            complete: None,
            logging: false,
        }
    }
}

/// Emit a `pub(crate) fn $name(&self, server, ctx, params) -> Option<BoxFuture>`
/// that invokes the registered handler in `self.$field`, if present.
macro_rules! dispatch_fn {
    ($name:ident, $field:ident, $ctx:ty, $params:ty, $result:ty) => {
        pub(crate) fn $name(
            &self,
            server: S,
            ctx: $ctx,
            params: $params,
        ) -> Option<BoxFuture<'static, McpResult<$result>>> {
            self.$field.as_ref().map(|h| h(server, ctx, params))
        }
    };
}

impl<S: McpServerCore> MethodRouter<S> {
    /// An empty router — no capabilities registered.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register the `tools/*` methods. Available only when `S: WithTools`; the
    /// bound is erased into the stored closures so the dispatcher stays generic
    /// over plain `McpServerCore`.
    #[must_use]
    pub fn with_tools(mut self) -> Self
    where
        S: WithTools,
    {
        self.list_tools = Some(Box::new(|server: S, ctx, params| {
            Box::pin(async move { server.list_tools(&ctx, params).await })
        }));
        self.call_tool = Some(Box::new(|server: S, ctx, params| {
            Box::pin(async move { server.call_tool(&ctx, params).await })
        }));
        self
    }

    /// Register the `resources/*` methods. Available only when `S: WithResources`.
    #[must_use]
    pub fn with_resources(mut self) -> Self
    where
        S: WithResources,
    {
        self.list_resources = Some(Box::new(|server: S, ctx, params| {
            Box::pin(async move { server.list_resources(&ctx, params).await })
        }));
        self.read_resource = Some(Box::new(|server: S, ctx, params| {
            Box::pin(async move { server.read_resource(&ctx, params).await })
        }));
        self.list_resource_templates = Some(Box::new(|server: S, ctx, params| {
            Box::pin(async move { server.list_resource_templates(&ctx, params).await })
        }));
        self
    }

    /// Register the `prompts/*` methods. Available only when `S: WithPrompts`.
    #[must_use]
    pub fn with_prompts(mut self) -> Self
    where
        S: WithPrompts,
    {
        self.list_prompts = Some(Box::new(|server: S, ctx, params| {
            Box::pin(async move { server.list_prompts(&ctx, params).await })
        }));
        self.get_prompt = Some(Box::new(|server: S, ctx, params| {
            Box::pin(async move { server.get_prompt(&ctx, params).await })
        }));
        self
    }

    /// Register `completion/complete`. Available only when `S: WithCompletions`.
    #[must_use]
    pub fn with_completions(mut self) -> Self
    where
        S: WithCompletions,
    {
        self.complete = Some(Box::new(|server: S, ctx, params| {
            Box::pin(async move { server.complete(&ctx, params).await })
        }));
        self
    }

    /// Enable the `logging` capability (framework-implemented, no handler
    /// trait): handlers gain a live `ctx.log` when the client opts in —
    /// `logging/setLevel` per session on `2025-11-25`, the per-request
    /// `_meta` `io.modelcontextprotocol/logLevel` on the draft (where the
    /// feature is deprecated by SEP-2577 but remains functional).
    #[must_use]
    pub fn with_logging(mut self) -> Self {
        self.logging = true;
        self
    }

    /// Whether `tools/*` is served (drives capability advertisement).
    #[must_use]
    pub fn has_tools(&self) -> bool {
        self.list_tools.is_some()
    }

    /// Whether `resources/*` is served.
    #[must_use]
    pub fn has_resources(&self) -> bool {
        self.list_resources.is_some()
    }

    /// Whether `prompts/*` is served.
    #[must_use]
    pub fn has_prompts(&self) -> bool {
        self.list_prompts.is_some()
    }

    /// Whether `completion/complete` is served.
    #[must_use]
    pub fn has_completions(&self) -> bool {
        self.complete.is_some()
    }

    /// Whether the `logging` capability is enabled.
    #[must_use]
    pub fn has_logging(&self) -> bool {
        self.logging
    }

    dispatch_fn!(
        dispatch_list_tools,
        list_tools,
        ListToolsContext,
        neutral::ListParams,
        neutral::ListToolsResult
    );
    dispatch_fn!(
        dispatch_call_tool,
        call_tool,
        CallToolContext,
        neutral::CallToolParams,
        neutral::CallToolResult
    );
    dispatch_fn!(
        dispatch_list_resources,
        list_resources,
        ListResourcesContext,
        neutral::ListParams,
        neutral::ListResourcesResult
    );
    dispatch_fn!(
        dispatch_read_resource,
        read_resource,
        ReadResourceContext,
        neutral::ReadResourceParams,
        neutral::ReadResourceResult
    );
    dispatch_fn!(
        dispatch_list_resource_templates,
        list_resource_templates,
        ListResourceTemplatesContext,
        neutral::ListParams,
        neutral::ListResourceTemplatesResult
    );
    dispatch_fn!(
        dispatch_list_prompts,
        list_prompts,
        ListPromptsContext,
        neutral::ListParams,
        neutral::ListPromptsResult
    );
    dispatch_fn!(
        dispatch_get_prompt,
        get_prompt,
        GetPromptContext,
        neutral::GetPromptParams,
        neutral::GetPromptResult
    );
    dispatch_fn!(
        dispatch_complete,
        complete,
        CompleteContext,
        neutral::CompleteParams,
        neutral::CompleteResult
    );
}
