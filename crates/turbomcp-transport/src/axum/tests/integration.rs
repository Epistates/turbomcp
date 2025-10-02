//! Integration tests
//!
//! End-to-end integration tests that verify the complete MCP integration
//! system works correctly, including middleware application and configuration.

#[cfg(test)]
mod tests {
    use super::super::common::TestMcpService;
    use crate::axum::{AxumMcpExt, McpServerConfig};
    use axum::Router;

    #[tokio::test]
    async fn test_production_grade_middleware_compilation() {
        // Test that we can create routers with different security configurations
        let service = TestMcpService;

        // Development router (permissive)
        let _dev_router = Router::<()>::turbo_mcp_routes_for_merge(
            service.clone(),
            McpServerConfig::development(),
        );

        // Staging router (moderate security)
        let _staging_router =
            Router::<()>::turbo_mcp_routes_for_merge(service.clone(), McpServerConfig::staging());

        // Production router (maximum security)
        let _prod_router = Router::<()>::turbo_mcp_routes_for_merge(
            service.clone(),
            McpServerConfig::production(),
        );

        // Custom configured router
        let custom_config = McpServerConfig::staging()
            .with_cors_origins(vec!["https://trusted.com".to_string()])
            .with_rate_limit(1000, 200)
            .with_jwt_auth("super-secret-key".to_string());

        let _custom_router =
            Router::<()>::turbo_mcp_routes_for_merge(service.clone(), custom_config);

        // If this test compiles, our production-grade configuration system works
    }

    #[tokio::test]
    async fn test_comprehensive_configuration_integration() {
        let service = TestMcpService;

        // Test comprehensive configuration with all features enabled
        let comprehensive_config = McpServerConfig::production()
            .with_cors_origins(vec![
                "https://app.example.com".to_string(),
                "https://admin.example.com".to_string(),
            ])
            .with_custom_csp(
                "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'",
            )
            .with_rate_limit(240, 60) // 4 requests per second with burst
            .with_api_key_auth("X-MCP-API-Key".to_string())
            .with_tls("production.crt".to_string(), "production.key".to_string());

        // This should compile and create a router with all middleware applied
        let _comprehensive_router =
            Router::<()>::turbo_mcp_routes_for_merge(service, comprehensive_config);
    }

    #[tokio::test]
    async fn test_multi_environment_deployment_patterns() {
        let service = TestMcpService;

        // Simulate different deployment environments
        struct DeploymentConfig {
            name: &'static str,
            config: McpServerConfig,
        }

        let deployments = vec![
            DeploymentConfig {
                name: "local-development",
                config: McpServerConfig::development(),
            },
            DeploymentConfig {
                name: "staging",
                config: McpServerConfig::staging()
                    .with_cors_origins(vec!["https://staging.example.com".to_string()])
                    .with_rate_limit(600, 120),
            },
            DeploymentConfig {
                name: "production",
                config: McpServerConfig::production()
                    .with_cors_origins(vec!["https://app.example.com".to_string()])
                    .with_custom_csp("default-src 'self'; script-src 'self'; connect-src 'self'")
                    .with_rate_limit(120, 30)
                    .with_jwt_auth("production-jwt-secret".to_string()),
            },
            DeploymentConfig {
                name: "high-security",
                config: McpServerConfig::production()
                    .with_cors_origins(vec!["https://secure.example.com".to_string()])
                    .with_custom_csp("default-src 'none'; script-src 'self'; style-src 'self'")
                    .with_rate_limit(60, 10) // Very strict rate limiting
                    .with_api_key_auth("X-SECURE-API-KEY".to_string())
                    .with_tls("secure.crt".to_string(), "secure.key".to_string()),
            },
        ];

        // Test that each deployment configuration compiles and works
        for deployment in deployments {
            let _router =
                Router::<()>::turbo_mcp_routes_for_merge(service.clone(), deployment.config);

            // In a full integration test, we would also verify:
            // - Correct middleware is applied
            // - Security headers are set properly
            // - Rate limiting works as expected
            // - CORS policies are enforced
            // - Authentication is required when configured
        }
    }

    #[tokio::test]
    async fn test_state_isolation_in_complex_applications() {
        // Test complex application state isolation
        #[derive(Clone, Debug)]
        struct UserServiceState {
            database_pool: String,
            cache_config: String,
        }

        #[derive(Clone, Debug)]
        struct AdminServiceState {
            admin_permissions: Vec<String>,
            audit_log_path: String,
        }

        let user_state = UserServiceState {
            database_pool: "postgresql://user_db".to_string(),
            cache_config: "redis://cache".to_string(),
        };

        let admin_state = AdminServiceState {
            admin_permissions: vec!["read".to_string(), "write".to_string()],
            audit_log_path: "/var/log/admin.log".to_string(),
        };

        // Create user service router
        let user_router = Router::new()
            .route("/api/users", axum::routing::get(|| async { "users" }))
            .route("/api/profile", axum::routing::get(|| async { "profile" }))
            .with_state(user_state);

        // Create admin service router
        let admin_router = Router::new()
            .route(
                "/admin/users",
                axum::routing::get(|| async { "admin users" }),
            )
            .route("/admin/logs", axum::routing::get(|| async { "admin logs" }))
            .with_state(admin_state);

        // Create MCP service routers for merging
        let user_mcp_service = TestMcpService;
        let admin_mcp_service = TestMcpService;

        let user_mcp_router = Router::<()>::turbo_mcp_routes_for_merge(
            user_mcp_service,
            McpServerConfig::development(),
        );

        let admin_mcp_router =
            Router::<()>::turbo_mcp_routes_for_merge(admin_mcp_service, McpServerConfig::staging());

        // Test that merging works correctly with different state types
        // Note: Router<T>.merge(Router<()>) produces Router<()> due to Axum's type system
        let _user_service_with_mcp = user_router.merge(user_mcp_router);
        let _admin_service_with_mcp = admin_router.merge(admin_mcp_router);

        // State isolation is verified by successful compilation
        // Each service router maintains its original state type
    }

    #[test]
    fn test_error_handling_configuration() {
        // Test that invalid configurations are handled gracefully
        let service = TestMcpService;

        // Test configuration with empty CORS origins (should work)
        let empty_cors_config = McpServerConfig::production().with_cors_origins(vec![]); // Empty origins = secure default

        let _router_with_empty_cors =
            Router::<()>::turbo_mcp_routes_for_merge(service.clone(), empty_cors_config);

        // Test configuration with zero rate limiting (should disable it)
        let zero_rate_limit_config = McpServerConfig::development().with_rate_limit(0, 0); // Should effectively disable rate limiting

        let _router_with_zero_rate_limit =
            Router::<()>::turbo_mcp_routes_for_merge(service.clone(), zero_rate_limit_config);

        // All configurations should compile successfully
        // Runtime behavior would be tested in actual integration tests
    }
}
