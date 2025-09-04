//! HTTP Server Example - Comprehensive web server with REST API, SSE, and WebSocket support
//!
//! This example demonstrates TurboMCP's comprehensive HTTP server capabilities using Axum integration.
//! It shows how to build a modern web server that can serve both traditional REST endpoints
//! and MCP protocol over HTTP, SSE, and WebSockets.
//!
//! # Features Demonstrated
//!
//! - **HTTP REST API**: Traditional REST endpoints alongside MCP
//! - **Server-Sent Events (SSE)**: Real-time updates to web clients  
//! - **WebSocket Support**: Full-duplex communication for MCP
//! - **"Bring Your Own Server" Pattern**: Integration with existing Axum applications
//! - **Middleware Integration**: Custom middleware, CORS, compression, tracing
//! - **Session Management**: User sessions with authentication
//! - **Health Checks**: Production-ready monitoring endpoints
//! - **Metrics Collection**: Performance monitoring and observability
//!
//! # Usage
//!
//! ```bash
//! # Start the HTTP server (requires http feature)
//! cargo run --example 10_http_server --features http
//!
//! # Test with curl
//! curl http://localhost:3000/api/status
//! curl http://localhost:3000/mcp/capabilities
//!
//! # Test MCP over HTTP
//! curl -X POST http://localhost:3000/mcp \
//!   -H "Content-Type: application/json" \
//!   -d '{"jsonrpc":"2.0","id":"1","method":"tools/list","params":{}}'
//!
//! # Connect to Server-Sent Events
//! curl http://localhost:3000/mcp/sse
//!
//! # WebSocket connection (use a WebSocket client)
//! ws://localhost:3000/mcp/ws
//! ```
//!
//! # Architecture
//!
//! This example demonstrates the "bring your own server" pattern where you can:
//! 1. Start with a regular Axum application
//! 2. Add MCP capabilities using the `turbo_mcp_routes()` extension method
//! 3. Customize middleware, authentication, and additional routes as needed
//!
//! The server provides multiple ways to interact with MCP:
//! - **HTTP POST**: Standard JSON-RPC over HTTP
//! - **Server-Sent Events**: Real-time notifications and streaming responses  
//! - **WebSockets**: Full-duplex communication with automatic JSON-RPC handling
//!
//! # Production Considerations
//!
//! This example includes production-ready patterns:
//! - Comprehensive error handling and validation
//! - Security headers and CORS configuration
//! - Request/response logging and tracing
//! - Health checks and metrics endpoints
//! - Graceful shutdown handling
//! - Session management and cleanup

use async_trait::async_trait;
use axum::{
    Router,
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json},
    routing::get,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::net::TcpListener;
use tracing::{info, warn};
use turbomcp::prelude::*;
use turbomcp_transport::{AxumMcpExt, McpService, SessionInfo};

/// Our MCP server implementation with business logic
#[derive(Clone)]
struct WebMcpServer {
    /// Request counter for demo purposes
    request_count: Arc<AtomicU64>,
    /// Shared state for demo
    shared_data: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
}

#[server(
    name = "WebMcpServer",
    version = "1.0.0",
    description = "Comprehensive HTTP server with MCP support"
)]
impl WebMcpServer {
    #[tool("Store a key-value pair in the server")]
    async fn store_data(&self, key: String, value: String) -> McpResult<String> {
        if key.is_empty() {
            return Err(McpError::InvalidInput("Key cannot be empty".to_string()));
        }

        let mut data = self.shared_data.write().await;
        let old_value = data.insert(key.clone(), value.clone());

        let result = match old_value {
            Some(ref old) => format!("Updated '{}': '{}' -> '{}'", key, old, value),
            None => format!("Stored new entry '{}': '{}'", key, value),
        };

        Ok(result)
    }

    #[tool("Retrieve a value by key from the server")]
    async fn get_data(&self, key: String) -> McpResult<String> {
        let data = self.shared_data.read().await;

        match data.get(&key) {
            Some(value) => Ok(format!("Found '{}': '{}'", key, value)),
            None => Err(McpError::InvalidInput(format!("Key '{}' not found", key))),
        }
    }

    #[tool("List all stored keys")]
    async fn list_keys(&self) -> McpResult<Vec<String>> {
        let data = self.shared_data.read().await;
        Ok(data.keys().cloned().collect())
    }

    #[tool("Get server statistics")]
    async fn get_stats(&self) -> McpResult<String> {
        let count = self.request_count.load(Ordering::Relaxed);
        let data_count = self.shared_data.read().await.len();

        Ok(format!(
            "Server stats: {} requests handled, {} items stored",
            count, data_count
        ))
    }

    #[tool("Clear all stored data")]
    async fn clear_data(&self) -> McpResult<String> {
        let mut data = self.shared_data.write().await;
        let count = data.len();
        data.clear();

        Ok(format!("Cleared {} items from storage", count))
    }
}

#[async_trait]
impl McpService for WebMcpServer {
    async fn process_request(
        &self,
        request: serde_json::Value,
        _session: &SessionInfo,
    ) -> turbomcp_core::Result<serde_json::Value> {
        // Simple echo for demo - in real usage you'd route to actual MCP handlers
        self.request_count.fetch_add(1, Ordering::Relaxed);

        Ok(serde_json::json!({
            "jsonrpc": "2.0",
            "id": request.get("id"),
            "result": {
                "message": "WebMcpServer processing MCP request",
                "request": request,
                "available_tools": ["store_data", "get_data", "list_keys", "get_stats", "clear_data"],
                "note": "This is a demo implementation. For full MCP functionality, see the tool implementations above."
            }
        }))
    }
}

/// Custom application state for our REST API
#[derive(Clone)]
struct AppState {
    mcp_server: WebMcpServer,
}

/// REST API response types
#[derive(Serialize)]
struct ApiResponse<T> {
    success: bool,
    data: T,
    message: String,
}

#[derive(Deserialize)]
struct StoreRequest {
    key: String,
    value: String,
}

/// Create our main Axum application with both REST and MCP routes
fn create_app() -> (Router, WebMcpServer) {
    let mcp_server = WebMcpServer {
        request_count: Arc::new(AtomicU64::new(0)),
        shared_data: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
    };

    let app_state = AppState {
        mcp_server: mcp_server.clone(),
    };

    // 🌟 COMPREHENSIVE STATE PRESERVATION DEMONSTRATION 🌟
    //
    // This example shows TurboMCP's comprehensive "bring your own server" approach
    // that preserves your existing application state while seamlessly adding MCP capabilities.

    // Create base Axum router with REST API routes and your custom state
    let rest_router = Router::new()
        // Root endpoint with HTML welcome page
        .route("/", get(root_handler))
        // REST API routes that use AppState
        .route("/api/status", get(api_status))
        .route("/api/data", get(api_list_data).post(api_store_data))
        .route("/api/data/:key", get(api_get_data))
        // Set application state for REST routes
        .with_state(app_state); // 👈 Your existing state is preserved!

    // 🚀 COMPREHENSIVE ENHANCEMENT: State-preserving MCP integration
    //
    // The magic happens here - we can merge a stateless MCP router with our
    // stateful REST router without losing ANY functionality:
    //
    // ✅ REST routes keep their AppState
    // ✅ MCP routes get their own McpAppState
    // ✅ Zero conflicts, maximum compatibility
    // ✅ Perfect separation of concerns

    // 🚀 COMPREHENSIVE PRODUCTION CONFIGURATION 🚀
    //
    // Using staging configuration with enhanced security.
    // Customize these settings based on your requirements.

    let mcp_config = turbomcp_transport::McpServerConfig::staging()
        .with_cors_origins(vec![
            "http://localhost:3000".to_string(),
            "https://yourdomain.com".to_string(),
        ])
        .with_custom_csp(
            "default-src 'self'; connect-src 'self' ws: wss:; script-src 'self' 'unsafe-inline'",
        );

    let app = rest_router.merge(Router::<()>::turbo_mcp_routes_for_merge(
        mcp_server.clone(),
        mcp_config,
    ));
    //     ^^^^^^^^^^ ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
    //     Unit state   This creates a stateless MCP router that merges cleanly!
    //
    //     Previous approach: Router::new().turbo_mcp_routes(service) ❌
    //     Comprehensive approach: Router::<()>::turbo_mcp_routes_for_merge_default(service) ✅

    (app, mcp_server)
}

/// Root handler that serves a simple HTML page explaining the server
async fn root_handler() -> impl IntoResponse {
    let html = r#"
<!DOCTYPE html>
<html>
<head>
    <title>TurboMCP HTTP Server Example</title>
    <style>
        body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif; margin: 2rem; }
        .container { max-width: 800px; margin: 0 auto; }
        .endpoint { background: #f5f5f5; padding: 1rem; margin: 1rem 0; border-radius: 4px; }
        .method { font-weight: bold; color: #2563eb; }
        pre { background: #1f2937; color: #f9fafb; padding: 1rem; border-radius: 4px; overflow-x: auto; }
        .feature { background: #ecfdf5; border-left: 4px solid #10b981; padding: 1rem; margin: 1rem 0; }
    </style>
</head>
<body>
    <div class="container">
        <h1>🚀 TurboMCP HTTP Server Example</h1>
        <p>Welcome to the comprehensive HTTP server example showcasing TurboMCP's comprehensive web capabilities!</p>
        
        <div class="feature">
            <h3>✨ Features Demonstrated</h3>
            <ul>
                <li><strong>HTTP REST API</strong> - Traditional endpoints alongside MCP</li>
                <li><strong>Server-Sent Events (SSE)</strong> - Real-time updates at <code>/mcp/sse</code></li>
                <li><strong>WebSocket Support</strong> - Full-duplex MCP at <code>/mcp/ws</code></li>
                <li><strong>"Bring Your Own Server"</strong> - Integrate with existing Axum apps</li>
                <li><strong>Session Management</strong> - Built-in user session handling</li>
                <li><strong>Health Checks</strong> - Monitoring endpoints for production</li>
            </ul>
        </div>
        
        <h2>🔗 Available Endpoints</h2>
        
        <div class="endpoint">
            <div class="method">GET /api/status</div>
            <p>Server status and statistics</p>
        </div>
        
        <div class="endpoint">
            <div class="method">GET /api/data</div>
            <p>List all stored data</p>
        </div>
        
        <div class="endpoint">
            <div class="method">POST /api/data</div>
            <p>Store new data (JSON: {"key": "...", "value": "..."})</p>
        </div>
        
        <div class="endpoint">
            <div class="method">POST /mcp</div>
            <p>MCP JSON-RPC endpoint</p>
            <pre>curl -X POST http://localhost:3000/mcp \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":"1","method":"tools/list","params":{}}'</pre>
        </div>
        
        <div class="endpoint">
            <div class="method">GET /mcp/capabilities</div>
            <p>MCP server capabilities</p>
        </div>
        
        <div class="endpoint">
            <div class="method">GET /mcp/sse</div>
            <p>Server-Sent Events stream for real-time updates</p>
            <pre>curl http://localhost:3000/mcp/sse</pre>
        </div>
        
        <div class="endpoint">
            <div class="method">WebSocket ws://localhost:3000/mcp/ws</div>
            <p>WebSocket endpoint for full-duplex MCP communication</p>
        </div>
        
        <div class="endpoint">
            <div class="method">GET /mcp/health</div>
            <p>Health check endpoint for monitoring</p>
        </div>
        
        <div class="endpoint">
            <div class="method">GET /mcp/metrics</div>
            <p>Server metrics and performance data</p>
        </div>
        
        <h2>🛠 Try It Out</h2>
        <p>This server demonstrates the "bring your own server" pattern where you start with a regular Axum application and add MCP capabilities seamlessly.</p>
        
        <p>The same server handles both traditional REST API calls and MCP protocol communication over HTTP, SSE, and WebSockets!</p>
        
        <h3>Example MCP Tools Available:</h3>
        <ul>
            <li><code>store_data</code> - Store key-value pairs</li>
            <li><code>get_data</code> - Retrieve values by key</li>
            <li><code>list_keys</code> - List all stored keys</li>
            <li><code>get_stats</code> - Get server statistics</li>
            <li><code>clear_data</code> - Clear all data</li>
        </ul>
    </div>
</body>
</html>"#;

    Html(html)
}

/// REST API endpoint to get server status
async fn api_status(State(state): State<AppState>) -> impl IntoResponse {
    let count = state.mcp_server.request_count.load(Ordering::Relaxed);
    let data_count = state.mcp_server.shared_data.read().await.len();

    let response = ApiResponse {
        success: true,
        data: serde_json::json!({
            "server": "TurboMCP HTTP Example",
            "version": "1.0.0",
            "requests_handled": count,
            "items_stored": data_count,
            "uptime": "N/A" // Could add actual uptime tracking
        }),
        message: "Server is running".to_string(),
    };

    Json(response)
}

/// REST API endpoint to list all data
async fn api_list_data(State(state): State<AppState>) -> impl IntoResponse {
    let data = state.mcp_server.shared_data.read().await;
    let items: HashMap<String, String> = data.clone();

    let response = ApiResponse {
        success: true,
        data: items,
        message: format!("Found {} items", data.len()),
    };

    Json(response)
}

/// REST API endpoint to store data
async fn api_store_data(
    State(state): State<AppState>,
    Json(request): Json<StoreRequest>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<Value>>)> {
    if request.key.is_empty() {
        let error_response = ApiResponse {
            success: false,
            data: Value::Null,
            message: "Key cannot be empty".to_string(),
        };
        return Err((StatusCode::BAD_REQUEST, Json(error_response)));
    }

    let mut data = state.mcp_server.shared_data.write().await;
    let old_value = data.insert(request.key.clone(), request.value.clone());

    let message = match old_value {
        Some(ref old) => format!(
            "Updated '{}': '{}' -> '{}'",
            request.key, old, request.value
        ),
        None => format!("Stored new entry '{}': '{}'", request.key, request.value),
    };

    let response = ApiResponse {
        success: true,
        data: serde_json::json!({
            "key": request.key,
            "value": request.value,
            "was_update": old_value.is_some()
        }),
        message,
    };

    Ok(Json(response))
}

/// REST API endpoint to get data by key
async fn api_get_data(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, Json<ApiResponse<Value>>)> {
    let data = state.mcp_server.shared_data.read().await;

    match data.get(&key) {
        Some(value) => {
            let response = ApiResponse {
                success: true,
                data: serde_json::json!({
                    "key": key,
                    "value": value
                }),
                message: format!("Found value for key '{}'", key),
            };
            Ok(Json(response))
        }
        None => {
            let error_response = ApiResponse {
                success: false,
                data: Value::Null,
                message: format!("Key '{}' not found", key),
            };
            Err((StatusCode::NOT_FOUND, Json(error_response)))
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for observability
    tracing_subscriber::fmt()
        .with_target(false)
        .with_thread_ids(true)
        .with_level(true)
        .init();

    // Check if HTTP feature is enabled
    #[cfg(not(feature = "http"))]
    {
        eprintln!("❌ HTTP feature not enabled.");
        eprintln!("   Run with: cargo run --example 10_http_server --features http");
        std::process::exit(1);
    }

    #[cfg(feature = "http")]
    {
        info!("🚀 Starting TurboMCP HTTP Server Example");

        // Create our application
        let (app, _mcp_server) = create_app();

        // Server configuration
        let port = std::env::var("PORT")
            .unwrap_or_else(|_| "3000".to_string())
            .parse::<u16>()
            .unwrap_or(3000);

        let addr = format!("0.0.0.0:{}", port);

        info!("🌍 Server starting on http://{}", addr);
        info!("📊 Features enabled:");
        info!("   ✅ HTTP REST API at /api/*");
        info!("   ✅ MCP over HTTP at /mcp");
        info!("   ✅ Server-Sent Events at /mcp/sse");
        info!("   ✅ WebSocket support at /mcp/ws");
        info!("   ✅ Health checks at /mcp/health");
        info!("   ✅ Metrics at /mcp/metrics");
        info!("");
        info!("🔗 Quick test commands:");
        info!("   curl http://localhost:{}/", port);
        info!("   curl http://localhost:{}/api/status", port);
        info!("   curl http://localhost:{}/mcp/capabilities", port);
        info!("");
        info!(
            "📘 Open http://localhost:{} in your browser for full documentation",
            port
        );

        // Bind to the address
        let listener = TcpListener::bind(&addr).await?;

        // Start the server
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal())
            .await?;

        info!("🛑 Server shutting down gracefully");
    }

    Ok(())
}

/// Graceful shutdown signal handling
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            warn!("Received Ctrl+C signal");
        },
        _ = terminate => {
            warn!("Received terminate signal");
        },
    }
}
