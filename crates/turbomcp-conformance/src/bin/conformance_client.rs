//! The TurboMCP client under test, driven by the official conformance harness.
//!
//! This is the mirror image of the server suite. There the harness connects to
//! us as a client; here it *is* the server: for each scenario it stands up a
//! mock MCP server with deliberately awkward behaviour, spawns this binary
//! against it, and referees what we did on the wire.
//!
//! The contract the harness expects (matching every other SDK's client runner):
//!
//! - `argv[1]` — the mock server's URL. Defaults to the harness's usual port so
//!   the binary is still runnable by hand.
//! - `MCP_CONFORMANCE_SCENARIO` — which scenario to run. Unset means
//!   `initialize`, the harness's own default.
//! - `MCP_CONFORMANCE_CONTEXT` — scenario-supplied JSON (e.g. which tools to
//!   call). Absent or unparseable is not an error; it means "no extras".
//! - `MCP_CONFORMANCE_PROTOCOL_VERSION` — the revision this run is scored
//!   against, which decides whether we take the legacy or the stateless path.
//!
//! An unknown scenario exits non-zero rather than silently passing: the harness
//! scores what appeared on the wire, so a scenario we never implemented would
//! otherwise look like a clean run that simply asserted nothing.

use std::process::ExitCode;

use serde::Deserialize;
use serde_json::{Map, Value};
use turbomcp::client::{Client, ClientBuilder, ClientHandler, ConnectMode, connect_http};
use turbomcp::neutral;

/// Where the harness serves from when it doesn't say otherwise.
const DEFAULT_URL: &str = "http://127.0.0.1:3000/mcp";

// ─── Scenario context ────────────────────────────────────────────────────────

/// A tool call the scenario wants us to make.
#[derive(Debug, Default, Deserialize)]
struct ContextToolCall {
    name: String,
    #[serde(default)]
    arguments: Option<Map<String, Value>>,
}

/// The scenario-supplied `MCP_CONFORMANCE_CONTEXT` payload.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
struct ScenarioContext {
    #[serde(default)]
    tool_calls: Vec<ContextToolCall>,
}

impl ScenarioContext {
    fn from_env() -> Self {
        let Ok(raw) = std::env::var("MCP_CONFORMANCE_CONTEXT") else {
            return Self::default();
        };
        // Logged rather than swallowed: "the scenario asked for specific tool
        // calls and we silently made up our own" is otherwise invisible, and
        // it looks exactly like a scenario that supplied no context at all.
        match serde_json::from_str(&raw) {
            Ok(context) => context,
            Err(err) => {
                eprintln!("context parse failed ({err}); raw: {raw}");
                Self::default()
            }
        }
    }
}

// ─── Client handlers ─────────────────────────────────────────────────────────

/// Declines everything. `elicit` has no safe default, so even the do-nothing
/// handler has to answer it — declining is always a valid response.
struct Basic;

#[turbomcp::client::async_trait]
impl ClientHandler for Basic {
    async fn elicit(&self, _request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        neutral::ElicitOutcome::new(neutral::ElicitAction::Decline, Map::new())
    }
}

/// Accepts every elicitation, filling the requested form from its own schema.
///
/// SEP-1034 marks form fields with a `default`, and the scenario checks that a
/// client offering `elicitation` honours them exactly rather than declining or
/// inventing values — so [`sample_value`] consults `default` before anything
/// else. Fields without one still get a type-appropriate value: the MRTR
/// scenarios elicit a plain `confirmed: boolean` with no default, and a client
/// that answered nothing would stall the loop they exist to score.
struct AutoAnswer;

#[turbomcp::client::async_trait]
impl ClientHandler for AutoAnswer {
    async fn elicit(&self, request: neutral::ElicitParams) -> neutral::ElicitOutcome {
        let mut content = Map::new();
        if let Some(properties) = request
            .requested_schema
            .get("properties")
            .and_then(Value::as_object)
        {
            for (name, property) in properties {
                content.insert(name.clone(), sample_value(property));
            }
        }
        neutral::ElicitOutcome::new(neutral::ElicitAction::Accept, content)
    }
}

/// The capability set a client must advertise to be *offered* elicitation at
/// all. `formats: ["form"]` is the form-mode declaration both wires use.
fn elicitation_capabilities() -> Value {
    serde_json::json!({
        "elicitation": { "formats": ["form"] }
    })
}

// ─── Connecting ──────────────────────────────────────────────────────────────

/// Which handshake this run should take.
///
/// The harness tells us the revision it is scoring; `2026-07-28` is the
/// stateless `server/discover` path and the dated revisions before it are the
/// `initialize` handshake. We pin the mode rather than letting [`ConnectMode`]
/// probe, because a scenario scoring the legacy handshake must not be answered
/// by a `server/discover` that happens to succeed.
fn connect_mode() -> ConnectMode {
    match std::env::var("MCP_CONFORMANCE_PROTOCOL_VERSION").as_deref() {
        Ok("2026-07-28") => ConnectMode::Modern,
        Ok(_) => ConnectMode::Legacy,
        // The harness only omits this for scenarios that predate the split.
        Err(_) => ConnectMode::Auto,
    }
}

async fn connect(url: &str, handler: impl ClientHandler, capabilities: Option<Value>) -> Client {
    let mut builder = ClientBuilder::new("turbomcp-conformance-client", env!("CARGO_PKG_VERSION"))
        .with_handler(handler)
        .with_connect_mode(connect_mode());
    if let Some(capabilities) = capabilities {
        builder = builder.with_capabilities(capabilities);
    }
    match connect_http(builder, url).await {
        Ok(client) => client,
        Err(err) => fatal(format!("connect to {url}: {err}")),
    }
}

// ─── Scenario bodies ─────────────────────────────────────────────────────────

/// Connect, enumerate tools, disconnect.
///
/// Enough for every scenario that scores the handshake itself, or that checks
/// we *didn't* do something while listing (`json-schema-ref-no-deref` passes
/// when we never fetch the network `$ref` in a tool's schema).
async fn run_handshake_only(url: &str) {
    let client = connect(url, Basic, None).await;
    report_tools(&client).await;
    close(client).await;
}

/// Connect and call the tools the scenario named — or, when it named none,
/// every tool the server advertises.
///
/// Arguments come from the scenario when it supplies them and are otherwise
/// synthesized from each tool's `inputSchema`: the harness sets
/// `MCP_CONFORMANCE_CONTEXT` only for scenarios that ask for something
/// specific, so a scenario refereeing a plain `tools/call` (`add_numbers`
/// wants two *numbers*) is unreachable with empty arguments.
async fn run_tool_calls(url: &str, context: &ScenarioContext) {
    // Elicitation is advertised unconditionally: a tool call is exactly where a
    // server asks the client something back, and the MRTR scenarios only reach
    // their checks if we are eligible to be asked.
    let client = connect(url, AutoAnswer, Some(elicitation_capabilities())).await;

    // Always list first, even when the scenario dictates the calls: listing is
    // what teaches the client which arguments a tool marks `x-mcp-header`, and
    // without it `tools/call` mirrors nothing and the SEP-2243 header checks
    // have no headers to referee.
    let tools = list_tools(&client).await;

    if context.tool_calls.is_empty() {
        for tool in &tools {
            let arguments =
                synthesize_arguments(&tool.input_schema, populates_optional_headers(&tool.name));
            call(&client, &tool.name, arguments).await;
        }
    } else {
        for spec in &context.tool_calls {
            call(
                &client,
                &spec.name,
                spec.arguments.clone().unwrap_or_default(),
            )
            .await;
        }
    }

    close(client).await;
}

/// List tools, then hand one tool's `inputSchema` straight back to the server
/// through the scenario's echo tool.
///
/// SEP-2106: a client must carry a tool's schema verbatim — `$defs`,
/// `$anchor`, `$ref`, `if`/`then`/`else` and every other 2020-12 keyword it
/// does not itself understand. The referee compares what it served against
/// what we echoed, so anything we normalize away shows up as a diff.
async fn run_schema_echo(url: &str) {
    const SUBJECT: &str = "json_schema_2020_12_tool";
    const ECHO: &str = "json_schema_echo";

    let client = connect(url, Basic, None).await;
    let tools = list_tools(&client).await;

    match tools.iter().find(|t| t.name == SUBJECT) {
        Some(subject) => {
            let mut arguments = Map::new();
            arguments.insert("schema".to_string(), subject.input_schema.clone());
            call(&client, ECHO, arguments).await;
        }
        None => eprintln!("{SUBJECT} not advertised; nothing to echo"),
    }

    close(client).await;
}

/// Call the tool whose stream the scenario severs, so the client's SSE retry is
/// what gets scored.
async fn run_sse_retry(url: &str) {
    let client = connect(url, Basic, None).await;
    for tool in report_tools(&client).await {
        if tool.contains("reconnect") {
            call(&client, &tool, Map::new()).await;
        }
    }
    close(client).await;
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// List every tool, logging what came back.
async fn list_tools(client: &Client) -> Vec<neutral::Tool> {
    match client.list_all_tools().await {
        Ok(tools) => {
            let names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
            eprintln!("tools: {}", names.join(", "));
            tools
        }
        Err(err) => {
            eprintln!("list_tools failed: {err}");
            Vec::new()
        }
    }
}

/// List tools only for their names (scenarios that score the listing itself).
async fn report_tools(client: &Client) -> Vec<String> {
    list_tools(client)
        .await
        .into_iter()
        .map(|t| t.name)
        .collect()
}

/// Call one tool, logging the outcome either way.
///
/// A failure is not this binary's failure: several scenarios provoke a
/// tool-level error deliberately, and the referee scores what appeared on the
/// wire rather than what we did with the answer.
async fn call(client: &Client, name: &str, arguments: Map<String, Value>) {
    match client.call_tool(name, arguments).await {
        Ok(_) => eprintln!("called {name}"),
        Err(err) => eprintln!("call {name} failed: {err}"),
    }
}

/// Whether to populate a tool's *optional* `x-mcp-header` parameters.
///
/// The custom-header scenario serves two tools whose schemas are byte-identical
/// — same properties, same annotations, same `required` list — and asks
/// opposite things of them. One must carry every mirrored parameter, so the
/// base64 encoder is exercised on values that aren't token-safe. The other must
/// leave the optional ones absent, so their headers are omitted. Nothing in the
/// schema tells them apart, so no schema-driven rule can satisfy both and the
/// runner has to carry the scenario's intent itself. The name is the only
/// signal the scenario gives, which is why it is read here.
fn populates_optional_headers(tool_name: &str) -> bool {
    !tool_name.ends_with("_null")
}

/// Build an argument object satisfying a tool's `inputSchema`.
///
/// The `required` properties are always filled, because the call is invalid
/// without them. Properties annotated `x-mcp-header` are filled too when
/// `optional_headers` is set: the annotation declares that the value is
/// mirrored into an `Mcp-Param-*` header, so omitting it means the mirroring
/// under test never happens. Everything else is left absent — an optional
/// property is legitimately omittable, and inventing values risks tripping a
/// scenario watching for exactly what we send.
fn synthesize_arguments(schema: &Value, optional_headers: bool) -> Map<String, Value> {
    let mut arguments = Map::new();
    let Some(properties) = schema.get("properties").and_then(Value::as_object) else {
        return arguments;
    };
    let required: Vec<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|names| names.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    for (name, property) in properties {
        let mirrored = optional_headers && property.get("x-mcp-header").is_some();
        if !mirrored && !required.contains(&name.as_str()) {
            continue;
        }
        let value = if mirrored {
            header_sample_value(property)
        } else {
            sample_value(property)
        };
        arguments.insert(name.clone(), value);
    }
    arguments
}

/// A value for a parameter that will be mirrored into an `Mcp-Param-*` header.
///
/// Same as [`sample_value`], except that a string gets one that is *not*
/// token-safe. SEP-2243 requires such a value to be base64-encoded on the wire,
/// and a client that only ever mirrors plain ASCII never exercises the encoder
/// — the suite reports that as a check it declared but never got to observe,
/// which is a hole in the testing rather than a passing client.
fn header_sample_value(property: &Value) -> Value {
    match property.get("type").and_then(Value::as_str) {
        Some("string") if property.get("default").is_none() && property.get("enum").is_none() => {
            Value::String("héllo wörld".to_string())
        }
        _ => sample_value(property),
    }
}

/// A value satisfying one property schema: whatever the schema itself suggests
/// (`default`, then the first `enum` member), else a placeholder for its type.
fn sample_value(property: &Value) -> Value {
    if let Some(default) = property.get("default") {
        return default.clone();
    }
    if let Some(first) = property
        .get("enum")
        .and_then(Value::as_array)
        .and_then(|members| members.first())
    {
        return first.clone();
    }
    match property.get("type").and_then(Value::as_str) {
        Some("number" | "integer") => Value::from(1),
        Some("boolean") => Value::Bool(true),
        Some("array") => Value::Array(Vec::new()),
        Some("object") => Value::Object(Map::new()),
        // Absent or unrecognized `type`: a string is the safest guess, and a
        // schema too exotic to sample is the scenario's business, not ours.
        _ => Value::String("conformance".to_string()),
    }
}

/// End the session and give the transport time to say so.
///
/// TurboMCP has no explicit `Client::close()`: teardown is drop-driven, and the
/// connection actor sends the HTTP transport's session-terminating `DELETE`
/// only after the last clone drops. In a long-lived program that is invisible,
/// but this binary exits immediately afterwards and would race the runtime
/// shutdown, so the settle below is what actually gets the `DELETE` onto the
/// wire. Replace it with an awaited close once the client grows one.
async fn close(client: Client) {
    drop(client);
    tokio::time::sleep(SETTLE).await;
}

/// How long to let the connection actor finish its teardown. Generous: it is
/// paid once per scenario process and only ever shortens the harness's wait.
const SETTLE: std::time::Duration = std::time::Duration::from_millis(250);

fn fatal(message: String) -> ! {
    eprintln!("conformance-client: {message}");
    std::process::exit(1)
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> ExitCode {
    let url = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("no server URL in argv[1]; defaulting to {DEFAULT_URL}");
        DEFAULT_URL.to_string()
    });
    let scenario =
        std::env::var("MCP_CONFORMANCE_SCENARIO").unwrap_or_else(|_| "initialize".to_string());
    let context = ScenarioContext::from_env();

    eprintln!("scenario {scenario} against {url} ({:?})", connect_mode());

    match scenario.as_str() {
        // Handshake, and the scenarios that score a plain listing.
        "initialize" | "json-schema-ref-no-deref" | "request-metadata" => {
            run_handshake_only(&url).await
        }

        "json-schema-2020-12-preservation" => run_schema_echo(&url).await,

        // Scenarios that need a `tools/call` on the wire to referee — including
        // the MRTR ones, whose whole subject is what the client does with an
        // `input_required` result it gets back from one.
        "tools_call"
        | "elicitation-sep1034-client-defaults"
        | "sep-2322-client-request-state"
        | "http-standard-headers"
        | "http-custom-headers"
        | "http-invalid-tool-headers" => run_tool_calls(&url, &context).await,

        "sse-retry" => run_sse_retry(&url).await,

        unknown => {
            // Not a pass. See the module docs: an unimplemented scenario that
            // exited 0 would be indistinguishable from a clean run.
            eprintln!("conformance-client: unsupported scenario {unknown}");
            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}
