//! Route mapping configuration for OpenAPI to MCP conversion.

use regex::Regex;

use crate::error::Result;

/// MCP component type that an OpenAPI operation maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum McpType {
    /// Map to MCP tool (callable operation).
    #[default]
    Tool,
    /// Map to MCP resource (readable content).
    Resource,
    /// Skip this operation (don't expose via MCP).
    Skip,
}

/// A single route mapping rule.
#[derive(Debug, Clone)]
pub struct RouteRule {
    /// HTTP methods this rule applies to (empty = all methods).
    pub methods: Vec<String>,
    /// Path pattern (regex) this rule applies to (None = all paths).
    pub pattern: Option<Regex>,
    /// What MCP type to map matching operations to.
    pub mcp_type: McpType,
    /// Priority (higher = checked first).
    pub priority: i32,
}

impl RouteRule {
    /// Create a new route rule.
    pub fn new(mcp_type: McpType) -> Self {
        Self {
            methods: Vec::new(),
            pattern: None,
            mcp_type,
            priority: 0,
        }
    }

    /// Set HTTP methods for this rule.
    #[must_use]
    pub fn methods<I, S>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.methods = methods.into_iter().map(Into::into).collect();
        self
    }

    /// Set path pattern for this rule.
    pub fn pattern(mut self, pattern: &str) -> Result<Self> {
        self.pattern = Some(Regex::new(pattern)?);
        Ok(self)
    }

    /// Set priority for this rule.
    #[must_use]
    pub fn priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    /// Check if this rule matches a given method and path.
    pub fn matches(&self, method: &str, path: &str) -> bool {
        // Check method
        if !self.methods.is_empty() && !self.methods.iter().any(|m| m.eq_ignore_ascii_case(method))
        {
            return false;
        }

        // Check path pattern
        if let Some(ref pattern) = self.pattern
            && !pattern.is_match(path)
        {
            return false;
        }

        true
    }
}

/// Configuration for mapping OpenAPI operations to MCP components.
#[derive(Debug, Clone, Default)]
pub struct RouteMapping {
    rules: Vec<RouteRule>,
}

impl RouteMapping {
    /// Create a new empty route mapping.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create default mapping rules:
    /// - GET → Resource
    /// - POST, PUT, PATCH, DELETE → Tool
    pub fn default_rules() -> Self {
        Self::new()
            .map_methods(["GET"], McpType::Resource)
            .map_methods(["POST", "PUT", "PATCH", "DELETE"], McpType::Tool)
    }

    /// Add a rule to map specific HTTP methods to an MCP type.
    #[must_use]
    pub fn map_methods<I, S>(mut self, methods: I, mcp_type: McpType) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.rules.push(RouteRule::new(mcp_type).methods(methods));
        self
    }

    /// Add a rule to map a specific HTTP method to an MCP type.
    #[must_use]
    pub fn map_method(self, method: &str, mcp_type: McpType) -> Self {
        self.map_methods([method], mcp_type)
    }

    /// Add a rule to map paths matching a pattern to an MCP type.
    pub fn map_pattern(mut self, pattern: &str, mcp_type: McpType) -> Result<Self> {
        self.rules.push(RouteRule::new(mcp_type).pattern(pattern)?);
        Ok(self)
    }

    /// Add a rule to map specific methods and pattern to an MCP type.
    pub fn map_rule<I, S>(
        mut self,
        methods: I,
        pattern: &str,
        mcp_type: McpType,
        priority: i32,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.rules.push(
            RouteRule::new(mcp_type)
                .methods(methods)
                .pattern(pattern)?
                .priority(priority),
        );
        Ok(self)
    }

    /// Add a custom route rule.
    #[must_use]
    pub fn add_rule(mut self, rule: RouteRule) -> Self {
        self.rules.push(rule);
        self
    }

    /// Skip operations matching a pattern.
    pub fn skip_pattern(self, pattern: &str) -> Result<Self> {
        self.map_pattern(pattern, McpType::Skip)
    }

    /// Determine the MCP type for a given HTTP method and path.
    ///
    /// Rules are checked in order of priority (highest first), then insertion order.
    /// Returns `McpType::Tool` as default if no rule matches.
    pub fn get_mcp_type(&self, method: &str, path: &str) -> McpType {
        // Sort rules by priority (highest first)
        let mut sorted_rules: Vec<_> = self.rules.iter().collect();
        sorted_rules.sort_by(|a, b| b.priority.cmp(&a.priority));

        for rule in sorted_rules {
            if rule.matches(method, path) {
                return rule.mcp_type;
            }
        }

        // Default: use method-based heuristic
        match method.to_uppercase().as_str() {
            "GET" => McpType::Resource,
            _ => McpType::Tool,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_rules() {
        let mapping = RouteMapping::default_rules();

        assert_eq!(mapping.get_mcp_type("GET", "/users"), McpType::Resource);
        assert_eq!(mapping.get_mcp_type("POST", "/users"), McpType::Tool);
        assert_eq!(mapping.get_mcp_type("PUT", "/users/1"), McpType::Tool);
        assert_eq!(mapping.get_mcp_type("DELETE", "/users/1"), McpType::Tool);
    }

    #[test]
    fn test_pattern_matching() {
        let mapping = RouteMapping::new()
            .map_pattern(r"/admin/.*", McpType::Skip)
            .unwrap()
            .map_methods(["GET"], McpType::Resource);

        assert_eq!(mapping.get_mcp_type("GET", "/admin/users"), McpType::Skip);
        assert_eq!(mapping.get_mcp_type("GET", "/users"), McpType::Resource);
    }

    #[test]
    fn test_priority() {
        let mapping = RouteMapping::new()
            .add_rule(
                RouteRule::new(McpType::Resource)
                    .methods(["GET"])
                    .priority(0),
            )
            .add_rule(
                RouteRule::new(McpType::Tool)
                    .pattern(r"/api/.*")
                    .unwrap()
                    .priority(10),
            );

        // Higher priority rule (pattern) should win
        assert_eq!(mapping.get_mcp_type("GET", "/api/users"), McpType::Tool);
        // Lower priority rule should apply when pattern doesn't match
        assert_eq!(mapping.get_mcp_type("GET", "/users"), McpType::Resource);
    }

    #[test]
    fn test_route_rule_matches() {
        let rule = RouteRule::new(McpType::Tool)
            .methods(["POST", "PUT"])
            .pattern(r"/users/\d+")
            .unwrap();

        assert!(rule.matches("POST", "/users/123"));
        assert!(rule.matches("PUT", "/users/456"));
        assert!(!rule.matches("GET", "/users/123")); // Wrong method
        assert!(!rule.matches("POST", "/users/abc")); // Doesn't match pattern
    }
}
