//! Environment configuration for different deployment contexts

/// Environment configuration
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Environment {
    /// Development environment with permissive settings
    #[default]
    Development,
    /// Staging environment with moderate security
    Staging,
    /// Production environment with maximum security
    Production,
}

impl Environment {
    /// Check if this is a development environment
    pub fn is_development(&self) -> bool {
        matches!(self, Environment::Development)
    }

    /// Check if this is a staging environment
    pub fn is_staging(&self) -> bool {
        matches!(self, Environment::Staging)
    }

    /// Check if this is a production environment
    pub fn is_production(&self) -> bool {
        matches!(self, Environment::Production)
    }

    /// Get environment as string
    pub fn as_str(&self) -> &'static str {
        match self {
            Environment::Development => "development",
            Environment::Staging => "staging",
            Environment::Production => "production",
        }
    }
}
