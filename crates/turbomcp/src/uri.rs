//! URI template matching and parameter extraction

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

use crate::{McpError, McpResult};

/// URI template matcher
pub struct UriTemplate {
    pattern: Regex,
    parameter_names: Vec<String>,
}

impl UriTemplate {
    /// Create a new URI template
    pub fn new(template: &str) -> McpResult<Self> {
        // Convert template like "config://settings/{section}" to regex
        let mut parameter_names = Vec::new();
        let mut regex_pattern = template.to_string();

        // Find parameter placeholders
        static PARAM_REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"\{([^}]+)\}").expect("URI parameter regex should be valid"));

        for cap in PARAM_REGEX.captures_iter(template) {
            if let Some(param_name) = cap.get(1) {
                parameter_names.push(param_name.as_str().to_string());
                // Replace {param} with named capture group (require non-empty match)
                regex_pattern = regex_pattern.replace(
                    &format!("{{{}}}", param_name.as_str()),
                    &format!("(?P<{}>.+?)", param_name.as_str()),
                );
            }
        }

        // Escape other regex special characters but preserve our named groups
        let pattern = Regex::new(&format!("^{regex_pattern}$"))
            .map_err(|e| McpError::Resource(format!("Invalid URI template regex: {e}")))?;

        Ok(Self {
            pattern,
            parameter_names,
        })
    }

    /// Match a URI against this template and extract parameters
    #[must_use]
    pub fn matches(&self, uri: &str) -> Option<HashMap<String, String>> {
        if let Some(captures) = self.pattern.captures(uri) {
            let mut params = HashMap::new();

            for param_name in &self.parameter_names {
                if let Some(value) = captures.name(param_name) {
                    params.insert(param_name.clone(), value.as_str().to_string());
                }
            }

            Some(params)
        } else {
            None
        }
    }

    /// Check if URI matches template (without extracting parameters)
    #[must_use]
    pub fn is_match(&self, uri: &str) -> bool {
        self.pattern.is_match(uri)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_template_simple() {
        let template = UriTemplate::new("config://settings/{section}").unwrap();

        let params = template.matches("config://settings/database").unwrap();
        assert_eq!(params.get("section"), Some(&"database".to_string()));

        assert!(template.matches("file://not-matching").is_none());
    }

    #[test]
    fn test_uri_template_multiple_params() {
        let template = UriTemplate::new("api://v{version}/users/{id}").unwrap();

        let params = template.matches("api://v1/users/123").unwrap();
        assert_eq!(params.get("version"), Some(&"1".to_string()));
        assert_eq!(params.get("id"), Some(&"123".to_string()));
    }
}
