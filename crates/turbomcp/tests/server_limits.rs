//! The limits a server applies to its own work, end to end through a real
//! client: a call deadline, so a hung handler can't hold its slot for ever.
#![cfg(feature = "client")]

use std::time::Duration;

use serde_json::Map;
use turbomcp::client::ClientBuilder;
use turbomcp::prelude::*;

#[derive(Clone)]
struct Slow;

#[server(name = "slow", version = "1.0.0")]
impl Slow {
    /// Never finishes.
    #[tool]
    async fn hang(&self) -> String {
        std::future::pending::<()>().await;
        unreachable!()
    }

    /// Finishes at once.
    #[tool]
    async fn quick(&self) -> String {
        "done".into()
    }
}

/// `call_timeout` cancels a call still running at the limit and answers it
/// with a timeout, while calls that finish in time are untouched.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_call_past_the_limit_is_answered_with_a_timeout() {
    let client = turbomcp::testing::connect(
        Slow.into_server()
            .with_call_timeout(Duration::from_millis(100)),
        ClientBuilder::new("limits", "1.0.0"),
    )
    .await
    .expect("connect");

    let started = std::time::Instant::now();
    let err = client
        .call_tool("hang", Map::new())
        .await
        .expect_err("timed out");
    assert!(started.elapsed() < Duration::from_secs(5), "{err}");
    assert!(err.to_string().contains("time limit"), "{err}");

    let quick = client.call_tool("quick", Map::new()).await.expect("quick");
    assert!(!quick.is_error);
}
