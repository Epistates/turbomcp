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
use serde_json::{Map, Value, json};
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
    /// Pre-issued client id, for scenarios that skip dynamic registration.
    /// An `https://` value is a Client ID Metadata Document URL, not a plain
    /// id — that is how the CIMD scenarios hand one over.
    #[serde(default)]
    client_id: Option<String>,
    #[serde(default)]
    client_secret: Option<String>,
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

/// The revision this run is scored against, defaulting the way the harness
/// itself does when it says nothing.
fn protocol_version() -> String {
    std::env::var("MCP_CONFORMANCE_PROTOCOL_VERSION").unwrap_or_else(|_| "2025-11-25".to_string())
}

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

// ─── Auth ────────────────────────────────────────────────────────────────────

/// Where the mock authorization server sends the browser back. Never listened
/// on: [`run_auth`] reads the redirect out of the `Location` header instead of
/// completing it, which is what makes the flow runnable headlessly.
const REDIRECT_URI: &str = "http://localhost:8090/callback";

/// How many times to re-authorize in response to `insufficient_scope` before
/// concluding the server is never going to be satisfied.
const MAX_AUTH_ROUNDS: usize = 3;

/// The cheapest request that still requires authorization.
const LIST_PROBE: &str = r#"{"jsonrpc":"2.0","id":"probe","method":"tools/list","params":{}}"#;

/// Send one MCP request and return the `WWW-Authenticate` challenge if the
/// server refused it. `None` means the request was accepted.
///
/// Raw HTTP rather than the typed client, because at this point the question is
/// only whether the credential is good enough — a refusal here is the input to
/// the next authorization round, not a failure to report. It has to be raw for
/// a second reason too: the challenge lives in a *header*, and by the time a
/// refusal reaches the typed client it is an error value with the headers long
/// discarded.
async fn probe(
    http: &reqwest::Client,
    url: &str,
    bearer: Option<&str>,
    body: &str,
) -> Option<turbomcp::auth::client::BearerChallenge> {
    let mut req = http
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        // Required from the first post-handshake request onward, and these
        // probes bypass the handshake entirely — without it the stateless
        // revision answers differently and the refusal never shows up.
        .header("mcp-protocol-version", protocol_version())
        .body(body.to_owned());
    if let Some(token) = bearer {
        req = req.bearer_auth(token);
    }
    let resp = req.send().await.ok()?;
    resp.headers()
        .get("www-authenticate")
        .and_then(|v| v.to_str().ok())
        .and_then(turbomcp::auth::client::parse_bearer_challenge)
}

/// A `tools/call` probe body for `name`.
fn call_probe(name: &str) -> String {
    json!({
        "jsonrpc": "2.0",
        "id": "probe-call",
        "method": "tools/call",
        "params": { "name": name, "arguments": {} },
    })
    .to_string()
}

/// The first tool the server lists, read straight off the wire.
async fn first_tool_name(http: &reqwest::Client, url: &str, bearer: &str) -> Option<String> {
    let resp = http
        .post(url)
        .header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream")
        // Required from the first post-handshake request onward, and these
        // probes bypass the handshake entirely — without it the stateless
        // revision answers differently and the refusal never shows up.
        .header("mcp-protocol-version", protocol_version())
        .bearer_auth(bearer)
        .body(LIST_PROBE)
        .send()
        .await
        .ok()?;
    let text = resp.text().await.ok()?;
    let body: Value = serde_json::from_str(&text).ok().or_else(|| {
        // Streamable HTTP lets the server answer either way, and which one it
        // picks is not ours to assume — pull the frame out of the SSE event
        // when that is what came back.
        text.lines()
            .find_map(|line| line.strip_prefix("data:"))
            .and_then(|data| serde_json::from_str(data.trim()).ok())
    })?;
    body.pointer("/result/tools/0/name")?
        .as_str()
        .map(std::string::ToString::to_string)
}

/// Drive the full OAuth 2.1 flow against the scenario's mock authorization
/// server, then use the token on a real MCP session.
///
/// Errors propagate. Roughly half the `auth/*` scenarios are negative — a
/// mismatched `iss`, an issuer that doesn't match its metadata, a resource the
/// token was not issued for — and for those the *correct* client behaviour is
/// to refuse. The harness marks them `allowClientError`, so exiting non-zero is
/// how we report "we declined", while on a positive scenario the same exit is
/// the failure it looks like.
async fn run_auth(url: &str, context: &ScenarioContext) -> Result<(), String> {
    use turbomcp::auth::client::{
        BearerChallenge, ClientCredentials, DynamicRegistration, OAuthClient, RegistrationStrategy,
    };

    // No redirect following anywhere in this flow: the authorization response
    // *is* a redirect, and a client that chased it would throw away the `code`,
    // `state` and `iss` the exchange needs.
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("build http client: {e}"))?;

    // An unauthenticated request first: its `WWW-Authenticate` is where the
    // resource metadata URL and the required scopes come from.
    let mut challenge = probe(&http, url, None, LIST_PROBE).await;
    eprintln!("challenge: {challenge:?}");

    let strategy = match context.client_id.as_deref() {
        // A `client_id` that is an HTTPS URL is a metadata document, used
        // verbatim as the id rather than registered for.
        Some(id) if id.starts_with("https://") => RegistrationStrategy::MetadataDocument {
            client_id_url: id.to_string(),
        },
        Some(id) => RegistrationStrategy::Preregistered {
            credentials: match context.client_secret.as_deref() {
                Some(secret) => ClientCredentials {
                    client_id: id.to_string(),
                    client_secret: Some(secret.to_string()),
                },
                None => ClientCredentials::public(id),
            },
            // The scenario hands us credentials without naming their issuer;
            // binding them to whatever discovery finds is the scenario's
            // premise, so there is nothing to check them against.
            issuer: None,
        },
        None => RegistrationStrategy::Dynamic(DynamicRegistration::native(
            "turbomcp-conformance-client",
            vec![REDIRECT_URI.to_string()],
        )),
    };

    let oauth = OAuthClient::new(url, REDIRECT_URI, strategy).with_http_client(http.clone());

    let mut scopes: Vec<String> = Vec::new();
    let mut granted = None;
    for round in 0..MAX_AUTH_ROUNDS {
        // Re-discovered every round, not hoisted. A server may move to a
        // different authorization server between challenges (SEP-2352), and
        // the whole point of the migration scenario is that the client notices
        // rather than re-presenting credentials issued by the old one.
        let discovered = oauth
            .discover(challenge.as_ref())
            .await
            .map_err(|e| format!("discovery: {e}"))?;
        let credentials = oauth
            .credentials(&discovered)
            .await
            .map_err(|e| format!("credentials: {e}"))?;

        scopes = if round == 0 {
            OAuthClient::select_scopes(challenge.as_ref(), &discovered)
        } else {
            // A step-up asks for the *union*, not just the newly demanded
            // scopes: re-authorizing with only the latter would silently drop
            // permissions the user already granted (SEP-2350).
            let demanded = challenge
                .as_ref()
                .map(BearerChallenge::scopes)
                .unwrap_or_default();
            OAuthClient::step_up_scopes(&scopes, &demanded)
        };
        eprintln!(
            "round {round}: issuer {} scopes {scopes:?}",
            discovered.server.issuer
        );

        let pending = oauth
            .begin(&discovered, &credentials, &scopes)
            .map_err(|e| format!("authorization request: {e}"))?;
        let callback = authorize(&http, &pending.authorize_url).await?;
        let tokens = oauth
            .complete(&discovered, &credentials, pending, &callback)
            .await
            .map_err(|e| format!("token exchange: {e}"))?;

        // Two probes, because scopes are per-operation. A token can be good
        // enough to *list* tools and still be refused when one is *called* —
        // which is exactly the shape of a step-up, and checking only the
        // listing would declare success and never escalate.
        let refusal = match probe(&http, url, Some(&tokens.access_token), LIST_PROBE).await {
            Some(refusal) => Some(refusal),
            None => match first_tool_name(&http, url, &tokens.access_token).await {
                Some(name) => {
                    probe(&http, url, Some(&tokens.access_token), &call_probe(&name)).await
                }
                None => None,
            },
        };

        match refusal {
            // The server wants more than we asked for; go round again.
            Some(next) if next.is_insufficient_scope() => {
                eprintln!("insufficient_scope, stepping up to {:?}", next.scopes());
                challenge = Some(next);
                continue;
            }
            // Any other challenge is a refusal we cannot fix by re-asking.
            Some(other) => return Err(format!("server rejected the token: {other:?}")),
            None => {}
        }

        // Run the real session here rather than after the loop, because a
        // refusal can still arrive at this point: a server may move to a
        // different authorization server mid-session (SEP-2352), and the token
        // that just passed both probes stops being accepted. Re-probing turns
        // that into another round, which re-discovers, finds the new issuer,
        // and registers there — the session error itself is useless for this,
        // since the challenge is a header the typed client has already dropped.
        match session(url, &tokens.access_token).await {
            Ok(()) => {
                granted = Some(tokens);
                break;
            }
            Err(refusal) => {
                // Re-probe with the operation that was actually refused, since
                // a token can be fine for the handshake and short for one call.
                let body = refusal
                    .tool
                    .as_deref()
                    .map_or_else(|| LIST_PROBE.to_owned(), call_probe);
                match probe(&http, url, Some(&tokens.access_token), &body).await {
                    Some(next) => {
                        eprintln!("session refused ({}); re-authorizing", refusal.message);
                        challenge = Some(next);
                    }
                    // No challenge means the failure was not about auth.
                    None => return Err(refusal.message),
                }
            }
        }
    }

    // Bounded on purpose. A server that answers every token with
    // `insufficient_scope` would otherwise have the client re-authorizing
    // forever, which is what the retry-limit scenario checks we don't do.
    granted.map(|_| ()).ok_or_else(|| {
        format!("gave up after {MAX_AUTH_ROUNDS} authorization rounds still short of scope")
    })
}

/// A session that the server would not serve with the token it was given.
struct Refusal {
    message: String,
    /// The tool whose call was refused, when it was a call rather than the
    /// handshake. It is what the caller re-probes with: scopes are
    /// per-operation, so only asking about *this* operation gets the challenge
    /// that says which scope is missing.
    tool: Option<String>,
}

/// The point of the whole exercise: an authenticated MCP session.
async fn session(url: &str, access_token: &str) -> Result<(), Refusal> {
    let transport = turbomcp::client::HttpClientTransport::new(url)
        .map_err(|e| Refusal {
            message: format!("build transport: {e}"),
            tool: None,
        })?
        .with_bearer(access_token.to_owned());
    let client = ClientBuilder::new("turbomcp-conformance-client", env!("CARGO_PKG_VERSION"))
        .with_handler(AutoAnswer)
        .with_connect_mode(connect_mode())
        .with_capabilities(elicitation_capabilities())
        .connect(transport)
        .await
        .map_err(|e| Refusal {
            message: format!("authenticated handshake: {e}"),
            tool: None,
        })?;

    let mut refusal = None;
    for tool in list_tools(&client).await {
        let arguments =
            synthesize_arguments(&tool.input_schema, populates_optional_headers(&tool.name));
        match client.call_tool(&tool.name, arguments).await {
            Ok(_) => eprintln!("called {}", tool.name),
            Err(err) => {
                let message = err.to_string();
                eprintln!("call {} failed: {message}", tool.name);
                // Only an authorization refusal is worth another round. A
                // tool-level error is a legitimate outcome several scenarios
                // provoke on purpose, and re-authorizing over one would turn a
                // working session into a retry loop.
                if refusal.is_none() && is_authorization_refusal(&message) {
                    refusal = Some(Refusal {
                        message,
                        tool: Some(tool.name.clone()),
                    });
                }
            }
        }
    }
    close(client).await;
    refusal.map_or(Ok(()), Err)
}

/// Whether a failed call looks like the server withholding authorization
/// rather than the tool itself failing.
fn is_authorization_refusal(message: &str) -> bool {
    message.contains("401") || message.contains("403")
}

/// Follow the authorization request one hop and read the response out of the
/// `Location` header.
///
/// A real client opens a browser here and receives the redirect on a loopback
/// listener. The mock server auto-approves, so the redirect comes straight back
/// on the response and there is nothing to listen for.
async fn authorize(
    http: &reqwest::Client,
    authorize_url: &str,
) -> Result<turbomcp::auth::client::CallbackParams, String> {
    let resp = http
        .get(authorize_url)
        .send()
        .await
        .map_err(|e| format!("authorization request failed: {e}"))?;
    let status = resp.status();
    let location = resp
        .headers()
        .get(reqwest::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| {
            format!("authorization endpoint answered {status} with no Location header")
        })?;
    Ok(turbomcp::auth::client::CallbackParams::from_query(location))
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

        // Everything namespaced `auth/` runs the same OAuth flow; what varies
        // is how the scenario's mock authorization server misbehaves, and the
        // referee scores what we did about it.
        auth if auth.starts_with("auth/") => {
            if let Err(err) = run_auth(&url, &context).await {
                eprintln!("conformance-client: {auth}: {err}");
                return ExitCode::FAILURE;
            }
        }

        unknown => {
            // Not a pass. See the module docs: an unimplemented scenario that
            // exited 0 would be indistinguishable from a clean run.
            eprintln!("conformance-client: unsupported scenario {unknown}");
            return ExitCode::FAILURE;
        }
    }

    ExitCode::SUCCESS
}
