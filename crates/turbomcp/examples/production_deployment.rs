//! Deployment Patterns Example
//!
//! This example shows real-world deployment patterns using TurboMCP's progressive enhancement.
//! It demonstrates how the same server can be deployed in different environments with
//! different transport configurations.

use turbomcp::prelude::*;

#[derive(Debug, Clone)]
enum Environment {
    Development,
    Testing,
    Staging,
    Production,
}

impl Environment {
    fn from_env() -> Self {
        match std::env::var("ENVIRONMENT").as_deref() {
            Ok("dev") | Ok("development") => Self::Development,
            Ok("test") | Ok("testing") => Self::Testing,
            Ok("stage") | Ok("staging") => Self::Staging,
            Ok("prod") | Ok("production") => Self::Production,
            _ => {
                eprintln!("No ENVIRONMENT set, defaulting to development");
                Self::Development
            }
        }
    }
}

#[derive(Clone)]
struct ApiGateway {
    environment: Environment,
    version: String,
}

#[server]
impl ApiGateway {
    #[tool("Get service status")]
    async fn status(&self) -> McpResult<serde_json::Value> {
        Ok(serde_json::json!({
            "service": "api-gateway",
            "version": self.version,
            "environment": format!("{:?}", self.environment),
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339()
        }))
    }

    #[tool("Get deployment info")]
    async fn deployment_info(&self) -> McpResult<serde_json::Value> {
        let transport = std::env::var("TRANSPORT").unwrap_or_else(|_| "stdio".to_string());

        Ok(serde_json::json!({
            "environment": format!("{:?}", self.environment),
            "transport": transport,
            "host": std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_string()),
            "pid": std::process::id(),
            "features": {
                "tcp": cfg!(feature = "tcp"),
                "unix": cfg!(all(feature = "unix", unix)),
                "http": cfg!(feature = "http"),
                "websocket": cfg!(feature = "websocket")
            }
        }))
    }

    #[tool("Simulate API request")]
    async fn api_request(&self, endpoint: String, method: Option<String>) -> McpResult<String> {
        let method = method.unwrap_or_else(|| "GET".to_string());
        let latency = match self.environment {
            Environment::Development => 10, // Fast local development
            Environment::Testing => 25,     // Controlled test environment
            Environment::Staging => 50,     // Similar to production
            Environment::Production => 100, // Real network latency
        };

        // Simulate network latency
        tokio::time::sleep(tokio::time::Duration::from_millis(latency)).await;

        Ok(format!(
            "{} {} -> 200 OK ({}ms, env: {:?})",
            method, endpoint, latency, self.environment
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing with environment-specific configuration
    let env = Environment::from_env();
    tracing_subscriber::fmt::init();

    let server = ApiGateway {
        environment: env,
        version: "1.0.0".to_string(),
    };

    println!("🚀 API Gateway - Deployment Pattern Example");
    println!("Environment: {:?}", server.environment);

    // Environment-specific transport configuration
    match server.environment {
        Environment::Development => {
            // Development: Use STDIO for easy debugging with IDE integration
            println!("🛠️  Development mode: Using STDIO transport for easy debugging");
            println!("   IDE integration works seamlessly");
            server.run_stdio().await?;
        }

        Environment::Testing => {
            // Testing: Use Unix sockets for isolated test environments
            let socket_path = "/tmp/api-gateway-test.sock";
            println!("🧪 Testing mode: Using Unix socket at {}", socket_path);
            println!("   Isolated testing environment");

            #[cfg(all(feature = "unix", unix))]
            {
                let _ = std::fs::remove_file(socket_path);
                server.run_unix(socket_path).await?;
            }
            #[cfg(not(all(feature = "unix", unix)))]
            {
                println!("   Unix sockets not available, falling back to STDIO");
                server.run_stdio().await?;
            }
        }

        Environment::Staging => {
            // Staging: Use TCP on non-standard port for staging tests
            let port = std::env::var("STAGING_PORT")
                .unwrap_or_else(|_| "9090".to_string())
                .parse::<u16>()
                .unwrap_or(9090);

            println!("🎭 Staging mode: Using TCP on port {}", port);
            println!("   Production-like environment for final testing");

            #[cfg(feature = "tcp")]
            {
                server.run_tcp(format!("0.0.0.0:{}", port)).await?;
            }
            #[cfg(not(feature = "tcp"))]
            {
                println!("   TCP transport not available, falling back to STDIO");
                server.run_stdio().await?;
            }
        }

        Environment::Production => {
            // Production: Use TCP with standard port and proper error handling
            let port = std::env::var("PORT")
                .unwrap_or_else(|_| "8080".to_string())
                .parse::<u16>()
                .map_err(|_| "Invalid PORT environment variable")?;

            let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
            let bind_addr = format!("{}:{}", host, port);

            println!("🏭 Production mode: Using TCP on {}", bind_addr);
            println!("   High-performance network transport");
            println!("   Ready for load balancer integration");

            #[cfg(feature = "tcp")]
            {
                server.run_tcp(bind_addr).await?;
            }
            #[cfg(not(feature = "tcp"))]
            {
                return Err(
                    "TCP transport required for production deployment but not available".into(),
                );
            }
        }
    }

    Ok(())
}
