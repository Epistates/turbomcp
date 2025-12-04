#[cfg(all(test, feature = "mcp-tasks"))]
mod tasks_tests {
    use crate::Client;
    use turbomcp_transport::stdio::StdioTransport;
    
    // This is a compilation test to ensure the methods are exposed and have correct signatures
    // We don't actually run it because we don't have a server connected
    #[tokio::test]
    async fn test_tasks_api_compilation() {
        let transport = StdioTransport::new();
        let client = Client::new(transport);
        
        // These calls should compile if the feature is enabled
        let _ = client.get_task("task-123").await;
        let _ = client.cancel_task("task-123").await;
        let _ = client.list_tasks(None, None).await;
        let _ = client.get_task_result("task-123").await;
    }
}
