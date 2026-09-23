//! Responding to an MCP server's authorization challenges.
//!
//! A server that requires authorization answers a request without a usable
//! token `401` — or `403` for a missing scope — with a `WWW-Authenticate`
//! challenge. MCP's authorization spec requires a client to parse it: its
//! `resource_metadata` names the server's Protected Resource Metadata, the
//! starting point for discovering an authorization server and obtaining a
//! token.
//!
//! Obtaining one is application work — it usually involves the user in a
//! browser — so the transport hands the parsed [`AuthChallenge`] to an
//! [`AuthProvider`] and, if the provider now has a token, retries the request
//! once. `turbomcp_auth::discovery::ClientDiscovery` takes a provider from the
//! challenge to the authorization server's endpoints.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

/// Future returned by [`AuthProvider`] methods.
pub type AuthFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Supplies the bearer token the transport presents, and replaces it when the
/// server refuses it.
pub trait AuthProvider: Send + Sync + fmt::Debug {
    /// The token to present, if there is one yet.
    fn token(&self) -> AuthFuture<'_, Option<String>>;

    /// The server refused the request with `challenge`.
    ///
    /// Return `true` once a token that should satisfy it is available — the
    /// request is then retried once with it — or `false` to fail the request.
    fn on_challenge<'a>(&'a self, challenge: &'a AuthChallenge) -> AuthFuture<'a, bool>;
}

/// A server's `WWW-Authenticate: Bearer …` challenge (RFC 6750 §3, with
/// RFC 9728's `resource_metadata`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthChallenge {
    /// `401` for a missing or invalid token, `403` for insufficient scope.
    pub status: u16,
    /// URL of the server's Protected Resource Metadata.
    pub resource_metadata: Option<String>,
    /// Scopes the server asks for.
    pub scope: Option<String>,
    /// `invalid_token`, `insufficient_scope`, … — absent when the request
    /// carried no credentials.
    pub error: Option<String>,
    /// Human-readable detail.
    pub error_description: Option<String>,
}

impl AuthChallenge {
    /// Parse the `Bearer` challenge out of a `WWW-Authenticate` value.
    ///
    /// Parameters of any other scheme are ignored; a value with no `Bearer`
    /// challenge yields one with only `status` set.
    pub fn parse(status: u16, www_authenticate: &str) -> Self {
        let mut challenge = Self {
            status,
            resource_metadata: None,
            scope: None,
            error: None,
            error_description: None,
        };

        let Some(start) = find_bearer(www_authenticate) else {
            return challenge;
        };
        for (name, value) in auth_params(&www_authenticate[start + "bearer".len()..]) {
            match name.to_ascii_lowercase().as_str() {
                "resource_metadata" => challenge.resource_metadata = Some(value),
                "scope" => challenge.scope = Some(value),
                "error" => challenge.error = Some(value),
                "error_description" => challenge.error_description = Some(value),
                _ => {}
            }
        }
        challenge
    }
}

impl fmt::Display for AuthChallenge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HTTP {}", self.status)?;
        if let Some(error) = &self.error {
            write!(f, ": {error}")?;
        }
        if let Some(description) = &self.error_description {
            write!(f, " ({description})")?;
        }
        if let Some(metadata) = &self.resource_metadata {
            write!(f, "; resource metadata at {metadata}")?;
        }
        Ok(())
    }
}

/// Byte offset of the `Bearer` scheme token.
fn find_bearer(value: &str) -> Option<usize> {
    let lower = value.to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = lower[from..].find("bearer") {
        let at = from + at;
        let starts_token = at == 0 || matches!(lower.as_bytes()[at - 1], b' ' | b',');
        let ends_token = lower
            .as_bytes()
            .get(at + "bearer".len())
            .is_none_or(|b| *b == b' ');
        if starts_token && ends_token {
            return Some(at);
        }
        from = at + 1;
    }
    None
}

/// The `name=value` / `name="quoted value"` pairs after a scheme, up to the
/// next challenge.
fn auth_params(input: &str) -> Vec<(String, String)> {
    let mut params = Vec::new();
    let mut rest = input.trim_start();
    loop {
        let Some(eq) = rest.find('=') else { break };
        let name = rest[..eq].trim().trim_start_matches(',').trim();
        // A bare token here is the next challenge's scheme.
        if name.is_empty() || name.contains(' ') {
            break;
        }
        rest = &rest[eq + 1..];

        let value;
        if let Some(quoted) = rest.strip_prefix('"') {
            let mut unescaped = String::new();
            let mut chars = quoted.char_indices();
            let mut end = quoted.len();
            while let Some((i, c)) = chars.next() {
                match c {
                    '\\' => {
                        if let Some((_, escaped)) = chars.next() {
                            unescaped.push(escaped);
                        }
                    }
                    '"' => {
                        end = i + 1;
                        break;
                    }
                    c => unescaped.push(c),
                }
            }
            value = unescaped;
            rest = &quoted[end.min(quoted.len())..];
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            value = rest[..end].trim().to_owned();
            rest = &rest[end..];
        }
        params.push((name.to_owned(), value));
        rest = rest.trim_start().trim_start_matches(',').trim_start();
    }
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_challenge_names_the_metadata() {
        let challenge = AuthChallenge::parse(
            401,
            r#"Bearer resource_metadata="https://mcp.example.com/.well-known/oauth-protected-resource/mcp", scope="files:read files:write""#,
        );
        assert_eq!(
            challenge.resource_metadata.as_deref(),
            Some("https://mcp.example.com/.well-known/oauth-protected-resource/mcp")
        );
        assert_eq!(challenge.scope.as_deref(), Some("files:read files:write"));
        assert_eq!(challenge.error, None);
    }

    #[test]
    fn errors_and_escapes_are_read() {
        let challenge = AuthChallenge::parse(
            403,
            r#"Bearer error="insufficient_scope", scope="files:write", error_description="needs \"write\"", resource_metadata="https://x/m""#,
        );
        assert_eq!(challenge.error.as_deref(), Some("insufficient_scope"));
        assert_eq!(
            challenge.error_description.as_deref(),
            Some(r#"needs "write""#)
        );
        assert_eq!(challenge.resource_metadata.as_deref(), Some("https://x/m"));
    }

    #[test]
    fn only_the_bearer_challenge_is_read() {
        let challenge = AuthChallenge::parse(
            401,
            r#"Basic realm="other", Bearer error=invalid_token, resource_metadata="https://x/m""#,
        );
        assert_eq!(challenge.error.as_deref(), Some("invalid_token"));
        assert_eq!(challenge.resource_metadata.as_deref(), Some("https://x/m"));

        let none = AuthChallenge::parse(401, r#"Basic realm="x""#);
        assert_eq!(none.resource_metadata, None);
    }
}
