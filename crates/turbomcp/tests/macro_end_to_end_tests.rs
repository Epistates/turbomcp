//! End-to-end tests using TurboMCP macros to dogfood our implementation
//! These tests verify that our macros work correctly for real-world scenarios

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use turbomcp_macros::{prompt, resource, server, tool};

/// Type alias for config validator function
type ConfigValidator = Box<dyn Fn(&str) -> bool + Send + Sync>;

// ============================================================================
// Test 1: Simple Calculator Server
// ============================================================================

#[derive(Clone)]
struct CalculatorServer {
    history: Arc<Mutex<Vec<String>>>,
}

#[server(
    name = "Calculator",
    version = "1.0.0",
    description = "A simple calculator server"
)]
impl CalculatorServer {
    #[tool("Add two numbers")]
    async fn add(&self, a: f64, b: f64) -> turbomcp::McpResult<f64> {
        let result = a + b;
        self.history
            .lock()
            .unwrap()
            .push(format!("{a} + {b} = {result}"));
        Ok(result)
    }

    #[tool("Subtract two numbers")]
    async fn subtract(&self, a: f64, b: f64) -> turbomcp::McpResult<f64> {
        let result = a - b;
        self.history
            .lock()
            .unwrap()
            .push(format!("{a} - {b} = {result}"));
        Ok(result)
    }

    #[tool("Multiply two numbers")]
    async fn multiply(&self, a: f64, b: f64) -> turbomcp::McpResult<f64> {
        let result = a * b;
        self.history
            .lock()
            .unwrap()
            .push(format!("{a} * {b} = {result}"));
        Ok(result)
    }

    #[tool("Divide two numbers")]
    async fn divide(&self, a: f64, b: f64) -> turbomcp::McpResult<f64> {
        if b == 0.0 {
            return Err(turbomcp::McpError::Tool("Division by zero".to_string()));
        }
        let result = a / b;
        self.history
            .lock()
            .unwrap()
            .push(format!("{a} / {b} = {result}"));
        Ok(result)
    }

    #[tool("Get calculation history")]
    async fn get_history(&self) -> turbomcp::McpResult<Vec<String>> {
        Ok(self.history.lock().unwrap().clone())
    }

    #[tool("Clear history")]
    async fn clear_history(&self) -> turbomcp::McpResult<String> {
        let mut history = self.history.lock().unwrap();
        let count = history.len();
        history.clear();
        Ok(format!("Cleared {count} entries"))
    }
}

#[tokio::test]
async fn test_calculator_server() {
    let server = CalculatorServer {
        history: Arc::new(Mutex::new(Vec::new())),
    };

    // Test basic operations
    assert_eq!(server.add(2.0, 3.0).await.unwrap(), 5.0);
    assert_eq!(server.subtract(10.0, 4.0).await.unwrap(), 6.0);
    assert_eq!(server.multiply(3.0, 7.0).await.unwrap(), 21.0);
    assert_eq!(server.divide(15.0, 3.0).await.unwrap(), 5.0);

    // Test division by zero
    assert!(server.divide(10.0, 0.0).await.is_err());

    // Test history
    let history = server.get_history().await.unwrap();
    assert_eq!(history.len(), 4);
    assert!(history[0].contains("2 + 3 = 5"));

    // Test clear history
    let result = server.clear_history().await.unwrap();
    assert_eq!(result, "Cleared 4 entries");
    assert_eq!(server.get_history().await.unwrap().len(), 0);
}

// ============================================================================
// Test 2: Database-like Server with CRUD Operations
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct User {
    id: usize,
    name: String,
    email: String,
    age: u32,
}

#[derive(Clone)]
struct UserDatabase {
    users: Arc<Mutex<HashMap<usize, User>>>,
    next_id: Arc<AtomicUsize>,
}

#[server(name = "UserDB", version = "2.0.0")]
impl UserDatabase {
    #[tool("Create a new user")]
    async fn create_user(
        &self,
        name: String,
        email: String,
        age: u32,
    ) -> turbomcp::McpResult<User> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let user = User {
            id,
            name,
            email,
            age,
        };

        self.users.lock().unwrap().insert(id, user.clone());
        Ok(user)
    }

    #[tool("Get user by ID")]
    async fn get_user(&self, id: usize) -> turbomcp::McpResult<User> {
        self.users
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| turbomcp::McpError::Tool(format!("User {id} not found")))
    }

    #[tool("Update user")]
    async fn update_user(
        &self,
        id: usize,
        name: Option<String>,
        email: Option<String>,
        age: Option<u32>,
    ) -> turbomcp::McpResult<User> {
        let mut users = self.users.lock().unwrap();
        let user = users
            .get_mut(&id)
            .ok_or_else(|| turbomcp::McpError::Tool(format!("User {id} not found")))?;

        if let Some(name) = name {
            user.name = name;
        }
        if let Some(email) = email {
            user.email = email;
        }
        if let Some(age) = age {
            user.age = age;
        }

        Ok(user.clone())
    }

    #[tool("Delete user")]
    async fn delete_user(&self, id: usize) -> turbomcp::McpResult<String> {
        self.users
            .lock()
            .unwrap()
            .remove(&id)
            .map(|user| format!("Deleted user: {}", user.name))
            .ok_or_else(|| turbomcp::McpError::Tool(format!("User {id} not found")))
    }

    #[tool("List all users")]
    async fn list_users(&self) -> turbomcp::McpResult<Vec<User>> {
        Ok(self.users.lock().unwrap().values().cloned().collect())
    }

    #[tool("Search users by name")]
    async fn search_users(&self, query: String) -> turbomcp::McpResult<Vec<User>> {
        let query_lower = query.to_lowercase();
        Ok(self
            .users
            .lock()
            .unwrap()
            .values()
            .filter(|u| u.name.to_lowercase().contains(&query_lower))
            .cloned()
            .collect())
    }
}

#[tokio::test]
async fn test_user_database() {
    let server = UserDatabase {
        users: Arc::new(Mutex::new(HashMap::new())),
        next_id: Arc::new(AtomicUsize::new(1)),
    };

    // Create users
    let user1 = server
        .create_user(
            "Alice Smith".to_string(),
            "alice@example.com".to_string(),
            30,
        )
        .await
        .unwrap();
    assert_eq!(user1.id, 1);
    assert_eq!(user1.name, "Alice Smith");

    let user2 = server
        .create_user("Bob Johnson".to_string(), "bob@example.com".to_string(), 25)
        .await
        .unwrap();
    assert_eq!(user2.id, 2);

    // Get user
    let fetched = server.get_user(1).await.unwrap();
    assert_eq!(fetched.name, "Alice Smith");

    // Update user
    let updated = server
        .update_user(
            1,
            None,
            Some("alice.smith@example.com".to_string()),
            Some(31),
        )
        .await
        .unwrap();
    assert_eq!(updated.email, "alice.smith@example.com");
    assert_eq!(updated.age, 31);

    // List users
    let all_users = server.list_users().await.unwrap();
    assert_eq!(all_users.len(), 2);

    // Search users
    let search_results = server.search_users("alice".to_string()).await.unwrap();
    assert_eq!(search_results.len(), 1);
    assert_eq!(search_results[0].name, "Alice Smith");

    // Delete user
    let delete_msg = server.delete_user(2).await.unwrap();
    assert!(delete_msg.contains("Bob Johnson"));

    // Verify deletion
    assert!(server.get_user(2).await.is_err());
    assert_eq!(server.list_users().await.unwrap().len(), 1);
}

// ============================================================================
// Test 3: Content Management Server with Resources and Prompts
// ============================================================================

#[derive(Clone)]
struct ContentServer {
    documents: Arc<Mutex<HashMap<String, String>>>,
    #[allow(dead_code)]
    templates: Arc<Mutex<HashMap<String, String>>>,
}

#[server(name = "ContentManager", version = "1.5.0")]
impl ContentServer {
    #[tool("Save document")]
    async fn save_document(&self, name: String, content: String) -> turbomcp::McpResult<String> {
        self.documents.lock().unwrap().insert(name.clone(), content);
        Ok(format!("Document '{name}' saved"))
    }

    #[resource("document://")]
    async fn get_document(&self, uri: String) -> turbomcp::McpResult<String> {
        let name = uri
            .strip_prefix("document://")
            .ok_or_else(|| turbomcp::McpError::Tool("Invalid URI format".to_string()))?;

        self.documents
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| turbomcp::McpError::Tool(format!("Document '{name}' not found")))
    }

    #[prompt("Generate a blog post")]
    async fn blog_post_prompt(
        &self,
        _ctx: turbomcp::Context,
        args: Option<serde_json::Value>,
    ) -> turbomcp::McpResult<String> {
        let (topic, style) = if let Some(args) = args {
            let topic = args
                .get("topic")
                .and_then(|v| v.as_str())
                .unwrap_or("technology");
            let style = args
                .get("style")
                .and_then(|v| v.as_str())
                .unwrap_or("technical");
            (topic.to_string(), style.to_string())
        } else {
            ("technology".to_string(), "technical".to_string())
        };

        Ok(format!(
            "Write a {style} style blog post about {topic}. Include an introduction, \
            3 main points with examples, and a conclusion."
        ))
    }

    #[prompt("Code review template")]
    async fn code_review_prompt(
        &self,
        _ctx: turbomcp::Context,
        args: Option<serde_json::Value>,
    ) -> turbomcp::McpResult<String> {
        let (language, areas) = if let Some(args) = args {
            let language = args
                .get("language")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let focus_areas = args
                .get("focus_areas")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            (language.to_string(), focus_areas.join(", "))
        } else {
            ("unknown".to_string(), "general".to_string())
        };

        Ok(format!(
            "Review the following {language} code, focusing on: {areas}. \
            Provide specific feedback on code quality, potential bugs, \
            and suggestions for improvement."
        ))
    }

    #[tool("List all documents")]
    async fn list_documents(&self) -> turbomcp::McpResult<Vec<String>> {
        Ok(self.documents.lock().unwrap().keys().cloned().collect())
    }

    #[tool("Delete document")]
    async fn delete_document(&self, name: String) -> turbomcp::McpResult<String> {
        self.documents
            .lock()
            .unwrap()
            .remove(&name)
            .map(|_| format!("Document '{name}' deleted"))
            .ok_or_else(|| turbomcp::McpError::Tool(format!("Document '{name}' not found")))
    }
}

#[tokio::test]
async fn test_content_server() {
    let server = ContentServer {
        documents: Arc::new(Mutex::new(HashMap::new())),
        templates: Arc::new(Mutex::new(HashMap::new())),
    };

    // Save documents
    let result = server
        .save_document(
            "readme.md".to_string(),
            "# Project README\nThis is a test project.".to_string(),
        )
        .await
        .unwrap();
    assert!(result.contains("saved"));

    server
        .save_document(
            "config.json".to_string(),
            r#"{"version": "1.0.0"}"#.to_string(),
        )
        .await
        .unwrap();

    // Get document via resource
    let content = server
        .get_document("document://readme.md".to_string())
        .await
        .unwrap();
    assert!(content.contains("Project README"));

    // List documents
    let docs = server.list_documents().await.unwrap();
    assert_eq!(docs.len(), 2);
    assert!(docs.contains(&"readme.md".to_string()));

    // Test prompts
    let blog_args = serde_json::json!({
        "topic": "Rust programming",
        "style": "technical"
    });
    let request_context = turbomcp::RequestContext::default();
    let handler_metadata = turbomcp::HandlerMetadata {
        name: "test".to_string(),
        handler_type: "prompt".to_string(),
        description: Some("test prompt".to_string()),
    };
    let ctx = turbomcp::Context::new(request_context, handler_metadata);
    let blog_prompt = server.blog_post_prompt(ctx, Some(blog_args)).await.unwrap();
    assert!(blog_prompt.contains("Rust programming"));
    assert!(blog_prompt.contains("technical"));

    let review_args = serde_json::json!({
        "language": "Python",
        "focus_areas": ["performance", "security"]
    });
    let request_context = turbomcp::RequestContext::default();
    let handler_metadata = turbomcp::HandlerMetadata {
        name: "test".to_string(),
        handler_type: "prompt".to_string(),
        description: Some("test prompt".to_string()),
    };
    let ctx = turbomcp::Context::new(request_context, handler_metadata);
    let review_prompt = server
        .code_review_prompt(ctx, Some(review_args))
        .await
        .unwrap();
    assert!(review_prompt.contains("Python"));
    assert!(review_prompt.contains("performance"));
    assert!(review_prompt.contains("security"));

    // Delete document
    let delete_result = server
        .delete_document("config.json".to_string())
        .await
        .unwrap();
    assert!(delete_result.contains("deleted"));
    assert_eq!(server.list_documents().await.unwrap().len(), 1);
}

// ============================================================================
// Test 4: Analytics Server with Complex State
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Metric {
    name: String,
    value: f64,
    timestamp: u64,
    tags: HashMap<String, String>,
}

#[derive(Clone)]
struct AnalyticsServer {
    metrics: Arc<Mutex<Vec<Metric>>>,
    aggregates: Arc<Mutex<HashMap<String, f64>>>,
}

#[server(
    name = "Analytics",
    version = "3.0.0",
    description = "Real-time analytics server"
)]
impl AnalyticsServer {
    #[tool("Record metric")]
    async fn record_metric(
        &self,
        name: String,
        value: f64,
        tags: HashMap<String, String>,
    ) -> turbomcp::McpResult<String> {
        let metric = Metric {
            name: name.clone(),
            value,
            timestamp: 1234567890, // Fixed for testing
            tags,
        };

        self.metrics.lock().unwrap().push(metric);

        // Update aggregates
        let mut aggregates = self.aggregates.lock().unwrap();
        *aggregates.entry(name.clone()).or_insert(0.0) += value;

        Ok(format!("Recorded metric: {name}"))
    }

    #[tool("Get metrics by name")]
    async fn get_metrics(&self, name: String) -> turbomcp::McpResult<Vec<Metric>> {
        Ok(self
            .metrics
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.name == name)
            .cloned()
            .collect())
    }

    #[tool("Get aggregate value")]
    async fn get_aggregate(&self, name: String) -> turbomcp::McpResult<f64> {
        self.aggregates
            .lock()
            .unwrap()
            .get(&name)
            .copied()
            .ok_or_else(|| turbomcp::McpError::Tool(format!("No aggregate for metric: {name}")))
    }

    #[tool("Get metrics by tag")]
    async fn get_metrics_by_tag(
        &self,
        tag_name: String,
        tag_value: String,
    ) -> turbomcp::McpResult<Vec<Metric>> {
        Ok(self
            .metrics
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.tags.get(&tag_name) == Some(&tag_value))
            .cloned()
            .collect())
    }

    #[tool("Calculate average")]
    async fn calculate_average(&self, name: String) -> turbomcp::McpResult<f64> {
        let metrics: Vec<_> = self
            .metrics
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.name == name)
            .map(|m| m.value)
            .collect();

        if metrics.is_empty() {
            return Err(turbomcp::McpError::Tool(format!(
                "No metrics found for: {name}"
            )));
        }

        let sum: f64 = metrics.iter().sum();
        Ok(sum / metrics.len() as f64)
    }

    #[tool("Clear all metrics")]
    async fn clear_metrics(&self) -> turbomcp::McpResult<String> {
        let count = self.metrics.lock().unwrap().len();
        self.metrics.lock().unwrap().clear();
        self.aggregates.lock().unwrap().clear();
        Ok(format!("Cleared {count} metrics"))
    }
}

#[tokio::test]
async fn test_analytics_server() {
    let server = AnalyticsServer {
        metrics: Arc::new(Mutex::new(Vec::new())),
        aggregates: Arc::new(Mutex::new(HashMap::new())),
    };

    // Record metrics
    let mut tags1 = HashMap::new();
    tags1.insert("region".to_string(), "us-west".to_string());
    tags1.insert("service".to_string(), "api".to_string());

    server
        .record_metric("response_time".to_string(), 125.5, tags1.clone())
        .await
        .unwrap();
    server
        .record_metric("response_time".to_string(), 98.3, tags1.clone())
        .await
        .unwrap();

    let mut tags2 = HashMap::new();
    tags2.insert("region".to_string(), "us-east".to_string());
    tags2.insert("service".to_string(), "api".to_string());

    server
        .record_metric("response_time".to_string(), 145.2, tags2)
        .await
        .unwrap();
    server
        .record_metric("error_count".to_string(), 3.0, tags1)
        .await
        .unwrap();

    // Get metrics by name
    let response_metrics = server
        .get_metrics("response_time".to_string())
        .await
        .unwrap();
    assert_eq!(response_metrics.len(), 3);

    // Get aggregate
    let aggregate = server
        .get_aggregate("response_time".to_string())
        .await
        .unwrap();
    assert_eq!(aggregate, 125.5 + 98.3 + 145.2);

    // Get metrics by tag
    let west_metrics = server
        .get_metrics_by_tag("region".to_string(), "us-west".to_string())
        .await
        .unwrap();
    assert_eq!(west_metrics.len(), 3); // 2 response_time + 1 error_count

    // Calculate average
    let avg = server
        .calculate_average("response_time".to_string())
        .await
        .unwrap();
    assert!((avg - 123.0).abs() < 1.0); // Approximately 123.0

    // Clear metrics
    let clear_result = server.clear_metrics().await.unwrap();
    assert!(clear_result.contains("4"));
    assert_eq!(
        server
            .get_metrics("response_time".to_string())
            .await
            .unwrap()
            .len(),
        0
    );
}

// ============================================================================
// Test 5: File System Server
// ============================================================================

#[derive(Clone)]
struct FileSystemServer {
    files: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    directories: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

#[server(name = "FileSystem", version = "1.0.0")]
impl FileSystemServer {
    #[tool("Create file")]
    async fn create_file(&self, path: String, content: Vec<u8>) -> turbomcp::McpResult<String> {
        if self.files.lock().unwrap().contains_key(&path) {
            return Err(turbomcp::McpError::Tool(format!(
                "File already exists: {path}"
            )));
        }

        self.files.lock().unwrap().insert(path.clone(), content);

        // Update parent directory
        if let Some(parent) = path.rsplit_once('/').map(|(p, _)| p.to_string()) {
            let filename = path.rsplit_once('/').map(|(_, f)| f.to_string()).unwrap();
            self.directories
                .lock()
                .unwrap()
                .entry(parent)
                .or_default()
                .push(filename);
        }

        Ok(format!("Created file: {path}"))
    }

    #[tool("Read file")]
    async fn read_file(&self, path: String) -> turbomcp::McpResult<Vec<u8>> {
        self.files
            .lock()
            .unwrap()
            .get(&path)
            .cloned()
            .ok_or_else(|| turbomcp::McpError::Tool(format!("File not found: {path}")))
    }

    #[tool("Delete file")]
    async fn delete_file(&self, path: String) -> turbomcp::McpResult<String> {
        self.files
            .lock()
            .unwrap()
            .remove(&path)
            .map(|_| format!("Deleted file: {path}"))
            .ok_or_else(|| turbomcp::McpError::Tool(format!("File not found: {path}")))
    }

    #[tool("List directory")]
    async fn list_directory(&self, path: String) -> turbomcp::McpResult<Vec<String>> {
        self.directories
            .lock()
            .unwrap()
            .get(&path)
            .cloned()
            .ok_or_else(|| turbomcp::McpError::Tool(format!("Directory not found: {path}")))
    }

    #[tool("File exists")]
    async fn file_exists(&self, path: String) -> turbomcp::McpResult<bool> {
        Ok(self.files.lock().unwrap().contains_key(&path))
    }

    #[tool("Get file size")]
    async fn get_file_size(&self, path: String) -> turbomcp::McpResult<usize> {
        self.files
            .lock()
            .unwrap()
            .get(&path)
            .map(|content| content.len())
            .ok_or_else(|| turbomcp::McpError::Tool(format!("File not found: {path}")))
    }
}

#[tokio::test]
async fn test_filesystem_server() {
    let server = FileSystemServer {
        files: Arc::new(Mutex::new(HashMap::new())),
        directories: Arc::new(Mutex::new(HashMap::new())),
    };

    // Create files
    server
        .create_file("/home/user/test.txt".to_string(), b"Hello, World!".to_vec())
        .await
        .unwrap();

    server
        .create_file(
            "/home/user/data.bin".to_string(),
            vec![0x00, 0x01, 0x02, 0x03],
        )
        .await
        .unwrap();

    // Read file
    let content = server
        .read_file("/home/user/test.txt".to_string())
        .await
        .unwrap();
    assert_eq!(content, b"Hello, World!");

    // Check file exists
    assert!(
        server
            .file_exists("/home/user/test.txt".to_string())
            .await
            .unwrap()
    );
    assert!(
        !server
            .file_exists("/home/user/missing.txt".to_string())
            .await
            .unwrap()
    );

    // Get file size
    let size = server
        .get_file_size("/home/user/test.txt".to_string())
        .await
        .unwrap();
    assert_eq!(size, 13);

    // List directory
    let files = server
        .list_directory("/home/user".to_string())
        .await
        .unwrap();
    assert_eq!(files.len(), 2);
    assert!(files.contains(&"test.txt".to_string()));
    assert!(files.contains(&"data.bin".to_string()));

    // Delete file
    server
        .delete_file("/home/user/data.bin".to_string())
        .await
        .unwrap();
    assert!(
        !server
            .file_exists("/home/user/data.bin".to_string())
            .await
            .unwrap()
    );

    // Try to create duplicate
    let duplicate_result = server
        .create_file("/home/user/test.txt".to_string(), b"duplicate".to_vec())
        .await;
    assert!(duplicate_result.is_err());
}

// ============================================================================
// Test 6: Configuration Server with Validation
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Config {
    key: String,
    value: String,
    validated: bool,
}

#[derive(Clone)]
struct ConfigServer {
    configs: Arc<Mutex<HashMap<String, Config>>>,
    validators: Arc<Mutex<HashMap<String, ConfigValidator>>>,
}

#[server(name = "ConfigManager", version = "2.1.0")]
impl ConfigServer {
    #[tool("Set configuration")]
    async fn set_config(&self, key: String, value: String) -> turbomcp::McpResult<Config> {
        // Validate if validator exists
        let validated = if let Some(validator) = self.validators.lock().unwrap().get(&key) {
            validator(&value)
        } else {
            true // No validator means auto-valid
        };

        if !validated {
            return Err(turbomcp::McpError::Tool(format!(
                "Validation failed for key: {key}"
            )));
        }

        let config = Config {
            key: key.clone(),
            value,
            validated,
        };

        self.configs.lock().unwrap().insert(key, config.clone());
        Ok(config)
    }

    #[tool("Get configuration")]
    async fn get_config(&self, key: String) -> turbomcp::McpResult<Config> {
        self.configs
            .lock()
            .unwrap()
            .get(&key)
            .cloned()
            .ok_or_else(|| turbomcp::McpError::Tool(format!("Config not found: {key}")))
    }

    #[tool("Delete configuration")]
    async fn delete_config(&self, key: String) -> turbomcp::McpResult<String> {
        self.configs
            .lock()
            .unwrap()
            .remove(&key)
            .map(|_| format!("Deleted config: {key}"))
            .ok_or_else(|| turbomcp::McpError::Tool(format!("Config not found: {key}")))
    }

    #[tool("List all configurations")]
    async fn list_configs(&self) -> turbomcp::McpResult<Vec<Config>> {
        Ok(self.configs.lock().unwrap().values().cloned().collect())
    }

    #[tool("Validate all configurations")]
    async fn validate_all(&self) -> turbomcp::McpResult<Vec<String>> {
        let mut invalid = Vec::new();
        let configs = self.configs.lock().unwrap();
        let validators = self.validators.lock().unwrap();

        for (key, config) in configs.iter() {
            if let Some(validator) = validators.get(key)
                && !validator(&config.value)
            {
                invalid.push(key.clone());
            }
        }

        if invalid.is_empty() {
            Ok(vec!["All configurations are valid".to_string()])
        } else {
            Ok(invalid)
        }
    }
}

#[tokio::test]
async fn test_config_server() {
    let server = ConfigServer {
        configs: Arc::new(Mutex::new(HashMap::new())),
        validators: Arc::new(Mutex::new(HashMap::new())),
    };

    // Add validators
    server.validators.lock().unwrap().insert(
        "port".to_string(),
        Box::new(|v: &str| v.parse::<u16>().is_ok()),
    );
    server
        .validators
        .lock()
        .unwrap()
        .insert("email".to_string(), Box::new(|v: &str| v.contains('@')));

    // Set valid configurations
    let port_config = server
        .set_config("port".to_string(), "8080".to_string())
        .await
        .unwrap();
    assert!(port_config.validated);

    let email_config = server
        .set_config("email".to_string(), "admin@example.com".to_string())
        .await
        .unwrap();
    assert!(email_config.validated);

    // Try invalid configuration
    let invalid_port = server
        .set_config("port".to_string(), "not_a_number".to_string())
        .await;
    assert!(invalid_port.is_err());

    // Set config without validator
    let other_config = server
        .set_config("name".to_string(), "MyApp".to_string())
        .await
        .unwrap();
    assert!(other_config.validated); // Auto-validated

    // Get configuration
    let fetched = server.get_config("port".to_string()).await.unwrap();
    assert_eq!(fetched.value, "8080");

    // List all
    let all_configs = server.list_configs().await.unwrap();
    assert_eq!(all_configs.len(), 3);

    // Validate all
    let validation_result = server.validate_all().await.unwrap();
    assert_eq!(validation_result[0], "All configurations are valid");

    // Delete config
    server.delete_config("email".to_string()).await.unwrap();
    assert_eq!(server.list_configs().await.unwrap().len(), 2);
}
