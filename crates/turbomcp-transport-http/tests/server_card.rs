//! SEP-2127: the Server Card at `GET {path}/server-card`, filled from the
//! running server, open to every origin, cached and revalidated.

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;
use turbomcp_core::{Implementation, McpResult};
use turbomcp_protocol::neutral;
use turbomcp_protocol::server_card::{MEDIA_TYPE, Remote, ServerCard};
use turbomcp_server::{
    CallToolContext, ListToolsContext, McpServerCore, MethodRouter, Server, VersionDispatcher,
    WithTools,
};
use turbomcp_transport_http::{HttpConfig, router};

#[derive(Clone)]
struct Forecast;

impl McpServerCore for Forecast {
    fn server_info(&self) -> Implementation {
        Implementation::new("forecast", "3.1.4")
            .with_title("Forecast")
            .with_website_url("https://example.com/forecast")
    }
}

impl WithTools for Forecast {
    async fn list_tools(
        &self,
        _ctx: &ListToolsContext,
        _params: neutral::ListParams,
    ) -> McpResult<neutral::ListToolsResult> {
        Ok(neutral::ListToolsResult::new(vec![]))
    }

    async fn call_tool(
        &self,
        _ctx: &CallToolContext,
        _params: neutral::CallToolParams,
    ) -> McpResult<neutral::CallToolResult> {
        Ok(neutral::CallToolResult::text("ok"))
    }
}

fn card() -> ServerCard {
    ServerCard::new("com.example/forecast", "Weather forecasts for any city.")
        .with_remote(Remote::streamable_http("https://api.example.com/mcp"))
}

fn app(config: HttpConfig) -> axum::Router {
    router(
        Server::new(VersionDispatcher::new(
            Forecast,
            MethodRouter::new().with_tools(),
        )),
        config,
    )
}

fn get(headers: &[(header::HeaderName, &str)]) -> Request<Body> {
    let mut request = Request::builder().uri("/mcp/server-card");
    for (name, value) in headers {
        request = request.header(name, *value);
    }
    request.body(Body::empty()).unwrap()
}

#[tokio::test]
async fn the_card_is_served_filled_from_the_running_server() {
    let config = HttpConfig::new().with_server_card(card()).unwrap();
    let resp = app(config)
        .oneshot(get(&[(header::ACCEPT, MEDIA_TYPE)]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let h = resp.headers();
    assert_eq!(h[header::CONTENT_TYPE], MEDIA_TYPE);
    assert_eq!(h[header::CACHE_CONTROL], "public, max-age=3600");
    assert!(h.contains_key(header::ETAG));
    assert_eq!(h[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    assert_eq!(h[header::ACCESS_CONTROL_EXPOSE_HEADERS], "ETag");
    let body: Value =
        serde_json::from_slice(&resp.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(
        body,
        json!({
            "$schema": "https://static.modelcontextprotocol.io/schemas/v1/server-card.schema.json",
            "name": "com.example/forecast",
            "version": "3.1.4",
            "description": "Weather forecasts for any city.",
            "title": "Forecast",
            "websiteUrl": "https://example.com/forecast",
            "remotes": [{
                "type": "streamable-http",
                "url": "https://api.example.com/mcp",
                "supportedProtocolVersions": ["2025-06-18", "2025-11-25", "2026-07-28"]
            }]
        })
    );
}

#[tokio::test]
async fn a_matching_if_none_match_is_answered_not_modified() {
    let app = app(HttpConfig::new().with_server_card(card()).unwrap());
    let first = app.clone().oneshot(get(&[])).await.unwrap();
    let etag = first.headers()[header::ETAG].to_str().unwrap().to_owned();
    let again = app
        .oneshot(get(&[(header::IF_NONE_MATCH, &etag)]))
        .await
        .unwrap();
    assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(again.headers()[header::ETAG], etag.as_str());
    assert!(
        again
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
}

/// The endpoint admits one origin; the card, being public metadata, admits
/// every origin, preflight included (`If-None-Match` makes a browser ask).
#[tokio::test]
async fn any_origin_may_read_the_card_whatever_the_endpoint_allows() {
    let app = app(HttpConfig::new()
        .allow_origin("https://app.example.com")
        .with_server_card(card())
        .unwrap());
    let resp = app
        .clone()
        .oneshot(get(&[(header::ORIGIN, "https://elsewhere.example")]))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(resp.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");

    let preflight = Request::builder()
        .method("OPTIONS")
        .uri("/mcp/server-card")
        .header(header::ORIGIN, "https://elsewhere.example")
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "if-none-match")
        .body(Body::empty())
        .unwrap();
    let resp = app.oneshot(preflight).await.unwrap();
    assert!(resp.status().is_success(), "{}", resp.status());
    let h = resp.headers();
    assert_eq!(h[header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
    assert_eq!(h[header::ACCESS_CONTROL_ALLOW_METHODS], "GET");
    assert_eq!(
        h[header::ACCESS_CONTROL_ALLOW_HEADERS],
        "Content-Type, If-None-Match"
    );
}

#[tokio::test]
async fn no_card_no_route_and_a_bad_card_is_refused_up_front() {
    let resp = app(HttpConfig::new()).oneshot(get(&[])).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let err = HttpConfig::new()
        .with_server_card(ServerCard::new("forecast", "Forecasts."))
        .unwrap_err();
    assert_eq!(err.field, "name");
}

/// The client half: fetch the card a served endpoint publishes, then
/// revalidate it with the `ETag` it came with.
#[cfg(feature = "client")]
#[tokio::test]
async fn a_client_fetches_and_revalidates_the_card() {
    use turbomcp_transport_http::{FetchedCard, fetch_server_card};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = app(HttpConfig::new().with_server_card(card()).unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await });
    let url = turbomcp_protocol::server_card::default_url(&format!("http://{addr}/mcp"));

    let FetchedCard::Card { card, etag } = fetch_server_card(&url, None).await.unwrap() else {
        panic!("no card on the first fetch");
    };
    assert_eq!(card.name, "com.example/forecast");
    assert_eq!(card.version, "3.1.4");
    let etag = etag.expect("an ETag");
    assert_eq!(
        fetch_server_card(&url, Some(&etag)).await.unwrap(),
        FetchedCard::NotModified
    );
    // A location with no card is an error, not an empty card.
    let missing = fetch_server_card(&format!("http://{addr}/nope"), None).await;
    assert!(missing.is_err());
}
