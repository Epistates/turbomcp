//! Resource limitation and monitoring for file operations

use crate::error::{SecurityError, SecurityResult};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, warn};

/// Resource policy for limiting file operations
#[derive(Debug, Clone)]
pub struct ResourcePolicy {
    /// Maximum file size for regular file operations (bytes)
    pub max_file_size: u64,
    /// Maximum file size for memory mapping (bytes)
    pub max_mmap_size: u64,
    /// Maximum concurrent file operations
    pub max_concurrent_files: usize,
    /// Maximum concurrent memory maps
    pub max_concurrent_mmaps: usize,
    /// Maximum memory usage for all file operations (bytes)
    pub max_memory_usage: u64,
    /// Maximum number of open file handles
    pub max_open_handles: usize,
    /// Rate limiting: max operations per second
    pub max_operations_per_second: u64,
    /// Enable resource monitoring and metrics
    pub enable_monitoring: bool,
}

impl Default for ResourcePolicy {
    fn default() -> Self {
        Self {
            max_file_size: 10 * 1024 * 1024,  // 10MB
            max_mmap_size: 100 * 1024 * 1024, // 100MB per mmap
            max_concurrent_files: 100,
            max_concurrent_mmaps: 10,
            max_memory_usage: 1024 * 1024 * 1024, // 1GB total
            max_open_handles: 1000,
            max_operations_per_second: 1000,
            enable_monitoring: true,
        }
    }
}

impl ResourcePolicy {
    /// Set maximum file size
    pub fn max_file_size(mut self, size: u64) -> Self {
        self.max_file_size = size;
        self
    }

    /// Set maximum memory map size
    pub fn max_mmap_size(mut self, size: u64) -> Self {
        self.max_mmap_size = size;
        self
    }

    /// Set maximum concurrent file operations
    pub fn max_concurrent_files(mut self, count: usize) -> Self {
        self.max_concurrent_files = count;
        self
    }

    /// Set maximum concurrent memory maps
    pub fn max_concurrent_mmaps(mut self, count: usize) -> Self {
        self.max_concurrent_mmaps = count;
        self
    }

    /// Set maximum total memory usage
    pub fn max_memory_usage(mut self, size: u64) -> Self {
        self.max_memory_usage = size;
        self
    }

    /// Set rate limiting
    pub fn max_operations_per_second(mut self, rate: u64) -> Self {
        self.max_operations_per_second = rate;
        self
    }

    /// Enable or disable monitoring
    pub fn enable_monitoring(mut self, enable: bool) -> Self {
        self.enable_monitoring = enable;
        self
    }
}

/// Resource usage statistics
#[derive(Debug, Default, Clone)]
pub struct ResourceStats {
    /// Current number of open files
    pub open_files: usize,
    /// Current number of memory maps
    pub active_mmaps: usize,
    /// Current memory usage (bytes)
    pub memory_usage: u64,
    /// Total file operations performed
    pub total_operations: u64,
    /// Total file operations denied
    pub denied_operations: u64,
    /// Current rate of operations per second
    pub current_rate: f64,
}

/// Resource limiter for controlling file access
#[derive(Debug)]
pub struct ResourceLimiter {
    policy: ResourcePolicy,
    /// Semaphore for concurrent file operations
    file_semaphore: Arc<Semaphore>,
    /// Semaphore for concurrent memory maps
    mmap_semaphore: Arc<Semaphore>,
    /// Current resource usage statistics
    stats: Arc<Mutex<ResourceStats>>,
    /// Rate limiter state
    rate_limiter: Arc<Mutex<RateLimiter>>,
    /// Active file operations tracking
    active_files: Arc<Mutex<HashMap<String, FileOperation>>>,
}

#[derive(Debug)]
#[allow(dead_code)] // Fields used in planned audit/monitoring features
struct FileOperation {
    path: String,
    size: u64,
    operation_type: OperationType,
    started_at: std::time::Instant,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Variants used in planned audit/monitoring features
enum OperationType {
    Read,
    Write,
    MemoryMap,
}

/// Simple rate limiter implementation
#[derive(Debug)]
struct RateLimiter {
    operations: Vec<std::time::Instant>,
    window_size: std::time::Duration,
    max_operations: usize,
}

impl RateLimiter {
    fn new(max_ops_per_second: u64) -> Self {
        Self {
            operations: Vec::new(),
            window_size: std::time::Duration::from_secs(1),
            max_operations: max_ops_per_second as usize,
        }
    }

    fn check_rate_limit(&mut self) -> bool {
        let now = std::time::Instant::now();

        // Remove old operations outside the window
        self.operations
            .retain(|&op_time| now.duration_since(op_time) < self.window_size);

        if self.operations.len() < self.max_operations {
            self.operations.push(now);
            true
        } else {
            false
        }
    }

    fn current_rate(&self) -> f64 {
        self.operations.len() as f64
    }
}

impl ResourceLimiter {
    /// Create a new resource limiter
    pub fn new(policy: ResourcePolicy) -> Self {
        let file_semaphore = Arc::new(Semaphore::new(policy.max_concurrent_files));
        let mmap_semaphore = Arc::new(Semaphore::new(policy.max_concurrent_mmaps));
        let rate_limiter = Arc::new(Mutex::new(RateLimiter::new(
            policy.max_operations_per_second,
        )));

        Self {
            policy,
            file_semaphore,
            mmap_semaphore,
            stats: Arc::new(Mutex::new(ResourceStats::default())),
            rate_limiter,
            active_files: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Check if file access is allowed and acquire resources
    pub async fn check_file_access(&self, path: &Path) -> SecurityResult<FileAccessGuard> {
        // Check rate limiting
        {
            let mut rate_limiter = self.rate_limiter.lock().await;
            if !rate_limiter.check_rate_limit() {
                let mut stats = self.stats.lock().await;
                stats.denied_operations += 1;
                return Err(SecurityError::ResourceLimitExceeded {
                    resource_type: "rate_limit".to_string(),
                    details: format!(
                        "Rate limit exceeded: {} operations/sec",
                        self.policy.max_operations_per_second
                    ),
                });
            }
        }

        // Get file metadata for size checking
        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|e| SecurityError::IoError(format!("Failed to get file metadata: {}", e)))?;

        let file_size = metadata.len();

        // Check file size limit
        if file_size > self.policy.max_file_size {
            let mut stats = self.stats.lock().await;
            stats.denied_operations += 1;
            return Err(SecurityError::FileSizeLimitExceeded {
                actual: file_size,
                limit: self.policy.max_file_size,
            });
        }

        // Try to acquire semaphore permit (fail immediately if none available)
        let permit = self
            .file_semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| SecurityError::ResourceLimitExceeded {
                resource_type: "concurrent_files".to_string(),
                details: format!(
                    "Concurrent file limit exceeded: {} files already open",
                    self.policy.max_concurrent_files
                ),
            })?;

        // Check memory usage
        {
            let mut stats = self.stats.lock().await;
            if stats.memory_usage + file_size > self.policy.max_memory_usage {
                stats.denied_operations += 1;
                return Err(SecurityError::ResourceLimitExceeded {
                    resource_type: "memory_usage".to_string(),
                    details: format!(
                        "Would exceed memory limit: {} + {} > {}",
                        stats.memory_usage, file_size, self.policy.max_memory_usage
                    ),
                });
            }

            // Update stats
            stats.open_files += 1;
            stats.memory_usage += file_size;
            stats.total_operations += 1;
        }

        // Track active file operation
        let operation = FileOperation {
            path: path.to_string_lossy().to_string(),
            size: file_size,
            operation_type: OperationType::Read,
            started_at: std::time::Instant::now(),
        };

        let operation_id = uuid::Uuid::new_v4().to_string();
        {
            let mut active = self.active_files.lock().await;
            active.insert(operation_id.clone(), operation);
        }

        debug!(
            "File access granted: {} ({} bytes)",
            path.display(),
            file_size
        );

        Ok(FileAccessGuard {
            _permit: permit,
            operation_id,
            file_size,
            limiter: self.clone(),
        })
    }

    /// Check if memory map access is allowed
    pub async fn check_mmap_access(&self, size: usize) -> SecurityResult<MmapAccessGuard> {
        let size = size as u64;

        // Check mmap size limit
        if size > self.policy.max_mmap_size {
            let mut stats = self.stats.lock().await;
            stats.denied_operations += 1;
            return Err(SecurityError::FileSizeLimitExceeded {
                actual: size,
                limit: self.policy.max_mmap_size,
            });
        }

        // Try to acquire mmap semaphore permit (fail immediately if none available)
        let permit = self
            .mmap_semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| SecurityError::ResourceLimitExceeded {
                resource_type: "concurrent_mmaps".to_string(),
                details: format!(
                    "Concurrent mmap limit exceeded: {} mmaps already active",
                    self.policy.max_concurrent_mmaps
                ),
            })?;

        // Check memory usage
        {
            let mut stats = self.stats.lock().await;
            if stats.memory_usage + size > self.policy.max_memory_usage {
                stats.denied_operations += 1;
                return Err(SecurityError::ResourceLimitExceeded {
                    resource_type: "memory_usage".to_string(),
                    details: format!(
                        "Mmap would exceed memory limit: {} + {} > {}",
                        stats.memory_usage, size, self.policy.max_memory_usage
                    ),
                });
            }

            // Update stats
            stats.active_mmaps += 1;
            stats.memory_usage += size;
            stats.total_operations += 1;
        }

        debug!("Memory map access granted: {} bytes", size);

        Ok(MmapAccessGuard {
            _permit: permit,
            size,
            limiter: self.clone(),
        })
    }

    /// Check directory access permissions
    pub async fn check_directory_access(&self, path: &Path) -> SecurityResult<()> {
        let metadata = tokio::fs::metadata(path).await.map_err(|e| {
            SecurityError::PermissionDenied(format!(
                "Cannot access directory {}: {}",
                path.display(),
                e
            ))
        })?;

        if !metadata.is_dir() {
            return Err(SecurityError::InvalidInput(format!(
                "Path is not a directory: {}",
                path.display()
            )));
        }

        // Check if directory is readable
        if metadata.permissions().readonly() {
            warn!("Directory is read-only: {}", path.display());
        }

        Ok(())
    }

    /// Get current resource usage statistics
    pub async fn get_stats(&self) -> ResourceStats {
        let mut stats = self.stats.lock().await;
        let rate_limiter = self.rate_limiter.lock().await;
        stats.current_rate = rate_limiter.current_rate();
        stats.clone()
    }

    /// Force cleanup of resources (emergency use)
    pub async fn force_cleanup(&self) {
        warn!("Forcing resource cleanup - this may affect ongoing operations");

        // Clear active operations tracking
        {
            let mut active = self.active_files.lock().await;
            active.clear();
        }

        // Reset stats (but keep counters for metrics)
        {
            let mut stats = self.stats.lock().await;
            stats.open_files = 0;
            stats.active_mmaps = 0;
            stats.memory_usage = 0;
            // Keep total_operations and denied_operations for metrics
        }
    }
}

impl Clone for ResourceLimiter {
    fn clone(&self) -> Self {
        Self {
            policy: self.policy.clone(),
            file_semaphore: Arc::clone(&self.file_semaphore),
            mmap_semaphore: Arc::clone(&self.mmap_semaphore),
            stats: Arc::clone(&self.stats),
            rate_limiter: Arc::clone(&self.rate_limiter),
            active_files: Arc::clone(&self.active_files),
        }
    }
}

/// RAII guard for file access operations
pub struct FileAccessGuard {
    _permit: tokio::sync::OwnedSemaphorePermit,
    operation_id: String,
    file_size: u64,
    limiter: ResourceLimiter,
}

impl Drop for FileAccessGuard {
    fn drop(&mut self) {
        // Update stats on drop (async operation in sync drop - using spawn)
        let limiter = self.limiter.clone();
        let operation_id = self.operation_id.clone();
        let file_size = self.file_size;

        tokio::spawn(async move {
            // Remove from active operations
            {
                let mut active = limiter.active_files.lock().await;
                active.remove(&operation_id);
            }

            // Update stats
            {
                let mut stats = limiter.stats.lock().await;
                stats.open_files = stats.open_files.saturating_sub(1);
                stats.memory_usage = stats.memory_usage.saturating_sub(file_size);
            }

            debug!("File access guard dropped: {} bytes released", file_size);
        });
    }
}

/// RAII guard for memory map operations
pub struct MmapAccessGuard {
    _permit: tokio::sync::OwnedSemaphorePermit,
    size: u64,
    limiter: ResourceLimiter,
}

impl Drop for MmapAccessGuard {
    fn drop(&mut self) {
        // Update stats on drop
        let limiter = self.limiter.clone();
        let size = self.size;

        tokio::spawn(async move {
            let mut stats = limiter.stats.lock().await;
            stats.active_mmaps = stats.active_mmaps.saturating_sub(1);
            stats.memory_usage = stats.memory_usage.saturating_sub(size);

            debug!("Memory map guard dropped: {} bytes released", size);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_resource_policy_builder() {
        let policy = ResourcePolicy::default()
            .max_file_size(5 * 1024 * 1024)
            .max_concurrent_files(50)
            .max_operations_per_second(100);

        assert_eq!(policy.max_file_size, 5 * 1024 * 1024);
        assert_eq!(policy.max_concurrent_files, 50);
        assert_eq!(policy.max_operations_per_second, 100);
    }

    #[tokio::test]
    async fn test_file_size_limit() {
        let policy = ResourcePolicy::default().max_file_size(100); // 100 bytes
        let limiter = ResourceLimiter::new(policy);
        let temp_dir = TempDir::new().unwrap();

        // Create a file larger than the limit
        let large_file = temp_dir.path().join("large.txt");
        let large_data = vec![b'x'; 200]; // 200 bytes
        tokio::fs::write(&large_file, large_data).await.unwrap();

        // Should be rejected
        let result = limiter.check_file_access(&large_file).await;
        assert!(result.is_err());

        if let Err(SecurityError::FileSizeLimitExceeded { actual, limit }) = result {
            assert_eq!(actual, 200);
            assert_eq!(limit, 100);
        } else {
            panic!("Expected FileSizeLimitExceeded error");
        }
    }

    #[tokio::test]
    async fn test_concurrent_file_limit() {
        let policy = ResourcePolicy::default().max_concurrent_files(2);
        let limiter = ResourceLimiter::new(policy);
        let temp_dir = TempDir::new().unwrap();

        // Create test files
        let file1 = temp_dir.path().join("file1.txt");
        let file2 = temp_dir.path().join("file2.txt");
        let file3 = temp_dir.path().join("file3.txt");

        tokio::fs::write(&file1, b"data1").await.unwrap();
        tokio::fs::write(&file2, b"data2").await.unwrap();
        tokio::fs::write(&file3, b"data3").await.unwrap();

        // Acquire first two permits
        let _guard1 = limiter.check_file_access(&file1).await.unwrap();
        let _guard2 = limiter.check_file_access(&file2).await.unwrap();

        // Third should fail
        let result = limiter.check_file_access(&file3).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_memory_usage_tracking() {
        let policy = ResourcePolicy::default().max_memory_usage(1000);
        let limiter = ResourceLimiter::new(policy);
        let temp_dir = TempDir::new().unwrap();

        // Create files that together exceed memory limit
        let file1 = temp_dir.path().join("file1.txt");
        let file2 = temp_dir.path().join("file2.txt");

        tokio::fs::write(&file1, vec![b'x'; 600]).await.unwrap();
        tokio::fs::write(&file2, vec![b'x'; 600]).await.unwrap();

        // First file should succeed
        let _guard1 = limiter.check_file_access(&file1).await.unwrap();

        // Second file should fail (would exceed memory limit)
        let result = limiter.check_file_access(&file2).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let policy = ResourcePolicy::default().max_operations_per_second(2);
        let limiter = ResourceLimiter::new(policy);
        let temp_dir = TempDir::new().unwrap();

        let test_file = temp_dir.path().join("test.txt");
        tokio::fs::write(&test_file, b"data").await.unwrap();

        // First two operations should succeed
        let _guard1 = limiter.check_file_access(&test_file).await.unwrap();
        let _guard2 = limiter.check_file_access(&test_file).await.unwrap();

        // Third should fail due to rate limiting
        let result = limiter.check_file_access(&test_file).await;
        assert!(result.is_err());

        if let Err(SecurityError::ResourceLimitExceeded { resource_type, .. }) = result {
            assert_eq!(resource_type, "rate_limit");
        } else {
            panic!("Expected rate limit error");
        }
    }

    #[tokio::test]
    async fn test_mmap_size_limit() {
        let policy = ResourcePolicy::default().max_mmap_size(100);
        let limiter = ResourceLimiter::new(policy);

        // Should succeed
        let _guard1 = limiter.check_mmap_access(50).await.unwrap();

        // Should fail
        let result = limiter.check_mmap_access(200).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_resource_stats() {
        let policy = ResourcePolicy::default();
        let limiter = ResourceLimiter::new(policy);
        let temp_dir = TempDir::new().unwrap();

        let test_file = temp_dir.path().join("stats_test.txt");
        tokio::fs::write(&test_file, b"test data").await.unwrap();

        // Initial stats
        let stats = limiter.get_stats().await;
        assert_eq!(stats.open_files, 0);
        assert_eq!(stats.total_operations, 0);

        // Access file
        let _guard = limiter.check_file_access(&test_file).await.unwrap();

        // Updated stats
        let stats = limiter.get_stats().await;
        assert_eq!(stats.open_files, 1);
        assert_eq!(stats.total_operations, 1);
        assert!(stats.memory_usage > 0);
    }

    #[tokio::test]
    async fn test_guard_cleanup() {
        let policy = ResourcePolicy::default();
        let limiter = ResourceLimiter::new(policy);
        let temp_dir = TempDir::new().unwrap();

        let test_file = temp_dir.path().join("cleanup_test.txt");
        tokio::fs::write(&test_file, b"test data").await.unwrap();

        {
            let _guard = limiter.check_file_access(&test_file).await.unwrap();

            // Stats should show active file
            let stats = limiter.get_stats().await;
            assert_eq!(stats.open_files, 1);
        } // Guard drops here

        // Give tokio a chance to run the cleanup task
        tokio::task::yield_now().await;

        // Stats should be cleaned up
        let stats = limiter.get_stats().await;
        assert_eq!(stats.open_files, 0);
    }
}
