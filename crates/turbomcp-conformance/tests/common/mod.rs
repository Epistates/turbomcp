//! A representative "everything"-style TurboMCP server, exposed for the
//! conformance harness to drive over Streamable HTTP.
//!
//! The official `@modelcontextprotocol/conformance` *server* scenarios probe
//! for **fixed, well-known handler names** (the "everything server" contract):
//! e.g. a tool named `test_simple_text`, a resource at `test://static-text`, a
//! prompt named `test_simple_prompt`. TurboMCP derives the wire name of a
//! `#[tool]` / `#[prompt]` directly from the *method identifier* (there is no
//! `name =` override), so these methods are named exactly as the scenarios
//! expect. Capabilities are derived from the markers present; `logging` is
//! opted in explicitly by the harness driver via `ServerBuilder::with_logging`.
#![allow(clippy::unused_async)]

use base64::Engine as _;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use turbomcp::prelude::*;

/// A 1x1 transparent PNG, base64 (image tool / prompt / mixed content).
pub const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==";

/// A tiny WAV (header + no samples), base64 (audio tool).
pub const WAV_TINY: &str = "UklGRiQAAABXQVZFZm10IBAAAAABAAEAQB8AAEAfAAABAAgAZGF0YQAAAAA=";

#[derive(Serialize, JsonSchema)]
pub struct Stats {
    /// A running count.
    pub count: u64,
    /// A computed mean.
    pub mean: f64,
}

/// A nested-object argument for the JSON Schema 2020-12 scenario. The conformance
/// harness looks for a `$defs/address` subschema on the tool's `inputSchema`;
/// `#[schemars(rename = "address")]` sets that exact `$defs` key. `$anchor` is
/// part of what SEP-2106 requires a server to preserve, so it is declared here
/// rather than left to schemars.
#[derive(Deserialize, JsonSchema)]
#[schemars(rename = "address", extend("$anchor" = "addressDef"))]
#[allow(dead_code, reason = "only its generated schema is under test")]
pub struct Address {
    pub street: String,
    pub city: String,
}

/// Which contact method the JSON Schema 2020-12 tool's conditional branch keys
/// off. Present so the generated schema carries a real `enum`.
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ContactMethod {
    Phone,
    Email,
}

#[derive(Clone)]
pub struct Everything;

#[server(
    name = "turbomcp-everything",
    version = "1.0.0",
    instructions = "A TurboMCP conformance fixture server: tools, resources, prompts, completion."
)]
impl Everything {
    // ---- tools (method names are the wire tool names — load-bearing) --------

    /// A tool with no arguments that returns a single text content block.
    #[tool(description = "Return simple text content")]
    async fn test_simple_text(&self) -> McpResult<String> {
        Ok("Hello, conformance!".to_string())
    }

    /// A tool that returns a single image content block.
    #[tool(description = "Return image content")]
    async fn test_image_content(&self) -> McpResult<Image> {
        Ok(Image {
            data: PNG_1X1.to_string(),
            mime_type: "image/png".to_string(),
        })
    }

    /// A tool that returns a single audio content block.
    #[tool(description = "Return audio content")]
    async fn test_audio_content(&self) -> McpResult<Audio> {
        Ok(Audio {
            data: WAV_TINY.to_string(),
            mime_type: "audio/wav".to_string(),
        })
    }

    /// A tool that returns structured output (generates an `outputSchema`).
    #[tool(description = "Return structured output")]
    async fn test_structured_output(&self) -> McpResult<Json<Stats>> {
        Ok(Json(Stats {
            count: 3,
            mean: 1.5,
        }))
    }

    /// A tool that always returns a tool-level error (`isError: true`).
    #[tool(description = "Return a tool error")]
    async fn test_error_handling(&self) -> McpResult<String> {
        Err(McpError::tool_execution_failed(
            "test_error_handling",
            "intentional failure",
        ))
    }

    /// A tool that reports progress (≥3 increasing steps) before finishing.
    #[tool(description = "Report progress then finish")]
    async fn test_tool_with_progress(&self, ctx: &CallToolContext) -> McpResult<String> {
        ctx.progress.report(0.25, Some(1.0), Some("25%")).await;
        ctx.progress.report(0.5, Some(1.0), Some("50%")).await;
        ctx.progress.report(0.75, Some(1.0), Some("75%")).await;
        ctx.progress.report(1.0, Some(1.0), Some("done")).await;
        Ok("complete".to_string())
    }

    /// A tool that emits ≥3 log notifications before finishing.
    #[tool(description = "Emit log messages then finish")]
    async fn test_tool_with_logging(&self, ctx: &CallToolContext) -> McpResult<String> {
        ctx.log.debug(json!({ "message": "starting" })).await;
        ctx.log.info(json!({ "message": "working" })).await;
        ctx.log.warning(json!({ "message": "almost done" })).await;
        ctx.log.info(json!({ "message": "finished" })).await;
        Ok("logged".to_string())
    }

    /// A tool that asks the client to elicit user input.
    #[tool(description = "Request user input via elicitation")]
    async fn test_elicitation(
        &self,
        ctx: &CallToolContext,
        #[description("A message to show the user")] message: String,
    ) -> McpResult<String> {
        let outcome = ctx
            .client
            .elicit(
                "test_elicitation",
                neutral::ElicitParams::new(
                    message,
                    json!({
                        "type": "object",
                        "properties": { "name": { "type": "string", "title": "Name" } },
                        "required": ["name"],
                    }),
                ),
            )
            .await?;
        Ok(if outcome.accepted() {
            "got input".to_string()
        } else {
            "no input".to_string()
        })
    }

    /// A tool that asks the client to sample an LLM completion.
    #[tool(description = "Request LLM sampling via the client")]
    #[allow(deprecated)] // create_message is the sampling API; deprecation is upstream-only.
    async fn test_sampling(
        &self,
        ctx: &CallToolContext,
        #[description("A prompt to sample")] prompt: String,
    ) -> McpResult<String> {
        let result = ctx
            .client
            .create_message(
                "test_sampling",
                json!({
                    "messages": [{
                        "role": "user",
                        "content": { "type": "text", "text": prompt }
                    }],
                    "maxTokens": 64
                }),
            )
            .await?;
        Ok(result
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("(sampled)")
            .to_string())
    }

    /// A tool whose input schema exercises the JSON Schema 2020-12 vocabulary
    /// the spec requires a server to pass through untouched: `$schema`,
    /// `$defs` + `$anchor`, `additionalProperties` (SEP-1613), and the
    /// composition / conditional keywords `allOf`, `anyOf`, `if`, `then`,
    /// `else` (SEP-2106).
    ///
    /// The composition and conditional keywords express a cross-field rule no
    /// Rust signature can (“give a phone or an email, and whichever one
    /// `contactMethod` names”), so they are declared with `schemars(extend)`
    /// rather than derived. That is exactly the case the SEP exists for: the
    /// server must not rewrite or drop a schema it does not itself interpret.
    #[tool(
        description = "Tool with JSON Schema 2020-12 features",
        schema_extend = r#"{
            "allOf": [{ "anyOf": [{ "required": ["phone"] }, { "required": ["email"] }] }],
            "if": { "properties": { "contactMethod": { "const": "phone" } },
                    "required": ["contactMethod"] },
            "then": { "required": ["phone"] },
            "else": { "required": ["email"] }
        }"#
    )]
    async fn json_schema_2020_12_tool(
        &self,
        name: Option<String>,
        address: Option<Address>,
        contact_method: Option<ContactMethod>,
        phone: Option<String>,
        email: Option<String>,
    ) -> McpResult<String> {
        let _ = (name, address, contact_method, phone, email);
        Ok("ok".to_string())
    }

    /// A tool returning a single embedded-resource content block.
    #[tool(description = "Return embedded resource content")]
    async fn test_embedded_resource(&self) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::new(vec![
            neutral::Content::resource(
                neutral::ResourceContents::text(
                    "test://embedded-resource",
                    "embedded resource contents",
                )
                .with_mime_type("text/plain"),
            ),
        ]))
    }

    /// A tool returning multiple content blocks: text, image, and resource.
    #[tool(description = "Return multiple content types")]
    async fn test_multiple_content_types(&self) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::new(vec![
            neutral::Content::text("mixed content"),
            neutral::Content::image(PNG_1X1, "image/png"),
            neutral::Content::resource(
                neutral::ResourceContents::text("test://mixed-resource", "resource part")
                    .with_mime_type("text/plain"),
            ),
        ]))
    }

    /// SEP-1034: request elicitation whose schema exercises typed default values.
    #[tool(description = "Elicit with SEP-1034 default values")]
    async fn test_elicitation_sep1034_defaults(&self, ctx: &CallToolContext) -> McpResult<String> {
        ctx.client
            .elicit(
                "sep1034",
                neutral::ElicitParams::new(
                    "Please provide your details".to_string(),
                    json!({
                        "type": "object",
                        "properties": {
                            "name": { "type": "string", "default": "John Doe" },
                            "age": { "type": "integer", "default": 30 },
                            "score": { "type": "number", "default": 95.5 },
                            "status": {
                                "type": "string",
                                "enum": ["active", "inactive", "pending"],
                                "default": "active"
                            },
                            "verified": { "type": "boolean", "default": true }
                        }
                    }),
                ),
            )
            .await?;
        Ok("elicited".to_string())
    }

    /// SEP-1330: request elicitation whose schema exercises the enum styles
    /// (untitled `enum`, titled `oneOf`, legacy `enumNames`, and array variants).
    #[tool(description = "Elicit with SEP-1330 enum styles")]
    async fn test_elicitation_sep1330_enums(&self, ctx: &CallToolContext) -> McpResult<String> {
        ctx.client
            .elicit(
                "sep1330",
                neutral::ElicitParams::new(
                    "Please choose".to_string(),
                    json!({
                        "type": "object",
                        "properties": {
                            "untitledSingle": {
                                "type": "string",
                                "enum": ["option1", "option2", "option3"]
                            },
                            "titledSingle": {
                                "type": "string",
                                "oneOf": [
                                    { "const": "value1", "title": "Value One" },
                                    { "const": "value2", "title": "Value Two" }
                                ]
                            },
                            "legacyEnum": {
                                "type": "string",
                                "enum": ["opt1", "opt2"],
                                "enumNames": ["Option 1", "Option 2"]
                            },
                            "untitledMulti": {
                                "type": "array",
                                "items": { "type": "string", "enum": ["option1", "option2"] }
                            },
                            "titledMulti": {
                                "type": "array",
                                "items": {
                                    "anyOf": [
                                        { "const": "value1", "title": "Value One" },
                                        { "const": "value2", "title": "Value Two" }
                                    ]
                                }
                            }
                        }
                    }),
                ),
            )
            .await?;
        Ok("elicited".to_string())
    }

    // ---- SEP-2322 InputRequiredResult (MRTR) --------------------------------
    //
    // On the `2026-07-28` stateless wire these handlers do NOT block: the first
    // `ctx.client.*` call for a key the client hasn't answered records the
    // input request and aborts with `McpError::InputRequired`, and the
    // dispatcher renders the collected requests as an `InputRequiredResult`.
    // The client retries the same call with `inputResponses`, the handler
    // re-executes, and the call now returns the cached answer. So the elicit
    // *key* is the `inputRequests` key the harness looks for — it is part of
    // the wire contract here, not an internal label.

    /// A1: one elicitation input request, keyed `user_name`.
    #[tool(description = "InputRequiredResult with a single elicitation request")]
    async fn test_input_required_result_elicitation(
        &self,
        ctx: &CallToolContext,
    ) -> McpResult<String> {
        let outcome = ctx
            .client
            .elicit(
                "user_name",
                neutral::ElicitParams::new(
                    "What is your name?",
                    json!({
                        "type": "object",
                        "properties": { "name": { "type": "string" } },
                        "required": ["name"],
                    }),
                ),
            )
            .await?;
        let name = outcome
            .content
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("world");
        Ok(format!("Hello, {name}!"))
    }

    /// A2: one sampling input request, keyed `capital_question`.
    #[tool(description = "InputRequiredResult with a single sampling request")]
    #[allow(deprecated)] // create_message is the sampling API; deprecation is upstream-only.
    async fn test_input_required_result_sampling(
        &self,
        ctx: &CallToolContext,
    ) -> McpResult<String> {
        let answer = ctx
            .client
            .create_message(
                "capital_question",
                json!({
                    "messages": [{
                        "role": "user",
                        "content": { "type": "text", "text": "What is the capital of France?" }
                    }],
                    "maxTokens": 100
                }),
            )
            .await?;
        Ok(answer
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("(sampled)")
            .to_string())
    }

    /// A3: one `roots/list` input request, keyed `client_roots`.
    #[tool(description = "InputRequiredResult with a single roots/list request")]
    #[allow(deprecated)] // list_roots is the roots API; deprecation is upstream-only.
    async fn test_input_required_result_list_roots(
        &self,
        ctx: &CallToolContext,
    ) -> McpResult<String> {
        let roots = ctx.client.list_roots("client_roots").await?;
        let count = roots
            .get("roots")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        Ok(format!("client reported {count} root(s)"))
    }

    /// A4: an elicitation request plus signed `requestState` round-tripped
    /// across the retry. The reply must say `state-ok` so the harness can tell
    /// the state was actually read back, not merely echoed.
    #[tool(description = "InputRequiredResult carrying requestState")]
    async fn test_input_required_result_request_state(
        &self,
        ctx: &CallToolContext,
    ) -> McpResult<String> {
        ctx.client.store_state(&json!({ "phase": "confirm" }))?;
        let outcome = ctx
            .client
            .elicit(
                "confirm",
                neutral::ElicitParams::new(
                    "Please confirm",
                    json!({
                        "type": "object",
                        "properties": { "ok": { "type": "boolean" } },
                        "required": ["ok"],
                    }),
                ),
            )
            .await?;
        let state: Option<Value> = ctx.client.load_state()?;
        let phase = state
            .as_ref()
            .and_then(|s| s.get("phase"))
            .and_then(Value::as_str);
        Ok(match phase {
            Some("confirm") => format!("state-ok (accepted={})", outcome.accepted()),
            _ => "state-missing".to_string(),
        })
    }

    /// A5: three input requests in one round trip — elicitation, sampling and
    /// `roots/list` — collected into a single `InputRequiredResult`.
    #[tool(description = "InputRequiredResult with multiple input requests")]
    #[allow(deprecated)] // create_message is the sampling API; deprecation is upstream-only.
    async fn test_input_required_result_multiple_inputs(
        &self,
        ctx: &CallToolContext,
    ) -> McpResult<String> {
        ctx.client.store_state(&json!({ "phase": "multi" }))?;
        // `elicit_all` batches into one abort; the other two are recorded by
        // the same mechanism, so all three travel in one `inputRequests`.
        let name_outcome = ctx
            .client
            .elicit(
                "user_name",
                neutral::ElicitParams::new(
                    "What is your name?",
                    json!({
                        "type": "object",
                        "properties": { "name": { "type": "string" } },
                        "required": ["name"],
                    }),
                ),
            )
            .await;
        let greeting = ctx
            .client
            .create_message(
                "greeting",
                json!({
                    "messages": [{
                        "role": "user",
                        "content": { "type": "text", "text": "Generate a greeting" }
                    }],
                    "maxTokens": 50
                }),
            )
            .await;
        let roots = ctx.client.list_roots("client_roots").await;
        // Any one still outstanding aborts the execution; the dispatcher ships
        // every request recorded so far together.
        let (name_outcome, _greeting, _roots) = (name_outcome?, greeting?, roots?);
        Ok(format!(
            "collected all three (accepted={})",
            name_outcome.accepted()
        ))
    }

    /// A6: two sequential rounds. Re-execution makes this fall out naturally —
    /// round 2 replays `step1` from the carried answers and aborts on `step2`,
    /// round 3 replays both and completes. The handler never has to know which
    /// round it is in.
    #[tool(description = "Multi-round InputRequiredResult with evolving state")]
    async fn test_input_required_result_multi_round(
        &self,
        ctx: &CallToolContext,
    ) -> McpResult<String> {
        ctx.client.store_state(&json!({ "round": 1 }))?;
        let step1 = ctx
            .client
            .elicit(
                "step1",
                neutral::ElicitParams::new(
                    "Step 1: What is your name?",
                    json!({
                        "type": "object",
                        "properties": { "name": { "type": "string" } },
                        "required": ["name"],
                    }),
                ),
            )
            .await?;
        ctx.client.store_state(&json!({ "round": 2 }))?;
        let step2 = ctx
            .client
            .elicit(
                "step2",
                neutral::ElicitParams::new(
                    "Step 2: What is your favorite color?",
                    json!({
                        "type": "object",
                        "properties": { "color": { "type": "string" } },
                        "required": ["color"],
                    }),
                ),
            )
            .await?;
        let name = step1
            .content
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("friend");
        let color = step2
            .content
            .get("color")
            .and_then(Value::as_str)
            .unwrap_or("none");
        Ok(format!("{name} likes {color}"))
    }

    /// A7: the same shape as A4, used to prove a *tampered* `requestState` is
    /// rejected. The rejection is the framework's — state is HMAC-signed and
    /// verified before a handler ever sees it.
    #[tool(description = "InputRequiredResult with integrity-protected state")]
    async fn test_input_required_result_tampered_state(
        &self,
        ctx: &CallToolContext,
    ) -> McpResult<String> {
        ctx.client.store_state(&json!({ "secret": "value" }))?;
        let outcome = ctx
            .client
            .elicit(
                "confirm",
                neutral::ElicitParams::new(
                    "Please confirm",
                    json!({
                        "type": "object",
                        "properties": { "ok": { "type": "boolean" } },
                        "required": ["ok"],
                    }),
                ),
            )
            .await?;
        Ok(format!("state verified (accepted={})", outcome.accepted()))
    }

    /// A8: asks only for sampling, so a client that declared just `sampling`
    /// still gets a well-formed `InputRequiredResult`.
    #[tool(description = "InputRequiredResult respecting declared capabilities")]
    #[allow(deprecated)] // create_message is the sampling API; deprecation is upstream-only.
    async fn test_input_required_result_capabilities(
        &self,
        ctx: &CallToolContext,
    ) -> McpResult<String> {
        let answer = ctx
            .client
            .create_message(
                "sampling_only",
                json!({
                    "messages": [{
                        "role": "user",
                        "content": { "type": "text", "text": "Say hello" }
                    }],
                    "maxTokens": 50
                }),
            )
            .await?;
        Ok(answer
            .get("content")
            .and_then(|c| c.get("text"))
            .and_then(Value::as_str)
            .unwrap_or("(sampled)")
            .to_string())
    }

    /// SEP-2575: a tool that needs a client capability, so a caller that did
    /// not declare `sampling` is refused with
    /// `MissingRequiredClientCapabilityError` rather than being served.
    #[tool(description = "Requires the sampling client capability")]
    #[allow(deprecated)] // create_message is the sampling API; deprecation is upstream-only.
    async fn test_missing_capability(&self, ctx: &CallToolContext) -> McpResult<String> {
        let answer = ctx
            .client
            .create_message(
                "needs_sampling",
                json!({
                    "messages": [{
                        "role": "user",
                        "content": { "type": "text", "text": "ping" }
                    }],
                    "maxTokens": 16
                }),
            )
            .await?;
        Ok(answer.to_string())
    }

    /// SEP-2243: a tool with an `#[mcp_header]` parameter, so the custom-header
    /// validation scenario has a mirror to exercise (the server validates the
    /// `Mcp-Param-*` header against the body; it never sources the argument
    /// from the header).
    #[tool(description = "Echo a parameter that is mirrored to an HTTP header")]
    async fn test_custom_headers(
        &self,
        #[description("A value mirrored to Mcp-Param-region")]
        #[mcp_header]
        region: String,
    ) -> McpResult<String> {
        Ok(format!("region={region}"))
    }

    // ---- resources (URIs are load-bearing) ----------------------------------

    /// A static text resource.
    #[resource("test://static-text")]
    async fn static_text(&self) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::new(vec![
            neutral::ResourceContents::text("test://static-text", "static text contents")
                .with_mime_type("text/plain"),
        ]))
    }

    /// A static binary resource (base64 blob + MIME type).
    #[resource("test://static-binary")]
    async fn static_binary(&self) -> McpResult<neutral::ReadResourceResult> {
        Ok(neutral::ReadResourceResult::new(vec![
            neutral::ResourceContents::blob("test://static-binary", PNG_1X1)
                .with_mime_type("image/png"),
        ]))
    }

    /// A templated resource: `test://template/{id}/data`.
    #[resource("test://template/{id}/data")]
    async fn template_data(&self, id: String) -> McpResult<neutral::ReadResourceResult> {
        let uri = format!("test://template/{id}/data");
        Ok(neutral::ReadResourceResult::new(vec![
            neutral::ResourceContents::text(uri, format!("data for id {id}"))
                .with_mime_type("text/plain"),
        ]))
    }

    // ---- prompts (method names are the wire prompt names — load-bearing) ----

    /// A simple prompt with no arguments.
    #[prompt(description = "A simple prompt")]
    async fn test_simple_prompt(&self) -> McpResult<String> {
        Ok("This is a simple prompt.".to_string())
    }

    /// A prompt that substitutes two arguments (the conformance scenario checks
    /// both `arg1` and `arg2` appear in the rendered message).
    #[prompt(description = "A prompt with arguments")]
    async fn test_prompt_with_arguments(
        &self,
        #[description("First argument")] arg1: String,
        #[description("Second argument")] arg2: String,
    ) -> McpResult<String> {
        Ok(format!("Prompt with arg1={arg1} arg2={arg2}"))
    }

    /// A prompt whose message carries an embedded resource. The `resource_uri`
    /// argument is optional so the harness's camelCase `resourceUri` (which does
    /// not bind to the snake_case parameter) doesn't fail a required-arg check.
    #[prompt(description = "A prompt with an embedded resource")]
    async fn test_prompt_with_embedded_resource(
        &self,
        #[description("The resource URI to embed")] resource_uri: Option<String>,
    ) -> McpResult<neutral::GetPromptResult> {
        let uri = resource_uri.unwrap_or_else(|| "test://example-resource".to_string());
        Ok(neutral::GetPromptResult::new(vec![
            neutral::PromptMessage::user(neutral::Content::resource(
                neutral::ResourceContents::text(uri, "embedded prompt resource")
                    .with_mime_type("text/plain"),
            )),
        ]))
    }

    /// A prompt whose message carries an image content block.
    #[prompt(description = "A prompt with an image")]
    async fn test_prompt_with_image(&self) -> McpResult<neutral::GetPromptResult> {
        Ok(neutral::GetPromptResult::new(vec![
            neutral::PromptMessage::user_text("Describe this image:"),
            neutral::PromptMessage::user(neutral::Content::image(PNG_1X1, "image/png")),
        ]))
    }

    /// SEP-2322 B1: `InputRequiredResult` is universal, not a `tools/call`
    /// special case — this prompt asks for elicitation the same way a tool
    /// does, over `prompts/get`.
    #[prompt(description = "A prompt that requires elicitation input")]
    async fn test_input_required_result_prompt(
        &self,
        ctx: &GetPromptContext,
    ) -> McpResult<neutral::GetPromptResult> {
        let outcome = ctx
            .client
            .elicit(
                "user_context",
                neutral::ElicitParams::new(
                    "What context should the prompt use?",
                    json!({
                        "type": "object",
                        "properties": { "context": { "type": "string" } },
                        "required": ["context"],
                    }),
                ),
            )
            .await?;
        let context = outcome
            .content
            .get("context")
            .and_then(Value::as_str)
            .unwrap_or("(none)");
        Ok(neutral::GetPromptResult::new(vec![
            neutral::PromptMessage::user_text(format!("Using context: {context}")),
        ]))
    }

    // ---- completion ---------------------------------------------------------

    /// Suggest completions for a prompt/resource argument.
    #[completion]
    async fn complete(
        &self,
        params: neutral::CompleteParams,
    ) -> McpResult<neutral::CompleteResult> {
        let prefix = params.argument.value;
        Ok(neutral::CompleteResult::new(vec![
            format!("{prefix}-one"),
            format!("{prefix}-two"),
        ]))
    }
}

/// Decode a base64 fixture (used in tests to sanity-check the constants).
#[allow(dead_code)]
pub fn decode_b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD
        .decode(s)
        .expect("valid base64 fixture")
}
