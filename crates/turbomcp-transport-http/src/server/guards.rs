//! The checks every request passes before dispatch: Origin, Host, `Accept`,
//! authentication, and rate limits.

use std::convert::Infallible;
use std::net::{IpAddr, SocketAddr};

use axum::extract::{ConnectInfo, FromRequestParts};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use ipnet::IpNet;
use turbomcp_core::Identity;
use turbomcp_service::{AuthDecision, RateKey};

use super::HttpState;
use super::config::{HostPolicy, OriginPolicy};
use super::reject::{challenge_response, too_many_requests};

// ---- auth --------------------------------------------------------------------

/// A caller the authenticator let in.
pub(super) struct Authenticated {
    /// The rate-limit identity (issuer + subject), when the principal names one.
    pub(super) subject: Option<String>,
    /// Who it is, attached to the request for the dispatcher.
    pub(super) identity: Identity,
}

/// Run the configured authenticator. `Err` carries the challenge response
/// (401/403 + `WWW-Authenticate`); `Ok(None)` is an open endpoint (no
/// authenticator configured, so anonymous).
///
/// The rejection is boxed: an axum `Response` is 128 bytes, and this is
/// awaited on the request path of three handlers, so the unboxed `Result`
/// would widen each of their futures for a branch that only runs when auth
/// fails. The allocation lands on the failure path alone.
pub(super) async fn enforce_auth<S>(
    state: &HttpState<S>,
    headers: &HeaderMap,
) -> Result<Option<Authenticated>, Box<Response>> {
    let Some(authenticator) = state.authenticator.as_ref() else {
        return Ok(None);
    };
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());
    match authenticator.authenticate(authorization).await {
        AuthDecision::Allow(identity) => Ok(Some(Authenticated {
            subject: identity.principal_key(),
            identity,
        })),
        AuthDecision::Challenge {
            status,
            www_authenticate,
        } => Err(Box::new(challenge_response(status, &www_authenticate))),
    }
}

// ---- rate limiting -----------------------------------------------------------

/// The request's peer IP, if axum captured one. An infallible extractor: a
/// real socket carries `ConnectInfo<SocketAddr>` in the request extensions
/// (set by `into_make_service_with_connect_info`); oneshot/test harnesses and
/// mounts without connect info simply have none. `Option<ConnectInfo<_>>` can't
/// be used directly — axum 0.8's `Option` extractor needs
/// `OptionalFromRequestParts`, which `ConnectInfo` doesn't implement.
pub(super) struct PeerIp {
    socket: Option<IpAddr>,
    forwarded: Option<String>,
}

impl PeerIp {
    /// The effective client IP for rate limiting. If the direct socket peer is a
    /// trusted proxy, walk `X-Forwarded-For` from the right to the first hop that
    /// isn't itself trusted; otherwise use the socket peer as-is.
    pub(super) fn client_ip(&self, trusted: &[IpNet]) -> Option<IpAddr> {
        let socket = self.socket?;
        let is_trusted = |ip: &IpAddr| trusted.iter().any(|net| net.contains(ip));
        if !is_trusted(&socket) {
            return Some(socket);
        }
        let mut candidate = socket;
        for raw in self.forwarded.as_deref().unwrap_or("").rsplit(',') {
            // Never skip an unknown hop to trust an address farther left.
            let Ok(hop) = raw.trim().parse::<IpAddr>() else {
                return Some(socket);
            };
            candidate = hop;
            if !is_trusted(&hop) {
                return Some(hop);
            }
        }
        Some(candidate)
    }

    /// Read the socket peer and every `X-Forwarded-For` line, joined in
    /// order (RFC 9110 §5.3: several field lines are one comma-separated
    /// list). A line that isn't text makes the whole chain unusable, which
    /// the walk above treats as "stop at the socket".
    pub(super) fn from_parts(parts: &Parts) -> Self {
        let lines: Option<Vec<&str>> = parts
            .headers
            .get_all("x-forwarded-for")
            .iter()
            .map(|v| v.to_str().ok())
            .collect();
        PeerIp {
            socket: parts
                .extensions
                .get::<ConnectInfo<SocketAddr>>()
                .map(|ConnectInfo(addr)| addr.ip()),
            forwarded: match lines {
                Some(lines) if !lines.is_empty() => Some(lines.join(",")),
                Some(_) => None,
                None => Some("\u{0}".to_owned()),
            },
        }
    }
}

impl<St: Send + Sync> FromRequestParts<St> for PeerIp {
    type Rejection = Infallible;

    async fn from_request_parts(parts: &mut Parts, _state: &St) -> Result<Self, Infallible> {
        Ok(PeerIp::from_parts(parts))
    }
}

/// Enforce the rate limit when configured. Charges the request against an
/// identity-derived [`RateKey`] — per authenticated `subject`, else per source
/// IP, else a single global bucket — and returns `Some(429)` if over budget.
pub(super) fn enforce_rate_limit<S>(
    state: &HttpState<S>,
    subject: Option<&str>,
    peer_ip: Option<IpAddr>,
) -> Option<Response> {
    let limiter = state.rate_limiter.as_ref()?;
    let key = match subject {
        Some(sub) => RateKey::Subject(sub.to_owned()),
        None => peer_ip.map_or(RateKey::Global, RateKey::Ip),
    };
    match limiter.check(&key) {
        Ok(()) => None,
        Err(retry_after) => Some(too_many_requests(retry_after)),
    }
}

/// Whether the request's `Accept` header lists `required` as supported.
/// Media ranges are matched per RFC 9110 §12.5.1 — `*/*` and `type/*`
/// wildcards count, and parameters (`;q=…`) are ignored for the "listed as
/// supported" check the MCP transports spec makes. A missing `Accept` header
/// fails: the spec says the client MUST include one.
pub(super) fn accepts(headers: &HeaderMap, required: &mime::Mime) -> bool {
    let Some(accept) = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    accept
        .split(',')
        .filter_map(|part| part.trim().parse::<mime::Mime>().ok())
        .any(|range| {
            (range.type_() == mime::STAR || range.type_() == required.type_())
                && (range.subtype() == mime::STAR || range.subtype() == required.subtype())
        })
}

/// Returns `Some(rejection)` if the request's `Origin` is disallowed, else `None`.
pub(super) fn check_origin(policy: &OriginPolicy, headers: &HeaderMap) -> Option<Response> {
    let origin = headers.get(header::ORIGIN)?; // no Origin → non-browser → allowed
    match policy {
        OriginPolicy::Any => None,
        OriginPolicy::Allowlist(list) => {
            let origin = origin.to_str().unwrap_or_default();
            (!list.iter().any(|allowed| allowed == origin))
                .then(|| (StatusCode::FORBIDDEN, "origin not allowed").into_response())
        }
    }
}

/// Returns `Some(rejection)` if the request's `Host` is disallowed, else `None`.
/// Unlike `Origin`, `Host` is always present, so `Allowlist` mode rejects a
/// missing/unmatched `Host` — the point is to pin the server's expected host(s).
pub(super) fn check_host(policy: &HostPolicy, headers: &HeaderMap) -> Option<Response> {
    match policy {
        HostPolicy::Any => None,
        HostPolicy::Allowlist(list) => {
            let host = headers
                .get(header::HOST)
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            (!list.iter().any(|allowed| allowed == host))
                .then(|| (StatusCode::FORBIDDEN, "host not allowed").into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    fn net(s: &str) -> IpNet {
        s.parse::<IpNet>().unwrap_or_else(|_| ip(s).into())
    }

    /// A trusted range covers every address in it: a pod network or a cloud
    /// load balancer's subnet, which a list of single addresses can't name.
    #[test]
    fn client_ip_trusts_a_proxy_range() {
        let p = peer("10.1.2.3", Some("203.0.113.7, 10.9.8.7"));
        assert_eq!(p.client_ip(&[net("10.0.0.0/8")]), Some(ip("203.0.113.7")));
    }

    /// Every `X-Forwarded-For` line counts. A proxy that appends its own line
    /// puts the trustworthy entry in the *last* one; reading only the first
    /// line took whatever the client wrote there.
    #[test]
    fn client_ip_reads_every_forwarded_line() {
        let mut headers = HeaderMap::new();
        headers.append("x-forwarded-for", HeaderValue::from_static("6.6.6.6"));
        headers.append("x-forwarded-for", HeaderValue::from_static("203.0.113.7"));
        let mut request = axum::http::Request::builder().body(()).unwrap();
        *request.headers_mut() = headers;
        request
            .extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([10, 0, 0, 1], 443))));
        let (parts, ()) = request.into_parts();
        let p = PeerIp::from_parts(&parts);
        assert_eq!(p.client_ip(&[net("10.0.0.1")]), Some(ip("203.0.113.7")));
    }

    fn peer(socket: &str, xff: Option<&str>) -> PeerIp {
        PeerIp {
            socket: Some(ip(socket)),
            forwarded: xff.map(str::to_owned),
        }
    }

    #[test]
    fn client_ip_uses_socket_when_no_trusted_proxies() {
        // Even with an XFF header, an empty trust list ignores it (unspoofable).
        let p = peer("203.0.113.9", Some("1.2.3.4"));
        assert_eq!(p.client_ip(&[]), Some(ip("203.0.113.9")));
    }

    #[test]
    fn client_ip_ignores_xff_from_untrusted_peer() {
        let p = peer("203.0.113.9", Some("1.2.3.4"));
        assert_eq!(p.client_ip(&[net("10.0.0.1")]), Some(ip("203.0.113.9")));
    }

    #[test]
    fn client_ip_uses_xff_behind_trusted_proxy() {
        // Peer is the trusted LB; the real client is the rightmost untrusted hop.
        let p = peer("10.0.0.1", Some("9.9.9.9, 203.0.113.7"));
        assert_eq!(p.client_ip(&[net("10.0.0.1")]), Some(ip("203.0.113.7")));
    }

    #[test]
    fn client_ip_skips_trusted_hops_in_xff() {
        // Two trusted proxies chained: skip both, take the client.
        let p = peer("10.0.0.1", Some("203.0.113.7, 10.0.0.2"));
        let trusted = [net("10.0.0.1"), net("10.0.0.2")];
        assert_eq!(p.client_ip(&trusted), Some(ip("203.0.113.7")));
    }

    #[test]
    fn client_ip_does_not_skip_malformed_proxy_boundaries() {
        let trusted = [net("10.0.0.1"), net("10.0.0.2")];
        for header in ["1.2.3.4, unknown", "1.2.3.4,,10.0.0.2", "1.2.3.4, [::1]"] {
            assert_eq!(
                peer("10.0.0.1", Some(header)).client_ip(&trusted),
                Some(ip("10.0.0.1"))
            );
        }
        // Entries left of the first untrusted peer are controlled by that peer.
        assert_eq!(
            peer("10.0.0.1", Some("garbage, 2001:db8::7, 10.0.0.2")).client_ip(&trusted),
            Some(ip("2001:db8::7"))
        );
        assert_eq!(
            PeerIp {
                socket: None,
                forwarded: Some("1.2.3.4".into())
            }
            .client_ip(&trusted),
            None
        );
    }
}
