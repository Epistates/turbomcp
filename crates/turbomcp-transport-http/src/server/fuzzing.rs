//! Entry points for `fuzz/` into the header parsers a client reaches.
//! Compiled only under `cfg(fuzzing)`, which `cargo fuzz` sets; not API.

use std::net::SocketAddr;

use axum::http::{HeaderMap, HeaderName, HeaderValue, Request, Uri, header};
use ipnet::IpNet;
use turbomcp_core::JsonRpcMessage;

use super::config::{HostPolicy, OriginPolicy};
use super::guards::{self, PeerIp};

fn headers(pairs: &[(String, Vec<u8>)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_bytes(value),
        ) {
            map.append(name, value);
        }
    }
    map
}

/// Every check a request's headers go through before dispatch, over
/// arbitrary header lines, a body frame and an authority.
pub fn request_headers(pairs: &[(String, Vec<u8>)], body: &[u8], authority: &str) {
    let map = headers(pairs);
    let _ = guards::accepts(&map, &mime::APPLICATION_JSON);
    let _ = guards::accepts(&map, &mime::TEXT_EVENT_STREAM);
    let _ = guards::is_json(&map);
    for origin in map.get_all(header::ORIGIN) {
        if let Ok(origin) = origin.to_str() {
            let _ = guards::normalized_origin(origin);
        }
    }
    let origins = OriginPolicy::Allowlist(vec!["https://app.example".into()]);
    let _ = guards::check_origin(&origins, &map);
    if let Ok(uri) = format!("http://{authority}/mcp").parse::<Uri>() {
        let hosts = HostPolicy::Allowlist(vec!["localhost".into(), "api.example:8443".into()]);
        let _ = guards::check_host(&hosts, &map, &uri);
    }
    if let Some(id) = map.get("last-event-id").and_then(|v| v.to_str().ok()) {
        let _ = super::resume::parse_event_id(id);
    }
    if let Ok(msg) = serde_json::from_slice::<JsonRpcMessage>(body) {
        let _ = super::validate::validate_request_headers(&msg, &map);
    }
}

/// The client IP a rate limiter keys on, from the socket and every
/// `X-Forwarded-For` line, behind trusted proxy ranges.
pub fn client_ip(socket: SocketAddr, forwarded: &[Vec<u8>], trusted: &[&str]) {
    let mut request = Request::builder();
    for line in forwarded {
        if let Ok(value) = HeaderValue::from_bytes(line) {
            request = request.header("x-forwarded-for", value);
        }
    }
    let Ok(request) = request.body(()) else {
        return;
    };
    let (mut parts, ()) = request.into_parts();
    parts.extensions.insert(axum::extract::ConnectInfo(socket));
    let trusted: Vec<IpNet> = trusted.iter().filter_map(|n| n.parse().ok()).collect();
    let peer = PeerIp::from_parts(&parts);
    if let Some(ip) = peer.client_ip(&trusted) {
        // The answer is the socket or an address someone in the chain named.
        let named = forwarded
            .iter()
            .filter_map(|l| std::str::from_utf8(l).ok())
            .flat_map(|l| l.split(','))
            .any(|hop| hop.trim().parse() == Ok(ip));
        assert!(ip == socket.ip() || named, "{ip} came from nowhere");
    }
}
