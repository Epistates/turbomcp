//! Protocol operations against one server: list what it offers, call a tool,
//! read a resource, get a prompt, and probe what it speaks.
//!
//! Human output is for reading; `--json` prints the protocol's own shapes
//! (the `2026-07-28` wire types), for scripts.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use turbomcp::client::ConnectMode;
use turbomcp::neutral;
use turbomcp_protocol::v2026_07_28::types as wire;

use crate::target::Target;

/// The protocol's own JSON for `value`.
fn to_json<W: serde::Serialize>(value: W) -> Result<Value> {
    serde_json::to_value(value).context("rendering JSON")
}

fn print_json(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// `name — summary`, aligned.
fn print_rows(rows: &[(String, String)]) {
    let width = rows.iter().map(|(name, _)| name.len()).max().unwrap_or(0);
    for (name, summary) in rows {
        if summary.is_empty() {
            println!("{name}");
        } else {
            println!("{name:width$}  {summary}");
        }
    }
}

/// The first line of `text`, for a one-line summary.
fn first_line(text: Option<&str>) -> String {
    text.and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()))
        .unwrap_or_default()
        .to_owned()
}

pub async fn tools(target: &Target, json: bool) -> Result<()> {
    let connected = target.connect().await?;
    let tools = connected.client.list_all_tools().await;
    connected.close().await;
    let tools = tools.context("listing tools")?;
    if json {
        let wire: Vec<wire::Tool> = tools.into_iter().map(Into::into).collect();
        return print_json(&to_json(wire)?);
    }
    let rows: Vec<_> = tools
        .iter()
        .map(|t| {
            let summary = first_line(t.title.as_deref().or(t.description.as_deref()));
            (t.name.clone(), summary)
        })
        .collect();
    print_rows(&rows);
    Ok(())
}

pub async fn resources(target: &Target, json: bool) -> Result<()> {
    let connected = target.connect().await?;
    let client = &connected.client;
    let caps = client.server_capabilities().clone();
    let listed = async {
        if caps.resources.is_none() {
            return Ok((Vec::new(), Vec::new()));
        }
        let resources = client.list_all_resources().await?;
        let templates = client.list_all_resource_templates().await?;
        Ok::<_, turbomcp::client::ClientError>((resources, templates))
    }
    .await;
    connected.close().await;
    let (resources, templates) = listed.context("listing resources")?;
    if json {
        let resources: Vec<wire::Resource> = resources.into_iter().map(Into::into).collect();
        let templates: Vec<wire::ResourceTemplate> =
            templates.into_iter().map(Into::into).collect();
        return print_json(&json!({
            "resources": to_json(resources)?,
            "resourceTemplates": to_json(templates)?,
        }));
    }
    let mut rows: Vec<_> = resources
        .iter()
        .map(|r| {
            let summary = first_line(
                r.title
                    .as_deref()
                    .or(r.description.as_deref())
                    .or(Some(r.name.as_str())),
            );
            (r.uri.clone(), summary)
        })
        .collect();
    rows.extend(templates.iter().map(|t| {
        let summary = first_line(
            t.title
                .as_deref()
                .or(t.description.as_deref())
                .or(Some(t.name.as_str())),
        );
        (t.uri_template.clone(), format!("(template) {summary}"))
    }));
    print_rows(&rows);
    Ok(())
}

pub async fn prompts(target: &Target, json: bool) -> Result<()> {
    let connected = target.connect().await?;
    let prompts = connected.client.list_all_prompts().await;
    connected.close().await;
    let prompts = prompts.context("listing prompts")?;
    if json {
        let wire: Vec<wire::Prompt> = prompts.into_iter().map(Into::into).collect();
        return print_json(&to_json(wire)?);
    }
    let rows: Vec<_> = prompts
        .iter()
        .map(|p| {
            let args: Vec<String> = p
                .arguments
                .iter()
                .map(|a| {
                    if a.required {
                        a.name.clone()
                    } else {
                        format!("[{}]", a.name)
                    }
                })
                .collect();
            let summary = first_line(p.title.as_deref().or(p.description.as_deref()));
            let name = if args.is_empty() {
                p.name.clone()
            } else {
                format!("{} {}", p.name, args.join(" "))
            };
            (name, summary)
        })
        .collect();
    print_rows(&rows);
    Ok(())
}

/// `key=value` pairs as a JSON object: a value that parses as JSON (a
/// number, `true`, an object) is that, anything else is a string.
pub fn arguments(json_args: Option<&str>, pairs: &[String]) -> Result<Map<String, Value>> {
    let mut args = match json_args {
        Some(text) => match serde_json::from_str(text).context("--args is not JSON")? {
            Value::Object(map) => map,
            _ => bail!("--args must be a JSON object"),
        },
        None => Map::new(),
    };
    for pair in pairs {
        let (key, value) = pair
            .split_once('=')
            .with_context(|| format!("argument `{pair}` is not `key=value`"))?;
        let value = serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.into()));
        args.insert(key.to_owned(), value);
    }
    Ok(args)
}

/// Print content blocks (`content` of a tool result, a prompt message's
/// `content`) for reading: text as is, anything else summarized.
fn print_content(block: &Value) {
    match block.get("type").and_then(Value::as_str) {
        Some("text") => println!("{}", block["text"].as_str().unwrap_or_default()),
        Some(kind @ ("image" | "audio")) => println!(
            "[{kind} {}, {} base64 bytes]",
            block["mimeType"].as_str().unwrap_or("?"),
            block["data"].as_str().map_or(0, str::len)
        ),
        Some("resource_link") => println!("[link {}]", block["uri"].as_str().unwrap_or("?")),
        Some("resource") => print_resource_contents(&block["resource"]),
        _ => println!("{block}"),
    }
}

fn print_resource_contents(contents: &Value) {
    match (contents.get("text"), contents.get("blob")) {
        (Some(text), _) => println!("{}", text.as_str().unwrap_or_default()),
        (None, Some(blob)) => println!(
            "[{} {}, {} base64 bytes]",
            contents["uri"].as_str().unwrap_or("?"),
            contents["mimeType"].as_str().unwrap_or("binary"),
            blob.as_str().map_or(0, str::len)
        ),
        _ => println!("{contents}"),
    }
}

/// Call `tool`. A tool that reports an error makes the command fail.
pub async fn call(target: &Target, tool: &str, args: Map<String, Value>, json: bool) -> Result<()> {
    let connected = target.connect().await?;
    let result = connected.client.call_tool(tool, args).await;
    connected.close().await;
    let result = result.with_context(|| format!("calling `{tool}`"))?;
    let failed = result.is_error;
    let wire = to_json(wire::CallToolResult::from(result))?;
    if json {
        print_json(&wire)?;
    } else {
        for block in wire["content"].as_array().into_iter().flatten() {
            print_content(block);
        }
        if let Some(structured) = wire.get("structuredContent") {
            println!("{}", serde_json::to_string_pretty(structured)?);
        }
    }
    if failed {
        bail!("`{tool}` reported an error");
    }
    Ok(())
}

pub async fn read(target: &Target, uri: &str, json: bool) -> Result<()> {
    let connected = target.connect().await?;
    let result = connected.client.read_resource(uri).await;
    connected.close().await;
    let wire = to_json(wire::ReadResourceResult::from(
        result.with_context(|| format!("reading {uri}"))?,
    ))?;
    if json {
        return print_json(&wire);
    }
    for contents in wire["contents"].as_array().into_iter().flatten() {
        print_resource_contents(contents);
    }
    Ok(())
}

pub async fn prompt(
    target: &Target,
    name: &str,
    args: BTreeMap<String, String>,
    json: bool,
) -> Result<()> {
    let connected = target.connect().await?;
    let result = connected.client.get_prompt(name, args).await;
    connected.close().await;
    let wire = to_json(wire::GetPromptResult::from(
        result.with_context(|| format!("getting prompt `{name}`"))?,
    ))?;
    if json {
        return print_json(&wire);
    }
    for message in wire["messages"].as_array().into_iter().flatten() {
        print!("{}: ", message["role"].as_str().unwrap_or("?"));
        print_content(&message["content"]);
    }
    Ok(())
}

/// What a server speaks: which revisions it negotiates, who it says it is,
/// what it declares, and how much it offers.
pub async fn probe(target: &Target, json: bool) -> Result<()> {
    let mut revisions = Vec::new();
    for (label, mode) in [
        ("2026-07-28", ConnectMode::Modern),
        ("2025-*", ConnectMode::Legacy),
    ] {
        let outcome = match target.connect_as(mode).await {
            Ok(connected) => {
                let version = connected.client.protocol_version().clone();
                connected.close().await;
                Ok(version)
            }
            Err(e) => Err(format!("{e:#}")),
        };
        revisions.push((label, outcome));
    }
    let connected = target
        .connect_as(ConnectMode::Auto)
        .await
        .context("connecting")?;
    let client = &connected.client;
    let info = client.server_info().cloned();
    let instructions = client.instructions().map(str::to_owned);
    let version = client.protocol_version().clone();
    let caps = client.server_capabilities().clone();
    let counts = async {
        let tools = match caps.tools {
            Some(_) => Some(client.list_all_tools().await?.len()),
            None => None,
        };
        let resources = match caps.resources {
            Some(_) => Some((
                client.list_all_resources().await?.len(),
                client.list_all_resource_templates().await?.len(),
            )),
            None => None,
        };
        let prompts = match caps.prompts {
            Some(_) => Some(client.list_all_prompts().await?.len()),
            None => None,
        };
        Ok::<_, turbomcp::client::ClientError>((tools, resources, prompts))
    }
    .await;
    connected.close().await;
    let (tools, resources, prompts) = counts.context("listing what the server offers")?;

    if json {
        let revisions: Map<String, Value> = revisions
            .iter()
            .map(|(label, outcome)| {
                let value = match outcome {
                    Ok(version) => json!({ "negotiated": version.as_str() }),
                    Err(e) => json!({ "error": e }),
                };
                ((*label).to_owned(), value)
            })
            .collect();
        return print_json(&json!({
            "server": info.as_ref().map(|i| json!({
                "name": i.name, "version": i.version, "title": i.title,
            })),
            "instructions": instructions,
            "negotiated": version.as_str(),
            "stateful": version.is_stateful(),
            "revisions": revisions,
            "capabilities": capabilities(&caps),
            "counts": {
                "tools": tools,
                "resources": resources.map(|(r, _)| r),
                "resourceTemplates": resources.map(|(_, t)| t),
                "prompts": prompts,
            },
        }));
    }
    if let Some(info) = &info {
        let title = info
            .title
            .as_deref()
            .map(|t| format!(" ({t})"))
            .unwrap_or_default();
        println!("server        {} {}{title}", info.name, info.version);
    }
    println!(
        "negotiated    {} ({})",
        version.as_str(),
        if version.is_stateful() {
            "stateful"
        } else {
            "stateless"
        }
    );
    for (label, outcome) in &revisions {
        match outcome {
            Ok(version) => println!("  {label:<11} yes: {}", version.as_str()),
            Err(e) => println!("  {label:<11} no: {}", first_line(Some(e))),
        }
    }
    let declared = capabilities(&caps);
    let names: Vec<&str> = declared
        .as_object()
        .into_iter()
        .flat_map(|m| m.keys().map(String::as_str))
        .collect();
    println!("capabilities  {}", names.join(", "));
    if !caps.extensions.is_empty() {
        let ids: Vec<&str> = caps.extensions.keys().map(String::as_str).collect();
        println!("extensions    {}", ids.join(", "));
    }
    let count =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    let mut offered = Vec::new();
    if let Some(n) = tools {
        offered.push(count(n, "tool", "tools"));
    }
    if let Some((n, t)) = resources {
        offered.push(count(n, "resource", "resources"));
        offered.push(count(t, "template", "templates"));
    }
    if let Some(n) = prompts {
        offered.push(count(n, "prompt", "prompts"));
    }
    if !offered.is_empty() {
        println!("offers        {}", offered.join(", "));
    }
    if let Some(instructions) = instructions {
        println!("instructions  {}", first_line(Some(&instructions)));
    }
    Ok(())
}

/// What `caps` declares, as the wire spells it.
fn capabilities(caps: &neutral::ServerCapabilities) -> Value {
    let mut out = Map::new();
    if let Some(tools) = &caps.tools {
        out.insert("tools".into(), json!({ "listChanged": tools.list_changed }));
    }
    if let Some(resources) = &caps.resources {
        out.insert(
            "resources".into(),
            json!({ "subscribe": resources.subscribe, "listChanged": resources.list_changed }),
        );
    }
    if let Some(prompts) = &caps.prompts {
        out.insert(
            "prompts".into(),
            json!({ "listChanged": prompts.list_changed }),
        );
    }
    if caps.completions {
        out.insert("completions".into(), json!({}));
    }
    if caps.logging {
        out.insert("logging".into(), json!({}));
    }
    if let Some(tasks) = &caps.tasks {
        out.insert("tasks".into(), tasks.clone());
    }
    if !caps.extensions.is_empty() {
        out.insert("extensions".into(), json!(caps.extensions));
    }
    if !caps.experimental.is_empty() {
        out.insert("experimental".into(), json!(caps.experimental));
    }
    Value::Object(out)
}
