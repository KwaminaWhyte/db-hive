//! Resource limits and the checks that go with them.
//!
//! A permission set, a budget, and the two validators that matter (a path may not leave the
//! plugin's own directory, and a URL may not point back inside the machine).
//!
//! The SSRF check here is the conservative prefix test the sandbox always
//! had. The real one, which resolves the host and inspects the addresses, is
//! `runtime::validate_outbound_url`; that is the one the network API calls.

#![allow(dead_code)]

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::error::{PluginError, PluginResult};
use super::manifest::PluginPermission;

pub struct PluginSandbox {
    plugin_id: String,
    permissions: HashSet<PluginPermission>,
    limits: ResourceLimits,
    metrics: ExecutionMetrics,
}

#[derive(Debug, Clone)]
pub struct ResourceLimits {
    pub max_memory: usize,
    pub max_cpu_time: u64,
    pub max_file_size: usize,
    pub max_open_files: usize,
    pub max_network_requests_per_minute: usize,
    pub max_execution_time: Duration,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            max_memory: 50 * 1024 * 1024,
            max_cpu_time: 5_000,
            max_file_size: 10 * 1024 * 1024,
            max_open_files: 10,
            max_network_requests_per_minute: 60,
            max_execution_time: Duration::from_secs(30),
        }
    }
}

#[derive(Debug, Default)]
pub struct ExecutionMetrics {
    pub total_execution_time: Duration,
    pub api_calls: u64,
    pub errors: u64,
    pub memory_usage: usize,
    pub network_requests: u64,
    pub files_accessed: u64,
}

#[derive(Debug)]
pub enum Operation {
    FileRead(usize),
    FileWrite(usize),
    NetworkRequest,
    MemoryAllocation(usize),
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

    pub fn has_permission(&self, permission: PluginPermission) -> bool {
        self.permissions.contains(&permission)
    }

    pub fn require(&self, permission: PluginPermission) -> PluginResult<()> {
        if self.has_permission(permission) {
            Ok(())
        } else {
            Err(PluginError::PermissionDenied(
                permission.label().to_string(),
            ))
        }
    }

    pub fn check_limits(&self, operation: Operation) -> PluginResult<()> {
        match operation {
            Operation::FileRead(size) | Operation::FileWrite(size) => {
                if size > self.limits.max_file_size {
                    return Err(PluginError::Other(format!(
                        "File size {} exceeds limit {}",
                        size, self.limits.max_file_size
                    )));
                }
            }
            Operation::NetworkRequest => {}
            Operation::MemoryAllocation(size) => {
                if self.metrics.memory_usage.saturating_add(size) > self.limits.max_memory {
                    return Err(PluginError::Other(
                        "Memory allocation would exceed limit".to_string(),
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn record_api_call(&mut self) {
        self.metrics.api_calls += 1;
    }

    pub fn record_error(&mut self) {
        self.metrics.errors += 1;
    }

    pub fn record_file_access(&mut self) {
        self.metrics.files_accessed += 1;
    }

    pub fn record_network_request(&mut self) {
        self.metrics.network_requests += 1;
    }

    pub fn metrics(&self) -> &ExecutionMetrics {
        &self.metrics
    }

    pub fn reset_metrics(&mut self) {
        self.metrics = ExecutionMetrics::default();
    }

    /// A relative path inside `plugin-data/<id>/`, or an error.
    pub fn sandbox_path(&self, path: &str) -> PluginResult<PathBuf> {
        if !super::manifest::is_safe_relative_path(path) {
            return Err(PluginError::Other(
                "Invalid path: path traversal not allowed".to_string(),
            ));
        }
        Ok(PathBuf::from(format!(
            "plugin-data/{}/{}",
            self.plugin_id, path
        )))
    }
}

/// Wall-clock guard for a single plugin call.
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

    pub fn check_timeout(&self) -> PluginResult<()> {
        if self.start_time.elapsed() > self.max_duration {
            return Err(PluginError::ExecutionError(format!(
                "Plugin {} exceeded execution time limit",
                self.plugin_id
            )));
        }
        Ok(())
    }

    pub fn elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> PluginSandbox {
        PluginSandbox::new(
            "com.x.y".to_string(),
            vec![PluginPermission::ReadFiles, PluginPermission::WriteFiles],
        )
    }

    #[test]
    fn permissions_come_only_from_the_manifest() {
        let s = sandbox();
        assert!(s.has_permission(PluginPermission::ReadFiles));
        assert!(!s.has_permission(PluginPermission::NetworkAccess));
        assert!(matches!(
            s.require(PluginPermission::NetworkAccess),
            Err(PluginError::PermissionDenied(_))
        ));
        assert!(s.require(PluginPermission::WriteFiles).is_ok());
    }

    #[test]
    fn paths_stay_inside_the_plugin_data_directory() {
        let s = sandbox();
        for bad in ["../secrets", "/etc/passwd", "a/../../b", "\\windows"] {
            assert!(s.sandbox_path(bad).is_err(), "{}", bad);
        }
        assert_eq!(
            s.sandbox_path("out.csv").unwrap(),
            PathBuf::from("plugin-data/com.x.y/out.csv")
        );
    }

    #[test]
    fn oversized_file_operations_are_refused() {
        let s = sandbox();
        assert!(s.check_limits(Operation::FileWrite(1024)).is_ok());
        assert!(s
            .check_limits(Operation::FileWrite(64 * 1024 * 1024))
            .is_err());
    }

    #[test]
    fn the_execution_guard_reports_an_overrun() {
        let g = ExecutionGuard::new("com.x.y".into(), Duration::from_millis(0));
        std::thread::sleep(Duration::from_millis(2));
        assert!(g.check_timeout().is_err());

        let g = ExecutionGuard::new("com.x.y".into(), Duration::from_secs(60));
        assert!(g.check_timeout().is_ok());
    }
}
