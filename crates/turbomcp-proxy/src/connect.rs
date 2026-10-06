//! Opening the connection to an upstream.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::BufReader;
use turbomcp_client::{Client, ClientBuilder};
use turbomcp_core::codec::DefaultCodec;
use turbomcp_service::io::LineTransport;

use crate::process::ChildProcess;
use crate::{Inherit, OutboundAuth, ProxyError, Upstream};

/// How a connection to an upstream is made, beside where it goes.
pub(crate) struct ConnectOptions {
    pub(crate) auth: OutboundAuth,
    pub(crate) grace: Duration,
    /// Where an HTTP or WebSocket upstream may be (`None`: anywhere).
    #[cfg(feature = "http")]
    pub(crate) network: Option<turbomcp_auth::NetworkPolicy>,
}

/// Connect `client` to `upstream`, as `options` say.
pub(crate) async fn upstream(
    upstream: &Upstream,
    options: &ConnectOptions,
    client: ClientBuilder,
) -> Result<(Client, Option<ChildProcess>), ProxyError> {
    let (auth, grace) = (&options.auth, options.grace);
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
            let _ = auth;
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
        Upstream::Http { url } => {
            let mut transport = turbomcp_transport_http::HttpClientTransport::new(url.clone())
                .map_err(|e| ProxyError::Config(format!("{url}: {e}")))?;
            if let Some(policy) = &options.network {
                policy
                    .validate_url(url)
                    .map_err(|e| ProxyError::Config(format!("{url}: {e}")))?;
                let http = policy
                    .client_builder()
                    .build()
                    .map_err(|e| ProxyError::Config(format!("{url}: {e}")))?;
                transport = transport.with_client(http);
            }
            if let OutboundAuth::Static(token) = auth {
                transport = transport.with_bearer(token.as_str());
            }
            let client = client.connect(transport).await.map_err(connect_error)?;
            Ok((client, None))
        }
        #[cfg(feature = "websocket")]
        Upstream::WebSocket { url } => {
            use turbomcp_transport_http::WebSocketClientTransport;
            let mut request = http::Request::builder().uri(url.as_str());
            if let OutboundAuth::Static(token) = auth {
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
