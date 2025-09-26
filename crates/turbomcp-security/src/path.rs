//! Path validation and canonicalization for security

use crate::error::{SecurityError, SecurityResult};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// Security policy for path validation
#[derive(Debug, Clone)]
pub struct SecurityPolicy {
    /// Maximum allowed file size (bytes)
    pub max_file_size: u64,
    /// Maximum directory depth
    pub max_directory_depth: usize,
    /// Allowed file extensions (with leading dot)
    pub allowed_extensions: Option<HashSet<String>>,
    /// Forbidden file extensions (with leading dot)
    pub forbidden_extensions: HashSet<String>,
    /// Allowed base directories (must be absolute paths)
    pub allowed_base_paths: Option<HashSet<PathBuf>>,
    /// Forbidden paths (deny list)
    pub forbidden_paths: HashSet<PathBuf>,
    /// Allow symlinks (default: false for security)
    pub allow_symlinks: bool,
    /// Require absolute paths
    pub require_absolute_paths: bool,
}

impl Default for SecurityPolicy {
    fn default() -> Self {
        let mut forbidden_extensions = HashSet::new();
        forbidden_extensions.insert(".exe".to_string());
        forbidden_extensions.insert(".bat".to_string());
        forbidden_extensions.insert(".cmd".to_string());
        forbidden_extensions.insert(".com".to_string());
        forbidden_extensions.insert(".scr".to_string());
        forbidden_extensions.insert(".msi".to_string());
        forbidden_extensions.insert(".dll".to_string());

        let mut forbidden_paths = HashSet::new();
        forbidden_paths.insert(PathBuf::from("/etc"));
        forbidden_paths.insert(PathBuf::from("/proc"));
        forbidden_paths.insert(PathBuf::from("/sys"));
        forbidden_paths.insert(PathBuf::from("/dev"));
        forbidden_paths.insert(PathBuf::from("/root"));
        forbidden_paths.insert(PathBuf::from("/boot"));
        // Windows system paths
        forbidden_paths.insert(PathBuf::from("C:\\Windows"));
        forbidden_paths.insert(PathBuf::from("C:\\System32"));
        forbidden_paths.insert(PathBuf::from("C:\\Program Files"));

        Self {
            max_file_size: 10 * 1024 * 1024, // 10MB
            max_directory_depth: 10,
            allowed_extensions: None, // Allow all by default
            forbidden_extensions,
            allowed_base_paths: None, // Allow all by default (but check forbidden)
            forbidden_paths,
            allow_symlinks: false,
            require_absolute_paths: false,
        }
    }
}

impl SecurityPolicy {
    /// Set maximum file size limit
    pub fn max_file_size(mut self, size: u64) -> Self {
        self.max_file_size = size;
        self
    }

    /// Set maximum directory depth
    pub fn max_directory_depth(mut self, depth: usize) -> Self {
        self.max_directory_depth = depth;
        self
    }

    /// Set allowed file extensions (replaces current list)
    pub fn allowed_extensions(mut self, extensions: &[&str]) -> Self {
        let mut ext_set = HashSet::new();
        for ext in extensions {
            let ext = if ext.starts_with('.') {
                ext.to_string()
            } else {
                format!(".{}", ext)
            };
            ext_set.insert(ext);
        }
        self.allowed_extensions = Some(ext_set);
        self
    }

    /// Add forbidden file extensions
    pub fn forbidden_extensions(mut self, extensions: &[&str]) -> Self {
        for ext in extensions {
            let ext = if ext.starts_with('.') {
                ext.to_string()
            } else {
                format!(".{}", ext)
            };
            self.forbidden_extensions.insert(ext);
        }
        self
    }

    /// Set allowed base paths (replaces current list)
    pub fn allowed_base_paths(mut self, paths: &[&str]) -> Self {
        let mut path_set = HashSet::new();
        for path in paths {
            path_set.insert(PathBuf::from(path));
        }
        self.allowed_base_paths = Some(path_set);
        self
    }

    /// Add forbidden paths
    pub fn forbidden_paths(mut self, paths: &[&str]) -> Self {
        for path in paths {
            self.forbidden_paths.insert(PathBuf::from(path));
        }
        self
    }

    /// Allow or disallow symlinks
    pub fn allow_symlinks(mut self, allow: bool) -> Self {
        self.allow_symlinks = allow;
        self
    }

    /// Require absolute paths
    pub fn require_absolute_paths(mut self, require: bool) -> Self {
        self.require_absolute_paths = require;
        self
    }
}

/// Path validator implementing comprehensive security checks
#[derive(Debug, Clone)]
pub struct PathValidator {
    policy: SecurityPolicy,
}

impl PathValidator {
    /// Create a new path validator with the given policy
    pub fn new(policy: SecurityPolicy) -> Self {
        Self { policy }
    }

    /// Validate a path according to security policy
    pub fn validate_path(&self, path: &Path) -> SecurityResult<PathBuf> {
        debug!("Validating path: {:?}", path);

        // Check if path is absolute (if required)
        if self.policy.require_absolute_paths && !path.is_absolute() {
            return Err(SecurityError::InvalidInput(format!(
                "Relative paths not allowed: {}",
                path.display()
            )));
        }

        // Check for path traversal patterns FIRST (before canonicalization)
        self.check_path_traversal(path)?;

        // Canonicalize path to resolve symlinks and relative components
        let canonical_path = self.canonicalize_safely(path)?;

        // Check symlink policy
        if !self.policy.allow_symlinks {
            self.check_symlinks(path, &canonical_path)?;
        }

        // Check against forbidden paths
        self.check_forbidden_paths(&canonical_path)?;

        // Check against allowed base paths
        self.check_allowed_base_paths(&canonical_path)?;

        // Check directory depth
        self.check_directory_depth(&canonical_path)?;

        // Check file extension
        self.check_file_extension(&canonical_path)?;

        debug!("Path validation successful: {:?}", canonical_path);
        Ok(canonical_path)
    }

    /// Special validation for socket paths
    pub fn validate_socket_path(&self, path: &Path) -> SecurityResult<PathBuf> {
        debug!("Validating socket path: {:?}", path);

        // Socket files have different requirements
        let canonical_path = self.canonicalize_safely(path)?;

        // Check for path traversal
        self.check_path_traversal(path)?;

        // Socket files should not be in system directories
        let system_dirs = ["/proc", "/sys", "/dev", "/boot", "/etc"];
        for sys_dir in &system_dirs {
            if canonical_path.starts_with(sys_dir) {
                return Err(SecurityError::UnauthorizedPath(format!(
                    "Socket files not allowed in system directory: {}",
                    canonical_path.display()
                )));
            }
        }

        // Ensure parent directory exists and is writable
        if let Some(parent) = canonical_path.parent() {
            if !parent.exists() {
                return Err(SecurityError::InvalidInput(format!(
                    "Parent directory does not exist: {}",
                    parent.display()
                )));
            }
        }

        Ok(canonical_path)
    }

    /// Safely canonicalize a path with error handling
    fn canonicalize_safely(&self, path: &Path) -> SecurityResult<PathBuf> {
        match path.canonicalize() {
            Ok(canonical) => Ok(canonical),
            Err(e) => match e.kind() {
                std::io::ErrorKind::NotFound => {
                    // For non-existent files, canonicalize the parent directory
                    // and join with the filename
                    if let Some(parent) = path.parent() {
                        if let Some(filename) = path.file_name() {
                            let canonical_parent = parent.canonicalize().map_err(|e| {
                                SecurityError::IoError(format!(
                                    "Failed to canonicalize parent directory {}: {}",
                                    parent.display(),
                                    e
                                ))
                            })?;
                            Ok(canonical_parent.join(filename))
                        } else {
                            Err(SecurityError::InvalidInput(format!(
                                "Invalid path structure: {}",
                                path.display()
                            )))
                        }
                    } else {
                        Err(SecurityError::InvalidInput(format!(
                            "Path has no parent directory: {}",
                            path.display()
                        )))
                    }
                }
                _ => Err(SecurityError::IoError(format!(
                    "Failed to canonicalize path {}: {}",
                    path.display(),
                    e
                ))),
            },
        }
    }

    /// Check for path traversal patterns
    fn check_path_traversal(&self, path: &Path) -> SecurityResult<()> {
        let path_str = path.to_string_lossy();

        // Check for various traversal patterns
        let traversal_patterns = [
            "../",
            "..\\",
            ".../",
            "...\\",
            "..;/",
            "..;\\",
            "..%2f",
            "..%5c",
            "..%2F",
            "..%5C",
            "%2e%2e%2f",
            "%2e%2e%5c",
            "%2e%2e/",
            "%2e%2e\\",
        ];

        for pattern in &traversal_patterns {
            if path_str.contains(pattern) {
                warn!(
                    "Path traversal pattern detected: {} in {}",
                    pattern, path_str
                );
                return Err(SecurityError::PathTraversal(format!(
                    "Path contains traversal pattern '{}': {}",
                    pattern,
                    path.display()
                )));
            }
        }

        // Check for double dot components
        for component in path.components() {
            let component_str = component.as_os_str().to_string_lossy();
            if component_str == ".." {
                return Err(SecurityError::PathTraversal(format!(
                    "Path contains parent directory reference: {}",
                    path.display()
                )));
            }
        }

        Ok(())
    }

    /// Check for symlink attacks
    fn check_symlinks(&self, original: &Path, canonical: &Path) -> SecurityResult<()> {
        // If the canonical path differs from original, there might be symlinks
        if original.to_string_lossy() != canonical.to_string_lossy() {
            // Additional check: iterate through path components to detect symlinks
            let mut current = PathBuf::new();
            for component in original.components() {
                current.push(component);
                if current.is_symlink() {
                    warn!(
                        "Symlink detected in path: {} -> {}",
                        current.display(),
                        canonical.display()
                    );
                    return Err(SecurityError::SymlinkAttack(format!(
                        "Symlink found in path: {} points to {}",
                        current.display(),
                        canonical.display()
                    )));
                }
            }
        }
        Ok(())
    }

    /// Check against forbidden paths
    fn check_forbidden_paths(&self, path: &Path) -> SecurityResult<()> {
        for forbidden in &self.policy.forbidden_paths {
            if path.starts_with(forbidden) {
                warn!("Access to forbidden path attempted: {}", path.display());
                return Err(SecurityError::UnauthorizedPath(format!(
                    "Access denied to forbidden path: {}",
                    path.display()
                )));
            }
        }
        Ok(())
    }

    /// Check against allowed base paths
    fn check_allowed_base_paths(&self, path: &Path) -> SecurityResult<()> {
        if let Some(ref allowed_paths) = self.policy.allowed_base_paths {
            let mut is_allowed = false;
            for allowed in allowed_paths {
                if path.starts_with(allowed) {
                    is_allowed = true;
                    break;
                }
            }

            if !is_allowed {
                warn!("Access to unauthorized path attempted: {}", path.display());
                return Err(SecurityError::UnauthorizedPath(format!(
                    "Path not under allowed base paths: {}",
                    path.display()
                )));
            }
        }
        Ok(())
    }

    /// Check directory depth
    fn check_directory_depth(&self, path: &Path) -> SecurityResult<()> {
        let depth = path.components().count();
        if depth > self.policy.max_directory_depth {
            return Err(SecurityError::DirectoryDepthExceeded {
                actual: depth,
                limit: self.policy.max_directory_depth,
            });
        }
        Ok(())
    }

    /// Check file extension
    fn check_file_extension(&self, path: &Path) -> SecurityResult<()> {
        if let Some(extension) = path.extension() {
            let ext = format!(".{}", extension.to_string_lossy().to_lowercase());

            // Check forbidden extensions
            if self.policy.forbidden_extensions.contains(&ext) {
                return Err(SecurityError::ForbiddenExtension(format!(
                    "File extension '{}' is forbidden",
                    ext
                )));
            }

            // Check allowed extensions (if whitelist is configured)
            if let Some(ref allowed) = self.policy.allowed_extensions {
                if !allowed.contains(&ext) {
                    return Err(SecurityError::ForbiddenExtension(format!(
                        "File extension '{}' is not in allowed list",
                        ext
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_security_policy_builder() {
        let policy = SecurityPolicy::default()
            .max_file_size(5 * 1024 * 1024)
            .allowed_extensions(&[".json", ".txt"])
            .forbidden_paths(&["/sensitive"]);

        assert_eq!(policy.max_file_size, 5 * 1024 * 1024);
        assert!(policy.allowed_extensions.is_some());
        assert!(policy
            .forbidden_paths
            .contains(&PathBuf::from("/sensitive")));
    }

    #[test]
    fn test_path_traversal_detection() {
        let validator = PathValidator::new(SecurityPolicy::default());

        let malicious_paths = [
            "../etc/passwd",
            "..\\windows\\system32",
            "data/../../../sensitive",
            "./data/../../etc/hosts",
            "data/file../..",
        ];

        for path in &malicious_paths {
            let result = validator.validate_path(Path::new(path));
            assert!(result.is_err(), "Should reject path: {}", path);

            if let Err(SecurityError::PathTraversal(_)) = result {
                // Expected
            } else {
                panic!("Expected PathTraversal error for: {}", path);
            }
        }
    }

    #[test]
    fn test_forbidden_extension_detection() {
        let validator = PathValidator::new(SecurityPolicy::default());

        let malicious_files = ["/tmp/malware.exe", "/tmp/script.bat", "/tmp/payload.dll"];

        for file in &malicious_files {
            let result = validator.validate_path(Path::new(file));
            assert!(result.is_err(), "Should reject file: {}", file);
        }
    }

    #[test]
    fn test_allowed_extensions_whitelist() {
        let policy = SecurityPolicy::default()
            .allowed_extensions(&[".json", ".txt"])
            .allow_symlinks(true); // Allow symlinks for temp directory on macOS
        let validator = PathValidator::new(policy);

        // These should be allowed
        let temp_dir = TempDir::new().unwrap();
        let allowed_file = temp_dir.path().join("data.json");
        std::fs::write(&allowed_file, "{}").unwrap();
        assert!(validator.validate_path(&allowed_file).is_ok());

        // This should be rejected (not in whitelist)
        let forbidden_file = temp_dir.path().join("data.xml");
        std::fs::write(&forbidden_file, "<xml/>").unwrap();
        assert!(validator.validate_path(&forbidden_file).is_err());
    }

    #[test]
    fn test_directory_depth_limit() {
        let policy = SecurityPolicy::default()
            .max_directory_depth(3)
            .allow_symlinks(true); // Allow symlinks for temp directory on macOS
        let validator = PathValidator::new(policy);

        let temp_dir = TempDir::new().unwrap();

        // Create a deep directory structure
        let deep_dir = temp_dir
            .path()
            .join("a")
            .join("b")
            .join("c")
            .join("d")
            .join("e");
        std::fs::create_dir_all(&deep_dir).unwrap();
        let deep_file = deep_dir.join("file.txt");
        std::fs::write(&deep_file, "content").unwrap();

        let result = validator.validate_path(&deep_file);
        assert!(result.is_err());

        if let Err(SecurityError::DirectoryDepthExceeded { actual, limit }) = result {
            assert!(actual > limit);
        } else {
            panic!("Expected DirectoryDepthExceeded error");
        }
    }

    #[test]
    fn test_socket_path_validation() {
        let validator = PathValidator::new(SecurityPolicy::default());
        let temp_dir = TempDir::new().unwrap();

        // Valid socket path
        let socket_path = temp_dir.path().join("test.sock");
        let result = validator.validate_socket_path(&socket_path);
        assert!(result.is_ok());

        // Invalid socket path (system directory)
        let system_socket = Path::new("/proc/test.sock");
        let result = validator.validate_socket_path(system_socket);
        assert!(result.is_err());
    }

    #[test]
    fn test_safe_canonicalization() {
        let validator = PathValidator::new(SecurityPolicy::default());
        let temp_dir = TempDir::new().unwrap();

        // Test with non-existent file
        let non_existent = temp_dir.path().join("does_not_exist.txt");
        let result = validator.canonicalize_safely(&non_existent);
        assert!(result.is_ok());

        let canonical = result.unwrap();
        assert!(canonical.starts_with(temp_dir.path().canonicalize().unwrap()));
        assert!(canonical.ends_with("does_not_exist.txt"));
    }
}
