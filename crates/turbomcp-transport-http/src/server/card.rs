//! The Server Card (SEP-2127) at `GET {path}/server-card`.
//!
//! Public, read-only metadata, so its route answers every origin (the
//! extension's CORS requirement) and sits outside the endpoint's own CORS
//! policy and authentication. It is served with the card media type, a
//! one-hour public cache lifetime and a strong `ETag` that turns a matching
//! `If-None-Match` into `304 Not Modified`.

use std::sync::Arc;

use axum::Router;
use axum::extract::State;
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use bytes::Bytes;
use turbomcp_core::{Implementation, ProtocolVersion};
use turbomcp_protocol::server_card::{self, Icon, ServerCard, ServerCardError};

const CACHE_CONTROL: &str = "public, max-age=3600";

/// The card as served: the bytes and their validator.
pub(super) struct Card {
    body: Bytes,
    etag: HeaderValue,
}

impl Card {
    /// `card` with what the server already knows filled in, checked, and
    /// rendered once.
    pub(super) fn build(
        card: ServerCard,
        info: Option<&Implementation>,
        versions: &[ProtocolVersion],
    ) -> Result<Self, ServerCardError> {
        let card = derive(card, info, versions);
        card.validate()?;
        let body = serde_json::to_vec(&card).expect("a card serializes");
        let etag = etag(&body);
        Ok(Self {
            body: body.into(),
            etag,
        })
    }
}

/// Fill what the card leaves unset from the running server, so the card
/// can't contradict what `server/discover` will say ("SHOULD NOT contradict
/// the equivalent values"). A derived value the card's constraints refuse (a
/// title over 100 characters) is left out rather than failing the card.
pub(super) fn derive(
    mut card: ServerCard,
    info: Option<&Implementation>,
    versions: &[ProtocolVersion],
) -> ServerCard {
    let versions: Vec<String> = versions.iter().map(|v| v.as_str().to_owned()).collect();
    for remote in &mut card.remotes {
        if remote.supported_protocol_versions.is_empty() {
            remote.supported_protocol_versions.clone_from(&versions);
        }
    }
    let Some(info) = info else {
        return card;
    };
    let text = |key: &str| info.extra.get(key).and_then(serde_json::Value::as_str);
    if card.version.is_empty() {
        fill(&mut card, "version", |c| {
            c.version.clone_from(&info.version)
        });
    }
    if card.description.is_empty()
        && let Some(description) = text("description")
    {
        fill(&mut card, "description", |c| {
            c.description = description.to_owned()
        });
    }
    if card.title.is_none()
        && let Some(title) = &info.title
    {
        fill(&mut card, "title", |c| c.title = Some(title.clone()));
    }
    if card.website_url.is_none()
        && let Some(url) = text("websiteUrl")
    {
        fill(&mut card, "websiteUrl", |c| {
            c.website_url = Some(url.to_owned())
        });
    }
    if card.icons.is_empty()
        && let Some(icons) = info
            .extra
            .get("icons")
            .and_then(|i| serde_json::from_value::<Vec<Icon>>(i.clone()).ok())
    {
        fill(&mut card, "icons[].src", |c| c.icons = icons);
    }
    card
}

/// Apply `set`, and undo it if it is what makes the card invalid.
fn fill(card: &mut ServerCard, field: &str, set: impl FnOnce(&mut ServerCard)) {
    let before = card.clone();
    set(card);
    if card.validate().is_err_and(|e| e.field == field) {
        tracing::warn!(
            field,
            "left a derived Server Card field out: it breaks the card's constraints"
        );
        *card = before;
    }
}

/// A strong validator: the first 128 bits of the body's SHA-256, so every
/// replica serving the same card agrees on it.
fn etag(body: &[u8]) -> HeaderValue {
    use sha2::{Digest, Sha256};
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(body);
    let mut tag = String::with_capacity(34);
    tag.push('"');
    for byte in &digest[..16] {
        tag.push(char::from(HEX[usize::from(byte >> 4)]));
        tag.push(char::from(HEX[usize::from(byte & 0xf)]));
    }
    tag.push('"');
    HeaderValue::from_str(&tag).expect("hex is a header value")
}

/// The card's path for an endpoint at `path`.
pub(super) fn path(path: &str) -> String {
    server_card::default_url(path)
}

pub(super) fn routes(card: Card, endpoint: &str) -> Router {
    Router::new()
        .route(
            &path(endpoint),
            axum::routing::get(serve).options(preflight),
        )
        .with_state(Arc::new(card))
}

async fn serve(State(card): State<Arc<Card>>, headers: HeaderMap) -> Response {
    let mut response = if not_modified(&headers, &card.etag) {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        let mut response = card.body.clone().into_response();
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static(server_card::MEDIA_TYPE),
        );
        response
    };
    let out = response.headers_mut();
    out.insert(header::ETAG, card.etag.clone());
    out.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static(CACHE_CONTROL),
    );
    cors(out);
    response
}

/// A browser preflights the card's `GET` when it sends `If-None-Match`.
async fn preflight() -> Response {
    let mut response = StatusCode::NO_CONTENT.into_response();
    cors(response.headers_mut());
    response
}

fn cors(headers: &mut HeaderMap) {
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type, If-None-Match"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        HeaderValue::from_static("ETag"),
    );
}

/// RFC 9110 §13.1.2: `*`, or any listed tag that matches by the weak
/// comparison (opaque tags equal, `W/` ignored).
fn not_modified(headers: &HeaderMap, etag: &HeaderValue) -> bool {
    let ours = etag.to_str().unwrap_or_default();
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .any(|tag| tag == "*" || tag.strip_prefix("W/").unwrap_or(tag) == ours)
}

#[cfg(test)]
mod tests {
    use super::*;
    use turbomcp_protocol::server_card::Remote;

    fn info() -> Implementation {
        Implementation::new("weather", "3.1.4")
            .with_title("Weather")
            .with_website_url("https://example.com")
    }

    #[test]
    fn unset_fields_come_from_the_running_server() {
        let card = ServerCard::new("com.example/weather", "Forecasts.")
            .with_remote(Remote::streamable_http("https://example.com/mcp"));
        let card = derive(
            card,
            Some(&info()),
            &[ProtocolVersion::V2025_11_25, ProtocolVersion::V2026_07_28],
        );
        assert_eq!(card.version, "3.1.4");
        assert_eq!(card.title.as_deref(), Some("Weather"));
        assert_eq!(card.website_url.as_deref(), Some("https://example.com"));
        assert_eq!(
            card.remotes[0].supported_protocol_versions,
            ["2025-11-25", "2026-07-28"]
        );
        card.validate().unwrap();
    }

    #[test]
    fn what_the_card_sets_wins_and_an_invalid_derivation_is_left_out() {
        let long = Implementation::new("w", "1").with_title("t".repeat(101));
        let card = derive(
            ServerCard::new("com.example/w", "x").with_version("9.0.0"),
            Some(&long),
            &[],
        );
        assert_eq!(card.version, "9.0.0");
        assert!(card.title.is_none());
    }

    #[test]
    fn if_none_match_uses_the_weak_comparison_and_the_wildcard() {
        let etag = etag(b"card");
        let with = |v: &str| {
            let mut h = HeaderMap::new();
            h.insert(header::IF_NONE_MATCH, HeaderValue::from_str(v).unwrap());
            not_modified(&h, &etag)
        };
        let ours = etag.to_str().unwrap().to_owned();
        assert!(with(&ours));
        assert!(with(&format!("W/{ours}")));
        assert!(with(&format!("\"other\", {ours}")));
        assert!(with("*"));
        assert!(!with("\"other\""));
        assert!(!not_modified(&HeaderMap::new(), &etag));
    }
}
