//! # Parameter Validation for TurboMCP
//!
//! TurboMCP validates parameters at the application layer using **type-driven deserialization**.
//! This is the same pattern as Axum's extractors, but using `serde`'s `TryFrom` integration
//! instead of `FromRequest`.
//!
//! ## Philosophy
//!
//! TurboMCP validates data during deserialization, **before** your tool handler executes:
//!
//! ```text
//! JSON-RPC Request
//!   ↓
//! serde deserialize (with TryFrom)
//!   ↓
//! VALIDATION HAPPENS HERE ← Type-driven, automatic
//!   ↓
//! Tool handler (receives validated types)
//! ```
//!
//! This is equivalent to Axum's pattern:
//!
//! ```rust,ignore
//! // Axum: Validation during extraction
//! async fn handler(ValidatedJson(user): ValidatedJson<User>) -> impl IntoResponse {
//!     // user is already validated
//! }
//!
//! // TurboMCP: Validation during deserialization
//! #[tool("register")]
//! async fn register(&self, email: Email) -> McpResult<String> {
//!     // email is already validated (impossible to construct invalid Email)
//! }
//! ```
//!
//! ## Quick Start
//!
//! Use the patterns in this module as examples to copy and customize:
//!
//! ```rust,ignore
//! use turbomcp::validation::patterns::Email;
//!
//! #[turbomcp::server(name = "user-service", version = "1.0.0")]
//! impl MyServer {
//!     #[tool("Register user")]
//!     async fn register(&self, email: Email) -> McpResult<String> {
//!         // email is guaranteed valid by type system!
//!         Ok(format!("Registered: {}", email.as_str()))
//!     }
//! }
//! ```
//!
//! ## Validation Approaches
//!
//! TurboMCP supports multiple validation approaches. Choose based on your needs:
//!
//! ### 1. Manual Newtypes (Zero Dependencies - Built-in)
//!
//! **When to use:** Simple validation rules, maximum control, zero dependencies.
//!
//! ```rust,ignore
//! use serde::{Deserialize, Serialize};
//!
//! #[derive(Debug, Clone, Serialize, Deserialize)]
//! #[serde(try_from = "String", into = "String")]
//! pub struct Email(String);
//!
//! impl Email {
//!     pub fn new(s: String) -> Result<Self, EmailError> {
//!         if !s.contains('@') || !s.contains('.') {
//!             return Err(EmailError::Invalid);
//!         }
//!         Ok(Email(s))
//!     }
//!
//!     pub fn as_str(&self) -> &str {
//!         &self.0
//!     }
//! }
//!
//! impl TryFrom<String> for Email {
//!     type Error = EmailError;
//!     fn try_from(s: String) -> Result<Self, Self::Error> {
//!         Self::new(s)
//!     }
//! }
//!
//! impl From<Email> for String {
//!     fn from(email: Email) -> String {
//!         email.0
//!     }
//! }
//!
//! #[derive(Debug)]
//! pub enum EmailError {
//!     Invalid,
//! }
//! ```
//!
//! ### 2. Runtime Validation Libraries (User's Choice)
//!
//! **When to use:** Complex validation rules, nested validation, or ecosystem compatibility.
//!
//! #### Using `garde` (Modern, recommended for new projects)
//!
//! ```rust,ignore
//! use garde::Validate;
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, Validate)]
//! struct RegisterParams {
//!     #[garde(email)]
//!     email: String,
//!
//!     #[garde(length(min = 8))]
//!     #[garde(custom(check_password_strength))]
//!     password: String,
//! }
//!
//! fn check_password_strength(value: &str, _context: &()) -> garde::Result {
//!     if !value.chars().any(|c| c.is_numeric()) {
//!         return Err(garde::Error::new("Password must contain a number"));
//!     }
//!     Ok(())
//! }
//!
//! #[tool("Register user")]
//! async fn register(&self, params: serde_json::Value) -> McpResult<String> {
//!     let params: RegisterParams = serde_json::from_value(params)
//!         .map_err(|e| Error::invalid_params(format!("Parse error: {}", e)))?;
//!
//!     params.validate(&())
//!         .map_err(|e| Error::invalid_params(format!("Validation failed: {}", e)))?;
//!
//!     Ok(format!("Registered: {}", params.email))
//! }
//! ```
//!
//! #### Using `validator` (Mature, widely adopted)
//!
//! ```rust,ignore
//! use validator::Validate;
//! use serde::Deserialize;
//!
//! #[derive(Deserialize, Validate)]
//! struct RegisterParams {
//!     #[validate(email)]
//!     email: String,
//!
//!     #[validate(length(min = 8))]
//!     password: String,
//! }
//!
//! #[tool("Register user")]
//! async fn register(&self, params: serde_json::Value) -> McpResult<String> {
//!     let params: RegisterParams = serde_json::from_value(params)
//!         .map_err(|e| Error::invalid_params(format!("Parse error: {}", e)))?;
//!
//!     params.validate()
//!         .map_err(|e| Error::invalid_params(format!("Validation failed: {}", e)))?;
//!
//!     Ok(format!("Registered: {}", params.email))
//! }
//! ```
//!
//! ### 3. Type-Level Validation (Compile-Time Guarantees)
//!
//! **When to use:** Domain-Driven Design, strongest type safety, simple validation rules.
//!
//! #### Using `nutype`
//!
//! ```rust,ignore
//! use nutype::nutype;
//! use serde::{Deserialize, Serialize};
//!
//! #[nutype(
//!     sanitize(trim, lowercase),
//!     validate(email),
//!     derive(Debug, Clone, Serialize, Deserialize)
//! )]
//! pub struct Email(String);
//!
//! #[tool("Register user")]
//! async fn register(&self, email: Email) -> McpResult<String> {
//!     // Email is validated during deserialization - impossible to be invalid!
//!     Ok(format!("Registered: {}", email.as_str()))
//! }
//! ```
//!
//! ## Decision Tree
//!
//! ```text
//! Need validation?
//! │
//! ├─ Simple rules (email, length, range)?
//! │  ├─ Want zero dependencies?
//! │  │  └─► Manual newtypes (patterns module)
//! │  └─ Want automatic serde integration?
//! │     └─► nutype (add to your Cargo.toml)
//! │
//! ├─ Complex rules (nested, context-aware)?
//! │  ├─ Starting new project (2025)?
//! │  │  └─► garde (modern, best errors)
//! │  └─ Existing project or need ecosystem?
//! │     └─► validator (mature, widely adopted)
//! │
//! └─ Maximum control and flexibility?
//!    └─► Manual newtypes with custom logic
//! ```
//!
//! ## See Also
//!
//! - Examples: `validation_newtype.rs`, `validation_garde.rs`, `validation_nutype.rs`
//! - Guide: `VALIDATION_GUIDE.md` (comprehensive documentation)
//! - Industry patterns: Same as Axum, Actix-web, Rocket
//!
//! ## Why This Approach?
//!
//! **1. Industry Standard**
//! - Axum: Validation via extractors (type-driven)
//! - Actix-web: Validation via extractors (type-driven)
//! - TurboMCP: Validation via deserialization (type-driven)
//!
//! **2. Type Safety**
//! - Invalid values cannot exist (enforced by Rust's type system)
//! - Validation happens automatically during deserialization
//! - No need to remember to call `.validate()`
//!
//! **3. Zero Framework Dependencies**
//! - TurboMCP doesn't force any validation library
//! - User chooses validator (or none)
//! - Future-proof (new libraries can emerge)
//!
//! **4. Flexibility**
//! - Mix validation approaches freely
//! - Switch validators without changing TurboMCP
//! - Custom validation logic always possible

pub mod patterns {
    //! Example validated types (copy and customize for your needs)
    //!
    //! These are **patterns to learn from**, not production types. For production:
    //! - Simple cases: Copy these patterns and customize
    //! - Complex rules: Use `garde` or `validator` (add to your Cargo.toml)
    //! - DDD/Type-safety: Use `nutype` (add to your Cargo.toml)
    //!
    //! **Note:** These examples use basic validation logic. For production-grade
    //! email validation, use the `validator` crate internally in your newtype.

    use serde::{Deserialize, Serialize};
    use std::fmt;

    // =========================================================================
    // Email Address Validation
    // =========================================================================

    /// Validated email address
    ///
    /// Automatically validates during serde deserialization using the
    /// `TryFrom` pattern. Invalid emails cannot be constructed.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp::validation::patterns::Email;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// // Valid email
    /// let email = Email::new("user@example.com")?;
    /// assert_eq!(email.as_str(), "user@example.com");
    ///
    /// // Invalid email fails
    /// assert!(Email::new("invalid").is_err());
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Validation Rules
    ///
    /// - Must contain '@'
    /// - Must contain '.' after '@'
    /// - Length 3-254 characters (RFC 5321)
    ///
    /// # Production Usage
    ///
    /// For production, consider using the `validator` crate for RFC-compliant validation:
    ///
    /// ```rust,ignore
    /// impl TryFrom<String> for Email {
    ///     fn try_from(s: String) -> Result<Self, Self::Error> {
    ///         if validator::validate_email(&s) {
    ///             Ok(Email(s))
    ///         } else {
    ///             Err(EmailError::Invalid)
    ///         }
    ///     }
    /// }
    /// ```
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(try_from = "String", into = "String")]
    pub struct Email(String);

    impl Email {
        /// Create a validated email address
        pub fn new(s: impl Into<String>) -> Result<Self, EmailError> {
            let s = s.into();

            if s.len() < 3 {
                return Err(EmailError::TooShort);
            }
            if s.len() > 254 {
                return Err(EmailError::TooLong);
            }

            let at_pos = s.find('@').ok_or(EmailError::MissingAt)?;

            // Check for dot after @
            if !s[at_pos..].contains('.') {
                return Err(EmailError::MissingDomain);
            }

            // Basic local part checks
            if at_pos == 0 {
                return Err(EmailError::EmptyLocalPart);
            }

            // Basic domain checks
            if at_pos == s.len() - 1 {
                return Err(EmailError::EmptyDomain);
            }

            Ok(Self(s))
        }

        /// Get email as string slice
        pub fn as_str(&self) -> &str {
            &self.0
        }

        /// Consume and return inner string
        pub fn into_inner(self) -> String {
            self.0
        }
    }

    impl TryFrom<String> for Email {
        type Error = EmailError;

        fn try_from(s: String) -> Result<Self, Self::Error> {
            Self::new(s)
        }
    }

    impl From<Email> for String {
        fn from(email: Email) -> String {
            email.0
        }
    }

    impl fmt::Display for Email {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    /// Email validation error
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum EmailError {
        /// Email is too short (minimum 3 characters)
        TooShort,
        /// Email is too long (maximum 254 characters per RFC 5321)
        TooLong,
        /// Email must contain '@' symbol
        MissingAt,
        /// Email must have domain (text after '@' with '.')
        MissingDomain,
        /// Local part (before '@') cannot be empty
        EmptyLocalPart,
        /// Domain part (after '@') cannot be empty
        EmptyDomain,
    }

    impl fmt::Display for EmailError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                EmailError::TooShort => write!(f, "Email is too short (minimum 3 characters)"),
                EmailError::TooLong => write!(f, "Email is too long (maximum 254 characters)"),
                EmailError::MissingAt => write!(f, "Email must contain '@' symbol"),
                EmailError::MissingDomain => write!(f, "Email must have domain (e.g., '.com')"),
                EmailError::EmptyLocalPart => write!(f, "Email local part cannot be empty"),
                EmailError::EmptyDomain => write!(f, "Email domain cannot be empty"),
            }
        }
    }

    impl std::error::Error for EmailError {}

    // =========================================================================
    // Non-Empty String Validation
    // =========================================================================

    /// Non-empty string (after trimming whitespace)
    ///
    /// Ensures strings are not empty or contain only whitespace.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp::validation::patterns::NonEmpty;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// // Valid non-empty string
    /// let text = NonEmpty::new("Hello")?;
    /// assert_eq!(text.as_str(), "Hello");
    ///
    /// // Empty or whitespace-only strings fail
    /// assert!(NonEmpty::new("").is_err());
    /// assert!(NonEmpty::new("   ").is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(try_from = "String", into = "String")]
    pub struct NonEmpty(String);

    impl NonEmpty {
        /// Create a non-empty string
        pub fn new(s: impl Into<String>) -> Result<Self, NonEmptyError> {
            let s = s.into();
            if s.trim().is_empty() {
                Err(NonEmptyError::Empty)
            } else {
                Ok(Self(s))
            }
        }

        /// Get string as slice
        pub fn as_str(&self) -> &str {
            &self.0
        }

        /// Consume and return inner string
        pub fn into_inner(self) -> String {
            self.0
        }
    }

    impl TryFrom<String> for NonEmpty {
        type Error = NonEmptyError;

        fn try_from(s: String) -> Result<Self, Self::Error> {
            Self::new(s)
        }
    }

    impl From<NonEmpty> for String {
        fn from(val: NonEmpty) -> String {
            val.0
        }
    }

    impl fmt::Display for NonEmpty {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    /// Non-empty string validation error
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum NonEmptyError {
        /// String is empty or contains only whitespace
        Empty,
    }

    impl fmt::Display for NonEmptyError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "String cannot be empty or contain only whitespace")
        }
    }

    impl std::error::Error for NonEmptyError {}

    // =========================================================================
    // Bounded Value Validation
    // =========================================================================

    /// Value bounded within a range (inclusive)
    ///
    /// Generic type for numeric ranges. Ensures values are within [min, max].
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp::validation::patterns::Bounded;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// // Age between 0 and 120
    /// let age = Bounded::<u8>::new(25, 0, 120)?;
    /// assert_eq!(age.value(), 25);
    ///
    /// // Out of range fails
    /// assert!(Bounded::<u8>::new(150, 0, 120).is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    pub struct Bounded<T> {
        value: T,
        #[serde(skip)]
        min: T,
        #[serde(skip)]
        max: T,
    }

    impl<T> Bounded<T>
    where
        T: PartialOrd + Copy + fmt::Display,
    {
        /// Create a bounded value
        pub fn new(value: T, min: T, max: T) -> Result<Self, BoundedError> {
            if value < min || value > max {
                return Err(BoundedError::OutOfRange {
                    value: value.to_string(),
                    min: min.to_string(),
                    max: max.to_string(),
                });
            }
            Ok(Self { value, min, max })
        }

        /// Get the value
        pub fn value(&self) -> T {
            self.value
        }

        /// Get the minimum bound
        pub fn min(&self) -> T {
            self.min
        }

        /// Get the maximum bound
        pub fn max(&self) -> T {
            self.max
        }
    }

    /// Bounded value validation error
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum BoundedError {
        /// Value is outside the allowed range
        OutOfRange {
            /// The value that was provided
            value: String,
            /// The minimum allowed value
            min: String,
            /// The maximum allowed value
            max: String,
        },
    }

    impl fmt::Display for BoundedError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                BoundedError::OutOfRange { value, min, max } => {
                    write!(f, "Value {} is out of range [{}, {}]", value, min, max)
                }
            }
        }
    }

    impl std::error::Error for BoundedError {}

    // =========================================================================
    // URL Validation
    // =========================================================================

    /// Validated URL
    ///
    /// Basic URL validation. For production, consider using the `url` crate.
    ///
    /// # Example
    ///
    /// ```rust
    /// use turbomcp::validation::patterns::Url;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let url = Url::new("https://example.com")?;
    /// assert_eq!(url.as_str(), "https://example.com");
    ///
    /// assert!(Url::new("not a url").is_err());
    /// # Ok(())
    /// # }
    /// ```
    #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
    #[serde(try_from = "String", into = "String")]
    pub struct Url(String);

    impl Url {
        /// Create a validated URL
        pub fn new(s: impl Into<String>) -> Result<Self, UrlError> {
            let s = s.into();

            // Basic URL validation (production should use `url` crate)
            if !s.starts_with("http://") && !s.starts_with("https://") {
                return Err(UrlError::InvalidScheme);
            }

            if s.len() < 10 {
                // Minimum: "http://a.b"
                return Err(UrlError::TooShort);
            }

            Ok(Self(s))
        }

        /// Get URL as string slice
        pub fn as_str(&self) -> &str {
            &self.0
        }

        /// Consume and return inner string
        pub fn into_inner(self) -> String {
            self.0
        }
    }

    impl TryFrom<String> for Url {
        type Error = UrlError;

        fn try_from(s: String) -> Result<Self, Self::Error> {
            Self::new(s)
        }
    }

    impl From<Url> for String {
        fn from(url: Url) -> String {
            url.0
        }
    }

    impl fmt::Display for Url {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.0)
        }
    }

    /// URL validation error
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum UrlError {
        /// URL must start with http:// or https://
        InvalidScheme,
        /// URL is too short
        TooShort,
    }

    impl fmt::Display for UrlError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                UrlError::InvalidScheme => write!(f, "URL must start with http:// or https://"),
                UrlError::TooShort => write!(f, "URL is too short"),
            }
        }
    }

    impl std::error::Error for UrlError {}
}

#[cfg(test)]
mod tests {
    use super::patterns::*;

    #[test]
    fn test_email_validation() {
        // Valid emails
        assert!(Email::new("user@example.com").is_ok());
        assert!(Email::new("test.user@domain.co.uk").is_ok());
        assert!(Email::new("a@b.c").is_ok());

        // Invalid emails
        assert!(Email::new("").is_err());
        assert!(Email::new("notanemail").is_err());
        assert!(Email::new("missing@domain").is_err());
        assert!(Email::new("@example.com").is_err());
        assert!(Email::new("user@").is_err());
    }

    #[test]
    fn test_email_serde_roundtrip() {
        let email = Email::new("user@example.com").unwrap();
        let json = serde_json::to_string(&email).unwrap();
        assert_eq!(json, r#""user@example.com""#);

        let parsed: Email = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.as_str(), "user@example.com");
    }

    #[test]
    fn test_email_invalid_deserialization() {
        let result = serde_json::from_str::<Email>(r#""invalid""#);
        assert!(result.is_err());
    }

    #[test]
    fn test_non_empty_validation() {
        // Valid non-empty strings
        assert!(NonEmpty::new("hello").is_ok());
        assert!(NonEmpty::new("  hello  ").is_ok());

        // Invalid empty strings
        assert!(NonEmpty::new("").is_err());
        assert!(NonEmpty::new("   ").is_err());
    }

    #[test]
    fn test_bounded_validation() {
        // Valid bounded values
        assert!(Bounded::new(25, 0, 120).is_ok());
        assert!(Bounded::new(0, 0, 120).is_ok());
        assert!(Bounded::new(120, 0, 120).is_ok());

        // Invalid out of bounds
        assert!(Bounded::new(-1, 0, 120).is_err());
        assert!(Bounded::new(121, 0, 120).is_err());
    }

    #[test]
    fn test_url_validation() {
        // Valid URLs
        assert!(Url::new("https://example.com").is_ok());
        assert!(Url::new("http://localhost:8080").is_ok());

        // Invalid URLs
        assert!(Url::new("not a url").is_err());
        assert!(Url::new("ftp://example.com").is_err());
        assert!(Url::new("https://").is_err());
    }
}
