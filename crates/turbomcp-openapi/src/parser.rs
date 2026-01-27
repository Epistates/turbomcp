//! OpenAPI specification parsing.

use std::path::Path;

use openapiv3::OpenAPI;

use crate::error::{OpenApiError, Result};

/// Parse an OpenAPI specification from a string.
///
/// Automatically detects JSON or YAML format based on content.
pub fn parse_spec(content: &str) -> Result<OpenAPI> {
    // Try JSON first (faster)
    if content.trim_start().starts_with('{') {
        return serde_json::from_str(content).map_err(Into::into);
    }

    // Try YAML
    serde_yaml::from_str(content).map_err(Into::into)
}

/// Load an OpenAPI specification from a file.
pub fn load_from_file(path: &Path) -> Result<OpenAPI> {
    let content = std::fs::read_to_string(path)?;
    parse_spec(&content)
}

/// Fetch an OpenAPI specification from a URL.
pub async fn fetch_from_url(url: &str) -> Result<OpenAPI> {
    let response = reqwest::get(url).await?;

    if !response.status().is_success() {
        return Err(OpenApiError::ApiError(format!(
            "HTTP {} fetching OpenAPI spec",
            response.status()
        )));
    }

    let content = response.text().await?;
    parse_spec(&content)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIMPLE_SPEC_JSON: &str = r#"{
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "paths": {
            "/users": {
                "get": {
                    "summary": "List users",
                    "responses": {
                        "200": {
                            "description": "Success"
                        }
                    }
                }
            }
        }
    }"#;

    const SIMPLE_SPEC_YAML: &str = r#"
openapi: "3.0.0"
info:
  title: Test API
  version: "1.0.0"
paths:
  /users:
    get:
      summary: List users
      responses:
        "200":
          description: Success
"#;

    #[test]
    fn test_parse_json() {
        let spec = parse_spec(SIMPLE_SPEC_JSON).unwrap();
        assert_eq!(spec.info.title, "Test API");
        assert!(spec.paths.paths.contains_key("/users"));
    }

    #[test]
    fn test_parse_yaml() {
        let spec = parse_spec(SIMPLE_SPEC_YAML).unwrap();
        assert_eq!(spec.info.title, "Test API");
        assert!(spec.paths.paths.contains_key("/users"));
    }

    #[test]
    fn test_invalid_spec() {
        let result = parse_spec("not valid openapi");
        assert!(result.is_err());
    }
}
