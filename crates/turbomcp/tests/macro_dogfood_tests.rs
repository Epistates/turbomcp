//! Dogfooding tests - Using our own macro system for testing
//! This ensures our macros work for all common scenarios

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use turbomcp::{McpErrorConstructors, prompt, resource, server, tool};

// Basic calculator tests removed - covered comprehensively in macro_end_to_end_tests.rs

/// Test server with complex parameter types
#[cfg(test)]
mod complex_parameter_tests {
    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct User {
        id: u32,
        name: String,
        email: String,
        active: bool,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct SearchOptions {
        query: String,
        limit: Option<usize>,
        offset: Option<usize>,
        sort_by: Option<String>,
    }

    #[derive(Clone)]
    struct DataServer {
        users: Arc<Mutex<HashMap<u32, User>>>,
    }

    #[server]
    impl DataServer {
        fn new() -> Self {
            let mut users = HashMap::new();
            users.insert(
                1,
                User {
                    id: 1,
                    name: "Alice".to_string(),
                    email: "alice@example.com".to_string(),
                    active: true,
                },
            );
            users.insert(
                2,
                User {
                    id: 2,
                    name: "Bob".to_string(),
                    email: "bob@example.com".to_string(),
                    active: false,
                },
            );

            Self {
                users: Arc::new(Mutex::new(users)),
            }
        }

        #[tool("Create a new user")]
        async fn create_user(&self, user: User) -> turbomcp::McpResult<u32> {
            let mut users = self.users.lock().unwrap();
            if users.contains_key(&user.id) {
                return Err(turbomcp::McpError::tool(format!(
                    "User with ID {} already exists",
                    user.id
                )));
            }
            let id = user.id;
            users.insert(id, user);
            Ok(id)
        }

        #[tool("Get user by ID")]
        async fn get_user(&self, id: u32) -> turbomcp::McpResult<Option<User>> {
            let users = self.users.lock().unwrap();
            Ok(users.get(&id).cloned())
        }

        #[tool("Update user")]
        async fn update_user(
            &self,
            id: u32,
            name: Option<String>,
            email: Option<String>,
            active: Option<bool>,
        ) -> turbomcp::McpResult<User> {
            let mut users = self.users.lock().unwrap();
            let user = users
                .get_mut(&id)
                .ok_or_else(|| turbomcp::McpError::tool(format!("User {id} not found")))?;

            if let Some(n) = name {
                user.name = n;
            }
            if let Some(e) = email {
                user.email = e;
            }
            if let Some(a) = active {
                user.active = a;
            }

            Ok(user.clone())
        }

        #[tool("Search users")]
        async fn search_users(&self, options: SearchOptions) -> turbomcp::McpResult<Vec<User>> {
            let users = self.users.lock().unwrap();
            let mut results: Vec<User> = users
                .values()
                .filter(|u| {
                    u.name
                        .to_lowercase()
                        .contains(&options.query.to_lowercase())
                })
                .cloned()
                .collect();

            // Apply sorting
            if let Some(sort_field) = options.sort_by {
                match sort_field.as_str() {
                    "name" => results.sort_by(|a, b| a.name.cmp(&b.name)),
                    "id" => results.sort_by_key(|u| u.id),
                    _ => {}
                }
            }

            // Apply pagination
            let offset = options.offset.unwrap_or(0);
            let limit = options.limit.unwrap_or(results.len());

            Ok(results.into_iter().skip(offset).take(limit).collect())
        }

        #[tool("Delete user")]
        async fn delete_user(&self, id: u32) -> turbomcp::McpResult<bool> {
            let mut users = self.users.lock().unwrap();
            Ok(users.remove(&id).is_some())
        }
    }

    #[tokio::test]
    async fn test_user_crud_operations() {
        let server = DataServer::new();

        // Test get existing user
        let user = server.get_user(1).await.unwrap();
        assert!(user.is_some());
        assert_eq!(user.unwrap().name, "Alice");

        // Test create new user
        let new_user = User {
            id: 3,
            name: "Charlie".to_string(),
            email: "charlie@example.com".to_string(),
            active: true,
        };

        let id = server.create_user(new_user.clone()).await.unwrap();
        assert_eq!(id, 3);

        // Test duplicate creation fails
        let result = server.create_user(new_user).await;
        assert!(result.is_err());

        // Test update user
        let updated = server
            .update_user(3, Some("Charles".to_string()), None, Some(false))
            .await
            .unwrap();
        assert_eq!(updated.name, "Charles");
        assert!(!updated.active);
        assert_eq!(updated.email, "charlie@example.com"); // Unchanged

        // Test search
        let search_opts = SearchOptions {
            query: "a".to_string(), // Matches "Alice" and "Charles"
            limit: Some(10),
            offset: None,
            sort_by: Some("name".to_string()),
        };

        let results = server.search_users(search_opts).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].name, "Alice");
        assert_eq!(results[1].name, "Charles");

        // Test delete
        let deleted = server.delete_user(3).await.unwrap();
        assert!(deleted);

        let user = server.get_user(3).await.unwrap();
        assert!(user.is_none());
    }
}

/// Test server with resources and prompts
#[cfg(test)]
mod resource_prompt_tests {
    use super::*;

    #[derive(Clone)]
    struct ContentServer {
        documents: Arc<Mutex<HashMap<String, String>>>,
        #[allow(dead_code)]
        templates: HashMap<String, String>,
    }

    #[server(
        name = "ContentServer",
        version = "2.0.0",
        description = "Content management server"
    )]
    impl ContentServer {
        fn new() -> Self {
            let mut documents = HashMap::new();
            documents.insert(
                "readme".to_string(),
                "# Welcome\nThis is the readme.".to_string(),
            );
            documents.insert(
                "config".to_string(),
                "port=8080\nhost=localhost".to_string(),
            );

            let mut templates = HashMap::new();
            templates.insert("greeting".to_string(), "Hello, {name}!".to_string());
            templates.insert("error".to_string(), "Error: {message}".to_string());

            Self {
                documents: Arc::new(Mutex::new(documents)),
                templates,
            }
        }

        #[tool("Save document")]
        async fn save_document(
            &self,
            name: String,
            content: String,
        ) -> turbomcp::McpResult<String> {
            let mut docs = self.documents.lock().unwrap();
            docs.insert(name.clone(), content);
            Ok(format!("Document '{name}' saved"))
        }

        #[resource("doc://{name}")]
        async fn get_document(&self, uri: String) -> turbomcp::McpResult<String> {
            let name = uri.strip_prefix("doc://").unwrap_or(&uri);
            let docs = self.documents.lock().unwrap();
            docs.get(name)
                .cloned()
                .ok_or_else(|| turbomcp::McpError::tool(format!("Document '{name}' not found")))
        }

        #[resource("list://documents")]
        async fn list_documents(&self, _uri: String) -> turbomcp::McpResult<String> {
            let docs = self.documents.lock().unwrap();
            let list: Vec<String> = docs.keys().cloned().collect();
            Ok(list.join("\n"))
        }

        #[prompt("Generate content from template")]
        async fn generate_content(
            &self,
            _ctx: turbomcp::Context,
            args: Option<serde_json::Value>,
        ) -> turbomcp::McpResult<String> {
            let template_name = args
                .as_ref()
                .and_then(|v| v.get("template"))
                .and_then(|v| v.as_str())
                .unwrap_or("greeting");

            let template = self.templates.get(template_name).ok_or_else(|| {
                turbomcp::McpError::tool(format!("Template '{template_name}' not found"))
            })?;

            let mut result = template.clone();

            if let Some(params) = args.and_then(|v| v.get("params").cloned())
                && let Some(obj) = params.as_object()
            {
                for (key, value) in obj {
                    let placeholder = format!("{{{key}}}");
                    let replacement = value.as_str().unwrap_or("");
                    result = result.replace(&placeholder, replacement);
                }
            }

            Ok(result)
        }

        #[prompt("Suggest document improvements")]
        async fn suggest_improvements(
            &self,
            _ctx: turbomcp::Context,
            args: Option<serde_json::Value>,
        ) -> turbomcp::McpResult<String> {
            let doc_name = args
                .as_ref()
                .and_then(|v| v.get("document"))
                .and_then(|v| v.as_str())
                .unwrap_or("readme");

            let docs = self.documents.lock().unwrap();
            let content = docs.get(doc_name).ok_or_else(|| {
                turbomcp::McpError::tool(format!("Document '{doc_name}' not found"))
            })?;

            // Simple suggestions based on content
            let mut suggestions = Vec::new();

            if !content.contains("#") {
                suggestions.push("Add headers for better structure");
            }
            if content.len() < 100 {
                suggestions.push("Consider adding more detail");
            }
            if !content.contains("http") && !content.contains("www") {
                suggestions.push("Add links to relevant resources");
            }

            if suggestions.is_empty() {
                Ok("Document looks good!".to_string())
            } else {
                Ok(format!("Suggestions:\n- {}", suggestions.join("\n- ")))
            }
        }
    }

    #[tokio::test]
    async fn test_content_server_operations() {
        let server = ContentServer::new();

        // Test saving a document
        let result = server
            .save_document("test".to_string(), "Test content".to_string())
            .await
            .unwrap();
        assert_eq!(result, "Document 'test' saved");

        // Test retrieving document via resource
        let content = server
            .get_document("doc://readme".to_string())
            .await
            .unwrap();
        assert!(content.contains("Welcome"));

        // Test listing documents
        let list = server
            .list_documents("list://documents".to_string())
            .await
            .unwrap();
        assert!(list.contains("readme"));
        assert!(list.contains("config"));
        assert!(list.contains("test"));

        // Test document not found
        let result = server.get_document("doc://nonexistent".to_string()).await;
        assert!(result.is_err());

        // Prompt testing removed - covered comprehensively in integration_comprehensive.rs and mcp_protocol_compliance_comprehensive.rs
        // Non-MCP compliant Context::new() usage violates protocol specification
    }
}

/// Test server with state management and concurrency
#[cfg(test)]
mod concurrency_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone)]
    struct ConcurrentServer {
        counter: Arc<AtomicUsize>,
        events: Arc<Mutex<Vec<String>>>,
    }

    #[server]
    impl ConcurrentServer {
        fn new() -> Self {
            Self {
                counter: Arc::new(AtomicUsize::new(0)),
                events: Arc::new(Mutex::new(Vec::new())),
            }
        }

        #[tool("Increment counter")]
        async fn increment(&self) -> turbomcp::McpResult<usize> {
            let value = self.counter.fetch_add(1, Ordering::SeqCst) + 1;
            self.log_event(format!("Incremented to {value}"));
            Ok(value)
        }

        #[tool("Decrement counter")]
        async fn decrement(&self) -> turbomcp::McpResult<usize> {
            let current = self.counter.load(Ordering::SeqCst);
            if current == 0 {
                return Err(turbomcp::McpError::tool(
                    "Counter cannot go below zero".to_string(),
                ));
            }
            let value = self.counter.fetch_sub(1, Ordering::SeqCst) - 1;
            self.log_event(format!("Decremented to {value}"));
            Ok(value)
        }

        #[tool("Get counter value")]
        async fn get_value(&self) -> turbomcp::McpResult<usize> {
            Ok(self.counter.load(Ordering::SeqCst))
        }

        #[tool("Reset counter")]
        async fn reset(&self) -> turbomcp::McpResult<usize> {
            let old = self.counter.swap(0, Ordering::SeqCst);
            self.log_event(format!("Reset from {old}"));
            Ok(old)
        }

        #[resource("events://log")]
        async fn get_events(&self, _uri: String) -> turbomcp::McpResult<String> {
            let events = self.events.lock().unwrap();
            Ok(events.join("\n"))
        }

        fn log_event(&self, event: String) {
            let mut events = self.events.lock().unwrap();
            events.push(format!(
                "[{}] {}",
                chrono::Utc::now().format("%H:%M:%S"),
                event
            ));
            // Keep only last 100 events
            if events.len() > 100 {
                let drain_end = events.len().saturating_sub(100);
                events.drain(0..drain_end);
            }
        }
    }

    #[tokio::test]
    async fn test_concurrent_operations() {
        let server = Arc::new(ConcurrentServer::new());

        // Test sequential operations
        assert_eq!(server.increment().await.unwrap(), 1);
        assert_eq!(server.increment().await.unwrap(), 2);
        assert_eq!(server.get_value().await.unwrap(), 2);

        // Test concurrent increments
        let mut handles = Vec::new();
        for _ in 0..10 {
            let server_clone = Arc::clone(&server);
            handles.push(tokio::spawn(async move { server_clone.increment().await }));
        }

        for handle in handles {
            handle.await.unwrap().unwrap();
        }

        assert_eq!(server.get_value().await.unwrap(), 12);

        // Test reset
        let old = server.reset().await.unwrap();
        assert_eq!(old, 12);
        assert_eq!(server.get_value().await.unwrap(), 0);

        // Test decrement error
        let result = server.decrement().await;
        assert!(result.is_err());

        // Check events were logged
        let events = server.get_events("events://log".to_string()).await.unwrap();
        assert!(events.contains("Incremented"));
        assert!(events.contains("Reset from 12"));
    }
}

/// Test error handling and edge cases
#[cfg(test)]
mod error_handling_tests {
    use super::*;

    #[derive(Clone)]
    struct ValidationServer;

    #[server]
    impl ValidationServer {
        #[tool("Validate email")]
        async fn validate_email(&self, email: String) -> turbomcp::McpResult<bool> {
            if email.is_empty() {
                return Err(turbomcp::McpError::tool(
                    "Email cannot be empty".to_string(),
                ));
            }
            if !email.contains('@') {
                return Err(turbomcp::McpError::tool("Invalid email format".to_string()));
            }
            if email.len() > 254 {
                return Err(turbomcp::McpError::tool("Email too long".to_string()));
            }
            Ok(true)
        }

        #[tool("Parse integer")]
        async fn parse_int(&self, value: String) -> turbomcp::McpResult<i32> {
            value
                .parse::<i32>()
                .map_err(|e| turbomcp::McpError::tool(format!("Failed to parse integer: {e}")))
        }

        #[tool("Safe divide")]
        async fn safe_divide(&self, a: f64, b: f64) -> turbomcp::McpResult<f64> {
            if b == 0.0 {
                Err(turbomcp::McpError::tool("Division by zero".to_string()))
            } else if !a.is_finite() || !b.is_finite() {
                Err(turbomcp::McpError::tool(
                    "Invalid number (infinite or NaN)".to_string(),
                ))
            } else {
                Ok(a / b)
            }
        }

        #[resource("validate://{type}/{value}")]
        async fn validate_resource(&self, uri: String) -> turbomcp::McpResult<String> {
            let parts: Vec<&str> = uri
                .strip_prefix("validate://")
                .unwrap_or(&uri)
                .splitn(2, '/')
                .collect();

            if parts.len() != 2 {
                return Err(turbomcp::McpError::tool("Invalid URI format".to_string()));
            }

            match parts[0] {
                "email" => {
                    self.validate_email(parts[1].to_string()).await?;
                    Ok("Valid email".to_string())
                }
                "int" => {
                    let value = self.parse_int(parts[1].to_string()).await?;
                    Ok(format!("Valid integer: {value}"))
                }
                _ => Err(turbomcp::McpError::tool(format!(
                    "Unknown validation type: {}",
                    parts[0]
                ))),
            }
        }
    }

    #[tokio::test]
    async fn test_validation_server() {
        let server = ValidationServer;

        // Test email validation
        assert!(
            server
                .validate_email("test@example.com".to_string())
                .await
                .unwrap()
        );

        assert!(server.validate_email("".to_string()).await.is_err());
        assert!(
            server
                .validate_email("notanemail".to_string())
                .await
                .is_err()
        );

        // Test integer parsing
        assert_eq!(server.parse_int("42".to_string()).await.unwrap(), 42);
        assert_eq!(server.parse_int("-123".to_string()).await.unwrap(), -123);
        assert!(server.parse_int("not a number".to_string()).await.is_err());
        assert!(server.parse_int("1.5".to_string()).await.is_err());

        // Test safe division
        assert_eq!(server.safe_divide(10.0, 2.0).await.unwrap(), 5.0);
        assert!(server.safe_divide(10.0, 0.0).await.is_err());
        assert!(server.safe_divide(f64::INFINITY, 2.0).await.is_err());
        assert!(server.safe_divide(2.0, f64::NAN).await.is_err());

        // Test validation resource
        let result = server
            .validate_resource("validate://email/test@example.com".to_string())
            .await;
        assert_eq!(result.unwrap(), "Valid email");

        let result = server
            .validate_resource("validate://int/42".to_string())
            .await;
        assert_eq!(result.unwrap(), "Valid integer: 42");

        let result = server
            .validate_resource("validate://unknown/value".to_string())
            .await;
        assert!(result.is_err());
    }
}
