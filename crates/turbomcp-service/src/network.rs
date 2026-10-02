//! What a transport knows about the connection a message rides: the
//! OpenTelemetry `network.*` attributes, and the address of the other end
//! (`client.*` on a server, `server.*` on a client).

use std::net::SocketAddr;

/// The connection a message rides, as its transport sees it.
///
/// The bundled transports attach it to every request a server receives (in
/// [`McpRequest::extensions`](crate::McpRequest)), and a client reads it from
/// its transport, so telemetry can record the MCP semantic conventions'
/// network attributes: `network.transport` `pipe` for stdio and `tcp` for
/// Streamable HTTP and WebSocket, `network.protocol.name` `http` or
/// `websocket`, and the peer's address.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct NetworkFacts {
    /// `network.transport`: `pipe` or `tcp`.
    pub transport: &'static str,
    /// `network.protocol.name`: `http` or `websocket`; `None` over a pipe.
    pub protocol_name: Option<&'static str>,
    /// `network.protocol.version`: the HTTP version a request arrived on
    /// (`1.1`, `2`), when known.
    pub protocol_version: Option<&'static str>,
    /// The other end's address: the client's IP on a server (after
    /// trusted-proxy resolution), the server's host on a client.
    pub peer_address: Option<String>,
    /// The other end's port, when [`peer_address`](Self::peer_address) is
    /// the connection's own peer rather than one a proxy reported.
    pub peer_port: Option<u16>,
}

impl NetworkFacts {
    /// A byte pipe: stdio, or any stream a [`LineTransport`](crate::io::LineTransport)
    /// carries.
    #[must_use]
    pub fn pipe() -> Self {
        Self {
            transport: "pipe",
            protocol_name: None,
            protocol_version: None,
            peer_address: None,
            peer_port: None,
        }
    }

    /// Streamable HTTP, over HTTP `version` (`1.1`, `2`) when known.
    #[must_use]
    pub fn http(version: Option<&'static str>) -> Self {
        Self {
            transport: "tcp",
            protocol_name: Some("http"),
            protocol_version: version,
            peer_address: None,
            peer_port: None,
        }
    }

    /// A WebSocket.
    #[must_use]
    pub fn websocket() -> Self {
        Self {
            transport: "tcp",
            protocol_name: Some("websocket"),
            protocol_version: None,
            peer_address: None,
            peer_port: None,
        }
    }

    /// The other end is `address`, on `port` when that is known.
    #[must_use]
    pub fn with_peer(mut self, address: impl Into<String>, port: Option<u16>) -> Self {
        self.peer_address = Some(address.into());
        self.peer_port = port;
        self
    }

    /// The other end is the socket peer `addr`.
    #[must_use]
    pub fn with_socket_peer(self, addr: SocketAddr) -> Self {
        self.with_peer(addr.ip().to_string(), Some(addr.port()))
    }
}
