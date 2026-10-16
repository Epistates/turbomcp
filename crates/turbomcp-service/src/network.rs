//! What a transport knows about the connection a message rides: the
//! OpenTelemetry `network.*` attributes, and the address of the other end
//! (`client.*` on a server, `server.*` on a client).

use std::net::{IpAddr, Ipv6Addr, SocketAddr};

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

    /// What a per-client limit counts this peer against: see [`client_key`].
    #[must_use]
    pub fn client_key(&self) -> Option<String> {
        self.peer_address.as_deref().map(client_key)
    }
}

/// What a per-client limit counts a client `address` against: an IPv4
/// address as itself, an IPv4-mapped IPv6 one as the IPv4 client it is, and
/// any other IPv6 address by its `/64`, the block one subscriber is
/// typically handed (keying on the full address would give each client 2^64
/// identities). Anything that isn't an IP address is its own key.
#[must_use]
pub fn client_key(address: &str) -> String {
    match address.parse::<IpAddr>() {
        Ok(IpAddr::V6(v6)) => match v6.to_ipv4_mapped() {
            Some(v4) => v4.to_string(),
            None => {
                let network = u128::from(v6) & !u128::from(u64::MAX);
                format!("{}/64", Ipv6Addr::from(network))
            }
        },
        Ok(ip) => ip.to_string(),
        Err(_) => address.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::client_key;

    #[test]
    fn an_ipv6_client_is_keyed_by_its_64() {
        assert_eq!(client_key("2001:db8:1:2:aaaa::1"), "2001:db8:1:2::/64");
        assert_eq!(client_key("2001:db8:1:2:bbbb::9"), "2001:db8:1:2::/64");
        assert_ne!(client_key("2001:db8:1:3::1"), client_key("2001:db8:1:2::1"));
        assert_eq!(client_key("::ffff:203.0.113.7"), "203.0.113.7");
        assert_eq!(client_key("203.0.113.7"), "203.0.113.7");
        assert_eq!(client_key("not-an-ip"), "not-an-ip");
    }
}
