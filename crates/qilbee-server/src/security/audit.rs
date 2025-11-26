//! Audit logging service with persistent storage

use chrono::{DateTime, Utc};
use qilbee_core::Result;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::collections::VecDeque;
use uuid::Uuid;

/// Audit event representing a security-relevant action
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    /// Unique identifier for this event
    pub event_id: String,
    /// When the event occurred
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
    /// Transaction time (when the event was recorded)
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
#[derive(Debug, Clone)]
pub struct AuditFilter {
    pub user_id: Option<String>,
    pub action: Option<String>,
    pub resource: Option<String>,
    pub result: Option<AuditResult>,
    pub start_time: Option<DateTime<Utc>>,
    pub end_time: Option<DateTime<Utc>>,
}

impl AuditFilter {
    pub fn new() -> Self {
        Self {
            user_id: None,
            action: None,
            resource: None,
            result: None,
            start_time: None,
            end_time: None,
        }
    }

    pub fn user_id(mut self, user_id: String) -> Self {
        self.user_id = Some(user_id);
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

impl Default for AuditFilter {
    fn default() -> Self {
        Self::new()
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
}

impl Default for AuditConfig {
    fn default() -> Self {
        Self {
            max_events: 100000,
            retention_days: 90,
            enabled: true,
        }
    }
}

/// Main audit service
pub struct AuditService {
    log: Arc<AuditLog>,
    config: AuditConfig,
}

impl AuditService {
    /// Create new audit service with configuration
    pub fn new(config: AuditConfig) -> Self {
        Self {
            log: Arc::new(AuditLog::new(config.max_events, config.retention_days)),
            config,
        }
    }

    /// Log an audit event
    pub fn log_event(
        &self,
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

        self.log.log(event);
    }

    /// Convenience method for logging authentication events
    pub fn log_auth_event(
        &self,
        username: &str,
        action: &str,
        result: AuditResult,
        ip_address: Option<String>,
    ) {
        self.log_event(
            None,
            Some(username.to_string()),
            action.to_string(),
            "authentication".to_string(),
            result,
            ip_address,
            None,
            serde_json::json!({}),
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
        self.log_event(
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
    }

    #[test]
    fn test_audit_service() {
        let service = AuditService::new(AuditConfig::default());

        service.log_auth_event("testuser", "login", AuditResult::Success, Some("127.0.0.1".to_string()));
        service.log_access("user123", "testuser", "read", "graph:456", AuditResult::Success);

        assert_eq!(service.event_count(), 2);

        let events = service.get_recent_events(10);
        assert_eq!(events.len(), 2);
    }

    #[test]
    fn test_retention() {
        let log = AuditLog::new(100, 1); // 1 day retention

        // Add old event
        let mut old_event = AuditEvent {
            event_id: Uuid::new_v4().to_string(),
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
}
