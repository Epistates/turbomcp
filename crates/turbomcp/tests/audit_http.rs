#![cfg(all(feature = "client", feature = "http"))]
use axum::{Json, Router};
use serde_json::json;
#[tokio::test]
async fn http_error_preserves_rpc_body_and_auth_challenge() {
    use axum::{http::StatusCode, routing::post};
    use turbomcp_client::{Connection, HttpClientTransport};
    let app = Router::new().route("/mcp", post(|| async {
   (StatusCode::BAD_REQUEST, [("www-authenticate", "Bearer scope=\"admin\"")], Json(json!({"jsonrpc":"2.0","id":1,"error":{"code":-32022,"message":"Unsupported protocol","data":{"supported":["2026-07-28"]}}})))
 }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let conn = Connection::new(HttpClientTransport::new(url).unwrap());
    let e = conn.request("tools/list", None).await.unwrap_err();
    server.abort();
    assert_eq!(e.rpc_code(), Some(-32022));
    let turbomcp_client::ClientError::Http(failure) = e else {
        panic!("typed HTTP error required")
    };
    assert_eq!(failure.status, 400);
    assert_eq!(
        failure.www_authenticate.as_deref(),
        Some("Bearer scope=\"admin\"")
    );
    conn.close().await;
}
