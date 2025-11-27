//! Account lockout module for brute-force attack prevention
//!
//! Tracks failed login attempts and locks accounts after exceeding thresholds.
//! Supports both time-based automatic unlock and manual admin unlock.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::RwLock;

/// Configuration for account lockout behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockoutConfig {
    /// Maximum failed attempts before lockout
    pub max_failed_attempts: u32,
    /// Duration of lockout in minutes
    pub lockout_duration_minutes: u32,
    /// Window in minutes to count failed attempts (resets after this time of no failures)
    pub attempt_window_minutes: u32,
    /// Whether to track by IP in addition to username
    pub track_by_ip: bool,
    /// Progressive lockout - multiply duration by number of lockouts
    pub progressive_lockout: bool,
}

impl Default for LockoutConfig {
    fn default() -> Self {
        Self {
            max_failed_attempts: 5,
            lockout_duration_minutes: 15,
            attempt_window_minutes: 30,
            track_by_ip: true,
            progressive_lockout: true,
        }
    }
}

/// Record of failed login attempts for a user or IP
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailedAttemptRecord {
    /// Number of failed attempts in current window
    pub attempts: u32,
    /// Timestamp of first failed attempt in window
    pub window_start: DateTime<Utc>,
    /// Timestamp of last failed attempt
    pub last_attempt: DateTime<Utc>,
    /// Whether account is currently locked
    pub locked: bool,
    /// When the lockout expires (if locked)
    pub lockout_expires: Option<DateTime<Utc>>,
    /// Number of times this account has been locked
    pub lockout_count: u32,
    /// Reason for lockout (if manually locked)
    pub lockout_reason: Option<String>,
}

impl FailedAttemptRecord {
    fn new() -> Self {
        let now = Utc::now();
        Self {
            attempts: 0,
            window_start: now,
            last_attempt: now,
            locked: false,
            lockout_expires: None,
            lockout_count: 0,
            lockout_reason: None,
        }
    }
}

/// Status of an account's lockout state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockoutStatus {
    /// Whether the account is currently locked
    pub locked: bool,
    /// Number of failed attempts in current window
    pub failed_attempts: u32,
    /// Remaining attempts before lockout
    pub remaining_attempts: u32,
    /// When the lockout expires (if locked)
    pub lockout_expires: Option<DateTime<Utc>>,
    /// Seconds until lockout expires
    pub lockout_remaining_seconds: Option<i64>,
    /// Total number of times this account has been locked
    pub lockout_count: u32,
    /// Reason for lockout (if manually locked)
    pub lockout_reason: Option<String>,
}

/// Account lockout service
pub struct AccountLockoutService {
    config: LockoutConfig,
    /// Failed attempts by username
    user_attempts: RwLock<HashMap<String, FailedAttemptRecord>>,
    /// Failed attempts by IP address
    ip_attempts: RwLock<HashMap<String, FailedAttemptRecord>>,
}

impl AccountLockoutService {
    pub fn new(config: LockoutConfig) -> Self {
        Self {
            config,
            user_attempts: RwLock::new(HashMap::new()),
            ip_attempts: RwLock::new(HashMap::new()),
        }
    }

    /// Check if a login attempt should be allowed
    /// Returns Ok(()) if allowed, Err with LockoutStatus if blocked
    pub fn check_login_allowed(&self, username: &str, ip_address: Option<&str>) -> Result<(), LockoutStatus> {
        // Check user lockout
        let user_status = self.get_user_status(username);
        if user_status.locked {
            return Err(user_status);
        }

        // Check IP lockout if enabled
        if self.config.track_by_ip {
            if let Some(ip) = ip_address {
                let ip_status = self.get_ip_status(ip);
                if ip_status.locked {
                    return Err(ip_status);
                }
            }
        }

        Ok(())
    }

    /// Record a failed login attempt
    pub fn record_failed_attempt(&self, username: &str, ip_address: Option<&str>) -> LockoutStatus {
        let user_status = self.record_user_failed_attempt(username);

        // Also track by IP if enabled
        if self.config.track_by_ip {
            if let Some(ip) = ip_address {
                self.record_ip_failed_attempt(ip);
            }
        }

        user_status
    }

    /// Record a successful login (resets failed attempt counter)
    pub fn record_successful_login(&self, username: &str, ip_address: Option<&str>) {
        // Reset user attempts on successful login
        {
            let mut attempts = self.user_attempts.write().unwrap();
            attempts.remove(username);
        }

        // Reset IP attempts on successful login
        if self.config.track_by_ip {
            if let Some(ip) = ip_address {
                let mut attempts = self.ip_attempts.write().unwrap();
                attempts.remove(ip);
            }
        }
    }

    /// Get the lockout status for a user
    pub fn get_user_status(&self, username: &str) -> LockoutStatus {
        let attempts = self.user_attempts.read().unwrap();
        self.get_status_from_record(attempts.get(username))
    }

    /// Get the lockout status for an IP address
    pub fn get_ip_status(&self, ip_address: &str) -> LockoutStatus {
        let attempts = self.ip_attempts.read().unwrap();
        self.get_status_from_record(attempts.get(ip_address))
    }

    /// Manually lock a user account (admin action)
    pub fn lock_user(&self, username: &str, reason: Option<String>) {
        let mut attempts = self.user_attempts.write().unwrap();
        let record = attempts.entry(username.to_string()).or_insert_with(FailedAttemptRecord::new);
        record.locked = true;
        record.lockout_expires = None; // Manual locks don't auto-expire
        record.lockout_reason = reason;
        record.lockout_count += 1;
    }

    /// Manually unlock a user account (admin action)
    pub fn unlock_user(&self, username: &str) -> bool {
        let mut attempts = self.user_attempts.write().unwrap();
        if let Some(record) = attempts.get_mut(username) {
            record.locked = false;
            record.lockout_expires = None;
            record.lockout_reason = None;
            record.attempts = 0;
            record.window_start = Utc::now();
            true
        } else {
            false
        }
    }

    /// Manually unlock an IP address (admin action)
    pub fn unlock_ip(&self, ip_address: &str) -> bool {
        let mut attempts = self.ip_attempts.write().unwrap();
        if let Some(record) = attempts.get_mut(ip_address) {
            record.locked = false;
            record.lockout_expires = None;
            record.attempts = 0;
            record.window_start = Utc::now();
            true
        } else {
            false
        }
    }

    /// Get all currently locked users
    pub fn get_locked_users(&self) -> Vec<(String, LockoutStatus)> {
        let attempts = self.user_attempts.read().unwrap();
        attempts
            .iter()
            .filter_map(|(username, record)| {
                let status = self.get_status_from_record(Some(record));
                if status.locked {
                    Some((username.clone(), status))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all currently locked IPs
    pub fn get_locked_ips(&self) -> Vec<(String, LockoutStatus)> {
        let attempts = self.ip_attempts.read().unwrap();
        attempts
            .iter()
            .filter_map(|(ip, record)| {
                let status = self.get_status_from_record(Some(record));
                if status.locked {
                    Some((ip.clone(), status))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Clean up expired lockouts and old attempt records
    pub fn cleanup_expired(&self) -> usize {
        let now = Utc::now();
        let mut cleaned = 0;

        // Clean user attempts
        {
            let mut attempts = self.user_attempts.write().unwrap();
            let window_duration = Duration::minutes(self.config.attempt_window_minutes as i64);

            attempts.retain(|_, record| {
                // Check if lockout has expired
                if record.locked {
                    if let Some(expires) = record.lockout_expires {
                        if now > expires {
                            record.locked = false;
                            record.lockout_expires = None;
                        }
                    }
                }

                // Keep if locked or within attempt window
                let keep = record.locked || (now - record.last_attempt) < window_duration;
                if !keep {
                    cleaned += 1;
                }
                keep
            });
        }

        // Clean IP attempts
        {
            let mut attempts = self.ip_attempts.write().unwrap();
            let window_duration = Duration::minutes(self.config.attempt_window_minutes as i64);

            attempts.retain(|_, record| {
                // Check if lockout has expired
                if record.locked {
                    if let Some(expires) = record.lockout_expires {
                        if now > expires {
                            record.locked = false;
                            record.lockout_expires = None;
                        }
                    }
                }

                // Keep if locked or within attempt window
                let keep = record.locked || (now - record.last_attempt) < window_duration;
                if !keep {
                    cleaned += 1;
                }
                keep
            });
        }

        cleaned
    }

    /// Get the current configuration
    pub fn get_config(&self) -> &LockoutConfig {
        &self.config
    }

    // Internal helper to record a failed attempt for a user
    fn record_user_failed_attempt(&self, username: &str) -> LockoutStatus {
        let mut attempts = self.user_attempts.write().unwrap();
        let record = attempts.entry(username.to_string()).or_insert_with(FailedAttemptRecord::new);
        self.update_record_on_failure(record)
    }

    // Internal helper to record a failed attempt for an IP
    fn record_ip_failed_attempt(&self, ip_address: &str) {
        let mut attempts = self.ip_attempts.write().unwrap();
        let record = attempts.entry(ip_address.to_string()).or_insert_with(FailedAttemptRecord::new);
        self.update_record_on_failure(record);
    }

    // Update a record after a failed attempt
    fn update_record_on_failure(&self, record: &mut FailedAttemptRecord) -> LockoutStatus {
        let now = Utc::now();
        let window_duration = Duration::minutes(self.config.attempt_window_minutes as i64);

        // Check if we're still in the same attempt window
        if now - record.window_start > window_duration {
            // Reset window
            record.attempts = 0;
            record.window_start = now;
        }

        // Check if lockout has expired
        if record.locked {
            if let Some(expires) = record.lockout_expires {
                if now > expires {
                    record.locked = false;
                    record.lockout_expires = None;
                }
            }
        }

        // If not currently locked, increment attempts
        if !record.locked {
            record.attempts += 1;
            record.last_attempt = now;

            // Check if we should lock
            if record.attempts >= self.config.max_failed_attempts {
                record.locked = true;
                record.lockout_count += 1;

                // Calculate lockout duration (progressive if enabled)
                let base_duration = self.config.lockout_duration_minutes as i64;
                let multiplier = if self.config.progressive_lockout {
                    record.lockout_count as i64
                } else {
                    1
                };
                let lockout_duration = Duration::minutes(base_duration * multiplier);
                record.lockout_expires = Some(now + lockout_duration);
            }
        }

        self.get_status_from_record(Some(record))
    }

    // Convert a record to a status
    fn get_status_from_record(&self, record: Option<&FailedAttemptRecord>) -> LockoutStatus {
        let now = Utc::now();

        match record {
            Some(record) => {
                // Check if lockout has expired
                let mut locked = record.locked;
                let mut lockout_expires = record.lockout_expires;

                if locked {
                    if let Some(expires) = lockout_expires {
                        if now > expires {
                            locked = false;
                            lockout_expires = None;
                        }
                    }
                }

                let lockout_remaining_seconds = if locked {
                    lockout_expires.map(|e| (e - now).num_seconds().max(0))
                } else {
                    None
                };

                let remaining_attempts = if locked {
                    0
                } else {
                    self.config.max_failed_attempts.saturating_sub(record.attempts)
                };

                LockoutStatus {
                    locked,
                    failed_attempts: record.attempts,
                    remaining_attempts,
                    lockout_expires,
                    lockout_remaining_seconds,
                    lockout_count: record.lockout_count,
                    lockout_reason: record.lockout_reason.clone(),
                }
            }
            None => LockoutStatus {
                locked: false,
                failed_attempts: 0,
                remaining_attempts: self.config.max_failed_attempts,
                lockout_expires: None,
                lockout_remaining_seconds: None,
                lockout_count: 0,
                lockout_reason: None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration as StdDuration;

    fn test_config() -> LockoutConfig {
        LockoutConfig {
            max_failed_attempts: 3,
            lockout_duration_minutes: 1,
            attempt_window_minutes: 5,
            track_by_ip: true,
            progressive_lockout: false,
        }
    }

    #[test]
    fn test_no_lockout_initially() {
        let service = AccountLockoutService::new(test_config());
        let status = service.get_user_status("testuser");

        assert!(!status.locked);
        assert_eq!(status.failed_attempts, 0);
        assert_eq!(status.remaining_attempts, 3);
    }

    #[test]
    fn test_failed_attempts_increment() {
        let service = AccountLockoutService::new(test_config());

        service.record_failed_attempt("testuser", None);
        let status = service.get_user_status("testuser");

        assert!(!status.locked);
        assert_eq!(status.failed_attempts, 1);
        assert_eq!(status.remaining_attempts, 2);
    }

    #[test]
    fn test_lockout_after_max_attempts() {
        let service = AccountLockoutService::new(test_config());

        // 3 failed attempts should trigger lockout
        service.record_failed_attempt("testuser", None);
        service.record_failed_attempt("testuser", None);
        let status = service.record_failed_attempt("testuser", None);

        assert!(status.locked);
        assert_eq!(status.failed_attempts, 3);
        assert_eq!(status.remaining_attempts, 0);
        assert!(status.lockout_expires.is_some());
    }

    #[test]
    fn test_check_login_blocked_when_locked() {
        let service = AccountLockoutService::new(test_config());

        // Trigger lockout
        service.record_failed_attempt("testuser", None);
        service.record_failed_attempt("testuser", None);
        service.record_failed_attempt("testuser", None);

        // Login should be blocked
        let result = service.check_login_allowed("testuser", None);
        assert!(result.is_err());

        let status = result.unwrap_err();
        assert!(status.locked);
    }

    #[test]
    fn test_successful_login_resets_attempts() {
        let service = AccountLockoutService::new(test_config());

        // Record some failed attempts
        service.record_failed_attempt("testuser", None);
        service.record_failed_attempt("testuser", None);

        // Successful login should reset
        service.record_successful_login("testuser", None);

        let status = service.get_user_status("testuser");
        assert!(!status.locked);
        assert_eq!(status.failed_attempts, 0);
        assert_eq!(status.remaining_attempts, 3);
    }

    #[test]
    fn test_manual_lock_unlock() {
        let service = AccountLockoutService::new(test_config());

        // Manual lock
        service.lock_user("testuser", Some("Suspicious activity".to_string()));

        let status = service.get_user_status("testuser");
        assert!(status.locked);
        assert!(status.lockout_expires.is_none()); // Manual locks don't expire
        assert_eq!(status.lockout_reason, Some("Suspicious activity".to_string()));

        // Manual unlock
        service.unlock_user("testuser");

        let status = service.get_user_status("testuser");
        assert!(!status.locked);
        assert!(status.lockout_reason.is_none());
    }

    #[test]
    fn test_ip_tracking() {
        let config = LockoutConfig {
            track_by_ip: true,
            ..test_config()
        };
        let service = AccountLockoutService::new(config);

        // Failed attempts from same IP
        service.record_failed_attempt("user1", Some("192.168.1.1"));
        service.record_failed_attempt("user2", Some("192.168.1.1"));
        service.record_failed_attempt("user3", Some("192.168.1.1"));

        // IP should be locked
        let ip_status = service.get_ip_status("192.168.1.1");
        assert!(ip_status.locked);

        // Any user from this IP should be blocked
        let result = service.check_login_allowed("newuser", Some("192.168.1.1"));
        assert!(result.is_err());
    }

    #[test]
    fn test_progressive_lockout() {
        let config = LockoutConfig {
            max_failed_attempts: 3,
            lockout_duration_minutes: 5,
            progressive_lockout: true,
            ..test_config()
        };
        let service = AccountLockoutService::new(config);

        // First lockout
        service.record_failed_attempt("testuser", None);
        service.record_failed_attempt("testuser", None);
        let status = service.record_failed_attempt("testuser", None);

        assert!(status.locked);
        assert_eq!(status.lockout_count, 1);
        // First lockout: 5 minutes

        // Manually unlock and trigger second lockout
        service.unlock_user("testuser");
        service.record_failed_attempt("testuser", None);
        service.record_failed_attempt("testuser", None);
        let status = service.record_failed_attempt("testuser", None);

        assert!(status.locked);
        assert_eq!(status.lockout_count, 2);
        // Second lockout would be 10 minutes (2x)
    }

    #[test]
    fn test_get_locked_users() {
        let service = AccountLockoutService::new(test_config());

        // Lock some users
        service.lock_user("user1", None);
        service.lock_user("user2", Some("Test reason".to_string()));

        let locked = service.get_locked_users();
        assert_eq!(locked.len(), 2);

        let usernames: Vec<&str> = locked.iter().map(|(u, _)| u.as_str()).collect();
        assert!(usernames.contains(&"user1"));
        assert!(usernames.contains(&"user2"));
    }
}
