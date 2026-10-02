//! Typed interceptors: middleware over the version-neutral request and result
//! of each operation, after the wire is decoded and before it is encoded.
//!
//! Tower layers ([`ServerBuilder::layer`](crate::ServerBuilder::layer)) see
//! raw JSON-RPC frames, in three wire shapes. An [`Interceptor`] sees what a
//! handler sees: a typed context and neutral params going in, a neutral
//! result coming out, identical on every revision. It runs on every path a
//! call takes (plain, task-augmented, MRTR retries, a mounted
//! [`Composite`](crate::Composite) child), so a redaction or authorization
//! rule written once can't be bypassed per tool or per revision.
//!
//! Each hook receives the request and a [`Next`]; call
//! [`next.run(ctx, params)`](Next::run) to continue (with the context and
//! params as they are, or changed), inspect or rewrite the result, or return
//! without calling it to answer yourself. Every hook defaults to passing
//! straight through, so an interceptor implements only what it needs. For
//! the common one-hook case there are closures: [`on_call_tool`],
//! [`on_get_prompt`], [`on_read_resource`], [`on_list_tools`].
//!
//! ```ignore
//! use turbomcp::intercept::on_call_tool;
//!
//! // Redact every tool result, whichever tool and revision produced it.
//! MyServer.into_server().intercept(on_call_tool(|ctx, params, next| async move {
//!     let mut result = next.run(ctx, params).await?;
//!     redact(&mut result);
//!     Ok(result)
//! }))
//! ```
//!
//! Interceptors run in the order registered, the first outermost. An error
//! returned from a hook is answered as a handler's would be (a
//! `tool_execution_failed` becomes a tool error result the model reads). A
//! hook that holds client input in flight must pass
//! [`McpError::InputRequired`](turbomcp_core::McpError) through untouched:
//! it is how a multi-round-trip handler asks the client for input.
//!
//! Interceptors shape requests and results; they don't add components. To
//! contribute tools, prompts or resources, mount another server
//! ([`Composite`](crate::Composite)).

use std::sync::Arc;

use async_trait::async_trait;
use futures::future::BoxFuture;
use turbomcp_core::McpResult;
use turbomcp_protocol::neutral;

use crate::context::{
    CallToolContext, CompleteContext, GetPromptContext, ListPromptsContext,
    ListResourceTemplatesContext, ListResourcesContext, ListToolsContext, ReadResourceContext,
};

/// The rest of the chain: the interceptors after this one, then the
/// handler. Consumed by [`run`](Self::run).
pub struct Next<C, P, R> {
    chain: Arc<[Arc<dyn Interceptor>]>,
    index: usize,
    hook: Hook<C, P, R>,
    terminal: Terminal<C, P, R>,
}

/// One operation's hook on an interceptor.
pub(crate) type Hook<C, P, R> =
    fn(Arc<dyn Interceptor>, C, P, Next<C, P, R>) -> BoxFuture<'static, McpResult<R>>;

/// The handler at the end of the chain.
pub(crate) type Terminal<C, P, R> =
    Arc<dyn Fn(C, P) -> BoxFuture<'static, McpResult<R>> + Send + Sync>;

impl<C, P, R> core::fmt::Debug for Next<C, P, R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Next")
            .field("remaining", &(self.chain.len() - self.index))
            .finish_non_exhaustive()
    }
}

impl<C, P, R> Next<C, P, R> {
    pub(crate) fn new(
        chain: Arc<[Arc<dyn Interceptor>]>,
        hook: Hook<C, P, R>,
        terminal: Terminal<C, P, R>,
    ) -> Self {
        Self {
            chain,
            index: 0,
            hook,
            terminal,
        }
    }

    /// Continue with `ctx` and `params`: the next interceptor, or the
    /// handler.
    pub fn run(self, ctx: C, params: P) -> BoxFuture<'static, McpResult<R>> {
        match self.chain.get(self.index).cloned() {
            Some(interceptor) => {
                let next = Self {
                    chain: Arc::clone(&self.chain),
                    index: self.index + 1,
                    hook: self.hook,
                    terminal: self.terminal,
                };
                (self.hook)(interceptor, ctx, params, next)
            }
            None => (self.terminal)(ctx, params),
        }
    }
}

/// The rest of a `tools/call` chain.
pub type NextCallTool = Next<CallToolContext, neutral::CallToolParams, neutral::CallToolResult>;
/// The rest of a `prompts/get` chain.
pub type NextGetPrompt = Next<GetPromptContext, neutral::GetPromptParams, neutral::GetPromptResult>;
/// The rest of a `resources/read` chain.
pub type NextReadResource =
    Next<ReadResourceContext, neutral::ReadResourceParams, neutral::ReadResourceResult>;
/// The rest of a `tools/list` chain.
pub type NextListTools = Next<ListToolsContext, neutral::ListParams, neutral::ListToolsResult>;
/// The rest of a `prompts/list` chain.
pub type NextListPrompts =
    Next<ListPromptsContext, neutral::ListParams, neutral::ListPromptsResult>;
/// The rest of a `resources/list` chain.
pub type NextListResources =
    Next<ListResourcesContext, neutral::ListParams, neutral::ListResourcesResult>;
/// The rest of a `resources/templates/list` chain.
pub type NextListResourceTemplates =
    Next<ListResourceTemplatesContext, neutral::ListParams, neutral::ListResourceTemplatesResult>;
/// The rest of a `completion/complete` chain.
pub type NextComplete = Next<CompleteContext, neutral::CompleteParams, neutral::CompleteResult>;

/// Middleware over each operation's neutral request and result. See
/// [the module docs](self).
#[async_trait]
pub trait Interceptor: Send + Sync + 'static {
    /// `tools/call`.
    async fn call_tool(
        &self,
        ctx: CallToolContext,
        params: neutral::CallToolParams,
        next: NextCallTool,
    ) -> McpResult<neutral::CallToolResult> {
        next.run(ctx, params).await
    }

    /// `prompts/get`.
    async fn get_prompt(
        &self,
        ctx: GetPromptContext,
        params: neutral::GetPromptParams,
        next: NextGetPrompt,
    ) -> McpResult<neutral::GetPromptResult> {
        next.run(ctx, params).await
    }

    /// `resources/read`.
    async fn read_resource(
        &self,
        ctx: ReadResourceContext,
        params: neutral::ReadResourceParams,
        next: NextReadResource,
    ) -> McpResult<neutral::ReadResourceResult> {
        next.run(ctx, params).await
    }

    /// `tools/list`, one page.
    async fn list_tools(
        &self,
        ctx: ListToolsContext,
        params: neutral::ListParams,
        next: NextListTools,
    ) -> McpResult<neutral::ListToolsResult> {
        next.run(ctx, params).await
    }

    /// `prompts/list`, one page.
    async fn list_prompts(
        &self,
        ctx: ListPromptsContext,
        params: neutral::ListParams,
        next: NextListPrompts,
    ) -> McpResult<neutral::ListPromptsResult> {
        next.run(ctx, params).await
    }

    /// `resources/list`, one page.
    async fn list_resources(
        &self,
        ctx: ListResourcesContext,
        params: neutral::ListParams,
        next: NextListResources,
    ) -> McpResult<neutral::ListResourcesResult> {
        next.run(ctx, params).await
    }

    /// `resources/templates/list`, one page.
    async fn list_resource_templates(
        &self,
        ctx: ListResourceTemplatesContext,
        params: neutral::ListParams,
        next: NextListResourceTemplates,
    ) -> McpResult<neutral::ListResourceTemplatesResult> {
        next.run(ctx, params).await
    }

    /// `completion/complete`.
    async fn complete(
        &self,
        ctx: CompleteContext,
        params: neutral::CompleteParams,
        next: NextComplete,
    ) -> McpResult<neutral::CompleteResult> {
        next.run(ctx, params).await
    }
}

/// Each operation's [`Hook`], for [`Next::new`].
pub(crate) mod hooks {
    use super::*;

    macro_rules! hook {
        ($name:ident, $method:ident, $ctx:ty, $params:ty, $result:ty) => {
            pub(crate) fn $name(
                interceptor: Arc<dyn Interceptor>,
                ctx: $ctx,
                params: $params,
                next: Next<$ctx, $params, $result>,
            ) -> BoxFuture<'static, McpResult<$result>> {
                Box::pin(async move { interceptor.$method(ctx, params, next).await })
            }
        };
    }

    hook!(
        call_tool,
        call_tool,
        CallToolContext,
        neutral::CallToolParams,
        neutral::CallToolResult
    );
    hook!(
        get_prompt,
        get_prompt,
        GetPromptContext,
        neutral::GetPromptParams,
        neutral::GetPromptResult
    );
    hook!(
        read_resource,
        read_resource,
        ReadResourceContext,
        neutral::ReadResourceParams,
        neutral::ReadResourceResult
    );
    hook!(
        list_tools,
        list_tools,
        ListToolsContext,
        neutral::ListParams,
        neutral::ListToolsResult
    );
    hook!(
        list_prompts,
        list_prompts,
        ListPromptsContext,
        neutral::ListParams,
        neutral::ListPromptsResult
    );
    hook!(
        list_resources,
        list_resources,
        ListResourcesContext,
        neutral::ListParams,
        neutral::ListResourcesResult
    );
    hook!(
        list_resource_templates,
        list_resource_templates,
        ListResourceTemplatesContext,
        neutral::ListParams,
        neutral::ListResourceTemplatesResult
    );
    hook!(
        complete,
        complete,
        CompleteContext,
        neutral::CompleteParams,
        neutral::CompleteResult
    );
}

macro_rules! closure_interceptor {
    (
        $(#[$doc:meta])*
        $fn_name:ident, $ty:ident, $method:ident, $ctx:ty, $params:ty, $result:ty, $next:ty
    ) => {
        $(#[$doc])*
        pub fn $fn_name<F, Fut>(f: F) -> Arc<dyn Interceptor>
        where
            F: Fn($ctx, $params, $next) -> Fut + Send + Sync + 'static,
            Fut: core::future::Future<Output = McpResult<$result>> + Send + 'static,
        {
            Arc::new($ty(f))
        }

        struct $ty<F>(F);

        #[async_trait]
        impl<F, Fut> Interceptor for $ty<F>
        where
            F: Fn($ctx, $params, $next) -> Fut + Send + Sync + 'static,
            Fut: core::future::Future<Output = McpResult<$result>> + Send + 'static,
        {
            async fn $method(&self, ctx: $ctx, params: $params, next: $next) -> McpResult<$result> {
                (self.0)(ctx, params, next).await
            }
        }
    };
}

closure_interceptor!(
    /// An interceptor around every `tools/call`.
    on_call_tool,
    OnCallTool,
    call_tool,
    CallToolContext,
    neutral::CallToolParams,
    neutral::CallToolResult,
    NextCallTool
);
closure_interceptor!(
    /// An interceptor around every `prompts/get`.
    on_get_prompt,
    OnGetPrompt,
    get_prompt,
    GetPromptContext,
    neutral::GetPromptParams,
    neutral::GetPromptResult,
    NextGetPrompt
);
closure_interceptor!(
    /// An interceptor around every `resources/read`.
    on_read_resource,
    OnReadResource,
    read_resource,
    ReadResourceContext,
    neutral::ReadResourceParams,
    neutral::ReadResourceResult,
    NextReadResource
);
closure_interceptor!(
    /// An interceptor around every `tools/list` page.
    on_list_tools,
    OnListTools,
    list_tools,
    ListToolsContext,
    neutral::ListParams,
    neutral::ListToolsResult,
    NextListTools
);
