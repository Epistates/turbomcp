//! The server list MCP hosts already share: `{"mcpServers": {name: entry}}`
//! (Claude Desktop, Cursor, Windsurf), or `{"servers": …}` (VS Code).
//!
//! An entry is a command (`command`, `args`, `env`, `cwd`) or an endpoint
//! (`url`, `headers`, and a `type` of `http`/`streamable-http` or
//! `ws`/`websocket`, inferred from the URL when absent). `${VAR}` and
//! `${env:VAR}` in any string are replaced from the environment, so secrets
//! can stay out of the file. Entries with `"disabled": true` are skipped;
//! fields other hosts add are ignored.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use turbomcp::proxy::Upstream;

#[derive(Deserialize)]
struct File {
    #[serde(rename = "mcpServers", alias = "servers")]
    servers: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
struct Entry {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    cwd: Option<PathBuf>,
    url: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    #[serde(default)]
    disabled: bool,
}

/// One configured server.
pub struct Server {
    pub name: String,
    pub upstream: Upstream,
}

/// The enabled servers in the file at `path`, in name order.
pub fn load(path: &Path) -> Result<Vec<Server>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse(&text).with_context(|| format!("in {}", path.display()))
}

/// The enabled servers in `text`.
pub fn parse(text: &str) -> Result<Vec<Server>> {
    let file: File = serde_json::from_str(text).context("not an mcpServers configuration")?;
    let mut servers = Vec::new();
    for (name, entry) in file.servers {
        if entry.disabled {
            continue;
        }
        let upstream = entry
            .into_upstream()
            .with_context(|| format!("server `{name}`"))?;
        servers.push(Server { name, upstream });
    }
    Ok(servers)
}

impl Entry {
    fn into_upstream(self) -> Result<Upstream> {
        let kind = self.kind.as_deref().map(str::to_ascii_lowercase);
        match (self.command, self.url) {
            (Some(_), Some(_)) => bail!("has both a `command` and a `url`"),
            (None, None) => bail!("has neither a `command` nor a `url`"),
            (Some(command), None) => {
                if kind.as_deref().is_some_and(|k| k != "stdio") {
                    bail!(
                        "a `command` server is `stdio`, not `{}`",
                        kind.unwrap_or_default()
                    );
                }
                let args = self
                    .args
                    .iter()
                    .map(|a| interpolate(a))
                    .collect::<Result<Vec<_>>>()?;
                let mut upstream = Upstream::stdio(interpolate(&command)?, args);
                for (key, value) in &self.env {
                    upstream = upstream.env(key, interpolate(value)?);
                }
                if let Some(cwd) = self.cwd {
                    upstream = upstream.cwd(cwd);
                }
                Ok(upstream)
            }
            (None, Some(url)) => {
                let url = interpolate(&url)?;
                let websocket = url.starts_with("ws://") || url.starts_with("wss://");
                let mut upstream = match kind.as_deref() {
                    None if websocket => Upstream::websocket(url),
                    None | Some("http" | "streamable-http" | "streamablehttp") => {
                        Upstream::http(url)
                    }
                    Some("ws" | "websocket") => Upstream::websocket(url),
                    Some("sse") => bail!(
                        "uses the HTTP+SSE transport of 2024-11-05, which the current \
                         revisions replaced with Streamable HTTP; point it at the server's \
                         Streamable HTTP endpoint (type `http`)"
                    ),
                    Some(other) => bail!("has an unknown type `{other}`"),
                };
                for (name, value) in &self.headers {
                    upstream = upstream.header(name, interpolate(value)?);
                }
                Ok(upstream)
            }
        }
    }
}

/// `text` with every `${VAR}` and `${env:VAR}` replaced by the variable's
/// value. A variable that isn't set is an error naming it, not an empty
/// string.
pub fn interpolate(text: &str) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            bail!("unclosed `${{` in `{text}`");
        };
        let name = after[..end].strip_prefix("env:").unwrap_or(&after[..end]);
        if name.is_empty() {
            bail!("empty `${{}}` in `{text}`");
        }
        let value = std::env::var(name)
            .with_context(|| format!("environment variable `{name}` is not set"))?;
        out.push_str(&value);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_host_shape_parses() {
        let servers = parse(
            r#"{
                "mcpServers": {
                    "files": { "command": "npx", "args": ["-y", "server"], "env": { "A": "b" } },
                    "remote": { "url": "https://example.com/mcp", "headers": { "X-Key": "k" } },
                    "socket": { "url": "wss://example.com/ws" },
                    "typed": { "type": "streamable-http", "url": "https://example.com/mcp" },
                    "off": { "command": "nothing", "disabled": true },
                    "extra": { "command": "x", "alwaysAllow": ["t"], "autoApprove": [] }
                }
            }"#,
        )
        .unwrap();
        let names: Vec<_> = servers.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["extra", "files", "remote", "socket", "typed"]);
        assert!(matches!(servers[1].upstream, Upstream::Stdio { .. }));
        assert!(matches!(servers[2].upstream, Upstream::Http { .. }));
        assert!(matches!(servers[3].upstream, Upstream::WebSocket { .. }));
        // VS Code's top-level key.
        assert_eq!(
            parse(r#"{"servers": {"a": {"command": "x"}}}"#)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn a_retired_or_ambiguous_entry_is_refused_with_a_reason() {
        let err = |json: &str| format!("{:#}", parse(json).err().unwrap());
        assert!(
            err(r#"{"mcpServers": {"a": {"type": "sse", "url": "https://x/sse"}}}"#)
                .contains("Streamable HTTP")
        );
        assert!(
            err(r#"{"mcpServers": {"a": {"command": "x", "url": "https://x"}}}"#).contains("both")
        );
        assert!(err(r#"{"mcpServers": {"a": {}}}"#).contains("neither"));
    }

    #[test]
    fn variables_are_replaced_and_a_missing_one_is_named() {
        // PATH is set wherever the tests run.
        let path = std::env::var("PATH").unwrap();
        assert_eq!(interpolate("${PATH}").unwrap(), path);
        assert_eq!(
            interpolate("a ${env:PATH} b").unwrap(),
            format!("a {path} b")
        );
        assert_eq!(interpolate("no vars").unwrap(), "no vars");
        let missing = interpolate("${TURBOMCP_SURELY_UNSET_VAR}").unwrap_err();
        assert!(missing.to_string().contains("TURBOMCP_SURELY_UNSET_VAR"));
        assert!(interpolate("${OPEN").is_err());
    }
}
