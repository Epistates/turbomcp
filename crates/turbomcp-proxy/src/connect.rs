//! Opening the connection to an upstream.

use std::process::Stdio;
#[cfg(feature = "http")]
use std::sync::Arc;
use std::time::Duration;

use tokio::io::BufReader;
use turbomcp_client::{Client, ClientBuilder};
use turbomcp_core::codec::DefaultCodec;
use turbomcp_service::io::LineTransport;

#[cfg(feature = "http")]
use crate::OutboundAuth;
use crate::process::ChildProcess;
use crate::{Inherit, ProxyError, Upstream};

#[cfg(feature = "http")]
type Bearer = Arc<dyn turbomcp_transport_http::BearerSource>;

/// What an upstream connection presents.
#[cfg(feature = "http")]
pub(crate) enum Credential {
    None,
    /// One credential for every connection: the proxy's own.
    Shared(Bearer),
    /// A token exchanged per caller; the gateway's own (client credentials)
    /// for a connection acting for no caller.
    #[cfg(feature = "oauth")]
    Exchange {
        engine: turbomcp_auth::client::MachineAuthorization,
        scopes: Option<Vec<String>>,
    },
}

#[cfg(feature = "http")]
impl Credential {
    /// What `auth` presents to `upstream`, under `network` for any
    /// authorization server it has to reach.
    pub(crate) fn of(
        upstream: &Upstream,
        auth: &OutboundAuth,
        network: Option<&turbomcp_auth::NetworkPolicy>,
    ) -> Result<Self, ProxyError> {
        #[cfg(feature = "oauth")]
        let engine = |account: &crate::ServiceAccount| {
            let resource = upstream.resource().ok_or_else(|| {
                ProxyError::Config("OAuth credentials are for HTTP and WebSocket upstreams".into())
            })?;
            let engine =
                turbomcp_auth::client::MachineAuthorization::new(resource, account.credentials());
            match network {
                Some(policy) => engine
                    .with_network_policy(policy.clone())
                    .map_err(|e| ProxyError::Config(e.to_string())),
                None => Ok(engine),
            }
        };
        let _ = (upstream, network);
        Ok(match auth {
            OutboundAuth::None => Self::None,
            OutboundAuth::Static(token) => Self::Shared(Arc::new(token.clone())),
            #[cfg(feature = "oauth")]
            OutboundAuth::ClientCredentials(account) => {
                use turbomcp_transport_http::oauth::MachineSession;
                let mut session = MachineSession::client_credentials(engine(account)?);
                if let Some(scopes) = &account.scopes {
                    session = session.with_scopes(scopes.clone());
                }
                Self::Shared(Arc::new(session))
            }
            #[cfg(feature = "oauth")]
            OutboundAuth::TokenExchange(account) => Self::Exchange {
                engine: engine(account)?,
                scopes: account.scopes.clone(),
            },
        })
    }

    /// The bearer source of one connection, acting for `subject`'s caller.
    fn for_link(&self, subject: Option<&crate::link::Subject>) -> Option<Bearer> {
        let _ = subject;
        match self {
            Self::None => None,
            Self::Shared(bearer) => Some(Arc::clone(bearer)),
            #[cfg(feature = "oauth")]
            Self::Exchange { engine, scopes } => {
                use turbomcp_transport_http::oauth::MachineSession;
                let mut session = match subject {
                    Some(slot) => MachineSession::token_exchange(
                        engine.clone(),
                        Arc::new(SlotSource(Arc::clone(slot))),
                    ),
                    None => MachineSession::client_credentials(engine.clone()),
                };
                if let Some(scopes) = scopes {
                    session = session.with_scopes(scopes.clone());
                }
                Some(Arc::new(session))
            }
        }
    }
}

/// `headers` as an HTTP header map, refusing the `Mcp-*` headers the
/// protocol owns.
#[cfg(feature = "http")]
fn header_map(
    headers: &std::collections::BTreeMap<String, String>,
) -> Result<http::HeaderMap, String> {
    let mut map = http::HeaderMap::new();
    for (name, value) in headers {
        let name = http::HeaderName::from_bytes(name.as_bytes())
            .map_err(|e| format!("header `{name}`: {e}"))?;
        if name.as_str().starts_with("mcp-") {
            return Err(format!("header `{name}` is the protocol's own"));
        }
        let mut value =
            http::HeaderValue::from_str(value).map_err(|e| format!("header `{name}`: {e}"))?;
        value.set_sensitive(true);
        map.insert(name, value);
    }
    Ok(map)
}

/// A connection's caller's latest token, for exchange.
#[cfg(feature = "oauth")]
struct SlotSource(crate::link::Subject);

#[cfg(feature = "oauth")]
#[async_trait::async_trait]
impl turbomcp_transport_http::oauth::SubjectSource for SlotSource {
    async fn subject_token(&self) -> Option<zeroize::Zeroizing<String>> {
        let slot = self.0.lock().ok()?;
        slot.as_ref()
            .map(|token| zeroize::Zeroizing::new(token.secret().to_owned()))
    }
}

/// How a connection to an upstream is made, beside where it goes.
pub(crate) struct ConnectOptions {
    pub(crate) grace: Duration,
    /// The credential presented to an HTTP or WebSocket upstream.
    #[cfg(feature = "http")]
    pub(crate) credential: Credential,
    /// Where an HTTP or WebSocket upstream may be (`None`: anywhere).
    #[cfg(feature = "http")]
    pub(crate) network: Option<turbomcp_auth::NetworkPolicy>,
}

/// Connect `client` to `upstream`, as `options` say.
pub(crate) async fn upstream(
    upstream: &Upstream,
    options: &ConnectOptions,
    client: ClientBuilder,
    subject: Option<&crate::link::Subject>,
) -> Result<(Client, Option<ChildProcess>), ProxyError> {
    #[cfg(feature = "http")]
    let bearer = options.credential.for_link(subject);
    #[cfg(not(feature = "http"))]
    let _ = subject;
    let grace = options.grace;
    let label = upstream.label();
    let connect_error = |source| ProxyError::Connect {
        upstream: label.clone(),
        source: Box::new(source),
    };
    match upstream {
        Upstream::Stdio {
            command,
            args,
            env,
            inherit,
            cwd,
        } => {
            // A child's credentials are its environment, which the operator
            // configured; there is no request to put a bearer on.
            let mut cmd = tokio::process::Command::new(command);
            match inherit {
                Inherit::All => {}
                Inherit::Nothing => {
                    cmd.env_clear();
                }
                Inherit::Safe => {
                    cmd.env_clear();
                    for name in Inherit::SAFE {
                        // An exported bash function (`() { …`) is code, not
                        // configuration.
                        if let Some(value) = std::env::var_os(name)
                            && !value.to_string_lossy().starts_with("()")
                        {
                            cmd.env(name, value);
                        }
                    }
                }
            }
            cmd.args(args)
                .envs(env)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            if let Some(dir) = cwd {
                cmd.current_dir(dir);
            }
            // Its own process group, so shutdown reaches whatever it starts.
            #[cfg(unix)]
            cmd.process_group(0);
            let mut child = cmd.spawn().map_err(|source| ProxyError::Spawn {
                command: command.clone(),
                source,
            })?;
            let (Some(stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
                return Err(ProxyError::Spawn {
                    command: command.clone(),
                    source: std::io::Error::other("stdio pipes were not captured"),
                });
            };
            let process = ChildProcess::new(child, label.clone(), grace);
            let transport =
                LineTransport::new(BufReader::new(stdout), stdin, DefaultCodec::default());
            // On failure `process` drops, which kills the group.
            let client = client.connect(transport).await.map_err(connect_error)?;
            Ok((client, Some(process)))
        }
        #[cfg(feature = "http")]
        Upstream::Http { url, headers } => {
            let config = |e: &dyn std::fmt::Display| ProxyError::Config(format!("{url}: {e}"));
            let mut transport = turbomcp_transport_http::HttpClientTransport::new(url.clone())
                .map_err(|e| config(&e))?;
            let headers = header_map(headers).map_err(|e| config(&e))?;
            if let Some(policy) = &options.network {
                policy.validate_url(url).map_err(|e| config(&e))?;
                let http = policy
                    .client_builder()
                    .default_headers(headers)
                    .build()
                    .map_err(|e| config(&e))?;
                transport = transport.with_client(http);
            } else if !headers.is_empty() {
                transport = transport.with_headers(headers).map_err(|e| config(&e))?;
            }
            if let Some(bearer) = &bearer {
                transport = transport.with_bearer_source(Arc::clone(bearer));
            }
            let client = client.connect(transport).await.map_err(connect_error)?;
            Ok((client, None))
        }
        #[cfg(feature = "websocket")]
        Upstream::WebSocket { url, headers } => {
            use turbomcp_transport_http::WebSocketClientTransport;
            let mut request = http::Request::builder().uri(url.as_str());
            for (name, value) in headers {
                request = request.header(name.as_str(), value.as_str());
            }
            // One token per upgrade: a connection outliving it is closed by
            // the server, and the next one gets a fresh token.
            if let Some(bearer) = &bearer
                && let Some(token) = bearer.bearer().await
            {
                request = request.header("authorization", format!("Bearer {}", token.as_str()));
            }
            let request = request
                .body(())
                .map_err(|e| ProxyError::Config(format!("{url}: {e}")))?;
            let unreachable = |e: &dyn std::fmt::Display| ProxyError::Dial {
                upstream: label.clone(),
                source: std::io::Error::other(e.to_string()),
            };
            let transport =
                match &options.network {
                    None => WebSocketClientTransport::connect(request)
                        .await
                        .map_err(|e| unreachable(&e))?,
                    Some(policy) => {
                        // Checked as its HTTP equivalent, then connected to an
                        // address the policy resolved, so it can't be rebound.
                        let parsed: http::Uri = url
                            .parse()
                            .map_err(|e| ProxyError::Config(format!("{url}: {e}")))?;
                        let as_http = url.replacen("ws", "http", 1);
                        policy
                            .validate_url(&as_http)
                            .map_err(|e| ProxyError::Config(format!("{url}: {e}")))?;
                        let host = parsed
                            .host()
                            .ok_or_else(|| ProxyError::Config(format!("{url}: no host")))?;
                        let port = parsed.port_u16().unwrap_or(match parsed.scheme_str() {
                            Some("wss") => 443,
                            _ => 80,
                        });
                        let addresses = policy.resolve(host, port).await.map_err(|source| {
                            ProxyError::Dial {
                                upstream: label.clone(),
                                source,
                            }
                        })?;
                        let stream = tokio::net::TcpStream::connect(addresses.as_slice())
                            .await
                            .map_err(|source| ProxyError::Dial {
                                upstream: label.clone(),
                                source,
                            })?;
                        WebSocketClientTransport::connect_over(request, stream)
                            .await
                            .map_err(|e| unreachable(&e))?
                    }
                };
            let client = client.connect(transport).await.map_err(connect_error)?;
            Ok((client, None))
        }
    }
}
