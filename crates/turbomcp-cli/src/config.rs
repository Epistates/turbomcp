//! The server list MCP hosts already share: `{"mcpServers": {name: entry}}`
//! (Claude Desktop, Cursor, Windsurf), `{"servers": …}` (VS Code's
//! `mcp.json`), or `{"mcp": {"servers": …}}` (VS Code's `settings.json`).
//!
//! An entry is a command (`command`, `args`, `env`, `cwd`) or an endpoint
//! (`url`, `headers`, and a `type` of `http`/`streamable-http` or
//! `ws`/`websocket`, inferred from the URL when absent). In any string,
//! `${VAR}` and `${env:VAR}` are replaced from the environment (`${VAR:-x}`
//! falls back to `x`), `${workspaceFolder}` by the directory the command
//! runs in, `${userHome}` by the home directory, and `${pathSeparator}` (or
//! `${/}`) by the platform's, so secrets can stay out of the file. Entries
//! with `"disabled": true` are skipped; fields other hosts add are ignored.
//!
//! Each entry is read on its own: one this command can't use (the retired
//! `sse` transport, a typo) is skipped with its reason rather than failing
//! the file, unless it was asked for by name.
//!
//! Beyond what hosts write, an entry may say how the gateway treats it:
//!
//! ```json
//! {
//!   "inherit": "safe",            // what a command inherits: safe | all | none
//!   "timeout": 120,               // seconds an upstream may go silent on a call
//!   "key": "principal",           // connection per: global | principal | session
//!   "forwardSampling": false,     // let it run the caller's model
//!   "forwardRoots": false,        // let it see the caller's roots
//!   "tags": ["internal"],         // tags the gateway gives its components
//!   "auth": { "bearer": "${TOKEN}" }
//! }
//! ```
//!
//! `auth` is one of `{"bearer": token}`, `{"clientCredentials": account}` or
//! `{"tokenExchange": account}`, an account being `{"issuer", "clientId",
//! "clientSecret", "scopes"?}`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Deserialize;
use serde_json::Value;
use turbomcp::proxy::{Inherit, OutboundAuth, ServiceAccount, Upstream, UpstreamKey};

#[derive(Deserialize)]
struct File {
    #[serde(rename = "mcpServers", alias = "servers")]
    servers: Option<BTreeMap<String, Value>>,
    mcp: Option<Section>,
}

#[derive(Deserialize)]
struct Section {
    servers: BTreeMap<String, Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
    cwd: Option<String>,
    url: Option<String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
    inherit: Option<String>,
    timeout: Option<u64>,
    key: Option<String>,
    #[serde(default)]
    forward_sampling: bool,
    #[serde(default)]
    forward_roots: bool,
    #[serde(default)]
    tags: Vec<String>,
    auth: Option<Auth>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
enum Auth {
    Bearer(String),
    ClientCredentials(Account),
    TokenExchange(Account),
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Account {
    issuer: String,
    client_id: String,
    client_secret: String,
    scopes: Option<Vec<String>>,
}

/// One configured server.
pub struct Server {
    pub name: String,
    pub upstream: Upstream,
    /// How the gateway treats it.
    pub settings: Settings,
}

/// What an entry says about how the gateway treats its server.
#[derive(Default)]
pub struct Settings {
    pub auth: OutboundAuth,
    pub timeout: Option<Duration>,
    pub key: Option<UpstreamKey>,
    pub forward_sampling: bool,
    pub forward_roots: bool,
    pub tags: Vec<String>,
}

/// The servers a configuration yields, and those it had to skip.
pub struct Loaded {
    pub servers: Vec<Server>,
    /// Entries that couldn't be used, by name, with why.
    pub skipped: Vec<(String, anyhow::Error)>,
}

/// The enabled servers in the file at `path`, in name order: all of them,
/// or just those named in `only`.
pub fn load(path: &Path, only: &[String]) -> Result<Loaded> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    parse(&text, only).with_context(|| format!("in {}", path.display()))
}

/// The enabled servers in `text` (see [`load`]).
///
/// # Errors
/// When `text` isn't a server list, or a server named in `only` is missing,
/// disabled, or unusable. Any other unusable entry is skipped instead.
pub fn parse(text: &str, only: &[String]) -> Result<Loaded> {
    let file: File = serde_json::from_str(text).context("not an mcpServers configuration")?;
    let Some(mut entries) = file.servers.or(file.mcp.map(|section| section.servers)) else {
        bail!("not an mcpServers configuration: no `mcpServers`, `servers` or `mcp.servers`");
    };
    if !only.is_empty() {
        for name in only {
            if !entries.contains_key(name) {
                bail!("no server `{name}`");
            }
        }
        entries.retain(|name, _| only.contains(name));
    }
    let mut loaded = Loaded {
        servers: Vec::new(),
        skipped: Vec::new(),
    };
    for (name, value) in entries {
        let disabled = value.get("disabled").and_then(Value::as_bool) == Some(true);
        let chosen = only.contains(&name);
        if disabled {
            if chosen {
                bail!("server `{name}` is disabled");
            }
            continue;
        }
        match into_server(&name, value) {
            Ok(server) => loaded.servers.push(server),
            Err(e) if chosen => return Err(e).with_context(|| format!("server `{name}`")),
            Err(e) => loaded.skipped.push((name, e)),
        }
    }
    Ok(loaded)
}

fn into_server(name: &str, value: Value) -> Result<Server> {
    let entry: Entry = serde_json::from_value(value).context("not a server entry")?;
    let settings = entry.settings()?;
    let upstream = entry.into_upstream()?;
    if matches!(upstream, Upstream::Stdio { .. }) && !matches!(settings.auth, OutboundAuth::None) {
        bail!("`auth` is for HTTP and WebSocket servers; a command gets credentials from `env`");
    }
    Ok(Server {
        name: name.to_owned(),
        upstream,
        settings,
    })
}

impl Entry {
    fn settings(&self) -> Result<Settings> {
        let key = match self.key.as_deref() {
            None => None,
            Some("global") => Some(UpstreamKey::Global),
            Some("principal") => Some(UpstreamKey::Principal),
            Some("session") => Some(UpstreamKey::Session),
            Some(other) => bail!("unknown `key` `{other}` (global, principal or session)"),
        };
        let account = |a: &Account| -> Result<ServiceAccount> {
            let mut account = ServiceAccount::new(
                interpolate(&a.issuer)?,
                interpolate(&a.client_id)?,
                interpolate(&a.client_secret)?,
            );
            if let Some(scopes) = &a.scopes {
                account = account.scopes(scopes.clone());
            }
            Ok(account)
        };
        let auth = match &self.auth {
            None => OutboundAuth::None,
            Some(Auth::Bearer(token)) => OutboundAuth::bearer(interpolate(token)?),
            Some(Auth::ClientCredentials(a)) => OutboundAuth::ClientCredentials(account(a)?),
            Some(Auth::TokenExchange(a)) => OutboundAuth::TokenExchange(account(a)?),
        };
        Ok(Settings {
            auth,
            timeout: self.timeout.map(Duration::from_secs),
            key,
            forward_sampling: self.forward_sampling,
            forward_roots: self.forward_roots,
            tags: self.tags.clone(),
        })
    }

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
                if !self.headers.is_empty() {
                    bail!("has `headers`, which only an HTTP or WebSocket server takes");
                }
                let args = self
                    .args
                    .iter()
                    .map(|a| interpolate(a))
                    .collect::<Result<Vec<_>>>()?;
                let inherit = match self.inherit.as_deref() {
                    None | Some("safe") => Inherit::Safe,
                    Some("all") => Inherit::All,
                    Some("none") => Inherit::Nothing,
                    Some(other) => bail!("unknown `inherit` `{other}` (safe, all or none)"),
                };
                let mut upstream =
                    Upstream::stdio(interpolate(&command)?, args).inherit_env(inherit);
                for (key, value) in &self.env {
                    upstream = upstream.env(key, interpolate(value)?);
                }
                if let Some(cwd) = &self.cwd {
                    upstream = upstream.cwd(PathBuf::from(interpolate(cwd)?));
                }
                Ok(upstream)
            }
            (None, Some(url)) => {
                if !self.env.is_empty() || self.inherit.is_some() {
                    bail!("has `env` or `inherit`, which only a command server takes");
                }
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

/// `text` with its `${…}` variables replaced (see the module docs). A
/// variable that isn't set, with no fallback, is an error naming it, not an
/// empty string. Errors never quote `text`: it may hold a secret around the
/// variable.
pub fn interpolate(text: &str) -> Result<String> {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            bail!("a `${{` is never closed");
        };
        out.push_str(&variable(&after[..end])?);
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// The value of one `${…}` variable, `body` being what is between the
/// braces.
fn variable(body: &str) -> Result<String> {
    let (name, fallback) = match body.split_once(":-") {
        Some((name, fallback)) => (name, Some(fallback)),
        None => (body, None),
    };
    let found = match name {
        "" => bail!("an empty `${{}}`"),
        "workspaceFolder" => Some(
            std::env::current_dir()
                .context("the working directory, for `${workspaceFolder}`")?
                .to_string_lossy()
                .into_owned(),
        ),
        "userHome" => std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok(),
        "pathSeparator" | "/" => Some(std::path::MAIN_SEPARATOR.to_string()),
        _ if name.starts_with("input:") => bail!(
            "`${{{name}}}` asks an editor to prompt for a value; set it in the environment \
             and write `${{env:VAR}}` instead"
        ),
        _ => std::env::var(name.strip_prefix("env:").unwrap_or(name)).ok(),
    };
    match (found, fallback) {
        (Some(value), _) => Ok(value),
        (None, Some(fallback)) => Ok(fallback.to_owned()),
        (None, None) => bail!(
            "environment variable `{}` is not set",
            name.strip_prefix("env:").unwrap_or(name)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(loaded: &Loaded) -> Vec<&str> {
        loaded.servers.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn every_host_shape_parses() {
        let loaded = parse(
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
            &[],
        )
        .unwrap();
        assert_eq!(
            names(&loaded),
            ["extra", "files", "remote", "socket", "typed"]
        );
        assert!(matches!(loaded.servers[1].upstream, Upstream::Stdio { .. }));
        assert!(matches!(loaded.servers[2].upstream, Upstream::Http { .. }));
        assert!(matches!(
            loaded.servers[3].upstream,
            Upstream::WebSocket { .. }
        ));
        // VS Code's `mcp.json` and `settings.json`.
        for file in [
            r#"{"servers": {"a": {"command": "x"}}}"#,
            r#"{"mcp": {"servers": {"a": {"command": "x"}}}, "editor.fontSize": 12}"#,
        ] {
            assert_eq!(names(&parse(file, &[]).unwrap()), ["a"]);
        }
    }

    #[test]
    fn an_unusable_entry_is_skipped_unless_asked_for() {
        let file = r#"{"mcpServers": {
            "good": {"command": "x"},
            "old": {"type": "sse", "url": "https://x/sse"},
            "both": {"command": "x", "url": "https://x"},
            "empty": {},
            "typo": {"command": 7}
        }}"#;
        let loaded = parse(file, &[]).unwrap();
        assert_eq!(names(&loaded), ["good"]);
        let reasons: BTreeMap<&str, String> = loaded
            .skipped
            .iter()
            .map(|(name, e)| (name.as_str(), format!("{e:#}")))
            .collect();
        assert!(reasons["old"].contains("Streamable HTTP"), "{reasons:?}");
        assert!(reasons["both"].contains("both"), "{reasons:?}");
        assert!(reasons["empty"].contains("neither"), "{reasons:?}");
        assert!(reasons.contains_key("typo"));

        // `--only good` never looks at the others; naming a bad one fails.
        let only = parse(file, &["good".into()]).unwrap();
        assert_eq!(names(&only), ["good"]);
        assert!(only.skipped.is_empty());
        let named = format!("{:#}", parse(file, &["old".into()]).err().unwrap());
        assert!(named.contains("Streamable HTTP"), "{named}");
        assert!(parse(file, &["missing".into()]).is_err());
    }

    #[test]
    fn gateway_settings_are_read() {
        let loaded = parse(
            r#"{"mcpServers": {
                "a": {
                    "url": "https://example.com/mcp",
                    "timeout": 120, "key": "session", "forwardSampling": true,
                    "tags": ["internal"], "auth": {"bearer": "t"}
                },
                "b": {"command": "x", "inherit": "none"},
                "c": {"command": "x", "auth": {"bearer": "t"}},
                "d": {"url": "https://example.com/mcp", "key": "everyone"}
            }}"#,
            &[],
        )
        .unwrap();
        assert_eq!(names(&loaded), ["a", "b"]);
        let a = &loaded.servers[0].settings;
        assert_eq!(a.timeout, Some(Duration::from_secs(120)));
        assert_eq!(a.key, Some(UpstreamKey::Session));
        assert!(a.forward_sampling && !a.forward_roots);
        assert!(matches!(a.auth, OutboundAuth::Static(_)));
        assert!(matches!(
            loaded.servers[1].upstream,
            Upstream::Stdio {
                inherit: Inherit::Nothing,
                ..
            }
        ));
        let skipped: Vec<&str> = loaded.skipped.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(skipped, ["c", "d"]);
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
        assert_eq!(
            interpolate("${TURBOMCP_SURELY_UNSET_VAR:-fallback}").unwrap(),
            "fallback"
        );
        assert_eq!(interpolate("${PATH:-fallback}").unwrap(), path);
        assert_eq!(
            interpolate("${workspaceFolder}").unwrap(),
            std::env::current_dir().unwrap().to_string_lossy()
        );
        assert_eq!(
            interpolate("${/}").unwrap(),
            std::path::MAIN_SEPARATOR.to_string()
        );
        let missing = interpolate("${TURBOMCP_SURELY_UNSET_VAR}").unwrap_err();
        assert!(missing.to_string().contains("TURBOMCP_SURELY_UNSET_VAR"));
        assert!(interpolate("${input:token}").is_err());
        // An error never quotes the text around the variable.
        let open = interpolate("sk-secret ${OPEN").unwrap_err();
        assert!(!format!("{open:#}").contains("sk-secret"), "{open:#}");
    }
}
