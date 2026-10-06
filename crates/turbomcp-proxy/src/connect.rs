//! Opening the connection to an upstream.

use std::process::Stdio;
use std::time::Duration;

use tokio::io::BufReader;
use turbomcp_client::{Client, ClientBuilder};
use turbomcp_core::codec::DefaultCodec;
use turbomcp_service::io::LineTransport;

use crate::process::ChildProcess;
use crate::{OutboundAuth, ProxyError, Upstream};

/// Connect `client` to `upstream`, authenticated by `auth`.
pub(crate) async fn upstream(
    upstream: &Upstream,
    auth: &OutboundAuth,
    client: ClientBuilder,
    grace: Duration,
) -> Result<(Client, Option<ChildProcess>), ProxyError> {
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
            cwd,
        } => {
            // A child's credentials are its environment, which the operator
            // configured; there is no request to put a bearer on.
            let _ = auth;
            let mut cmd = tokio::process::Command::new(command);
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
            let transport = WebSocketClientTransport::connect(request)
                .await
                .map_err(|e| ProxyError::Config(format!("{url}: {e}")))?;
            let client = client.connect(transport).await.map_err(connect_error)?;
            Ok((client, None))
        }
    }
}
