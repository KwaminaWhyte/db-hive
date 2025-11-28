use super::{PluginError, PluginPermission, PluginResult};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Sandbox environment for plugin execution
pub struct PluginSandbox {
    /// Plugin ID for this sandbox
    plugin_id: String,

    /// Granted permissions
    permissions: HashSet<PluginPermission>,

    /// Resource limits
    limits: ResourceLimits,

    /// Execution metrics
    metrics: ExecutionMetrics,
}

#[derive(Debug, Clone)]
pub struct ResourceLimits {
    /// Maximum memory usage in bytes
    pub max_memory: usize,

    /// Maximum CPU time in milliseconds
    pub max_cpu_time: u64,

    /// Maximum file size for read/write operations
    pub max_file_size: usize,

    /// Maximum number of files that can be opened
    pub max_open_files: usize,

    /// Maximum network requests per minute
    pub max_network_requests_per_minute: usize,

    /// Maximum execution time for a single plugin call
    pub max_execution_time: Duration,
}

#[derive(Debug, Default)]
pub struct ExecutionMetrics {
    /// Total execution time
    pub total_execution_time: Duration,

    /// Number of API calls made
    pub api_calls: u64,

    /// Number of errors
    pub errors: u64,

    /// Memory usage
    pub memory_usage: usize,

    /// Network requests made
    pub network_requests: u64,

    /// Files accessed
    pub files_accessed: u64,
}

impl PluginSandbox {
    pub fn new(plugin_id: String, permissions: Vec<PluginPermission>) -> Self {
        Self {
            plugin_id,
            permissions: permissions.into_iter().collect(),
            limits: ResourceLimits::default(),
            metrics: ExecutionMetrics::default(),
        }
    }

    /// Check if a permission is granted
    pub fn has_permission(&self, permission: &PluginPermission) -> bool {
        self.permissions.contains(permission)
    }

    /// Check resource limits before an operation
    pub fn check_limits(&self, operation: Operation) -> PluginResult<()> {
        match operation {
            Operation::FileRead(size) => {
                if size > self.limits.max_file_size {
                    return Err(PluginError::Other(format!(
                        "File size {} exceeds limit {}",
                        size, self.limits.max_file_size
                    )));
                }
            }
            Operation::FileWrite(size) => {
                if size > self.limits.max_file_size {
                    return Err(PluginError::Other(format!(
                        "File size {} exceeds limit {}",
                        size, self.limits.max_file_size
                    )));
                }
            }
            Operation::NetworkRequest => {
                // Check rate limiting
                // TODO: Implement proper rate limiting
            }
            Operation::MemoryAllocation(size) => {
                if self.metrics.memory_usage + size > self.limits.max_memory {
                    return Err(PluginError::Other(format!(
                        "Memory allocation would exceed limit"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Record an API call
    pub fn record_api_call(&mut self) {
        self.metrics.api_calls += 1;
    }

    /// Record an error
    pub fn record_error(&mut self) {
        self.metrics.errors += 1;
    }

    /// Record file access
    pub fn record_file_access(&mut self) {
        self.metrics.files_accessed += 1;
    }

    /// Record network request
    pub fn record_network_request(&mut self) {
        self.metrics.network_requests += 1;
    }

    /// Update memory usage
    pub fn update_memory_usage(&mut self, bytes: usize) {
        self.metrics.memory_usage = bytes;
    }

    /// Get current metrics
    pub fn get_metrics(&self) -> &ExecutionMetrics {
        &self.metrics
    }

    /// Reset metrics
    pub fn reset_metrics(&mut self) {
        self.metrics = ExecutionMetrics::default();
    }

    /// Create a sandboxed file path
    pub fn sandbox_path(&self, path: &str) -> PluginResult<std::path::PathBuf> {
        // Ensure the path doesn't contain dangerous patterns
        if path.contains("..") || path.starts_with('/') || path.starts_with('\\') {
            return Err(PluginError::Other(
                "Invalid path: Path traversal not allowed".to_string(),
            ));
        }

        // Return sandboxed path within plugin's data directory
        Ok(std::path::PathBuf::from(format!(
            "plugin-data/{}/{}",
            self.plugin_id, path
        )))
    }

    /// Validate a URL for network requests
    pub fn validate_url(&self, url: &str) -> PluginResult<()> {
        // Block local network access unless explicitly allowed
        if url.starts_with("http://localhost")
            || url.starts_with("http://127.0.0.1")
            || url.starts_with("http://0.0.0.0")
        {
            return Err(PluginError::Other(
                "Access to local network is restricted".to_string(),
            ));
        }

        // Block file:// URLs
        if url.starts_with("file://") {
            return Err(PluginError::Other(
                "File URLs are not allowed".to_string(),
            ));
        }

        Ok(())
    }
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory: 50 * 1024 * 1024,        // 50 MB
            max_cpu_time: 5000,                  // 5 seconds
            max_file_size: 10 * 1024 * 1024,     // 10 MB
            max_open_files: 10,
            max_network_requests_per_minute: 60,
            max_execution_time: Duration::from_secs(30),
        }
    }
}

#[derive(Debug)]
pub enum Operation {
    FileRead(usize),
    FileWrite(usize),
    NetworkRequest,
    MemoryAllocation(usize),
}

/// Execution guard that enforces time limits
pub struct ExecutionGuard {
    start_time: Instant,
    max_duration: Duration,
    plugin_id: String,
}

impl ExecutionGuard {
    pub fn new(plugin_id: String, max_duration: Duration) -> Self {
        Self {
            start_time: Instant::now(),
            max_duration,
            plugin_id,
        }
    }

    /// Check if execution time limit has been exceeded
    pub fn check_timeout(&self) -> PluginResult<()> {
        if self.start_time.elapsed() > self.max_duration {
            return Err(PluginError::ExecutionError(format!(
                "Plugin {} exceeded execution time limit",
                self.plugin_id
            )));
        }
        Ok(())
    }

    /// Get elapsed time
    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}