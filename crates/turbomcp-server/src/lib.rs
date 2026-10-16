//! # turbomcp-server
//!
//! The server framework: the user-facing traits, the capability router, and the
//! `tower`-shaped dispatcher that connects them to a transport.
//!
//! - [`McpServerCore`] + capability traits ([`WithTools`], …) — what a user
//!   implements. Handlers speak `turbomcp_protocol::neutral` types, never wire
//!   types, so a server is portable across protocol versions.
//! - [`MethodRouter`] — registers the capabilities a server actually implements;
//!   advertised capabilities are *derived* from it, so they can't drift.
//! - [`VersionDispatcher`] — `Service<McpRequest>`: extracts the version,
//!   routes to the typed handler, and serializes the response. All per-version
//!   branching is concentrated here.
//!
//! Both protocol paths are live: the modern `2026-07-28` stateless path and
//! the legacy `2025-11-25` stateful path (`initialize` handshake +
//! [`SessionStore`]; see [`LegacySessionAdapter`] for byte-pipe transports).
//!
//! ## Two kinds of async trait
//!
//! The traits a server *is* ([`McpServerCore`], [`WithTools`], …) use native
//! `async fn` in traits: they are implemented once per server type and
//! monomorphized, so they cost no allocation per call. The seams the
//! framework holds as `dyn` objects, several per server ([`Interceptor`],
//! [`Extension`], [`SessionBackend`], [`TaskBackend`]), use
//! [`async_trait`](https://docs.rs/async-trait), because a native async
//! method can't be called through `dyn` yet. Implement those with
//! [`macro@async_trait`], re-exported here and by the facade.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod adapter;
mod builder;
pub mod bus;
mod catalog;
mod composite;
mod config;
mod context;
mod dispatcher;
mod extension;
mod inflight;
pub mod intercept;
mod logging;
mod masking;
mod mrtr;
mod progress;
mod response;
mod router;
mod runtime;
mod session;
mod subscriptions;
pub mod tags;
mod task_handle;
mod tasks;
mod traits;
mod uri_template;
pub mod visibility;

pub use adapter::LegacySessionAdapter;
/// The attribute the `dyn` seams ([`Interceptor`], [`Extension`],
/// [`SessionBackend`], [`TaskBackend`]) are implemented with.
pub use async_trait::async_trait;
pub use builder::{IntoServerBuilder, ServerBuilder};
pub use bus::{BusError, Change, LocalBus, NotificationBus};
pub use composite::{Composite, CompositeServer};
pub use config::ServerConfig;
pub use context::{
    CallToolContext, CompleteContext, GetPromptContext, InitializeContext, ListPromptsContext,
    ListResourceTemplatesContext, ListResourcesContext, ListToolsContext, ReadResourceContext,
};
pub use dispatcher::{
    CachePolicies, Cancellation, DispatcherSessionTerminator, RootsChangedHandler,
    VersionDispatcher,
};
pub use extension::{
    CallAugmentRequest, CallRunner, Extension, ExtensionRequest, SubscribeOutcome,
};
pub use intercept::Interceptor;
pub use logging::LogSender;
pub use mrtr::{ClientHandle, Elicited};
pub use progress::ProgressReporter;
pub use response::{
    Audio, Image, IntoCallToolResult, IntoGetPromptResult, IntoReadResourceResult, Json, Params,
};
pub use router::{CapabilityFlags, MethodRouter};
pub use runtime::Server;
pub use session::{ExpiredSession, SessionBackend, SessionError, SessionState, SessionStore};
pub use subscriptions::ServerNotifier;
pub use task_handle::{TaskHandle, TaskLink, TaskSlot};
pub use tasks::{
    InputWaiter, NewTask, TaskBackend, TaskError, TaskOutcome, TaskOwner, TaskSnapshot, TaskStatus,
    TaskStore, TaskUpdate, quota_key, ttl_elapsed,
};
pub use traits::{McpServerCore, WithCompletions, WithPrompts, WithResources, WithTools};
pub use uri_template::{UriTemplate, UriTemplateError};
pub use visibility::{ComponentKind, Visibility, VisibilityPolicy, VisibleComponent};

/// Support items called by `#[server]`-generated code. Not part of the stable
/// API — do not depend on it directly.
#[doc(hidden)]
pub mod __macro_support {
    use serde_json::Value;

    /// A prompt argument or template capture as a handler's parameter type:
    /// anything [`FromStr`](core::str::FromStr), so `n: u32` or
    /// `when: chrono::NaiveDate` work, with a failure answered as invalid
    /// params naming the argument.
    ///
    /// # Errors
    /// [`McpError::invalid_params`](turbomcp_core::McpError) when `value`
    /// doesn't parse.
    pub fn parse_argument<T>(name: &str, value: &str) -> turbomcp_core::McpResult<T>
    where
        T: ArgumentType,
    {
        value
            .parse::<T>()
            .map_err(|e| turbomcp_core::McpError::invalid_params(format!("argument `{name}`: {e}")))
    }

    /// What a prompt argument or template capture can be: parsed from its
    /// string form.
    #[diagnostic::on_unimplemented(
        message = "a prompt argument or URI-template capture is parsed from a string, so `{Self}` must implement `FromStr` with a `Display` error",
        label = "not parseable from a string",
        note = "take a `String` and convert it yourself, or use `Params<T>` (prompts) for a structured input"
    )]
    pub trait ArgumentType: core::str::FromStr<Err: core::fmt::Display> {}
    impl<T: core::str::FromStr<Err: core::fmt::Display>> ArgumentType for T {}

    /// A prompt's arguments from the properties of `schema` (a
    /// `Params<T>`'s): each property an argument, with its description, and
    /// required when the schema requires it.
    #[must_use]
    pub fn prompt_arguments(schema: &Value) -> Vec<turbomcp_protocol::neutral::PromptArgument> {
        let required: Vec<&str> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        schema
            .get("properties")
            .and_then(Value::as_object)
            .map(|props| {
                props
                    .iter()
                    .map(|(name, prop)| {
                        let mut arg = turbomcp_protocol::neutral::PromptArgument::new(name.clone())
                            .required(required.contains(&name.as_str()));
                        if let Some(desc) = prop.get("description").and_then(Value::as_str) {
                            arg = arg.with_description(desc);
                        }
                        if let Some(title) = prop.get("title").and_then(Value::as_str) {
                            arg.title = Some(title.to_owned());
                        }
                        arg
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `info` with `icons` (`#[server(icon(…))]`), as the wire carries them.
    #[must_use]
    pub fn with_server_icons(
        mut info: turbomcp_core::Implementation,
        icons: Vec<turbomcp_protocol::neutral::Icon>,
    ) -> turbomcp_core::Implementation {
        let wire: Vec<Value> = icons
            .into_iter()
            .map(|icon| {
                serde_json::to_value(turbomcp_protocol::v2026_07_28::types::Icon::from(icon))
                    .unwrap_or_default()
            })
            .collect();
        info.extra.insert("icons".into(), Value::Array(wire));
        info
    }

    /// The URI-template grammar, which `#[resource]` evaluates in a `const`
    /// so a template the matcher can't read is a compile error.
    pub use crate::uri_template::validate as validate_uri_template;

    /// A component's description from its doc comment (`raw`, the doc
    /// attributes joined by newlines), as a model should read it:
    /// paragraphs reflowed onto one line each (doc comments are hard-wrapped
    /// for editors, not for models), list items and code fences kept, and
    /// everything from the first rustdoc-only section (`# Errors`,
    /// `# Panics`, `# Safety`, `# Examples`) on dropped.
    #[must_use]
    pub fn doc_description(raw: &str) -> String {
        enum Block {
            Para(String),
            Item(String),
            Code(String),
        }
        let lines: Vec<&str> = raw.lines().collect();
        let indent = lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);
        let is_section = |t: &str| {
            let heading = t.trim_start_matches('#');
            heading.len() < t.len()
                && t.len() - heading.len() <= 3
                && matches!(
                    heading.trim(),
                    "Errors" | "Panics" | "Safety" | "Examples" | "Example"
                )
        };
        let is_item = |t: &str| {
            t.starts_with("- ")
                || t.starts_with("* ")
                || t.split_once(". ")
                    .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        };
        let mut blocks: Vec<Block> = Vec::new();
        let mut open: Option<Block> = None;
        let mut fence = false;
        for line in lines {
            // The indentation every line shares (a `/// ` comment's space) is
            // the comment's, not the text's.
            let shared = line.len() >= indent
                && line.as_bytes()[..indent]
                    .iter()
                    .all(u8::is_ascii_whitespace);
            let line = if shared {
                &line[indent..]
            } else {
                line.trim_start()
            };
            let t = line.trim_end();
            if t.trim_start().starts_with("```") {
                if let Some(Block::Code(code)) = &mut open {
                    code.push('\n');
                    code.push_str(t);
                    blocks.extend(open.take());
                } else {
                    blocks.extend(open.take());
                    open = Some(Block::Code(t.to_owned()));
                }
                fence = !fence;
                continue;
            }
            if fence {
                if let Some(Block::Code(code)) = &mut open {
                    code.push('\n');
                    code.push_str(t);
                }
                continue;
            }
            let t = t.trim_start();
            if is_section(t) {
                break;
            }
            if t.is_empty() {
                blocks.extend(open.take());
            } else if is_item(t) {
                blocks.extend(open.take());
                open = Some(Block::Item(t.to_owned()));
            } else {
                match &mut open {
                    Some(Block::Para(text) | Block::Item(text)) => {
                        text.push(' ');
                        text.push_str(t);
                    }
                    _ => {
                        blocks.extend(open.take());
                        open = Some(Block::Para(t.to_owned()));
                    }
                }
            }
        }
        blocks.extend(open);
        let mut out = String::new();
        let mut last_item = false;
        for block in blocks {
            let (text, item) = match block {
                Block::Para(t) | Block::Code(t) => (t, false),
                Block::Item(t) => (t, true),
            };
            if !out.is_empty() {
                out.push_str(if item && last_item { "\n" } else { "\n\n" });
            }
            out.push_str(&text);
            last_item = item;
        }
        out
    }

    /// Strip the schemars `title` bookkeeping (the arg-struct's Rust name, which
    /// is noise on a tool input schema). The `$schema` dialect declaration is
    /// **kept** — the MCP spec (and the official conformance suite) expect a tool
    /// `inputSchema` to advertise its JSON Schema dialect
    /// (`https://json-schema.org/draft/2020-12/schema`).
    #[must_use]
    pub fn normalize_input_schema(mut v: Value) -> Value {
        if let Some(obj) = v.as_object_mut() {
            obj.remove("title");
        }
        v
    }

    /// Close a generated object schema to additional properties. A tool
    /// `inputSchema` forbids unknown arguments — the function-calling norm (it
    /// keeps models from inventing parameters) and what the official MCP
    /// conformance suite's json-schema-2020-12 scenario checks. The
    /// dispatcher validates arguments against this schema, so an unknown
    /// argument is answered as invalid arguments (a tool error the model can
    /// correct), not silently dropped. No-op unless the root is an `object`
    /// that hasn't already set `additionalProperties`.
    #[must_use]
    pub fn close_object_schema(mut v: Value) -> Value {
        if let Some(obj) = v.as_object_mut()
            && obj.get("type").and_then(Value::as_str) == Some("object")
            && !obj.contains_key("additionalProperties")
        {
            obj.insert("additionalProperties".into(), Value::Bool(false));
        }
        v
    }

    /// Merge the `#[tool(schema_extend = "…")]` keywords into a generated
    /// `inputSchema`.
    ///
    /// The point is to state what a Rust signature can't (cross-field rules
    /// like `allOf` / `anyOf` / `if` / `then` / `else`, per-property
    /// constraints, an `$anchor`), which SEP-2106 requires a server to carry
    /// through untouched. Objects merge recursively and the caller's value
    /// wins at the leaves, so `{"properties":{"age":{"minimum":18}}}` adds a
    /// bound to `age` and keeps every other property. (A top-level replace
    /// used to swap out the whole `properties` map, leaving the others
    /// required but forbidden by `additionalProperties: false`: a tool no
    /// call could satisfy.) `required` is the union of both lists. The macro
    /// already parsed `extra` at compile time, so a parse failure here is
    /// unreachable and leaves the schema as-is rather than panicking in a
    /// handler's list path.
    #[must_use]
    pub fn extend_object_schema(mut v: Value, extra: &str) -> Value {
        let Ok(add @ Value::Object(_)) = serde_json::from_str::<Value>(extra) else {
            return v;
        };
        merge(&mut v, add);
        return v;

        fn merge(base: &mut Value, add: Value) {
            let (Value::Object(base), Value::Object(add)) = (&mut *base, &add) else {
                *base = add;
                return;
            };
            for (key, value) in add {
                match (key.as_str(), base.get_mut(key)) {
                    ("required", Some(Value::Array(have))) => {
                        for name in value.as_array().into_iter().flatten() {
                            if !have.contains(name) {
                                have.push(name.clone());
                            }
                        }
                    }
                    (_, Some(existing)) => merge(existing, value.clone()),
                    (_, None) => {
                        base.insert(key.clone(), value.clone());
                    }
                }
            }
        }
    }

    /// The parameter types `#[mcp_header]` accepts: "MUST only be applied to
    /// parameters with primitive types (integer, string, boolean). Parameters
    /// with type `number` are not permitted." `Option` of one is fine too;
    /// an absent value just omits the header.
    #[diagnostic::on_unimplemented(
        message = "`#[mcp_header]` requires a string, bool or integer of at most 64 bits (or an `Option` of one), not `{Self}`",
        label = "not a header-safe primitive",
        note = "headers carry primitives only; `number` (f32/f64), 128-bit integers (beyond JSON's) and structured types are not permitted"
    )]
    pub trait HeaderParam: sealed::Sealed {}

    mod sealed {
        pub trait Sealed {}
    }

    macro_rules! header_params {
        ($($ty:ty),*) => {$(
            impl sealed::Sealed for $ty {}
            impl HeaderParam for $ty {}
        )*};
    }
    header_params!(
        String, bool, i8, i16, i32, i64, isize, u8, u16, u32, u64, usize
    );
    impl<T: HeaderParam> sealed::Sealed for Option<T> {}
    impl<T: HeaderParam> HeaderParam for Option<T> {}

    /// Compile-time check that a `#[mcp_header]` parameter is a
    /// [`HeaderParam`]; the macro emits a call spanned at the parameter.
    pub fn assert_header_param<T: HeaderParam>() {}

    /// Mark a property as an MCP header parameter (SEP-2243). The annotation
    /// value is the **name portion** of the mirrored `Mcp-Param-{name}` header
    /// (the transports spec made `x-mcp-header` a string; the earlier boolean
    /// form is obsolete) — we use the property name itself, which satisfies
    /// the spec's constraints (RFC 9110 tchar, case-insensitively unique
    /// within one schema) for any valid Rust identifier.
    ///
    /// An `Option<T>` parameter renders as `"type": [T, "null"]`, which a
    /// strict client reads as "not a primitive" and drops the tool over. The
    /// parameter is already optional by being absent from `required`, so the
    /// `null` is removed and the property is typed as the plain primitive.
    ///
    /// An integer is bounded to the range a header can carry: "Integer values
    /// MUST be within the safe range for JavaScript (−2^53+1 to 2^53−1)", so
    /// a `u64` or `i64` parameter states the limit its schema must hold to.
    pub fn mark_mcp_header(schema: &mut Value, property: &str) {
        if let Some(prop) = schema
            .get_mut("properties")
            .and_then(|p| p.get_mut(property))
            .and_then(Value::as_object_mut)
        {
            if let Some(Value::Array(types)) = prop.get("type") {
                let non_null: Vec<&Value> = types.iter().filter(|t| *t != "null").collect();
                if let [only] = non_null.as_slice() {
                    let only = (*only).clone();
                    prop.insert("type".into(), only);
                }
            }
            debug_assert!(
                matches!(
                    prop.get("type").and_then(Value::as_str),
                    Some("string" | "integer" | "boolean")
                ),
                "#[mcp_header] on a non-primitive property: {prop:?}"
            );
            if prop.get("type").and_then(Value::as_str) == Some("integer") {
                const SAFE: i64 = (1 << 53) - 1;
                let min = prop.get("minimum").and_then(Value::as_i64).unwrap_or(-SAFE);
                let max = prop.get("maximum").and_then(Value::as_i64).unwrap_or(SAFE);
                prop.insert("minimum".into(), Value::from(min.max(-SAFE)));
                prop.insert("maximum".into(), Value::from(max.min(SAFE)));
            }
            prop.insert("x-mcp-header".into(), Value::String(property.to_owned()));
        }
    }

    /// Match a concrete `uri` against an RFC 6570 URI template, returning the
    /// decoded variables it binds (in template order) on a match. Every
    /// operator is supported; see [`UriTemplate`](crate::UriTemplate). Returns
    /// `None` if the template is malformed or doesn't match. Templates are
    /// compiled once and cached.
    #[must_use]
    pub fn match_uri_template(template: &str, uri: &str) -> Option<Vec<(String, String)>> {
        match crate::uri_template::compiled(template).as_ref() {
            Ok(compiled) => compiled.matches(uri),
            Err(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn doc_comments_read_as_descriptions() {
        use crate::__macro_support::doc_description as d;
        assert_eq!(d(" Greet someone."), "Greet someone.");
        assert_eq!(
            d(" Search the index for documents\n matching the query.\n\n Results are ranked."),
            "Search the index for documents matching the query.\n\nResults are ranked."
        );
        assert_eq!(
            d(" Modes:\n\n - fast: skip ranking\n - full: rank\n   everything"),
            "Modes:\n\n- fast: skip ranking\n- full: rank everything"
        );
        assert_eq!(
            d(" Run it.\n\n # Errors\n\n When the disk is full."),
            "Run it."
        );
        assert_eq!(
            d(" Example call:\n\n ```json\n {\"a\": 1}\n ```\n After."),
            "Example call:\n\n```json\n{\"a\": 1}\n```\n\nAfter."
        );
        assert_eq!(
            d("Plain text from include_str!\n\nTwo paragraphs."),
            "Plain text from include_str!\n\nTwo paragraphs."
        );
    }

    use super::*;

    use serde_json::json;
    use tower::{Service, ServiceExt};
    use turbomcp_core::{Implementation, JsonRpcMessage, JsonRpcRequest, McpResult};
    use turbomcp_protocol::neutral;

    #[derive(Clone)]
    struct Calculator;

    impl McpServerCore for Calculator {
        fn server_info(&self) -> Implementation {
            Implementation::new("calculator", "0.1.0")
        }
        fn instructions(&self) -> Option<String> {
            Some("A demo calculator server.".into())
        }
    }

    impl WithTools for Calculator {
        async fn list_tools(
            &self,
            _ctx: &ListToolsContext,
            _params: neutral::ListParams,
        ) -> McpResult<neutral::ListToolsResult> {
            Ok(neutral::ListToolsResult::new(vec![neutral::Tool::new(
                "add",
                json!({"type": "object", "properties": {"a": {"type": "number"}, "b": {"type": "number"}}}),
            )
            .with_description("Add two numbers")]))
        }

        async fn call_tool(
            &self,
            _ctx: &CallToolContext,
            params: neutral::CallToolParams,
        ) -> McpResult<neutral::CallToolResult> {
            if params.name != "add" {
                return Ok(neutral::CallToolResult::error(format!(
                    "unknown tool: {}",
                    params.name
                )));
            }
            let a = params
                .arguments
                .get("a")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            let b = params
                .arguments
                .get("b")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            Ok(neutral::CallToolResult::text(format!("{}", a + b)))
        }
    }

    fn dispatcher() -> VersionDispatcher<Calculator> {
        VersionDispatcher::new(Calculator, MethodRouter::new().with_tools())
    }

    /// Build draft `_meta` carrying the per-request protocol version.
    fn draft_meta() -> serde_json::Value {
        json!({
            "io.modelcontextprotocol/protocolVersion": "2026-07-28",
            "io.modelcontextprotocol/clientCapabilities": {},
        })
    }

    async fn call(svc: &mut VersionDispatcher<Calculator>, req: JsonRpcRequest) -> JsonRpcMessage {
        svc.ready()
            .await
            .unwrap()
            .call(req.into())
            .await
            .unwrap()
            .expect("request should produce a response")
    }

    /// The `2026-07-28` request envelope's fields must have the schema's
    /// types: `clientCapabilities` an object, not `null`.
    #[tokio::test]
    async fn a_mistyped_envelope_field_is_refused() {
        let mut svc = dispatcher();
        let resp = call(
            &mut svc,
            JsonRpcRequest::new(
                1,
                "tools/list",
                Some(json!({ "_meta": {
                    "io.modelcontextprotocol/protocolVersion": "2026-07-28",
                    "io.modelcontextprotocol/clientCapabilities": null,
                }})),
            ),
        )
        .await;
        let JsonRpcMessage::Response(r) = resp else {
            panic!()
        };
        let err = r.error.expect("refused");
        assert_eq!(err.code, turbomcp_core::codes::INVALID_PARAMS);
        assert_eq!(
            err.data.unwrap()["missingField"],
            "io.modelcontextprotocol/clientCapabilities"
        );
    }

    /// A second request under an id still in flight on the same connection
    /// is refused, rather than taking over the first's cancellation.
    #[tokio::test]
    async fn a_duplicate_in_flight_id_is_refused() {
        let mut svc = dispatcher();
        let conn = turbomcp_core::ConnectionId::new("dup-conn");
        let req = || {
            turbomcp_core::McpRequest::new(JsonRpcRequest::new(
                7,
                "tools/list",
                Some(json!({ "_meta": draft_meta() })),
            ))
            .with(conn.clone())
        };
        // The first is registered when called, before it is polled.
        let first = svc.ready().await.unwrap().call(req());
        let second = svc.ready().await.unwrap().call(req()).await.unwrap();
        let Some(JsonRpcMessage::Response(r)) = second else {
            panic!()
        };
        assert_eq!(
            r.error.expect("refused").code,
            turbomcp_core::codes::INVALID_REQUEST
        );
        let Some(JsonRpcMessage::Response(r)) = first.await.unwrap() else {
            panic!()
        };
        assert!(r.error.is_none(), "the first still answers");
    }

    /// A session speaks the revision it negotiated: a request stating
    /// another is refused rather than answered in the other's shape.
    #[tokio::test]
    async fn a_request_stating_another_revision_than_its_session_is_refused() {
        let mut svc = crate::LegacySessionAdapter::new(dispatcher());
        let init = |id| {
            JsonRpcRequest::new(
                id,
                "initialize",
                Some(json!({
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": { "name": "c", "version": "1" },
                })),
            )
        };
        svc.ready()
            .await
            .unwrap()
            .call(init(1).into())
            .await
            .unwrap();
        let resp = svc
            .ready()
            .await
            .unwrap()
            .call(
                JsonRpcRequest::new(
                    2,
                    "tools/list",
                    Some(json!({ "_meta": {
                        "io.modelcontextprotocol/protocolVersion": "2025-11-25"
                    }})),
                )
                .into(),
            )
            .await
            .unwrap();
        let Some(JsonRpcMessage::Response(r)) = resp else {
            panic!()
        };
        assert_eq!(
            r.error.expect("refused").code,
            turbomcp_core::codes::INVALID_REQUEST
        );
    }

    /// What a server says about itself beyond name and version
    /// (`description`, `websiteUrl`) reaches the client on both handshakes.
    #[tokio::test]
    async fn implementation_extras_reach_the_wire() {
        #[derive(Clone)]
        struct Described;
        impl McpServerCore for Described {
            fn server_info(&self) -> Implementation {
                let mut info = Implementation::new("described", "1.0.0");
                info.extra
                    .insert("description".into(), json!("A server with a story"));
                info.extra
                    .insert("websiteUrl".into(), json!("https://example.com"));
                info
            }
        }
        let mut svc = VersionDispatcher::new(Described, MethodRouter::new());
        let discover = svc
            .ready()
            .await
            .unwrap()
            .call(
                JsonRpcRequest::new(1, "server/discover", Some(json!({ "_meta": draft_meta() })))
                    .into(),
            )
            .await
            .unwrap();
        let Some(JsonRpcMessage::Response(r)) = discover else {
            panic!()
        };
        let info = &r.result.unwrap()["_meta"]["io.modelcontextprotocol/serverInfo"];
        assert_eq!(info["description"], "A server with a story");
        assert_eq!(info["websiteUrl"], "https://example.com");

        let mut svc = crate::LegacySessionAdapter::new(VersionDispatcher::new(
            Described,
            MethodRouter::new(),
        ));
        let init = svc
            .ready()
            .await
            .unwrap()
            .call(
                JsonRpcRequest::new(
                    2,
                    "initialize",
                    Some(json!({
                        "protocolVersion": "2025-11-25",
                        "capabilities": {},
                        "clientInfo": { "name": "c", "version": "1" },
                    })),
                )
                .into(),
            )
            .await
            .unwrap();
        let Some(JsonRpcMessage::Response(r)) = init else {
            panic!()
        };
        assert_eq!(
            r.result.unwrap()["serverInfo"]["description"],
            "A server with a story"
        );
    }

    #[tokio::test]
    async fn discover_advertises_tools_and_versions() {
        let mut svc = dispatcher();
        let resp = call(
            &mut svc,
            JsonRpcRequest::new(1, "server/discover", Some(json!({ "_meta": draft_meta() }))),
        )
        .await;
        let JsonRpcMessage::Response(r) = resp else {
            panic!("expected response")
        };
        let result = r.result.expect("discover result");
        // Identity rides in `_meta` on the frozen `2026-07-28` (the RC had
        // briefly made it a first-class `serverInfo` field).
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "calculator"
        );
        assert_eq!(result["capabilities"]["tools"]["listChanged"], true);
        assert_eq!(result["resultType"], "complete");
        let versions = result["supportedVersions"].as_array().unwrap();
        assert!(versions.iter().any(|v| v == "2026-07-28"));
        assert!(versions.iter().any(|v| v == "2025-11-25"));
        assert_eq!(result["instructions"], "A demo calculator server.");
    }

    #[tokio::test]
    async fn tools_list_returns_registered_tools() {
        let mut svc = dispatcher();
        let req = JsonRpcRequest::new(2, "tools/list", Some(json!({ "_meta": draft_meta() })));
        let JsonRpcMessage::Response(r) = call(&mut svc, req).await else {
            panic!()
        };
        let result = r.result.unwrap();
        assert_eq!(result["tools"][0]["name"], "add");
        assert_eq!(result["tools"][0]["description"], "Add two numbers");
        assert_eq!(result["resultType"], "complete");
        // "Servers SHOULD identify themselves in each result's `_meta`."
        assert_eq!(
            result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
            "calculator"
        );
    }

    #[tokio::test]
    async fn tools_call_invokes_handler() {
        let mut svc = dispatcher();
        let req = JsonRpcRequest::new(
            3,
            "tools/call",
            Some(json!({ "name": "add", "arguments": {"a": 2, "b": 3}, "_meta": draft_meta() })),
        );
        let JsonRpcMessage::Response(r) = call(&mut svc, req).await else {
            panic!()
        };
        let result = r.result.unwrap();
        assert_eq!(result["content"][0]["text"], "5");
        assert_eq!(result["isError"], false);
    }

    /// A request with no `_meta` is a malformed envelope, not a rejected
    /// version: SEP-2575 marks the fields required, so their absence is
    /// invalid params. The supported list still rides in `data`, so a client
    /// that simply forgot can re-issue.
    #[tokio::test]
    async fn missing_envelope_is_invalid_params_naming_the_field() {
        let mut svc = dispatcher();
        // tools/list without `_meta.protocolVersion`.
        let req = JsonRpcRequest::new(4, "tools/list", Some(json!({})));
        let JsonRpcMessage::Response(r) = call(&mut svc, req).await else {
            panic!()
        };
        let err = r.error.expect("should be an error");
        assert_eq!(err.code, -32602);
        let data = err.data.expect("names the missing field");
        assert_eq!(
            data["missingField"],
            "io.modelcontextprotocol/protocolVersion"
        );
        assert!(data["supported"].is_array(), "{data}");
    }

    /// The version is *named* but not served: that is negotiation, not a
    /// malformed request, and keeps the dedicated code.
    #[tokio::test]
    async fn unsupported_version_yields_its_own_code_with_the_list() {
        let mut svc = dispatcher();
        let meta = json!({
            "io.modelcontextprotocol/protocolVersion": "1999-01-01",
            "io.modelcontextprotocol/clientCapabilities": {},
        });
        let req = JsonRpcRequest::new(4, "tools/list", Some(json!({ "_meta": meta })));
        let JsonRpcMessage::Response(r) = call(&mut svc, req).await else {
            panic!()
        };
        let err = r.error.expect("should be an error");
        assert_eq!(err.code, turbomcp_core::codes::UNSUPPORTED_PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn legacy_version_without_session_is_not_initialized() {
        let mut svc = dispatcher();
        let meta = json!({ "io.modelcontextprotocol/protocolVersion": "2025-11-25" });
        let req = JsonRpcRequest::new(5, "tools/list", Some(json!({ "_meta": meta })));
        let JsonRpcMessage::Response(r) = call(&mut svc, req).await else {
            panic!()
        };
        let err = r.error.expect("legacy request without a session must fail");
        // Not `-32002`, which these revisions allocate to resource-not-found.
        assert_eq!(err.code, turbomcp_core::codes::NO_ACTIVE_SESSION);
        assert!(err.message.contains("initialize"));
    }

    #[tokio::test]
    async fn unknown_method_is_method_not_found() {
        let mut svc = dispatcher();
        let req = JsonRpcRequest::new(
            6,
            "tools/nonexistent",
            Some(json!({ "_meta": draft_meta() })),
        );
        let JsonRpcMessage::Response(r) = call(&mut svc, req).await else {
            panic!()
        };
        assert_eq!(r.error.unwrap().code, -32601);
    }

    #[tokio::test]
    async fn notification_produces_no_response() {
        let mut svc = dispatcher();
        let msg: JsonRpcMessage =
            turbomcp_core::JsonRpcNotification::new("notifications/initialized", None).into();
        let out = svc.ready().await.unwrap().call(msg.into()).await.unwrap();
        assert!(out.is_none());
    }

    /// A server without `WithTools` must not advertise tools.
    #[tokio::test]
    async fn server_without_tools_omits_capability() {
        #[derive(Clone)]
        struct Bare;
        impl McpServerCore for Bare {
            fn server_info(&self) -> Implementation {
                Implementation::new("bare", "0.0.0")
            }
        }
        let mut svc = VersionDispatcher::new(Bare, MethodRouter::<Bare>::new());
        // Reuse the dispatch path directly.
        let resp = svc
            .ready()
            .await
            .unwrap()
            .call(
                JsonRpcRequest::new(1, "server/discover", Some(json!({ "_meta": draft_meta() })))
                    .into(),
            )
            .await
            .unwrap()
            .unwrap();
        let JsonRpcMessage::Response(r) = resp else {
            panic!()
        };
        assert!(
            r.result
                .unwrap()
                .get("capabilities")
                .unwrap()
                .get("tools")
                .is_none()
        );
    }

    fn _is_send<T: Send>() {}
    #[test]
    fn dispatcher_is_send() {
        _is_send::<VersionDispatcher<Calculator>>();
    }

    #[tokio::test]
    async fn builder_registers_capabilities() {
        // `into_server()` (blanket) starts empty; `with_tools()` registers.
        let mut svc = Calculator.into_server().with_tools().build();
        let JsonRpcMessage::Response(r) = svc
            .ready()
            .await
            .unwrap()
            .call(
                JsonRpcRequest::new(1, "server/discover", Some(json!({ "_meta": draft_meta() })))
                    .into(),
            )
            .await
            .unwrap()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(
            r.result.unwrap()["capabilities"]["tools"]["listChanged"],
            true
        );
    }

    #[test]
    fn builder_without_registration_has_no_capabilities() {
        let dispatcher = ServerBuilder::new(Calculator).build();
        _is_send::<VersionDispatcher<Calculator>>();
        let _ = dispatcher; // built successfully with an empty router
    }

    // ---- resources / prompts / completions ----------------------------------

    /// A server implementing every capability trait, used to prove discover
    /// advertises each one and the dispatcher routes all method families.
    #[derive(Clone)]
    struct Everything;

    impl McpServerCore for Everything {
        fn server_info(&self) -> Implementation {
            Implementation::new("everything", "0.1.0")
        }
    }

    impl WithResources for Everything {
        async fn list_resources(
            &self,
            _ctx: &ListResourcesContext,
            _params: neutral::ListParams,
        ) -> McpResult<neutral::ListResourcesResult> {
            Ok(neutral::ListResourcesResult::new(vec![
                neutral::Resource::new("file://readme", "readme").with_mime_type("text/plain"),
            ]))
        }

        async fn read_resource(
            &self,
            _ctx: &ReadResourceContext,
            params: neutral::ReadResourceParams,
        ) -> McpResult<neutral::ReadResourceResult> {
            Ok(neutral::ReadResourceResult::text(
                params.uri,
                "file contents",
            ))
        }
    }

    impl WithPrompts for Everything {
        async fn list_prompts(
            &self,
            _ctx: &ListPromptsContext,
            _params: neutral::ListParams,
        ) -> McpResult<neutral::ListPromptsResult> {
            Ok(neutral::ListPromptsResult::new(vec![
                neutral::Prompt::new("greet")
                    .with_argument(neutral::PromptArgument::new("name").required(true)),
            ]))
        }

        async fn get_prompt(
            &self,
            _ctx: &GetPromptContext,
            params: neutral::GetPromptParams,
        ) -> McpResult<neutral::GetPromptResult> {
            let name = params.arguments.get("name").cloned().unwrap_or_default();
            Ok(neutral::GetPromptResult::new(vec![
                neutral::PromptMessage::user_text(format!("Greet {name}")),
            ]))
        }
    }

    impl WithCompletions for Everything {
        async fn complete(
            &self,
            _ctx: &CompleteContext,
            params: neutral::CompleteParams,
        ) -> McpResult<neutral::CompleteResult> {
            // Echo the partial value back as the single suggestion.
            Ok(neutral::CompleteResult::new(vec![params.argument.value]))
        }
    }

    fn everything() -> VersionDispatcher<Everything> {
        VersionDispatcher::new(
            Everything,
            MethodRouter::new()
                .with_resources()
                .with_prompts()
                .with_completions(),
        )
    }

    async fn call_everything(
        svc: &mut VersionDispatcher<Everything>,
        req: JsonRpcRequest,
    ) -> serde_json::Value {
        let JsonRpcMessage::Response(r) = svc
            .ready()
            .await
            .unwrap()
            .call(req.into())
            .await
            .unwrap()
            .expect("response")
        else {
            panic!("expected response")
        };
        r.result.expect("result")
    }

    #[tokio::test]
    async fn discover_advertises_all_capabilities() {
        let mut svc = everything();
        let result = call_everything(
            &mut svc,
            JsonRpcRequest::new(1, "server/discover", Some(json!({ "_meta": draft_meta() }))),
        )
        .await;
        let caps = &result["capabilities"];
        assert_eq!(caps["resources"]["listChanged"], true);
        assert_eq!(caps["resources"]["subscribe"], true);
        assert_eq!(caps["prompts"]["listChanged"], true);
        assert!(caps["completions"].is_object());
        // No tools were registered → no tools capability.
        assert!(caps.get("tools").is_none());
    }

    #[tokio::test]
    async fn resources_list_read_and_templates_route() {
        let mut svc = everything();
        let meta = json!({ "_meta": draft_meta() });

        let list = call_everything(
            &mut svc,
            JsonRpcRequest::new(2, "resources/list", Some(meta.clone())),
        )
        .await;
        assert_eq!(list["resources"][0]["uri"], "file://readme");
        assert_eq!(list["resources"][0]["mimeType"], "text/plain");

        let read = call_everything(
            &mut svc,
            JsonRpcRequest::new(
                3,
                "resources/read",
                Some(json!({ "uri": "file://readme", "_meta": draft_meta() })),
            ),
        )
        .await;
        assert_eq!(read["contents"][0]["uri"], "file://readme");
        assert_eq!(read["contents"][0]["text"], "file contents");

        // The default `list_resource_templates` answers with an empty list.
        let templates = call_everything(
            &mut svc,
            JsonRpcRequest::new(4, "resources/templates/list", Some(meta)),
        )
        .await;
        assert_eq!(templates["resourceTemplates"].as_array().unwrap().len(), 0);
        assert_eq!(templates["resultType"], "complete");
    }

    #[tokio::test]
    async fn prompts_list_and_get_route() {
        let mut svc = everything();
        let list = call_everything(
            &mut svc,
            JsonRpcRequest::new(5, "prompts/list", Some(json!({ "_meta": draft_meta() }))),
        )
        .await;
        assert_eq!(list["prompts"][0]["name"], "greet");
        assert_eq!(list["prompts"][0]["arguments"][0]["required"], true);

        let got = call_everything(
            &mut svc,
            JsonRpcRequest::new(
                6,
                "prompts/get",
                Some(
                    json!({ "name": "greet", "arguments": {"name": "Ada"}, "_meta": draft_meta() }),
                ),
            ),
        )
        .await;
        assert_eq!(got["messages"][0]["role"], "user");
        assert_eq!(got["messages"][0]["content"]["text"], "Greet Ada");
    }

    #[tokio::test]
    async fn completion_complete_routes_with_ref_union() {
        let mut svc = everything();
        let result = call_everything(
            &mut svc,
            JsonRpcRequest::new(
                7,
                "completion/complete",
                Some(json!({
                    "ref": { "type": "ref/prompt", "name": "greet" },
                    "argument": { "name": "name", "value": "Ad" },
                    "_meta": draft_meta(),
                })),
            ),
        )
        .await;
        assert_eq!(result["completion"]["values"][0], "Ad");
        assert_eq!(result["resultType"], "complete");
    }

    #[tokio::test]
    async fn unregistered_capability_is_method_not_found() {
        // `Everything` doesn't register tools; calling a tools method 404s.
        let mut svc = everything();
        let JsonRpcMessage::Response(r) = svc
            .ready()
            .await
            .unwrap()
            .call(
                JsonRpcRequest::new(8, "tools/list", Some(json!({ "_meta": draft_meta() }))).into(),
            )
            .await
            .unwrap()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(r.error.unwrap().code, -32601);
    }

    #[tokio::test]
    async fn malformed_completion_ref_is_invalid_params() {
        let mut svc = everything();
        let JsonRpcMessage::Response(r) = svc
            .ready()
            .await
            .unwrap()
            .call(
                JsonRpcRequest::new(
                    9,
                    "completion/complete",
                    Some(json!({
                        "ref": { "type": "ref/prompt" },
                        "argument": { "name": "x", "value": "" },
                        "_meta": draft_meta(),
                    })),
                )
                .into(),
            )
            .await
            .unwrap()
            .unwrap()
        else {
            panic!()
        };
        assert_eq!(r.error.unwrap().code, -32602);
    }
}
