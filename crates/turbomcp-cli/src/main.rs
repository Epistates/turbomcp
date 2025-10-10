#[tokio::main]
async fn main() {
    if let Err(e) = turbomcp_cli::run().await {
        // Display error with proper formatting
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
