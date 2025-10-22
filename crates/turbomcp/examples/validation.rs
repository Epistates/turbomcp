//! # Validation - Input Validation Patterns
//!
//! Demonstrates validation approaches in TurboMCP tools.
//!
//! Run with: `cargo run --example validation`

use turbomcp::prelude::*;

#[derive(Clone)]
struct ValidationServer;

#[turbomcp::server(name = "validation-demo", version = "1.0.0", transports = ["stdio"])]
impl ValidationServer {
    /// Basic validation with simple checks
    #[tool("Create user with age validation")]
    async fn create_user(&self, name: String, age: i32) -> McpResult<String> {
        // Simple validation
        if name.is_empty() {
            return Err(McpError::invalid_request("Name cannot be empty"));
        }

        if age < 0 {
            return Err(McpError::invalid_request("Age must be positive"));
        }

        if age < 18 {
            return Err(McpError::invalid_request("User must be 18 or older"));
        }

        if age > 120 {
            return Err(McpError::invalid_request("Age must be 120 or less"));
        }

        Ok(format!("✅ User created: {} (age {})", name, age))
    }

    /// Email validation with format checking
    #[tool("Subscribe with email validation")]
    async fn subscribe(&self, email: String) -> McpResult<String> {
        // Simple email format check
        if !email.contains('@') || !email.contains('.') {
            return Err(McpError::invalid_request(
                "Invalid email format (must contain @ and .)",
            ));
        }

        let parts: Vec<&str> = email.split('@').collect();
        if parts.len() != 2 {
            return Err(McpError::invalid_request("Email must have exactly one @"));
        }

        let domain = parts[1];
        if !domain.contains('.') {
            return Err(McpError::invalid_request(
                "Email domain must contain a period",
            ));
        }

        Ok(format!("✅ Subscribed: {}", email))
    }

    /// Range validation for numbers
    #[tool("Set temperature (0.0-1.0)")]
    async fn set_temperature(&self, temp: f64) -> McpResult<String> {
        if !(0.0..=1.0).contains(&temp) {
            return Err(McpError::invalid_request(
                "Temperature must be between 0.0 and 1.0",
            ));
        }

        Ok(format!("✅ Temperature set to {:.2}", temp))
    }

    /// String length validation
    #[tool("Create username (3-20 characters)")]
    async fn create_username(&self, username: String) -> McpResult<String> {
        let len = username.len();

        if len < 3 {
            return Err(McpError::invalid_request(
                "Username must be at least 3 characters",
            ));
        }

        if len > 20 {
            return Err(McpError::invalid_request(
                "Username must be 20 characters or less",
            ));
        }

        // Check alphanumeric
        if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(McpError::invalid_request(
                "Username can only contain letters, numbers, and underscores",
            ));
        }

        Ok(format!("✅ Username created: {}", username))
    }

    /// Multiple field validation
    #[tool("Register account with multiple validations")]
    async fn register(
        &self,
        username: String,
        email: String,
        age: i32,
        terms_accepted: bool,
    ) -> McpResult<String> {
        // Validate username
        if username.len() < 3 || username.len() > 20 {
            return Err(McpError::invalid_request(
                "Username must be 3-20 characters",
            ));
        }

        // Validate email
        if !email.contains('@') {
            return Err(McpError::invalid_request("Invalid email"));
        }

        // Validate age
        if age < 18 {
            return Err(McpError::invalid_request("Must be 18 or older"));
        }

        // Validate terms
        if !terms_accepted {
            return Err(McpError::invalid_request("Must accept terms of service"));
        }

        Ok(format!(
            "✅ Account registered:\n  Username: {}\n  Email: {}\n  Age: {}",
            username, email, age
        ))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    ValidationServer.run_stdio().await?;
    Ok(())
}
