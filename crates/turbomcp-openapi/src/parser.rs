//! OpenAPI specification parsing.

use std::path::Path;
use std::time::Duration;

use openapiv3::OpenAPI;
use url::Url;

use crate::error::{OpenApiError, Result};
use crate::security::SsrfGuard;

const DEFAULT_SPEC_FETCH_TIMEOUT_SECS: u64 = 30;

/// Parse an OpenAPI specification from a string.
///
/// Tries JSON first when the content starts with `{`, but falls back to YAML
/// on JSON failure — flow-style YAML documents (e.g. `{key: value}`) also
/// start with `{` and would otherwise be misclassified as malformed JSON.
pub fn parse_spec(content: &str) -> Result<OpenAPI> {
    if content.trim_start().starts_with('{') {
        match serde_json::from_str::<OpenAPI>(content) {
            Ok(spec) => return Ok(spec),
            Err(json_err) => {
                // Flow-style YAML (`{key: value}`) parses as bad JSON; try
                // YAML before surfacing the JSON diagnostic.
                if let Ok(spec) = serde_norway::from_str::<OpenAPI>(content) {
                    return Ok(spec);
                }
                return Err(json_err.into());
            }
        }
    }

    serde_norway::from_str(content).map_err(Into::into)
}

/// Load an OpenAPI specification from a file.
pub fn load_from_file(path: &Path) -> Result<OpenAPI> {
    let content = std::fs::read_to_string(path)?;
    parse_spec(&content)
}

/// Fetch an OpenAPI specification from a URL.
pub async fn fetch_from_url(url: &str) -> Result<OpenAPI> {
    let url = Url::parse(url)?;
    let ssrf = SsrfGuard::default();
    ssrf.check_request_target(&url).await?;

    let client = ssrf
        .client_builder()
        .timeout(Duration::from_secs(DEFAULT_SPEC_FETCH_TIMEOUT_SECS))
        .build()?;
    let response = client.get(url).send().await?;

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

    #[tokio::test]
    async fn test_fetch_from_url_blocks_localhost_before_request() {
        let result = fetch_from_url("http://127.0.0.1:9/openapi.json").await;
        assert!(matches!(result, Err(OpenApiError::SsrfBlocked(_))));
    }
}
