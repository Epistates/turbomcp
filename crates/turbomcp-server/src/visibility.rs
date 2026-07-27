//! Progressive disclosure: deciding, per caller, which components exist.
//!
//! A [`VisibilityPolicy`] is consulted once per component. Install one with
//! [`ServerBuilder::with_visibility`](crate::ServerBuilder::with_visibility) and
//! the dispatcher applies it to `tools/list`, `resources/list`,
//! `resources/templates/list`, and `prompts/list` — and to the *calls*:
//!
//! ```no_run
//! # use turbomcp_server::{IntoServerBuilder, McpServerCore, Visibility};
//! # use turbomcp_core::Implementation;
//! # use std::sync::Arc;
//! # #[derive(Clone)] struct MyServer;
//! # impl McpServerCore for MyServer {
//! #     fn server_info(&self) -> Implementation { Implementation::new("s", "1.0") }
//! # }
//! let dispatcher = MyServer
//!     .into_server()
//!     .with_visibility(Arc::new(
//!         Visibility::new()
//!             .hiding_tagged(["internal", "experimental"])
//!             .requiring_declared_scopes(),
//!     ))
//!     .build();
//! ```
//!
//! # Hidden means unreachable
//!
//! Filtering a list without refusing the call would be theatre — the names are
//! guessable and the list is not the only way to learn them. So a hidden
//! component is also not callable, and it is refused **exactly as one that does
//! not exist**: an unknown-tool result, an unknown-prompt error, a
//! resource-not-found. A distinct "forbidden" answer would disclose the very
//! existence the policy is hiding.
//!
//! That guarantee costs a list per call: the policy decides on a component's
//! metadata, and a `tools/call` carries only a name. The cost is paid only when
//! a policy is installed.
//!
//! # Visibility is not authorization
//!
//! [`Visibility::requiring_declared_scopes`] hides what the caller could not
//! call anyway — it is the missing *list* half of `#[tool(scopes(…))]`, which
//! has always refused the call. Enforcement remains the scope check on the call
//! path; this makes the catalog agree with it.
//!
//! # No session map
//!
//! v3's `VisibilityLayer` owned a per-session enable/disable map, and its own
//! documentation warned that the map leaks without explicit cleanup. There is no
//! such map here. A policy is a function of `(component, request)`, and
//! [`VisibleComponent::request`] carries the full [`RequestContext`] — so a
//! deployment that really does want per-session unlocking implements the trait
//! over its own store and owns that store's lifecycle, rather than inheriting a
//! leak from the framework.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::{Map, Value};
use turbomcp_core::{RequestContext, meta};
use turbomcp_protocol::neutral;

use crate::tags;

/// Which kind of component is being judged. `#[non_exhaustive]` because the
/// protocol may grow another listable kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ComponentKind {
    /// A tool (`tools/list`, `tools/call`).
    Tool,
    /// A concrete resource (`resources/list`, `resources/read`).
    Resource,
    /// A resource template (`resources/templates/list`).
    ResourceTemplate,
    /// A prompt (`prompts/list`, `prompts/get`).
    Prompt,
}

/// One component, as a [`VisibilityPolicy`] sees it.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct VisibleComponent<'a> {
    /// Which kind of component this is.
    pub kind: ComponentKind,
    /// How the component is addressed: a tool or prompt *name*, or a resource
    /// URI / URI template. Under composition a name is already the mounted,
    /// prefixed one (`weather.forecast`) — which is what the caller sends.
    pub id: &'a str,
    /// The component's `_meta`, holding whatever the markers wrote:
    /// [`tags`](crate::tags) and declared scopes among them.
    pub meta: &'a Map<String, Value>,
    /// The request asking. Carries `identity`, the protocol version, and the
    /// propagated `_meta` — everything a policy needs to key on a caller or a
    /// session without the framework owning a store.
    pub request: &'a RequestContext,
}

impl VisibleComponent<'_> {
    /// The component's tags (see [`crate::tags`]).
    pub fn tags(&self) -> impl Iterator<Item = &str> {
        tags::of(self.meta)
    }

    /// The OAuth scopes the component declares it requires
    /// (`#[tool(scopes(…))]`). Empty when it declares none.
    pub fn declared_scopes(&self) -> impl Iterator<Item = &str> {
        self.meta
            .get(meta::keys::SCOPES)
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
    }
}

/// Decides whether a caller may see — and therefore reach — a component.
///
/// Implement this for anything the built-in [`Visibility`] does not cover, such
/// as per-session progressive disclosure keyed on your own store.
pub trait VisibilityPolicy: Send + Sync + 'static {
    /// `false` hides the component from its list *and* makes it unreachable,
    /// answering as though it did not exist.
    ///
    /// Called once per component per list, and once per call — keep it cheap
    /// and non-blocking.
    fn is_visible(&self, component: &VisibleComponent<'_>) -> bool;
}

impl<F> VisibilityPolicy for F
where
    F: Fn(&VisibleComponent<'_>) -> bool + Send + Sync + 'static,
{
    fn is_visible(&self, component: &VisibleComponent<'_>) -> bool {
        self(component)
    }
}

/// The two policies deployments actually reach for: hide by tag, and hide what
/// the caller lacks the scopes to use. Both may be on at once; a component is
/// visible only if it passes both.
#[derive(Clone, Debug, Default)]
pub struct Visibility {
    hidden_tags: BTreeSet<String>,
    declared_scopes: bool,
}

impl Visibility {
    /// A policy that hides nothing. Chain the builders below.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Hide any component carrying one of `tags` (see
    /// [`tags(…)`](macro@turbomcp_macros::tool) on the markers).
    #[must_use]
    pub fn hiding_tagged<I, T>(mut self, tags: I) -> Self
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        self.hidden_tags.extend(tags.into_iter().map(Into::into));
        self
    }

    /// Hide any component whose declared scopes (`#[tool(scopes(…))]`) the
    /// caller does not hold — the list half of a check the call path has always
    /// made.
    #[must_use]
    pub fn requiring_declared_scopes(mut self) -> Self {
        self.declared_scopes = true;
        self
    }
}

impl VisibilityPolicy for Visibility {
    fn is_visible(&self, component: &VisibleComponent<'_>) -> bool {
        if !self.hidden_tags.is_empty() && component.tags().any(|t| self.hidden_tags.contains(t)) {
            return false;
        }
        if self.declared_scopes {
            let required: Vec<&str> = component.declared_scopes().collect();
            if !required.is_empty() && !component.request.identity.has_scopes(&required) {
                return false;
            }
        }
        true
    }
}

/// The dispatcher's installed policy, if any.
pub(crate) type Policy = Option<Arc<dyn VisibilityPolicy>>;

/// Retain only the components `policy` admits.
///
/// Each list type names its own id field, so the caller supplies it; everything
/// else is shared.
macro_rules! retain_visible {
    ($policy:expr, $ctx:expr, $kind:expr, $items:expr, $id:ident) => {
        if let Some(policy) = $policy {
            $items.retain(|item| {
                policy.is_visible(&VisibleComponent {
                    kind: $kind,
                    id: &item.$id,
                    meta: &item.meta,
                    request: $ctx,
                })
            });
        }
    };
}

/// Apply `policy` to a `tools/list` result in place.
pub(crate) fn filter_tools(
    policy: &Policy,
    ctx: &RequestContext,
    result: &mut neutral::ListToolsResult,
) {
    retain_visible!(policy, ctx, ComponentKind::Tool, result.tools, name);
}

/// Apply `policy` to a `resources/list` result in place.
pub(crate) fn filter_resources(
    policy: &Policy,
    ctx: &RequestContext,
    result: &mut neutral::ListResourcesResult,
) {
    retain_visible!(policy, ctx, ComponentKind::Resource, result.resources, uri);
}

/// Apply `policy` to a `resources/templates/list` result in place.
pub(crate) fn filter_resource_templates(
    policy: &Policy,
    ctx: &RequestContext,
    result: &mut neutral::ListResourceTemplatesResult,
) {
    retain_visible!(
        policy,
        ctx,
        ComponentKind::ResourceTemplate,
        result.resource_templates,
        uri_template
    );
}

/// Apply `policy` to a `prompts/list` result in place.
pub(crate) fn filter_prompts(
    policy: &Policy,
    ctx: &RequestContext,
    result: &mut neutral::ListPromptsResult,
) {
    retain_visible!(policy, ctx, ComponentKind::Prompt, result.prompts, name);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use turbomcp_core::{Claims, Identity, ProtocolVersion};

    fn caller(scopes: &str) -> RequestContext {
        let mut claims = Claims::new();
        claims.insert("scope".into(), json!(scopes));
        RequestContext::new(ProtocolVersion::V2025_11_25).with_identity(Identity::Bearer {
            sub: "alice".into(),
            claims,
        })
    }

    fn tool(meta: Value) -> neutral::Tool {
        let mut t = neutral::Tool::new("wipe", json!({"type": "object"}));
        if let Value::Object(m) = meta {
            t.meta = m;
        }
        t
    }

    fn visible(policy: &impl VisibilityPolicy, tool: &neutral::Tool, ctx: &RequestContext) -> bool {
        policy.is_visible(&VisibleComponent {
            kind: ComponentKind::Tool,
            id: &tool.name,
            meta: &tool.meta,
            request: ctx,
        })
    }

    #[test]
    fn a_bare_policy_hides_nothing() {
        let ctx = caller("read");
        assert!(visible(
            &Visibility::new(),
            &tool(json!({ "io.turbomcp/tags": ["internal"] })),
            &ctx
        ));
    }

    #[test]
    fn tagged_components_are_hidden() {
        let policy = Visibility::new().hiding_tagged(["internal"]);
        let ctx = caller("read");
        assert!(!visible(
            &policy,
            &tool(json!({ "io.turbomcp/tags": ["internal", "beta"] })),
            &ctx
        ));
        assert!(visible(
            &policy,
            &tool(json!({ "io.turbomcp/tags": ["beta"] })),
            &ctx
        ));
        assert!(visible(&policy, &tool(json!({})), &ctx));
    }

    #[test]
    fn declared_scopes_are_matched_against_the_caller() {
        let policy = Visibility::new().requiring_declared_scopes();
        let admin = tool(json!({ "io.turbomcp/scopes": ["admin"] }));

        assert!(!visible(&policy, &admin, &caller("read")));
        assert!(visible(&policy, &admin, &caller("read admin")));
        // A component declaring nothing is visible to anyone, including a
        // caller with no scopes at all.
        assert!(visible(&policy, &tool(json!({})), &caller("")));
        assert!(visible(
            &policy,
            &tool(json!({})),
            &RequestContext::new(ProtocolVersion::V2025_11_25)
        ));
    }

    #[test]
    fn every_enabled_check_must_pass() {
        let policy = Visibility::new()
            .hiding_tagged(["internal"])
            .requiring_declared_scopes();
        let ctx = caller("admin");
        // Holds the scope, but the tag still hides it.
        assert!(!visible(
            &policy,
            &tool(json!({ "io.turbomcp/tags": ["internal"], "io.turbomcp/scopes": ["admin"] })),
            &ctx
        ));
    }

    #[test]
    fn a_closure_is_a_policy() {
        // The escape hatch for anything the built-in doesn't cover — including
        // per-session state a deployment stores itself.
        let policy = |c: &VisibleComponent<'_>| c.id != "wipe";
        assert!(!visible(&policy, &tool(json!({})), &caller("read")));
    }

    #[test]
    fn filtering_a_list_leaves_only_what_is_visible() {
        let policy: Policy = Some(Arc::new(Visibility::new().hiding_tagged(["internal"])));
        let ctx = caller("read");
        let mut result = neutral::ListToolsResult::new(vec![
            tool(json!({ "io.turbomcp/tags": ["internal"] })),
            neutral::Tool::new("read", json!({"type": "object"})),
        ]);
        filter_tools(&policy, &ctx, &mut result);
        assert_eq!(
            result
                .tools
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>(),
            ["read"]
        );
    }
}
