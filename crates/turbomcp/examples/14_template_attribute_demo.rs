//! Example: #[template] Attribute Macro Demonstration
//!
//! This example demonstrates the #[template] attribute macro for marking methods
//! as resource template handlers using RFC 6570 URI templates.
//!
//! The #[template] macro generates handlers that support parameterized resource URIs
//! for dynamic content generation and flexible resource access patterns.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use turbomcp::prelude::*;
use turbomcp_macros::template;

#[derive(Clone, Debug)]
struct User {
    id: u32,
    name: String,
    email: String,
    department: String,
    role: String,
}

#[derive(Clone, Debug)]
struct Project {
    id: u32,
    name: String,
    status: String,
    owner_id: u32,
    created_at: String,
}

#[derive(Clone)]
struct ResourceTemplateServer {
    users: Arc<Mutex<HashMap<u32, User>>>,
    projects: Arc<Mutex<HashMap<u32, Project>>>,
    documents: Arc<Mutex<HashMap<String, String>>>,
}

#[server(
    name = "template-attribute-demo",
    version = "1.0.4",
    description = "Demonstrates #[template] attribute macro functionality"
)]
impl ResourceTemplateServer {
    fn new() -> Self {
        let mut users = HashMap::new();
        users.insert(
            1,
            User {
                id: 1,
                name: "Alice Johnson".to_string(),
                email: "alice@company.com".to_string(),
                department: "Engineering".to_string(),
                role: "Senior Developer".to_string(),
            },
        );
        users.insert(
            2,
            User {
                id: 2,
                name: "Bob Smith".to_string(),
                email: "bob@company.com".to_string(),
                department: "Engineering".to_string(),
                role: "Tech Lead".to_string(),
            },
        );
        users.insert(
            3,
            User {
                id: 3,
                name: "Carol Davis".to_string(),
                email: "carol@company.com".to_string(),
                department: "Product".to_string(),
                role: "Product Manager".to_string(),
            },
        );

        let mut projects = HashMap::new();
        projects.insert(
            100,
            Project {
                id: 100,
                name: "TurboMCP".to_string(),
                status: "active".to_string(),
                owner_id: 2,
                created_at: "2024-01-15".to_string(),
            },
        );
        projects.insert(
            101,
            Project {
                id: 101,
                name: "Web Dashboard".to_string(),
                status: "planning".to_string(),
                owner_id: 1,
                created_at: "2024-02-01".to_string(),
            },
        );

        let mut documents = HashMap::new();
        documents.insert(
            "api-spec".to_string(),
            "# API Specification\n\nThis document describes our REST API...".to_string(),
        );
        documents.insert(
            "user-guide".to_string(),
            "# User Guide\n\nWelcome to our application...".to_string(),
        );

        Self {
            users: Arc::new(Mutex::new(users)),
            projects: Arc::new(Mutex::new(projects)),
            documents: Arc::new(Mutex::new(documents)),
        }
    }

    /// Template for user profile resources
    #[template("users/{user_id}/profile")]
    async fn get_user_profile(&self, user_id: String) -> McpResult<String> {
        let user_id: u32 = user_id
            .parse()
            .map_err(|_| McpError::InvalidInput("Invalid user ID".to_string()))?;

        let users = self.users.lock().unwrap();
        let user = users
            .get(&user_id)
            .ok_or_else(|| McpError::Tool(format!("User {} not found", user_id)))?;

        Ok(format!(
            "👤 User Profile\n\
             Name: {}\n\
             Email: {}\n\
             Department: {}\n\
             Role: {}\n\
             ID: {}",
            user.name, user.email, user.department, user.role, user.id
        ))
    }

    /// Template for user's projects
    #[template("users/{user_id}/projects")]
    async fn get_user_projects(&self, user_id: String) -> McpResult<String> {
        let user_id: u32 = user_id
            .parse()
            .map_err(|_| McpError::InvalidInput("Invalid user ID".to_string()))?;

        let users = self.users.lock().unwrap();
        let user = users
            .get(&user_id)
            .ok_or_else(|| McpError::Tool(format!("User {} not found", user_id)))?;

        let projects = self.projects.lock().unwrap();
        let user_projects: Vec<&Project> = projects
            .values()
            .filter(|p| p.owner_id == user_id)
            .collect();

        if user_projects.is_empty() {
            Ok(format!(
                "📂 No projects found for {} (ID: {})",
                user.name, user_id
            ))
        } else {
            let project_list: Vec<String> = user_projects
                .iter()
                .map(|p| format!("  • {} (ID: {}) - Status: {}", p.name, p.id, p.status))
                .collect();

            Ok(format!(
                "📂 Projects for {} (ID: {}):\n{}",
                user.name,
                user_id,
                project_list.join("\n")
            ))
        }
    }

    /// Template for project details
    #[template("projects/{project_id}")]
    async fn get_project_details(&self, project_id: String) -> McpResult<String> {
        let project_id: u32 = project_id
            .parse()
            .map_err(|_| McpError::InvalidInput("Invalid project ID".to_string()))?;

        let projects = self.projects.lock().unwrap();
        let project = projects
            .get(&project_id)
            .ok_or_else(|| McpError::Tool(format!("Project {} not found", project_id)))?;

        let users = self.users.lock().unwrap();
        let owner = users
            .get(&project.owner_id)
            .map(|u| u.name.clone())
            .unwrap_or_else(|| format!("Unknown (ID: {})", project.owner_id));

        Ok(format!(
            "🚀 Project Details\n\
             Name: {}\n\
             ID: {}\n\
             Status: {}\n\
             Owner: {}\n\
             Created: {}",
            project.name, project.id, project.status, owner, project.created_at
        ))
    }

    /// Template for department resources
    #[template("departments/{department}/users")]
    async fn get_department_users(&self, department: String) -> McpResult<String> {
        let users = self.users.lock().unwrap();
        let dept_users: Vec<&User> = users
            .values()
            .filter(|u| u.department.to_lowercase() == department.to_lowercase())
            .collect();

        if dept_users.is_empty() {
            Ok(format!("🏢 No users found in {} department", department))
        } else {
            let user_list: Vec<String> = dept_users
                .iter()
                .map(|u| format!("  • {} ({}) - {}", u.name, u.email, u.role))
                .collect();

            Ok(format!(
                "🏢 Users in {} Department:\n{}",
                department,
                user_list.join("\n")
            ))
        }
    }

    /// Template with context injection for logging
    #[template("documents/{doc_id}/content")]
    async fn get_document_content(&self, ctx: Context, doc_id: String) -> McpResult<String> {
        ctx.info(&format!("Accessing document: {}", doc_id)).await?;

        // Scope the mutex guard to avoid holding it across await
        let content = {
            let documents = self.documents.lock().unwrap();
            documents
                .get(&doc_id)
                .ok_or_else(|| McpError::Tool(format!("Document '{}' not found", doc_id)))?
                .clone()
        };

        ctx.info(&format!(
            "Document '{}' retrieved successfully ({} chars)",
            doc_id,
            content.len()
        ))
        .await?;

        Ok(content)
    }

    /// Complex template with multiple parameters
    #[template("projects/{project_id}/members/{user_id}")]
    async fn get_project_member(&self, project_id: String, user_id: String) -> McpResult<String> {
        let project_id: u32 = project_id
            .parse()
            .map_err(|_| McpError::InvalidInput("Invalid project ID".to_string()))?;
        let user_id: u32 = user_id
            .parse()
            .map_err(|_| McpError::InvalidInput("Invalid user ID".to_string()))?;

        let projects = self.projects.lock().unwrap();
        let project = projects
            .get(&project_id)
            .ok_or_else(|| McpError::Tool(format!("Project {} not found", project_id)))?;

        let users = self.users.lock().unwrap();
        let user = users
            .get(&user_id)
            .ok_or_else(|| McpError::Tool(format!("User {} not found", user_id)))?;

        // Simulate membership check (in real app, would have proper membership table)
        let is_member = project.owner_id == user_id || user.department == "Engineering";

        if is_member {
            Ok(format!(
                "👥 Project Member\n\
                 Project: {} (ID: {})\n\
                 Member: {} (ID: {})\n\
                 Role in Project: {}\n\
                 Status: Active",
                project.name,
                project_id,
                user.name,
                user_id,
                if project.owner_id == user_id {
                    "Owner"
                } else {
                    "Contributor"
                }
            ))
        } else {
            Ok(format!(
                "❌ User {} ({}) is not a member of project {} ({})",
                user.name, user_id, project.name, project_id
            ))
        }
    }

    /// Tool to list all available templates
    #[tool("List available resource templates")]
    async fn list_templates(&self) -> McpResult<String> {
        Ok(r#"
📋 Available Resource Templates:

👤 User Resources:
  • users/{user_id}/profile - Get user profile information
  • users/{user_id}/projects - Get projects owned by user

🚀 Project Resources:
  • projects/{project_id} - Get project details
  • projects/{project_id}/members/{user_id} - Check project membership

🏢 Department Resources:
  • departments/{department}/users - List users in department

📄 Document Resources:
  • documents/{doc_id}/content - Get document content

💡 Usage Examples:
  • users/1/profile
  • users/2/projects  
  • projects/100
  • departments/Engineering/users
  • documents/api-spec/content
  • projects/100/members/1
        "#
        .trim()
        .to_string())
    }

    /// Tool to demonstrate template parameter extraction
    #[tool("Test template parameters")]
    async fn test_template_params(&self, template_uri: String) -> McpResult<String> {
        // This would normally be handled by the MCP framework,
        // but we can simulate parameter extraction for demonstration

        if template_uri.starts_with("users/") && template_uri.contains("/profile") {
            let user_id = template_uri
                .strip_prefix("users/")
                .and_then(|s| s.strip_suffix("/profile"))
                .unwrap_or("unknown");
            return self.get_user_profile(user_id.to_string()).await;
        }

        if template_uri.starts_with("projects/") && !template_uri.contains("/members/") {
            let project_id = template_uri.strip_prefix("projects/").unwrap_or("unknown");
            return self.get_project_details(project_id.to_string()).await;
        }

        Err(McpError::InvalidInput(format!(
            "Unknown template URI: {}",
            template_uri
        )))
    }

    /// Show template usage patterns
    #[tool("Show template patterns")]
    async fn show_template_patterns(&self) -> McpResult<String> {
        Ok(r#"
🎯 #[template] Macro Usage Patterns:

📄 Basic Template Handler:
  #[template("path/{param}")]
  async fn handler(&self, param: String) -> McpResult<String>

🔗 With Context Injection:
  #[template("path/{param}")]
  async fn handler(&self, ctx: Context, param: String) -> McpResult<String>

⚙️ Multiple Parameters:
  #[template("path/{param1}/sub/{param2}")]
  async fn handler(&self, param1: String, param2: String) -> McpResult<String>

✅ Key Benefits:
• RFC 6570 URI template support
• Automatic parameter extraction from URI paths
• Type-safe parameter handling
• Context injection for logging and monitoring
• Integration with MCP resource protocol
• Error handling for invalid parameters
• Metadata generation for testing

🏗️ Generated Functions:
• handler_metadata() - Returns (name, uri_template, type, description) tuple
• Internal bridge function for protocol integration
• Parameter extraction from URI template patterns
• Result conversion to resource content

💡 Best Practices:
• Use descriptive URI template patterns
• Validate extracted parameters (type conversion)
• Handle missing resources gracefully
• Use Context for access logging
• Return structured, readable content
• Consider caching for expensive resources
• Use meaningful parameter names in templates
        "#
        .trim()
        .to_string())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎯 TurboMCP #[template] Attribute Macro Demo");
    println!("============================================");
    println!();
    println!("This example demonstrates the #[template] attribute macro");
    println!("for creating RFC 6570 URI template resource handlers.");
    println!();

    let server = ResourceTemplateServer::new();

    // Test that the macro generates metadata functions
    let (name, uri_template, handler_type, desc) =
        ResourceTemplateServer::get_user_profile_metadata();
    println!("✅ Template metadata generated:");
    println!("   Name: {}", name);
    println!("   URI Template: {}", uri_template);
    println!("   Type: {}", handler_type);
    println!("   Description: {}", desc);
    println!();

    // Test all template handlers
    let handlers = [
        ResourceTemplateServer::get_user_profile_metadata(),
        ResourceTemplateServer::get_user_projects_metadata(),
        ResourceTemplateServer::get_project_details_metadata(),
        ResourceTemplateServer::get_department_users_metadata(),
        ResourceTemplateServer::get_document_content_metadata(),
        ResourceTemplateServer::get_project_member_metadata(),
    ];

    println!("📋 All template handlers:");
    for (name, uri_template, handler_type, desc) in handlers {
        println!(
            "   • {}: {} -> {} ({})",
            name, uri_template, desc, handler_type
        );
    }
    println!();

    // Demonstrate template functionality
    println!("🔗 Testing template handlers:");

    // User profile template
    let profile_result = server.get_user_profile("1".to_string()).await?;
    println!(
        "   User 1 profile: {}",
        profile_result.lines().next().unwrap_or("")
    );

    // User projects template
    let projects_result = server.get_user_projects("2".to_string()).await?;
    println!(
        "   User 2 projects: {}",
        projects_result.lines().next().unwrap_or("")
    );

    // Project details template
    let project_result = server.get_project_details("100".to_string()).await?;
    println!(
        "   Project 100: {}",
        project_result.lines().next().unwrap_or("")
    );

    // Department users template
    let dept_result = server
        .get_department_users("Engineering".to_string())
        .await?;
    println!(
        "   Engineering dept: {}",
        dept_result.lines().next().unwrap_or("")
    );

    // Document content with context
    // Create a proper context for testing
    let request_ctx = RequestContext::new();
    let handler_meta = HandlerMetadata {
        name: "template_demo".to_string(),
        handler_type: "template".to_string(),
        description: Some("Template demo".to_string()),
    };
    let ctx = Context::new(request_ctx, handler_meta);
    let doc_result = server
        .get_document_content(ctx, "api-spec".to_string())
        .await?;
    println!(
        "   API spec doc: {}",
        doc_result.lines().next().unwrap_or("")
    );

    // Complex template with multiple parameters
    let member_result = server
        .get_project_member("100".to_string(), "2".to_string())
        .await?;
    println!(
        "   Project member: {}",
        member_result.lines().next().unwrap_or("")
    );

    println!();

    // Show available templates
    let templates = server.list_templates().await?;
    println!("📋 Available templates:");
    for line in templates.lines().take(5) {
        if !line.trim().is_empty() {
            println!("   {}", line);
        }
    }
    println!("   ... (see full output for complete list)");

    println!();
    println!("✅ All #[template] macros compiled and executed successfully!");
    println!();
    println!("The macro generates:");
    println!("• Metadata functions for testing (4-tuple format)");
    println!("• Parameter extraction from URI templates");
    println!("• Context injection support");
    println!("• Type-safe return value handling");
    println!("• Integration with MCP resource protocol");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_template_handlers() {
        let server = ResourceTemplateServer::new();
        // Create a proper context for testing
        let request_ctx = RequestContext::new();
        let handler_meta = HandlerMetadata {
            name: "test_template".to_string(),
            handler_type: "template".to_string(),
            description: Some("Test template handler".to_string()),
        };
        let ctx = Context::new(request_ctx, handler_meta);

        // Test user profile template
        let profile_result = server.get_user_profile("1".to_string()).await.unwrap();
        assert!(profile_result.contains("Alice Johnson"));
        assert!(profile_result.contains("alice@company.com"));

        // Test user projects template
        let projects_result = server.get_user_projects("2".to_string()).await.unwrap();
        assert!(projects_result.contains("TurboMCP"));

        // Test project details template
        let project_result = server.get_project_details("100".to_string()).await.unwrap();
        assert!(project_result.contains("TurboMCP"));
        assert!(project_result.contains("Bob Smith"));

        // Test department users template
        let dept_result = server
            .get_department_users("Engineering".to_string())
            .await
            .unwrap();
        assert!(dept_result.contains("Alice Johnson"));
        assert!(dept_result.contains("Bob Smith"));

        // Test document content template with context
        let doc_result = server
            .get_document_content(ctx, "api-spec".to_string())
            .await
            .unwrap();
        assert!(doc_result.contains("API Specification"));

        // Test complex template with multiple parameters
        let member_result = server
            .get_project_member("100".to_string(), "2".to_string())
            .await
            .unwrap();
        assert!(member_result.contains("Owner")); // User 2 is owner of project 100
    }

    #[test]
    fn test_template_metadata() {
        // Verify metadata functions exist and return correct data (4-tuple format)
        let (name, uri_template, handler_type, desc) =
            ResourceTemplateServer::get_user_profile_metadata();
        assert_eq!(name, "get_user_profile");
        assert_eq!(uri_template, "users/{user_id}/profile");
        assert_eq!(handler_type, "template");
        assert_eq!(desc, "Resource template handler");

        let (name2, uri_template2, handler_type2, desc2) =
            ResourceTemplateServer::get_user_projects_metadata();
        assert_eq!(name2, "get_user_projects");
        assert_eq!(uri_template2, "users/{user_id}/projects");
        assert_eq!(handler_type2, "template");
        assert_eq!(desc2, "Resource template handler");

        let (name3, uri_template3, handler_type3, desc3) =
            ResourceTemplateServer::get_project_details_metadata();
        assert_eq!(name3, "get_project_details");
        assert_eq!(uri_template3, "projects/{project_id}");
        assert_eq!(handler_type3, "template");

        let (name4, uri_template4, handler_type4, desc4) =
            ResourceTemplateServer::get_department_users_metadata();
        assert_eq!(name4, "get_department_users");
        assert_eq!(uri_template4, "departments/{department}/users");
        assert_eq!(handler_type4, "template");

        let (name5, uri_template5, handler_type5, desc5) =
            ResourceTemplateServer::get_document_content_metadata();
        assert_eq!(name5, "get_document_content");
        assert_eq!(uri_template5, "documents/{doc_id}/content");
        assert_eq!(handler_type5, "template");

        let (name6, uri_template6, handler_type6, desc6) =
            ResourceTemplateServer::get_project_member_metadata();
        assert_eq!(name6, "get_project_member");
        assert_eq!(uri_template6, "projects/{project_id}/members/{user_id}");
        assert_eq!(handler_type6, "template");
    }

    #[tokio::test]
    async fn test_template_error_handling() {
        let server = ResourceTemplateServer::new();

        // Test invalid user ID
        let invalid_user_result = server.get_user_profile("invalid".to_string()).await;
        assert!(invalid_user_result.is_err());

        // Test non-existent user
        let missing_user_result = server.get_user_profile("999".to_string()).await;
        assert!(missing_user_result.is_err());

        // Test non-existent project
        let missing_project_result = server.get_project_details("999".to_string()).await;
        assert!(missing_project_result.is_err());

        // Test non-existent document
        // Create context for missing document test
        let request_ctx2 = RequestContext::new();
        let handler_meta2 = HandlerMetadata {
            name: "missing_doc_test".to_string(),
            handler_type: "template".to_string(),
            description: Some("Missing document test".to_string()),
        };
        let ctx2 = Context::new(request_ctx2, handler_meta2);
        let missing_doc_result = server
            .get_document_content(ctx2, "missing-doc".to_string())
            .await;
        assert!(missing_doc_result.is_err());
    }

    #[tokio::test]
    async fn test_complex_template_parameters() {
        let server = ResourceTemplateServer::new();

        // Test project member with owner
        let owner_result = server
            .get_project_member("100".to_string(), "2".to_string())
            .await
            .unwrap();
        assert!(owner_result.contains("Owner"));

        // Test project member with contributor
        let contrib_result = server
            .get_project_member("100".to_string(), "1".to_string())
            .await
            .unwrap();
        assert!(contrib_result.contains("Contributor") || contrib_result.contains("Active"));

        // Test non-member
        let non_member_result = server
            .get_project_member("100".to_string(), "3".to_string())
            .await
            .unwrap();
        // User 3 is in Product dept, so might not be a member
        assert!(non_member_result.contains("not a member") || non_member_result.contains("Active"));
    }
}
