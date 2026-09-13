//! Outbound OAuth/JWKS policy. Private HTTPS endpoints are supported by default;
//! operators accepting untrusted server URLs can select public-only egress.
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use ipnet::IpNet;

/// Network and body budgets for authorization traffic.
///
/// Construct with [`Default`] or [`public_only`](Self::public_only) and adjust
/// with the `with_*` methods; the field set grows as the policy does.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct NetworkPolicy {
    /// Deadline for an HTTP operation. SDK-built clients apply it across
    /// headers and body; custom clients also get separate header/body bounds.
    pub timeout: Duration,
    /// Maximum decompressed metadata, token, registration, or JWKS body.
    pub max_response_bytes: usize,
    /// Reject private/reserved literal and DNS-resolved addresses. Disables
    /// environment proxies so they cannot bypass address filtering.
    pub public_only: bool,
    /// Allow HTTP on loopback for native/local development servers.
    pub allow_loopback_http: bool,
    /// Ranges permitted despite [`public_only`](Self::public_only), consulted
    /// only after the reserved-range check. Empty by default.
    ///
    /// This is the difference between a policy an operator can adopt and one
    /// they have to turn off. A deployment that accepts untrusted endpoint URLs
    /// *and* runs its own authorization server on `10.42.0.0/16` would
    /// otherwise have to choose between SSRF exposure and not working — and
    /// the single switch reopens CGNAT, link-local, the cloud metadata address,
    /// and every other reserved range along with the one it needed.
    pub allow_ranges: Vec<IpNet>,
}

impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            max_response_bytes: 1024 * 1024,
            public_only: false,
            allow_loopback_http: true,
            allow_ranges: Vec::new(),
        }
    }
}

impl NetworkPolicy {
    /// Policy for server-side clients accepting untrusted endpoint URLs.
    #[must_use]
    pub fn public_only() -> Self {
        Self {
            public_only: true,
            allow_loopback_http: false,
            ..Self::default()
        }
    }

    /// Set the per-operation deadline.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Set the maximum decompressed response body.
    #[must_use]
    pub fn with_max_response_bytes(mut self, max: usize) -> Self {
        self.max_response_bytes = max;
        self
    }

    /// Choose whether only public addresses may be reached.
    #[must_use]
    pub fn with_public_only(mut self, public_only: bool) -> Self {
        self.public_only = public_only;
        self
    }

    /// Choose whether plaintext HTTP is allowed on loopback.
    #[must_use]
    pub fn with_loopback_http(mut self, allow: bool) -> Self {
        self.allow_loopback_http = allow;
        self
    }

    /// Permit these ranges despite [`public_only`](Self::public_only). Replaces
    /// any previously set ranges.
    #[must_use]
    pub fn with_allowed_ranges(mut self, ranges: impl IntoIterator<Item = IpNet>) -> Self {
        self.allow_ranges = ranges.into_iter().collect();
        self
    }

    /// Whether this policy will connect to `ip`.
    ///
    /// Both the URL check and the DNS resolver route through here, so a literal
    /// address and the same address arrived at by name are judged identically.
    fn permits(&self, ip: IpAddr) -> bool {
        if !self.public_only {
            return true;
        }
        // `::ffff:10.0.0.1` is `10.0.0.1`; normalize before either check so the
        // two spellings of one host cannot disagree.
        let ip = match ip {
            IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
            v4 => v4,
        };
        public_address(ip) || self.allow_ranges.iter().any(|net| net.contains(&ip))
    }

    /// Build an HTTP client with redirects disabled, deadlines, and this DNS policy.
    /// # Errors
    /// Returns an error if the TLS/HTTP client cannot be initialized.
    pub fn http_client(&self) -> Result<reqwest::Client, reqwest::Error> {
        let builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .dns_resolver(Arc::new(PolicyResolver {
                policy: self.clone(),
            }))
            .timeout(self.timeout)
            .connect_timeout(self.timeout.min(Duration::from_secs(10)));
        builder.build()
    }

    /// Validate a URL before making any connection.
    /// # Errors
    /// Rejects unsafe schemes, credentials, fragments, or disallowed addresses.
    pub fn validate_url(&self, raw: &str) -> Result<(), String> {
        let url = reqwest::Url::parse(raw).map_err(|_| "invalid endpoint URL")?;
        let host = url
            .host_str()
            .ok_or("endpoint has no host")?
            .trim_matches(['[', ']']);
        let ip = host.parse::<IpAddr>().ok();
        let local = ip.is_some_and(|ip| ip.is_loopback())
            || host.trim_end_matches('.').eq_ignore_ascii_case("localhost")
            || host.trim_end_matches('.').ends_with(".localhost");
        if !url.username().is_empty() || url.password().is_some() || url.fragment().is_some() {
            return Err("endpoint credentials and fragments are forbidden".into());
        }
        if url.scheme() != "https" && !(url.scheme() == "http" && local && self.allow_loopback_http)
        {
            return Err(
                "endpoint must use HTTPS (HTTP is allowed only on configured loopback)".into(),
            );
        }
        // A name is only checkable once resolved (the resolver does that), with
        // the exception of the loopback names, which resolve nowhere else.
        let literal = ip.or_else(|| local.then(|| IpAddr::from([127, 0, 0, 1])));
        if literal.is_some_and(|ip| !self.permits(ip)) {
            return Err("endpoint address is neither public nor in an allowed range".into());
        }
        Ok(())
    }

    pub(crate) async fn send(
        &self,
        client: &reqwest::Client,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, String> {
        let request = request.build().map_err(|e| e.to_string())?;
        self.validate_url(request.url().as_str())?;
        tokio::time::timeout(self.timeout, client.execute(request))
            .await
            .map_err(|_| "authorization request deadline exceeded".to_owned())?
            .map_err(|e| e.to_string())
    }

    pub(crate) async fn body(&self, mut response: reqwest::Response) -> Result<Vec<u8>, String> {
        tokio::time::timeout(self.timeout, async {
            let mut bytes = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
                if chunk.len() > self.max_response_bytes.saturating_sub(bytes.len()) {
                    return Err("authorization response exceeds byte limit".into());
                }
                bytes.extend_from_slice(&chunk);
            }
            Ok(bytes)
        })
        .await
        .map_err(|_| "authorization body deadline exceeded".to_owned())?
    }
}

#[derive(Debug)]
struct PolicyResolver {
    policy: NetworkPolicy,
}

impl reqwest::dns::Resolve for PolicyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let policy = self.policy.clone();
        Box::pin(async move {
            let host = name.as_str().trim_end_matches('.');
            let addresses: Vec<std::net::SocketAddr> =
                if host == "localhost" || host.ends_with(".localhost") {
                    // The plaintext exception must stay on loopback regardless
                    // of a resolver's search domains or DNS answers.
                    vec![std::net::SocketAddr::from(([127, 0, 0, 1], 0))]
                } else {
                    tokio::net::lookup_host((name.as_str(), 0)).await?.collect()
                };
            // Every answer must pass: a reply mixing a public address with a
            // private one is the shape a rebinding attack takes, and the
            // connector is free to pick either.
            if addresses.is_empty() || !addresses.iter().all(|a| policy.permits(a.ip())) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "DNS returned an address this network policy refuses",
                )
                .into());
            }
            Ok(Box::new(addresses.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || a == 0
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 192 && b == 0 && c == 0)
                || (a == 192 && b == 88 && c == 99)
                || (a == 198 && (b == 18 || b == 19)))
        }
        IpAddr::V6(ip) => {
            if let Some(ip) = ip.to_ipv4_mapped() {
                return public_address(IpAddr::V4(ip));
            }
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && !(s[0] == 0x2002)
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_endpoints_are_an_operator_policy() {
        let normal = NetworkPolicy::default();
        for url in [
            "https://10.0.0.5/token",
            "http://127.0.0.1/token",
            "http://[::1]/token",
        ] {
            assert!(normal.validate_url(url).is_ok(), "{url}");
            assert!(
                NetworkPolicy::public_only().validate_url(url).is_err(),
                "{url}"
            );
        }
        for url in [
            "http://example.com",
            "file:///tmp/key",
            "javascript:alert(1)",
            "https://user:secret@example.com",
            "https://example.com/#token",
        ] {
            assert!(normal.validate_url(url).is_err(), "{url}");
        }
        for ip in [
            "0.0.0.0",
            "100.64.0.1",
            "192.0.2.1",
            "198.18.0.1",
            "224.0.0.1",
            "::ffff:127.0.0.1",
            "2001:db8::1",
            "fc00::1",
            "fe80::1",
        ] {
            assert!(!public_address(ip.parse().unwrap()), "{ip}");
        }
    }

    /// The escape hatch from public-only egress is a named range, not the whole
    /// reserved space. A deployment that accepts untrusted endpoint URLs *and*
    /// runs one internal authorization server should not have to choose between
    /// SSRF exposure and not working.
    #[test]
    fn a_named_internal_range_does_not_reopen_the_rest() {
        let policy =
            NetworkPolicy::public_only().with_allowed_ranges(["10.42.0.0/16".parse().unwrap()]);
        assert!(policy.validate_url("https://10.42.0.5/token").is_ok());
        for url in [
            "https://10.43.0.5/token",                   // a neighbouring /16
            "https://169.254.169.254/latest/meta-data/", // cloud metadata
            "https://127.0.0.1/token",
            "https://[fd00::1]/token",
            "https://[::ffff:10.43.0.5]/token", // the same neighbour, mapped
        ] {
            assert!(policy.validate_url(url).is_err(), "{url}");
        }
        // Widening the address policy must not widen the scheme policy.
        assert!(policy.validate_url("http://10.42.0.5/token").is_err());
        // A mapped form of an allowed address reads as that address.
        assert!(
            policy
                .validate_url("https://[::ffff:10.42.0.5]/token")
                .is_ok()
        );
    }

    /// The allowlist has to reach the resolver too, or a hostname pointing into
    /// the named range still fails at connect time. Exercised through a real
    /// request, because the resolver is only installed on SDK-built clients.
    #[tokio::test]
    async fn an_allowed_range_is_reachable_by_name() {
        use axum::{Router, routing::get};
        let app = Router::new().route("/meta", get(|| async { "{}" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let refused = NetworkPolicy::public_only();
        let policy = refused
            .clone()
            .with_allowed_ranges(["127.0.0.0/8".parse().unwrap()])
            .with_loopback_http(true);
        let http = policy.http_client().unwrap();
        let url = format!("http://localhost:{port}/meta");
        let response = policy.send(&http, http.get(&url)).await.unwrap();
        assert!(response.status().is_success());
        // And the same name is still refused without the range.
        let http = refused.http_client().unwrap();
        assert!(refused.send(&http, http.get(&url)).await.is_err());
        task.abort();
    }

    #[tokio::test]
    async fn defaults_do_not_follow_redirects_and_bound_bodies() {
        use axum::{Router, response::Redirect, routing::get};
        let app = Router::new()
            .route("/redirect", get(|| async { Redirect::temporary("/body") }))
            .route("/body", get(|| async { "0123456789" }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let policy = NetworkPolicy::default().with_max_response_bytes(4);
        let http = policy.http_client().unwrap();
        let redirect = policy
            .send(&http, http.get(format!("{base}/redirect")))
            .await
            .unwrap();
        assert!(redirect.status().is_redirection());
        let body = policy
            .send(&http, http.get(format!("{base}/body")))
            .await
            .unwrap();
        assert!(policy.body(body).await.unwrap_err().contains("byte limit"));
        task.abort();
    }
}
