//! Integration tests for Database service using Testcontainers
//!
//! These tests demonstrate TurboMCP's zero-tolerance policy for mocks - 
//! we test against REAL PostgreSQL databases, not fake implementations.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use testcontainers::{core::{WaitFor, ContainerPort}, runners::AsyncRunner, GenericImage, ImageExt};
use tokio;
use turbomcp::injection::Database;

/// Test data structure matching our database schema
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestUser {
    id: String,
    name: String,
    email: String,
    created_at: String,
}

/// Test data for tools
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TestTool {
    id: String,
    name: String,
    description: Option<String>,
    schema_definition: serde_json::Value,
}

/// Waits for PostgreSQL to be healthy and ready to accept connections
/// This replaces the brittle sleep approach with proper health checking
async fn wait_for_postgres_health(connection_string: &str, timeout_secs: u64) -> Result<(), Box<dyn std::error::Error>> {
    let timeout_duration = Duration::from_secs(timeout_secs);
    let start = std::time::Instant::now();
    
    loop {
        if start.elapsed() > timeout_duration {
            return Err(format!("PostgreSQL health check timed out after {} seconds", timeout_secs).into());
        }
        
        // Try to connect and execute a simple query
        match Database::new(connection_string).await {
            Ok(db) => {
                // Use a simple non-SELECT statement for health check
                match db.execute("CREATE TABLE IF NOT EXISTS health_check (id INTEGER)").await {
                    Ok(_) => {
                        println!("PostgreSQL is healthy and ready");
                        return Ok(());
                    }
                    Err(e) => {
                        println!("PostgreSQL connection established but query failed: {}, retrying...", e);
                    }
                }
            }
            Err(e) => {
                println!("PostgreSQL connection failed: {}, retrying...", e);
            }
        }
        
        // Wait before retrying
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

/// Real PostgreSQL integration test using Testcontainers
/// This is what "ultrathink world class" looks like - real services, real data, real tests
#[tokio::test]
async fn test_database_real_postgresql_crud_operations() {
    // Start REAL PostgreSQL container - no mocks, no shortcuts
    let postgres = GenericImage::new("postgres", "16-alpine")
        .with_exposed_port(ContainerPort::Tcp(5432))
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections"
        ))
        .with_env_var("POSTGRES_USER", "test_user")
        .with_env_var("POSTGRES_PASSWORD", "test_password")
        .with_env_var("POSTGRES_DB", "turbomcp_test")
        .start()
        .await
        .expect("Failed to start PostgreSQL container");

    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let connection_string = format!(
        "postgres://test_user:test_password@localhost:{}/turbomcp_test",
        port
    );

    // Wait for PostgreSQL to be healthy and ready (replaces brittle sleep)
    wait_for_postgres_health(&connection_string, 30)
        .await
        .expect("PostgreSQL failed to become healthy");

    // Create REAL database connection - production-grade SQLx integration
    let database = Database::new(&connection_string)
        .await
        .expect("Failed to create database connection");

    // Test 1: Database health check
    database
        .health_check()
        .await
        .expect("Database health check failed");

    // Test 2: Create tables with real DDL
    let create_users_table = r#"
        CREATE TABLE test_users (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            name VARCHAR(255) NOT NULL,
            email VARCHAR(255) UNIQUE NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
    "#;

    let create_tools_table = r#"
        CREATE TABLE test_tools (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            name VARCHAR(255) UNIQUE NOT NULL,
            description TEXT,
            schema_definition JSONB NOT NULL DEFAULT '{}'::jsonb
        )
    "#;

    database
        .execute(create_users_table)
        .await
        .expect("Failed to create users table");

    database
        .execute(create_tools_table)
        .await
        .expect("Failed to create tools table");

    // Test 3: Insert real data using execute()
    let insert_user = r#"
        INSERT INTO test_users (name, email) 
        VALUES ('John Doe', 'john@example.com')
    "#;

    let rows_affected = database
        .execute(insert_user)
        .await
        .expect("Failed to insert user");
    assert_eq!(rows_affected, 1, "Expected 1 row to be affected by insert");

    // Insert a tool with JSON schema
    let insert_tool = r#"
        INSERT INTO test_tools (name, description, schema_definition) 
        VALUES (
            'echo_tool',
            'A simple echo tool for testing',
            '{"type": "object", "properties": {"message": {"type": "string", "description": "Message to echo"}}, "required": ["message"]}'::jsonb
        )
    "#;

    let tool_rows = database
        .execute(insert_tool)
        .await
        .expect("Failed to insert tool");
    assert_eq!(tool_rows, 1, "Expected 1 row to be affected by tool insert");

    // Test 4: Query real data with type-safe deserialization
    let select_users = "SELECT id::text, name, email, created_at::text FROM test_users";
    let users: Vec<TestUser> = database
        .query(select_users)
        .await
        .expect("Failed to query users");

    assert_eq!(users.len(), 1, "Expected exactly 1 user");
    assert_eq!(users[0].name, "John Doe");
    assert_eq!(users[0].email, "john@example.com");

    // Test 5: Query tools with complex JSON data
    let select_tools = "SELECT id::text, name, description, schema_definition FROM test_tools";
    let tools: Vec<TestTool> = database
        .query(select_tools)
        .await
        .expect("Failed to query tools");

    assert_eq!(tools.len(), 1, "Expected exactly 1 tool");
    assert_eq!(tools[0].name, "echo_tool");
    assert_eq!(tools[0].description, Some("A simple echo tool for testing".to_string()));
    
    // Verify JSON schema was preserved correctly
    let expected_schema: serde_json::Value = serde_json::from_str(r#"
        {
            "type": "object", 
            "properties": {
                "message": {
                    "type": "string", 
                    "description": "Message to echo"
                }
            }, 
            "required": ["message"]
        }
    "#).unwrap();
    
    assert_eq!(tools[0].schema_definition, expected_schema);

    // Test 6: Update operations
    let update_user = r#"
        UPDATE test_users 
        SET name = 'Jane Doe' 
        WHERE email = 'john@example.com'
    "#;

    let updated_rows = database
        .execute(update_user)
        .await
        .expect("Failed to update user");
    assert_eq!(updated_rows, 1, "Expected 1 row to be updated");

    // Verify the update worked
    let updated_users: Vec<TestUser> = database
        .query(select_users)
        .await
        .expect("Failed to query updated users");
    assert_eq!(updated_users[0].name, "Jane Doe");

    // Test 7: Delete operations
    let delete_user = "DELETE FROM test_users WHERE email = 'john@example.com'";
    let deleted_rows = database
        .execute(delete_user)
        .await
        .expect("Failed to delete user");
    assert_eq!(deleted_rows, 1, "Expected 1 row to be deleted");

    // Verify deletion
    let remaining_users: Vec<TestUser> = database
        .query(select_users)
        .await
        .expect("Failed to query remaining users");
    assert_eq!(remaining_users.len(), 0, "Expected no users after deletion");

    // Test 8: Security - verify that query() rejects non-SELECT statements
    let malicious_insert = "INSERT INTO test_users (name, email) VALUES ('hacker', 'hack@evil.com')";
    let query_result = database.query::<TestUser>(malicious_insert).await;
    
    assert!(query_result.is_err(), "query() should reject INSERT statements");
    
    let error_message = format!("{:?}", query_result.unwrap_err());
    assert!(error_message.contains("Only SELECT statements allowed"), 
            "Expected security error message");

    // Test 9: Error handling for invalid SQL
    let invalid_sql = "INVALID SQL SYNTAX HERE";
    let invalid_result = database.execute(invalid_sql).await;
    assert!(invalid_result.is_err(), "Should reject invalid SQL");

    // Test 10: Empty SQL validation
    let empty_result = database.query::<TestUser>("").await;
    assert!(empty_result.is_err(), "Should reject empty SQL");
    
    let empty_execute = database.execute("   ").await;
    assert!(empty_execute.is_err(), "Should reject empty SQL in execute()");
}

/// Test connection pooling and concurrent operations
#[tokio::test]
async fn test_database_concurrent_operations() {
    // Start PostgreSQL container
    let postgres = GenericImage::new("postgres", "16-alpine")
        .with_exposed_port(ContainerPort::Tcp(5432))
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections"
        ))
        .with_env_var("POSTGRES_USER", "concurrent_test")
        .with_env_var("POSTGRES_PASSWORD", "test_pwd")
        .with_env_var("POSTGRES_DB", "concurrent_db")
        .start()
        .await
        .expect("Failed to start PostgreSQL for concurrent test");

    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let connection_string = format!(
        "postgres://concurrent_test:test_pwd@localhost:{}/concurrent_db",
        port
    );
    
    // Wait for PostgreSQL to be healthy and ready (replaces brittle sleep)
    wait_for_postgres_health(&connection_string, 30)
        .await
        .expect("PostgreSQL failed to become healthy");

    let database = Database::new(&connection_string)
        .await
        .expect("Failed to create database for concurrent test");

    // Create test table
    database
        .execute(
            "CREATE TABLE concurrent_test (
                id SERIAL PRIMARY KEY, 
                value INTEGER NOT NULL,
                created_at TIMESTAMPTZ DEFAULT NOW()
            )"
        )
        .await
        .expect("Failed to create concurrent test table");

    // Test concurrent database operations using the connection pool
    let database_clone = database.clone(); // This should work due to Arc<PgPool>
    
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let db = database_clone.clone();
            tokio::spawn(async move {
                let sql = format!("INSERT INTO concurrent_test (value) VALUES ({})", i);
                db.execute(&sql).await
            })
        })
        .collect();

    // Wait for all concurrent operations to complete
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        assert!(result.is_ok(), "Concurrent insert should succeed");
    }

    // Verify all 10 records were inserted
    #[derive(Debug, Deserialize)]
    struct CountResult {
        count: i64,
    }

    let count_results: Vec<CountResult> = database
        .query("SELECT COUNT(*) as count FROM concurrent_test")
        .await
        .expect("Failed to count records");

    assert_eq!(count_results[0].count, 10, "Expected 10 concurrent inserts");
}

/// Test error scenarios with real database
#[tokio::test]
async fn test_database_error_handling() {
    let postgres = GenericImage::new("postgres", "16-alpine")
        .with_exposed_port(ContainerPort::Tcp(5432))
        .with_wait_for(WaitFor::message_on_stdout(
            "database system is ready to accept connections"
        ))
        .with_env_var("POSTGRES_USER", "error_test")
        .with_env_var("POSTGRES_PASSWORD", "test_pwd")
        .with_env_var("POSTGRES_DB", "error_db")
        .start()
        .await
        .expect("Failed to start PostgreSQL for error test");

    let port = postgres.get_host_port_ipv4(5432).await.unwrap();
    let connection_string = format!(
        "postgres://error_test:test_pwd@localhost:{}/error_db",
        port
    );
    
    // Wait for PostgreSQL to be healthy and ready (replaces brittle sleep)
    wait_for_postgres_health(&connection_string, 30)
        .await
        .expect("PostgreSQL failed to become healthy");

    let database = Database::new(&connection_string)
        .await
        .expect("Failed to create database for error test");

    // Test 1: SQL syntax error
    let syntax_error_result = database.execute("INVALID SQL SYNTAX").await;
    assert!(syntax_error_result.is_err());

    // Test 2: Constraint violation
    database
        .execute(
            "CREATE TABLE unique_test (
                id SERIAL PRIMARY KEY,
                unique_value VARCHAR(50) UNIQUE NOT NULL
            )"
        )
        .await
        .expect("Failed to create unique test table");

    database
        .execute("INSERT INTO unique_test (unique_value) VALUES ('test_value')")
        .await
        .expect("First insert should succeed");

    // Second insert with same value should fail
    let constraint_violation = database
        .execute("INSERT INTO unique_test (unique_value) VALUES ('test_value')")
        .await;
    assert!(constraint_violation.is_err(), "Should fail due to unique constraint");

    // Test 3: Table doesn't exist
    let table_not_found = database
        .query::<HashMap<String, String>>("SELECT * FROM non_existent_table")
        .await;
    assert!(table_not_found.is_err(), "Should fail for non-existent table");
}

/// Test that demonstrates the anti-pattern we DON'T want
/// This test would be the "old way" - testing against mocks instead of real databases
/// We include this as documentation of what NOT to do
#[allow(dead_code)]
fn example_of_testing_fraud() {
    // ❌ THIS IS BAD - testing against fake data
    // let fake_database = FakeDatabaseMock::new();
    // fake_database.expect_query().returning(|| Ok(vec![]));
    // 
    // This type of test passes but doesn't validate real functionality!
    // It tests the mock, not the actual database integration.
    // 
    // TurboMCP's zero-tolerance policy: NO MOCKS FOR CRITICAL INFRASTRUCTURE
}