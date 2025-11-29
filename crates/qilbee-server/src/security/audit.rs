//! Audit logging service with persistent storage
//!
//! Provides comprehensive bi-temporal audit logging for security events.
//! Events are stored both in-memory (for fast queries) and on disk (for persistence).

use chrono::{DateTime, Utc};
use qilbee_core::Result;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// Typed audit event categories for better filtering and analysis
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventType {
    // Authentication events
    Login,
    LoginFailed,
    Logout,
    TokenRefresh,
    TokenRefreshFailed,

    // API Key events
    ApiKeyCreated,
    ApiKeyRevoked,
    ApiKeyUsed,
    ApiKeyValidationFailed,

    // User management events
    UserCreated,
    UserUpdated,
    UserDeleted,
    UserPasswordChanged,

    // Role management events
    RoleAssigned,
    RoleRemoved,

    // Authorization events
    PermissionDenied,
    AccessGranted,

    // Rate limiting events
    RateLimitExceeded,

    // Token revocation events
    TokenRevoked,
    AllTokensRevoked,

    // Account lockout events
    AccountLocked,
    AccountUnlocked,
    AccountLockoutTriggered,

    // System events
    SystemStartup,
    SystemShutdown,
    ConfigurationChanged,

    // Memory operation events
    MemoryConsolidated,
    MemoryForgotten,
    MemoryCleared,
}

impl std::fmt::Display for AuditEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditEventType::Login => write!(f, "login"),
            AuditEventType::LoginFailed => write!(f, "login_failed"),
            AuditEventType::Logout => write!(f, "logout"),
            AuditEventType::TokenRefresh => write!(f, "token_refresh"),
            AuditEventType::TokenRefreshFailed => write!(f, "token_refresh_failed"),
            AuditEventType::ApiKeyCreated => write!(f, "api_key_created"),
            AuditEventType::ApiKeyRevoked => write!(f, "api_key_revoked"),
            AuditEventType::ApiKeyUsed => write!(f, "api_key_used"),
            AuditEventType::ApiKeyValidationFailed => write!(f, "api_key_validation_failed"),
            AuditEventType::UserCreated => write!(f, "user_created"),
            AuditEventType::UserUpdated => write!(f, "user_updated"),
            AuditEventType::UserDeleted => write!(f, "user_deleted"),
            AuditEventType::UserPasswordChanged => write!(f, "user_password_changed"),
            AuditEventType::RoleAssigned => write!(f, "role_assigned"),
            AuditEventType::RoleRemoved => write!(f, "role_removed"),
            AuditEventType::PermissionDenied => write!(f, "permission_denied"),
            AuditEventType::AccessGranted => write!(f, "access_granted"),
            AuditEventType::RateLimitExceeded => write!(f, "rate_limit_exceeded"),
            AuditEventType::TokenRevoked => write!(f, "token_revoked"),
            AuditEventType::AllTokensRevoked => write!(f, "all_tokens_revoked"),
            AuditEventType::AccountLocked => write!(f, "account_locked"),
            AuditEventType::AccountUnlocked => write!(f, "account_unlocked"),
            AuditEventType::AccountLockoutTriggered => write!(f, "account_lockout_triggered"),
            AuditEventType::SystemStartup => write!(f, "system_startup"),
            AuditEventType::SystemShutdown => write!(f, "system_shutdown"),
            AuditEventType::ConfigurationChanged => write!(f, "configuration_changed"),
            AuditEventType::MemoryConsolidated => write!(f, "memory_consolidated"),
            AuditEventType::MemoryForgotten => write!(f, "memory_forgotten"),
            AuditEventType::MemoryCleared => write!(f, "memory_cleared"),
        }
    }
}

/// Audit event representing a security-relevant action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique identifier for this event
    pub event_id: String,
    /// Typed event category
    pub event_type: AuditEventType,
    /// When the event occurred (event time in bi-temporal model)
    pub timestamp: DateTime<Utc>,
    /// ID of the user who performed the action (if authenticated)
    pub user_id: Option<String>,
    /// Username (for convenience)
    pub username: Option<String>,
    /// Action performed (e.g., "login", "create_node", "delete_graph")
    pub action: String,
    /// Resource affected (e.g., "graph:123", "user:456")
    pub resource: String,
    /// Result of the action (e.g., "success", "failure", "unauthorized")
    pub result: AuditResult,
    /// IP address of the client
    pub ip_address: Option<String>,
    /// User agent of the client
    pub user_agent: Option<String>,
    /// Additional metadata as JSON
    pub metadata: serde_json::Value,
    /// Transaction time (when the event was recorded - transaction time in bi-temporal model)
    pub transaction_time: DateTime<Utc>,
}

/// Audit result types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuditResult {
    Success,
    Failure,
    Unauthorized,
    Forbidden,
    Error,
}

impl std::fmt::Display for AuditResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuditResult::Success => write!(f, "success"),
            AuditResult::Failure => write!(f, "failure"),
            AuditResult::Unauthorized => write!(f, "unauthorized"),
            AuditResult::Forbidden => write!(f, "forbidden"),
            AuditResult::Error => write!(f, "error"),
        }
    }
}

/// Filter for querying audit events
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditFilter {
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub event_type: Option<AuditEventType>,
    pub action: Option<String>,
    pub resource: Option<String>,
    pub result: Option<AuditResult>,
    pub ip_address: Option<String>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

impl AuditFilter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
        self
    }

    pub fn username(mut self, username: String) -> Self {
        self.username = Some(username);
        self
    }

    pub fn event_type(mut self, event_type: AuditEventType) -> Self {
        self.event_type = Some(event_type);
        self
    }

    pub fn action(mut self, action: String) -> Self {
        self.action = Some(action);
        self
    }

    pub fn resource(mut self, resource: String) -> Self {
        self.resource = Some(resource);
        self
    }

    pub fn result(mut self, result: AuditResult) -> Self {
        self.result = Some(result);
        self
    }

    pub fn ip_address(mut self, ip: String) -> Self {
        self.ip_address = Some(ip);
        self
    }

    pub fn time_range(mut self, start: DateTime<Utc>, end: DateTime<Utc>) -> Self {
        self.start_time = Some(start);
        self.end_time = Some(end);
        self
    }

    pub fn matches(&self, event: &AuditEvent) -> bool {
        if let Some(ref user_id) = self.user_id {
            if event.user_id.as_ref() != Some(user_id) {
                return false;
            }
        }

        if let Some(ref username) = self.username {
            if event.username.as_ref() != Some(username) {
                return false;
            }
        }

        if let Some(ref event_type) = self.event_type {
            if &event.event_type != event_type {
                return false;
            }
        }

        if let Some(ref action) = self.action {
            if &event.action != action {
                return false;
            }
        }

        if let Some(ref resource) = self.resource {
            if !event.resource.contains(resource) {
                return false;
            }
        }

        if let Some(ref result) = self.result {
            if &event.result != result {
                return false;
            }
        }

        if let Some(ref ip) = self.ip_address {
            if event.ip_address.as_ref() != Some(ip) {
                return false;
            }
        }

        if let Some(start) = self.start_time {
            if event.timestamp < start {
                return false;
            }
        }

        if let Some(end) = self.end_time {
            if event.timestamp > end {
                return false;
            }
        }

        true
    }
}


/// In-memory audit log with configurable retention
pub struct AuditLog {
    events: Arc<RwLock<VecDeque<AuditEvent>>>,
    max_size: usize,
    retention_days: i64,
}

impl AuditLog {
    pub fn new(max_size: usize, retention_days: i64) -> Self {
        Self {
            events: Arc::new(RwLock::new(VecDeque::with_capacity(max_size))),
            max_size,
            retention_days,
        }
    }

    pub fn log(&self, event: AuditEvent) {
        let mut events = self.events.write().unwrap();

        // Remove oldest event if at capacity
        if events.len() >= self.max_size {
            events.pop_front();
        }

        events.push_back(event);
    }

    pub fn get_recent(&self, limit: usize) -> Vec<AuditEvent> {
        let events = self.events.read().unwrap();
        events.iter().rev().take(limit).cloned().collect()
    }

    pub fn query(&self, filter: &AuditFilter, limit: usize) -> Vec<AuditEvent> {
        let events = self.events.read().unwrap();
        events
            .iter()
            .rev()
            .filter(|e| filter.matches(e))
            .take(limit)
            .cloned()
            .collect()
    }

    pub fn count(&self) -> usize {
        self.events.read().unwrap().len()
    }

    /// Clean up events older than retention period
    pub fn cleanup_old_events(&self) {
        let cutoff = Utc::now() - chrono::Duration::days(self.retention_days);
        let mut events = self.events.write().unwrap();

        // Remove events older than retention period
        while let Some(front) = events.front() {
            if front.timestamp < cutoff {
                events.pop_front();
            } else {
                break;
            }
        }
    }
}

/// Configuration for audit service
#[derive(Debug, Clone)]
pub struct AuditConfig {
    /// Maximum number of events to keep in memory
    pub max_events: usize,
    /// Number of days to retain events
    pub retention_days: i64,
    /// Whether to enable audit logging
    pub enabled: bool,
    /// Path to audit log directory (for file persistence)
    pub log_path: Option<PathBuf>,
    /// Maximum size of each log file in bytes before rotation (default: 10MB)
    pub max_file_size: u64,
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            max_events: 100000,
            retention_days: 90,
            enabled: true,
            log_path: None, // In-memory only by default
            max_file_size: 10 * 1024 * 1024, // 10MB
        }
    }
}

impl AuditConfig {
    /// Create config with file persistence enabled
    pub fn with_file_logging<P: AsRef<Path>>(log_path: P) -> Self {
        Self {
            log_path: Some(log_path.as_ref().to_path_buf()),
            ..Default::default()
        }
    }
}

/// File-based audit log writer for persistence
pub struct AuditFileWriter {
    log_path: PathBuf,
    max_file_size: u64,
    current_file: RwLock<Option<PathBuf>>,
}

impl AuditFileWriter {
    pub fn new(log_path: PathBuf, max_file_size: u64) -> Self {
        // Create log directory if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(&log_path) {
            eprintln!("Warning: Failed to create audit log directory: {}", e);
        }

        Self {
            log_path,
            max_file_size,
            current_file: RwLock::new(None),
        }
    }

    /// Get the current log file path (creates new one if needed)
    fn get_current_file(&self) -> PathBuf {
        let mut current = self.current_file.write().unwrap();

        // Check if current file exists and is under size limit
        if let Some(ref path) = *current {
            if path.exists() {
                if let Ok(metadata) = std::fs::metadata(path) {
                    if metadata.len() < self.max_file_size {
                        return path.clone();
                    }
                }
            }
        }

        // Create new file with timestamp
        let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
        let filename = format!("audit_{}.jsonl", timestamp);
        let path = self.log_path.join(filename);
        *current = Some(path.clone());
        path
    }

    /// Append event to file (append-only for tamper evidence)
    pub fn write_event(&self, event: &AuditEvent) {
        let file_path = self.get_current_file();

        match OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)
        {
            Ok(mut file) => {
                if let Ok(json) = serde_json::to_string(event) {
                    if let Err(e) = writeln!(file, "{}", json) {
                        eprintln!("Warning: Failed to write audit event: {}", e);
                    }
                }
            }
            Err(e) => {
                eprintln!("Warning: Failed to open audit log file: {}", e);
            }
        }
    }

    /// Load events from all log files (for recovery)
    pub fn load_events(&self, filter: &AuditFilter, limit: usize) -> Vec<AuditEvent> {
        let mut events = Vec::new();

        // Get all .jsonl files sorted by name (oldest first)
        let mut files: Vec<_> = std::fs::read_dir(&self.log_path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().extension().map_or(false, |ext| ext == "jsonl"))
            .collect();

        files.sort_by(|a, b| b.path().cmp(&a.path())); // Newest first

        for entry in files {
            if let Ok(file) = File::open(entry.path()) {
                let reader = BufReader::new(file);
                for line in reader.lines().flatten() {
                    if let Ok(event) = serde_json::from_str::<AuditEvent>(&line) {
                        if filter.matches(&event) {
                            events.push(event);
                            if events.len() >= limit {
                                return events;
                            }
                        }
                    }
                }
            }
        }

        events
    }

    /// Clean up old log files beyond retention period
    pub fn cleanup_old_files(&self, retention_days: i64) {
        let cutoff = Utc::now() - chrono::Duration::days(retention_days);

        if let Ok(entries) = std::fs::read_dir(&self.log_path) {
            for entry in entries.flatten() {
                if let Ok(metadata) = entry.metadata() {
                    if let Ok(modified) = metadata.modified() {
                        let modified_time: DateTime<Utc> = modified.into();
                        if modified_time < cutoff {
                            let _ = std::fs::remove_file(entry.path());
                        }
                    }
                }
            }
        }
    }
}

/// Main audit service
pub struct AuditService {
    log: Arc<AuditLog>,
    file_writer: Option<Arc<AuditFileWriter>>,
    config: AuditConfig,
}

impl AuditService {
    /// Create new audit service with configuration
    pub fn new(config: AuditConfig) -> Self {
        let file_writer = config.log_path.as_ref().map(|path| {
            Arc::new(AuditFileWriter::new(path.clone(), config.max_file_size))
        });

        Self {
            log: Arc::new(AuditLog::new(config.max_events, config.retention_days)),
            file_writer,
            config,
        }
    }

    /// Log an audit event with typed event
    pub fn log_event(
        &self,
        event_type: AuditEventType,
        user_id: Option<String>,
        username: Option<String>,
        action: String,
        resource: String,
        result: AuditResult,
        ip_address: Option<String>,
        user_agent: Option<String>,
        metadata: serde_json::Value,
    ) {
        if !self.config.enabled {
            return;
        }

        let event = AuditEvent {
            event_id: Uuid::new_v4().to_string(),
            event_type,
            timestamp: Utc::now(),
            user_id,
            username,
            action,
            resource,
            result,
            ip_address,
            user_agent,
            metadata,
            transaction_time: Utc::now(),
        };

        // Write to in-memory log
        self.log.log(event.clone());

        // Write to file if enabled
        if let Some(ref writer) = self.file_writer {
            writer.write_event(&event);
        }
    }

    /// Convenience method for logging authentication events
    pub fn log_auth_event(
        &self,
        event_type: AuditEventType,
        username: &str,
        result: AuditResult,
        ip_address: Option<String>,
        user_agent: Option<String>,
    ) {
        self.log_event(
            event_type.clone(),
            None,
            Some(username.to_string()),
            event_type.to_string(),
            "authentication".to_string(),
            result,
            ip_address,
            user_agent,
            serde_json::json!({}),
        );
    }

    /// Convenience method for logging user management events
    pub fn log_user_event(
        &self,
        event_type: AuditEventType,
        actor_id: &str,
        actor_username: &str,
        target_user_id: &str,
        result: AuditResult,
        ip_address: Option<String>,
        metadata: serde_json::Value,
    ) {
        self.log_event(
            event_type.clone(),
            Some(actor_id.to_string()),
            Some(actor_username.to_string()),
            event_type.to_string(),
            format!("user:{}", target_user_id),
            result,
            ip_address,
            None,
            metadata,
        );
    }

    /// Convenience method for logging API key events
    pub fn log_api_key_event(
        &self,
        event_type: AuditEventType,
        user_id: &str,
        username: &str,
        key_id: &str,
        result: AuditResult,
        ip_address: Option<String>,
    ) {
        self.log_event(
            event_type.clone(),
            Some(user_id.to_string()),
            Some(username.to_string()),
            event_type.to_string(),
            format!("api_key:{}", key_id),
            result,
            ip_address,
            None,
            serde_json::json!({"key_id": key_id}),
        );
    }

    /// Convenience method for logging resource access
    pub fn log_access(
        &self,
        user_id: &str,
        username: &str,
        action: &str,
        resource: &str,
        result: AuditResult,
    ) {
        let event_type = if result == AuditResult::Success {
            AuditEventType::AccessGranted
        } else {
            AuditEventType::PermissionDenied
        };

        self.log_event(
            event_type,
            Some(user_id.to_string()),
            Some(username.to_string()),
            action.to_string(),
            resource.to_string(),
            result,
            None,
            None,
            serde_json::json!({}),
        );
    }

    /// Log rate limit exceeded event
    pub fn log_rate_limit_exceeded(
        &self,
        user_id: Option<String>,
        username: Option<String>,
        endpoint: &str,
        ip_address: Option<String>,
    ) {
        self.log_event(
            AuditEventType::RateLimitExceeded,
            user_id,
            username,
            "rate_limit_exceeded".to_string(),
            endpoint.to_string(),
            AuditResult::Forbidden,
            ip_address,
            None,
            serde_json::json!({"endpoint": endpoint}),
        );
    }

    /// Log memory operation event (consolidate, forget, clear)
    pub fn log_memory_event(
        &self,
        event_type: AuditEventType,
        user_id: Option<String>,
        username: Option<String>,
        agent_id: &str,
        result: AuditResult,
        ip_address: Option<String>,
        metadata: serde_json::Value,
    ) {
        self.log_event(
            event_type.clone(),
            user_id,
            username,
            event_type.to_string(),
            format!("agent_memory:{}", agent_id),
            result,
            ip_address,
            None,
            metadata,
        );
    }

    /// Get recent audit events
    pub fn get_recent_events(&self, limit: usize) -> Vec<AuditEvent> {
        self.log.get_recent(limit)
    }

    /// Query audit events with filter
    pub fn query_events(&self, filter: AuditFilter, limit: usize) -> Vec<AuditEvent> {
        self.log.query(&filter, limit)
    }

    /// Get total event count
    pub fn event_count(&self) -> usize {
        self.log.count()
    }

    /// Clean up old events (should be called periodically)
    pub fn cleanup(&self) {
        self.log.cleanup_old_events();
    }

    /// Get events for a specific user
    pub fn get_user_events(&self, user_id: &str, limit: usize) -> Vec<AuditEvent> {
        let filter = AuditFilter::new().user_id(user_id.to_string());
        self.query_events(filter, limit)
    }

    /// Get failed authentication attempts
    pub fn get_failed_auth_attempts(&self, username: &str, limit: usize) -> Vec<AuditEvent> {
        let filter = AuditFilter::new()
            .action("login".to_string())
            .result(AuditResult::Failure);

        self.query_events(filter, limit)
            .into_iter()
            .filter(|e| e.username.as_ref() == Some(&username.to_string()))
            .collect()
    }

    /// Export audit events to JSON
    pub fn export_events(&self, filter: Option<AuditFilter>, limit: usize) -> Result<String> {
        let events = if let Some(f) = filter {
            self.query_events(f, limit)
        } else {
            self.get_recent_events(limit)
        };

        serde_json::to_string_pretty(&events)
            .map_err(|e| qilbee_core::Error::Internal(format!("Failed to serialize events: {}", e)))
    }
}

impl Default for AuditService {
    fn default() -> Self {
        Self::new(AuditConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_log() {
        let log = AuditLog::new(100, 90);

        let event = AuditEvent {
            event_id: Uuid::new_v4().to_string(),
            event_type: AuditEventType::AccessGranted,
            timestamp: Utc::now(),
            user_id: Some("user123".to_string()),
            username: Some("testuser".to_string()),
            action: "create_node".to_string(),
            resource: "graph:456".to_string(),
            result: AuditResult::Success,
            ip_address: Some("127.0.0.1".to_string()),
            user_agent: None,
            metadata: serde_json::json!({"node_id": "789"}),
            transaction_time: Utc::now(),
        };

        log.log(event.clone());
        assert_eq!(log.count(), 1);

        let recent = log.get_recent(10);
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].action, "create_node");
    }

    #[test]
    fn test_audit_filter() {
        let log = AuditLog::new(100, 90);

        // Log multiple events
        for i in 0..5 {
            log.log(AuditEvent {
                event_id: Uuid::new_v4().to_string(),
                event_type: if i % 2 == 0 { AuditEventType::UserCreated } else { AuditEventType::UserDeleted },
                timestamp: Utc::now(),
                user_id: Some(format!("user{}", i)),
                username: Some(format!("user{}", i)),
                action: if i % 2 == 0 { "create".to_string() } else { "delete".to_string() },
                resource: format!("resource{}", i),
                result: if i % 2 == 0 { AuditResult::Success } else { AuditResult::Failure },
                ip_address: None,
                user_agent: None,
                metadata: serde_json::json!({}),
                transaction_time: Utc::now(),
            });
        }

        // Filter by action
        let filter = AuditFilter::new().action("create".to_string());
        let results = log.query(&filter, 10);
        assert_eq!(results.len(), 3);

        // Filter by result
        let filter = AuditFilter::new().result(AuditResult::Failure);
        let results = log.query(&filter, 10);
        assert_eq!(results.len(), 2);

        // Filter by event_type
        let filter = AuditFilter::new().event_type(AuditEventType::UserCreated);
        let results = log.query(&filter, 10);
        assert_eq!(results.len(), 3);
    }

    #[test]
    fn test_audit_service() {
        let service = AuditService::new(AuditConfig::default());

        service.log_auth_event(AuditEventType::Login, "testuser", AuditResult::Success, Some("127.0.0.1".to_string()), None);
        service.log_access("user123", "testuser", "read", "graph:456", AuditResult::Success);

        assert_eq!(service.event_count(), 2);

        let events = service.get_recent_events(10);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_retention() {
        let log = AuditLog::new(100, 1); // 1 day retention

        // Add old event
        let old_event = AuditEvent {
            event_id: Uuid::new_v4().to_string(),
            event_type: AuditEventType::Login,
            timestamp: Utc::now() - chrono::Duration::days(2),
            user_id: Some("user123".to_string()),
            username: Some("testuser".to_string()),
            action: "old_action".to_string(),
            resource: "old_resource".to_string(),
            result: AuditResult::Success,
            ip_address: None,
            user_agent: None,
            metadata: serde_json::json!({}),
            transaction_time: Utc::now() - chrono::Duration::days(2),
        };

        log.log(old_event);
        assert_eq!(log.count(), 1);

        // Cleanup
        log.cleanup_old_events();
        assert_eq!(log.count(), 0);
    }

    #[test]
    fn test_file_writer() {
        let temp_dir = std::env::temp_dir().join("qilbee_audit_test");
        let _ = std::fs::remove_dir_all(&temp_dir);

        let writer = AuditFileWriter::new(temp_dir.clone(), 1024 * 1024);

        let event = AuditEvent {
            event_id: Uuid::new_v4().to_string(),
            event_type: AuditEventType::Login,
            timestamp: Utc::now(),
            user_id: Some("user123".to_string()),
            username: Some("testuser".to_string()),
            action: "login".to_string(),
            resource: "authentication".to_string(),
            result: AuditResult::Success,
            ip_address: Some("127.0.0.1".to_string()),
            user_agent: Some("Test/1.0".to_string()),
            metadata: serde_json::json!({}),
            transaction_time: Utc::now(),
        };

        writer.write_event(&event);

        // Load events back
        let loaded = writer.load_events(&AuditFilter::default(), 100);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].username, Some("testuser".to_string()));

        // Cleanup
        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
