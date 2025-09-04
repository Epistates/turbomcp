//! Minimal TurboMCP stdio server (no macros)

use turbomcp_server::ServerBuilder;
use turbomcp_server::ServerError;

#[tokio::main]
async fn main() -> Result<(), ServerError> {
    tracing_subscriber::fmt::init();
    let server = ServerBuilder::new()
        .name("Minimal")
        .version("1.0.0")
        .build();
    server.run_stdio().await
}
