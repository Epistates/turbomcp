//! Outbound OAuth/JWKS policy. Private HTTPS endpoints are supported by default;
//! operators accepting untrusted server URLs can select public-only egress.
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

/// Network and body budgets for authorization traffic.
#[derive(Clone, Debug)]
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
        let policy = NetworkPolicy {
            max_response_bytes: 4,
            ..NetworkPolicy::default()
        };
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
impl Default for NetworkPolicy {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            max_response_bytes: 1024 * 1024,
            public_only: false,
            allow_loopback_http: true,
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

    /// Build an HTTP client with redirects disabled, deadlines, and this DNS policy.
    /// # Errors
    /// Returns an error if the TLS/HTTP client cannot be initialized.
    pub fn http_client(&self) -> Result<reqwest::Client, reqwest::Error> {
        let builder = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .dns_resolver(Arc::new(PolicyResolver {
                public_only: self.public_only,
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
        if self.public_only && (local || ip.is_some_and(|ip| !public_address(ip))) {
            return Err("endpoint is not a public address".into());
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
    public_only: bool,
}
impl reqwest::dns::Resolve for PolicyResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let public_only = self.public_only;
        Box::pin(async move {
            let host = name.as_str().trim_end_matches('.');
            if host == "localhost" || host.ends_with(".localhost") {
                if public_only {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "loopback is not public",
                    )
                    .into());
                }
                // The plaintext exception must stay on loopback regardless of
                // a resolver's search domains or DNS answers.
                let address = std::net::SocketAddr::from(([127, 0, 0, 1], 0));
                return Ok(Box::new(std::iter::once(address)) as reqwest::dns::Addrs);
            }
            let addresses: Vec<_> = tokio::net::lookup_host((name.as_str(), 0)).await?.collect();
            if addresses.is_empty()
                || (public_only && addresses.iter().any(|a| !public_address(a.ip())))
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "DNS returned a non-public address",
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
