//! A `#[tool]` can answer with a protocol error, not only a tool error: an
//! explicit `McpError::Rpc` (`url_elicitation_required`, say) reaches the
//! client as a JSON-RPC error with its code and data.
#![cfg(feature = "client")]

use serde_json::{Map, json};
use turbomcp::client::{ClientBuilder, ConnectMode};
use turbomcp::prelude::*;

#[derive(Clone)]
struct Gate;

#[server(name = "gate", version = "1.0.0")]
impl Gate {
    /// Needs the user to sign in first.
    #[tool]
    async fn connect(&self) -> McpResult<String> {
        Err(McpError::url_elicitation_required(
            "sign in first",
            vec![json!({
                "mode": "url",
                "message": "Sign in to continue",
                "url": "https://auth.example.com/start",
                "elicitationId": "e-1",
            })],
        ))
    }

    /// Fails the way a tool fails.
    #[tool]
    async fn flaky(&self) -> McpResult<String> {
        Err(McpError::internal("the backend is down"))
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_explicit_rpc_error_is_a_protocol_error() {
    let client = turbomcp::testing::connect(
        Gate.into_server(),
        ClientBuilder::new("t", "1.0.0").with_connect_mode(ConnectMode::Legacy),
    )
    .await
    .expect("connect");

    let err = client
        .call_tool("connect", Map::new())
        .await
        .expect_err("a protocol error");
    let rpc = err.as_rpc().expect("a JSON-RPC error");
    assert_eq!(rpc.code, turbomcp::codes::URL_ELICITATION_REQUIRED);
    assert_eq!(
        rpc.data.as_ref().unwrap()["elicitations"][0]["elicitationId"],
        "e-1"
    );

    // Any other error is still the tool's, for the model to read.
    let flaky = client
        .call_tool("flaky", Map::new())
        .await
        .expect("a result");
    assert!(flaky.is_error);
}
