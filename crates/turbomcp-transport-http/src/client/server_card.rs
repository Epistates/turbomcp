//! Fetching a Server Card (SEP-2127) before connecting.

use reqwest::header::{ACCEPT, ETAG, IF_NONE_MATCH};
use turbomcp_protocol::server_card::{MEDIA_TYPE, ServerCard};

use super::{HttpClientError, HttpClientLimits, bounded_body, default_http_client};

/// What [`fetch_server_card`] found.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum FetchedCard {
    /// The card, and the `ETag` to revalidate it with next time.
    Card {
        /// The card, checked against its constraints.
        card: Box<ServerCard>,
        /// The validator the host sent, if any.
        etag: Option<String>,
    },
    /// The card is unchanged since the `ETag` sent as `if_none_match`.
    NotModified,
}

/// Fetch the Server Card at `url` (from an AI Catalog entry, or a server's
/// [`default_url`](turbomcp_protocol::server_card::default_url)), asking for
/// the card media type. Pass the `ETag` a previous fetch returned as
/// `if_none_match`, and an unchanged card comes back as
/// [`FetchedCard::NotModified`] without its body.
///
/// A card is advisory: it says where and how to connect, and once connected
/// the server's `server/discover` answer is what to trust where they differ.
/// Never base a security decision on one.
///
/// # Errors
/// [`HttpClientError::ServerCard`] for a failed request, a status other than
/// `200`/`304`, a body over the client's response limit, or a document that
/// isn't a valid card.
pub async fn fetch_server_card(
    url: &str,
    if_none_match: Option<&str>,
) -> Result<FetchedCard, HttpClientError> {
    let http = default_http_client(reqwest::header::HeaderMap::new())?;
    let mut request = http.get(url).header(ACCEPT, MEDIA_TYPE);
    if let Some(etag) = if_none_match {
        request = request.header(IF_NONE_MATCH, etag);
    }
    let fail = |why: String| HttpClientError::ServerCard(format!("{url}: {why}"));
    let response = request.send().await.map_err(|e| fail(e.to_string()))?;
    match response.status() {
        reqwest::StatusCode::NOT_MODIFIED => return Ok(FetchedCard::NotModified),
        reqwest::StatusCode::OK => {}
        status => return Err(fail(format!("answered {status}"))),
    }
    let etag = response
        .headers()
        .get(ETAG)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let body = bounded_body(response, &HttpClientLimits::default())
        .await
        .map_err(fail)?;
    let card: ServerCard =
        serde_json::from_slice(&body).map_err(|e| fail(format!("not a Server Card: {e}")))?;
    card.validate().map_err(|e| fail(e.to_string()))?;
    Ok(FetchedCard::Card {
        card: Box::new(card),
        etag,
    })
}
