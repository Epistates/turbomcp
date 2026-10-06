//! The WebSocket client transport (features `client` + `websocket`).

use std::time::Duration;

use bytes::Bytes;
use futures::stream::SplitStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::http::HeaderValue;
use tokio_tungstenite::tungstenite::http::header::{
    CONNECTION, HOST, SEC_WEBSOCKET_KEY, SEC_WEBSOCKET_PROTOCOL, SEC_WEBSOCKET_VERSION, UPGRADE,
};
use tokio_tungstenite::tungstenite::protocol::CloseFrame;
use tokio_tungstenite::tungstenite::{Message, Utf8Bytes};
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};
use turbomcp_core::{InvalidFrame, JsonRpcMessage};
use turbomcp_service::Transport;

use crate::ws_link::{Frame, Keepalive, Link, Read, WsError};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

impl Frame for Message {
    fn text(frame: Bytes) -> Result<Self, WsError> {
        Utf8Bytes::try_from(frame)
            .map(Message::Text)
            .map_err(|_| WsError::Utf8)
    }

    fn ping() -> Self {
        Message::Ping(Bytes::new())
    }

    fn close(code: u16, reason: &'static str) -> Self {
        Message::Close(Some(CloseFrame {
            code: code.into(),
            reason: Utf8Bytes::from_static(reason),
        }))
    }

    fn read(&self) -> Read<'_> {
        match self {
            Message::Text(text) => Read::Data(text.as_bytes()),
            Message::Binary(bytes) => Read::Data(bytes),
            Message::Close(_) => Read::Close,
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => Read::Skip,
        }
    }
}

/// A WebSocket connection to an MCP server, for `turbomcp-client`'s `Client`.
///
/// Asks for the `mcp` subprotocol, which the official SDKs' servers (and this
/// crate's) select.
pub struct WebSocketClientTransport {
    link: Link<Message, SplitStream<Socket>>,
}

impl core::fmt::Debug for WebSocketClientTransport {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WebSocketClientTransport")
            .finish_non_exhaustive()
    }
}

impl WebSocketClientTransport {
    /// Connect with `request`: a `ws://` or `wss://` URL, or an
    /// `http::Request` carrying headers of your own (an `Authorization`
    /// bearer, a tenant header).
    ///
    /// # Errors
    /// [`WsError::Socket`] if the connection or the handshake fails.
    pub async fn connect(request: impl IntoClientRequest + Unpin) -> Result<Self, WsError> {
        let (request, network) = prepare(request)?;
        let (socket, _response) = tokio_tungstenite::connect_async(request)
            .await
            .map_err(|e| WsError::Socket(Box::new(e)))?;
        Ok(Self {
            link: Link::new(socket, None).with_network(network),
        })
    }

    /// Connect with `request` over `stream`, a TCP connection to the server
    /// you opened yourself (TLS is negotiated on it for `wss://`): for an
    /// address chosen under a policy of your own, such as one a network
    /// policy resolved and checked, so the name can't be rebound between the
    /// check and the connect.
    ///
    /// # Errors
    /// [`WsError::Socket`] if the handshake fails.
    pub async fn connect_over(
        request: impl IntoClientRequest + Unpin,
        stream: tokio::net::TcpStream,
    ) -> Result<Self, WsError> {
        let (request, network) = prepare(request)?;
        let (socket, _response) = tokio_tungstenite::client_async_tls(request, stream)
            .await
            .map_err(|e| WsError::Socket(Box::new(e)))?;
        Ok(Self {
            link: Link::new(socket, None).with_network(network),
        })
    }

    /// Ping the server after `interval` without inbound traffic, and give up
    /// after `max_idle_pings` unanswered pings (`None` pings forever). Off by
    /// default.
    #[must_use]
    pub fn with_keepalive(mut self, interval: Duration, max_idle_pings: Option<u32>) -> Self {
        self.link.set_keepalive(Some(Keepalive {
            interval,
            max_idle_pings,
        }));
        self
    }
}

/// The request with the `mcp` subprotocol asked for, and what it says about
/// the peer.
fn prepare(
    request: impl IntoClientRequest + Unpin,
) -> Result<
    (
        tokio_tungstenite::tungstenite::handshake::client::Request,
        turbomcp_service::NetworkFacts,
    ),
    WsError,
> {
    let mut request = request
        .into_client_request()
        .map_err(|e| WsError::Socket(Box::new(e)))?;
    // A URL gets the handshake headers from tungstenite; an `http::Request`
    // of the caller's own is passed through as built, so fill in what it
    // lacks rather than fail the upgrade.
    let host = request.uri().authority().map(|authority| {
        authority
            .as_str()
            .rsplit('@')
            .next()
            .unwrap_or_default()
            .to_owned()
    });
    let headers = request.headers_mut();
    if let Some(host) = host.and_then(|h| HeaderValue::from_str(&h).ok()) {
        headers.entry(HOST).or_insert(host);
    }
    headers
        .entry(CONNECTION)
        .or_insert(HeaderValue::from_static("Upgrade"));
    headers
        .entry(UPGRADE)
        .or_insert(HeaderValue::from_static("websocket"));
    headers
        .entry(SEC_WEBSOCKET_VERSION)
        .or_insert(HeaderValue::from_static("13"));
    if !headers.contains_key(SEC_WEBSOCKET_KEY) {
        let key = tokio_tungstenite::tungstenite::handshake::client::generate_key();
        if let Ok(key) = HeaderValue::from_str(&key) {
            headers.insert(SEC_WEBSOCKET_KEY, key);
        }
    }
    headers
        .entry(SEC_WEBSOCKET_PROTOCOL)
        .or_insert(HeaderValue::from_static("mcp"));
    let uri = request.uri();
    let mut network = turbomcp_service::NetworkFacts::websocket();
    if let Some(host) = uri.host() {
        let port = uri.port_u16().or(match uri.scheme_str() {
            Some("wss") => Some(443),
            Some("ws") => Some(80),
            _ => None,
        });
        network = network.with_peer(host, port);
    }
    Ok((request, network))
}

/// Connect to a WebSocket MCP server at `url` (`ws://…` or `wss://…`).
///
/// # Errors
/// [`WsError::Socket`] if the connection or the handshake fails.
pub async fn connect_websocket(url: &str) -> Result<WebSocketClientTransport, WsError> {
    WebSocketClientTransport::connect(url).await
}

impl Transport for WebSocketClientTransport {
    type Error = WsError;

    fn network(&self) -> Option<turbomcp_service::NetworkFacts> {
        self.link.network()
    }

    fn invalid_frame(error: WsError) -> Result<InvalidFrame, WsError> {
        <Link<Message, SplitStream<Socket>> as Transport>::invalid_frame(error)
    }

    async fn send(&mut self, msg: JsonRpcMessage) -> Result<(), WsError> {
        self.link.send(msg).await
    }

    async fn recv(&mut self) -> Result<Option<JsonRpcMessage>, WsError> {
        self.link.recv().await
    }

    async fn close(self) -> Result<(), WsError> {
        self.link.close().await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio_tungstenite::WebSocketStream;
    use tokio_tungstenite::tungstenite::protocol::Role;
    use turbomcp_core::JsonRpcNotification;
    use turbomcp_service::Transport;

    use crate::ws_link::{Keepalive, Link};

    /// Steady outbound traffic must not keep a dead peer alive. The drivers
    /// drop `recv` every time a write wins the race, and the keepalive used to
    /// keep its count in that future, so writing every 20ms to a peer that
    /// never answered meant it was never probed, let alone reaped.
    #[tokio::test]
    async fn outbound_traffic_does_not_reset_the_keepalive() {
        let (ours, theirs) = tokio::io::duplex(1 << 20);
        // The peer is never read, so it never answers a ping.
        let _peer = WebSocketStream::from_raw_socket(theirs, Role::Server, None).await;
        let socket = WebSocketStream::from_raw_socket(ours, Role::Client, None).await;
        let mut link = Link::new(
            socket,
            Some(Keepalive {
                interval: Duration::from_millis(50),
                max_idle_pings: Some(2),
            }),
        );

        let reaped = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                tokio::select! {
                    received = link.recv() => break received,
                    () = tokio::time::sleep(Duration::from_millis(20)) => {
                        let note = JsonRpcNotification::new("notifications/progress", None);
                        link.send(note.into()).await.expect("write");
                    }
                }
            }
        })
        .await
        .expect("the silent peer is reaped despite the outbound traffic");
        assert!(reaped.expect("clean end").is_none());
    }
}
