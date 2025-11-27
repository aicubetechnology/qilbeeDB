//! Token blacklist for JWT revocation
//!
//! Provides the ability to revoke JWT tokens before their natural expiration.
//! Uses in-memory storage with optional file-based persistence.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use qilbee_core::Result;

/// Reason for token revocation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RevocationReason {
    /// User logged out
    Logout,
    /// Admin revoked the token
    AdminRevoke,
    /// Security incident response
    SecurityIncident,
    /// Password changed
    PasswordChanged,
    /// Revoke all tokens for user
    RevokeAll,
}

impl std::fmt::Display for RevocationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RevocationReason::Logout => write!(f, "logout"),
            RevocationReason::AdminRevoke => write!(f, "admin_revoke"),
            RevocationReason::SecurityIncident => write!(f, "security_incident"),
            RevocationReason::PasswordChanged => write!(f, "password_changed"),
            RevocationReason::RevokeAll => write!(f, "revoke_all"),
        }
    }
}

/// A blacklisted token entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlacklistedToken {
    /// JWT ID (jti claim)
    pub token_id: String,
    /// User ID who owned the token
    pub user_id: String,
    /// Username for audit purposes
    pub username: String,
    /// When the token was revoked
    pub revoked_at: DateTime<Utc>,
    /// When the original token expires (for cleanup)
    pub expires_at: DateTime<Utc>,
    /// Reason for revocation
    pub reason: RevocationReason,
}

/// Configuration for the token blacklist
#[derive(Debug, Clone)]
pub struct BlacklistConfig {
    /// Path to the blacklist persistence file
    pub persistence_path: Option<PathBuf>,
    /// Whether to persist blacklist to disk
    pub persist_to_disk: bool,
}

impl Default for BlacklistConfig {
    fn default() -> Self {
        Self {
            persistence_path: None,
            persist_to_disk: false,
        }
    }
}

impl BlacklistConfig {
    /// Create a new config with persistence enabled
    pub fn with_persistence(path: PathBuf) -> Self {
        Self {
            persistence_path: Some(path),
            persist_to_disk: true,
        }
    }
}

/// Token blacklist service
///
/// Maintains a list of revoked JWT tokens to prevent their use.
/// Tokens are identified by their `jti` (JWT ID) claim.
pub struct TokenBlacklist {
    /// In-memory set of blacklisted token IDs for O(1) lookup
    blacklisted_ids: Arc<RwLock<HashSet<String>>>,
    /// Full blacklist entries for persistence and audit
    entries: Arc<RwLock<Vec<BlacklistedToken>>>,
    /// Configuration
    config: BlacklistConfig,
}

impl TokenBlacklist {
    /// Create a new token blacklist
    pub fn new(config: BlacklistConfig) -> Self {
        let blacklist = Self {
            blacklisted_ids: Arc::new(RwLock::new(HashSet::new())),
            entries: Arc::new(RwLock::new(Vec::new())),
            config,
        };

        // Load from disk if persistence is enabled
        if let Err(e) = blacklist.load_from_disk() {
            tracing::warn!("Failed to load token blacklist from disk: {}", e);
        }

        blacklist
    }

    /// Add a token to the blacklist
    pub fn revoke(
        &self,
        token_id: String,
        user_id: String,
        username: String,
        expires_at: DateTime<Utc>,
        reason: RevocationReason,
    ) -> Result<()> {
        let entry = BlacklistedToken {
            token_id: token_id.clone(),
            user_id,
            username,
            revoked_at: Utc::now(),
            expires_at,
            reason,
        };

        // Add to in-memory set
        self.blacklisted_ids.write().unwrap().insert(token_id);

        // Add to entries list
        self.entries.write().unwrap().push(entry.clone());

        // Persist to disk if enabled
        if self.config.persist_to_disk {
            self.append_to_disk(&entry)?;
        }

        Ok(())
    }

    /// Check if a token is revoked
    pub fn is_revoked(&self, token_id: &str) -> bool {
        self.blacklisted_ids.read().unwrap().contains(token_id)
    }

    /// Revoke all tokens for a user
    ///
    /// This doesn't actually add specific tokens to the blacklist,
    /// but records the revocation time. The middleware should check
    /// if a token was issued before this time.
    ///
    /// For simplicity, we track this by user_id -> revocation_time mapping.
    /// However, since we don't have access to all existing tokens,
    /// this method returns a marker entry that can be used for audit.
    ///
    /// Returns the count of tokens that were already blacklisted for this user.
    pub fn revoke_all_for_user(
        &self,
        user_id: &str,
        username: &str,
        reason: RevocationReason,
    ) -> Result<usize> {
        // Count existing entries for this user
        let existing_count = self.entries
            .read()
            .unwrap()
            .iter()
            .filter(|e| e.user_id == user_id)
            .count();

        // Add a marker entry with a far-future expiration
        let entry = BlacklistedToken {
            token_id: format!("revoke_all_{}", uuid::Uuid::new_v4()),
            user_id: user_id.to_string(),
            username: username.to_string(),
            revoked_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::days(365), // Keep for 1 year
            reason,
        };

        self.entries.write().unwrap().push(entry.clone());

        if self.config.persist_to_disk {
            self.append_to_disk(&entry)?;
        }

        Ok(existing_count)
    }

    /// Get the time when all tokens for a user were revoked (if any)
    pub fn get_user_revoke_all_time(&self, user_id: &str) -> Option<DateTime<Utc>> {
        self.entries
            .read()
            .unwrap()
            .iter()
            .filter(|e| {
                e.user_id == user_id &&
                (e.reason == RevocationReason::RevokeAll ||
                 e.reason == RevocationReason::PasswordChanged ||
                 e.reason == RevocationReason::SecurityIncident ||
                 e.reason == RevocationReason::AdminRevoke)
            })
            .max_by_key(|e| e.revoked_at)
            .map(|e| e.revoked_at)
    }

    /// Check if a token is invalid due to "revoke all"
    ///
    /// Returns true if the token was issued before or at the same time as a "revoke all" operation
    pub fn is_invalidated_by_revoke_all(&self, user_id: &str, token_issued_at: DateTime<Utc>) -> bool {
        if let Some(revoke_time) = self.get_user_revoke_all_time(user_id) {
            // Token is invalid if it was issued before or at the revoke-all time
            // We use <= because tokens issued at the exact same second should be invalidated
            token_issued_at <= revoke_time
        } else {
            false
        }
    }

    /// Clean up expired entries
    ///
    /// Removes entries where the original token has expired.
    /// Returns the number of entries removed.
    pub fn cleanup_expired(&self) -> usize {
        let now = Utc::now();
        let mut count = 0;

        // Clean up entries
        {
            let mut entries = self.entries.write().unwrap();
            let original_len = entries.len();
            entries.retain(|e| e.expires_at > now);
            count = original_len - entries.len();
        }

        // Rebuild the ID set from remaining entries
        {
            let entries = self.entries.read().unwrap();
            let mut ids = self.blacklisted_ids.write().unwrap();
            ids.clear();
            for entry in entries.iter() {
                if !entry.token_id.starts_with("revoke_all_") {
                    ids.insert(entry.token_id.clone());
                }
            }
        }

        // Rewrite persistence file with cleaned data
        if self.config.persist_to_disk && count > 0 {
            if let Err(e) = self.rewrite_persistence_file() {
                tracing::error!("Failed to rewrite blacklist file after cleanup: {}", e);
            }
        }

        count
    }

    /// Get total count of blacklisted tokens
    pub fn count(&self) -> usize {
        self.blacklisted_ids.read().unwrap().len()
    }

    /// Get total count of all entries (including revoke-all markers)
    pub fn entry_count(&self) -> usize {
        self.entries.read().unwrap().len()
    }

    /// Get all entries for a user (for audit purposes)
    pub fn get_user_entries(&self, user_id: &str) -> Vec<BlacklistedToken> {
        self.entries
            .read()
            .unwrap()
            .iter()
            .filter(|e| e.user_id == user_id)
            .cloned()
            .collect()
    }

    /// Load blacklist from disk
    fn load_from_disk(&self) -> Result<()> {
        let path = match &self.config.persistence_path {
            Some(p) => p,
            None => return Ok(()),
        };

        if !path.exists() {
            return Ok(());
        }

        let file = File::open(path)
            .map_err(|e| qilbee_core::Error::Internal(format!("Failed to open blacklist file: {}", e)))?;
        let reader = BufReader::new(file);

        let now = Utc::now();
        let mut loaded_count = 0;
        let mut expired_count = 0;

        for line in reader.lines() {
            let line = line
                .map_err(|e| qilbee_core::Error::Internal(format!("Failed to read blacklist line: {}", e)))?;

            if line.trim().is_empty() {
                continue;
            }

            let entry: BlacklistedToken = serde_json::from_str(&line)
                .map_err(|e| qilbee_core::Error::Internal(format!("Failed to parse blacklist entry: {}", e)))?;

            // Skip expired entries
            if entry.expires_at <= now {
                expired_count += 1;
                continue;
            }

            // Add to in-memory structures
            if !entry.token_id.starts_with("revoke_all_") {
                self.blacklisted_ids.write().unwrap().insert(entry.token_id.clone());
            }
            self.entries.write().unwrap().push(entry);
            loaded_count += 1;
        }

        tracing::info!(
            "Loaded {} blacklist entries ({} expired entries skipped)",
            loaded_count,
            expired_count
        );

        Ok(())
    }

    /// Append a single entry to the persistence file
    fn append_to_disk(&self, entry: &BlacklistedToken) -> Result<()> {
        let path = match &self.config.persistence_path {
            Some(p) => p,
            None => return Ok(()),
        };

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| qilbee_core::Error::Internal(format!("Failed to create blacklist directory: {}", e)))?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| qilbee_core::Error::Internal(format!("Failed to open blacklist file for append: {}", e)))?;

        let json = serde_json::to_string(entry)
            .map_err(|e| qilbee_core::Error::Internal(format!("Failed to serialize blacklist entry: {}", e)))?;

        writeln!(file, "{}", json)
            .map_err(|e| qilbee_core::Error::Internal(format!("Failed to write blacklist entry: {}", e)))?;

        Ok(())
    }

    /// Rewrite the entire persistence file with current entries
    fn rewrite_persistence_file(&self) -> Result<()> {
        let path = match &self.config.persistence_path {
            Some(p) => p,
            None => return Ok(()),
        };

        let entries = self.entries.read().unwrap();

        let mut file = File::create(path)
            .map_err(|e| qilbee_core::Error::Internal(format!("Failed to create blacklist file: {}", e)))?;

        for entry in entries.iter() {
            let json = serde_json::to_string(entry)
                .map_err(|e| qilbee_core::Error::Internal(format!("Failed to serialize blacklist entry: {}", e)))?;
            writeln!(file, "{}", json)
                .map_err(|e| qilbee_core::Error::Internal(format!("Failed to write blacklist entry: {}", e)))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn create_test_blacklist() -> TokenBlacklist {
        TokenBlacklist::new(BlacklistConfig::default())
    }

    #[test]
    fn test_revoke_and_check() {
        let blacklist = create_test_blacklist();

        let token_id = "test-token-123".to_string();
        let user_id = "user-456".to_string();
        let username = "testuser".to_string();
        let expires_at = Utc::now() + Duration::hours(24);

        // Token should not be revoked initially
        assert!(!blacklist.is_revoked(&token_id));

        // Revoke the token
        blacklist.revoke(
            token_id.clone(),
            user_id,
            username,
            expires_at,
            RevocationReason::Logout,
        ).unwrap();

        // Token should now be revoked
        assert!(blacklist.is_revoked(&token_id));
        assert_eq!(blacklist.count(), 1);
    }

    #[test]
    fn test_cleanup_expired() {
        let blacklist = create_test_blacklist();

        // Add an expired token
        let expired_id = "expired-token".to_string();
        blacklist.revoke(
            expired_id.clone(),
            "user".to_string(),
            "user".to_string(),
            Utc::now() - Duration::hours(1), // Already expired
            RevocationReason::Logout,
        ).unwrap();

        // Add a valid token
        let valid_id = "valid-token".to_string();
        blacklist.revoke(
            valid_id.clone(),
            "user".to_string(),
            "user".to_string(),
            Utc::now() + Duration::hours(24), // Expires in 24 hours
            RevocationReason::Logout,
        ).unwrap();

        assert_eq!(blacklist.count(), 2);

        // Cleanup expired
        let cleaned = blacklist.cleanup_expired();
        assert_eq!(cleaned, 1);
        assert_eq!(blacklist.count(), 1);

        // Expired token should no longer be tracked
        assert!(!blacklist.is_revoked(&expired_id));
        // Valid token should still be tracked
        assert!(blacklist.is_revoked(&valid_id));
    }

    #[test]
    fn test_revoke_all_for_user() {
        let blacklist = create_test_blacklist();

        let user_id = "user-789";
        let username = "testuser";

        // Add some tokens for the user
        for i in 0..3 {
            blacklist.revoke(
                format!("token-{}", i),
                user_id.to_string(),
                username.to_string(),
                Utc::now() + Duration::hours(24),
                RevocationReason::Logout,
            ).unwrap();
        }

        assert_eq!(blacklist.count(), 3);

        // Revoke all for user
        let count = blacklist.revoke_all_for_user(
            user_id,
            username,
            RevocationReason::RevokeAll,
        ).unwrap();

        assert_eq!(count, 3);

        // Check that revoke-all time is recorded
        let revoke_time = blacklist.get_user_revoke_all_time(user_id);
        assert!(revoke_time.is_some());

        // Token issued before revoke-all should be invalidated
        let old_token_time = Utc::now() - Duration::minutes(5);
        assert!(blacklist.is_invalidated_by_revoke_all(user_id, old_token_time));

        // Token issued after revoke-all should not be invalidated
        let new_token_time = Utc::now() + Duration::minutes(5);
        assert!(!blacklist.is_invalidated_by_revoke_all(user_id, new_token_time));
    }

    #[test]
    fn test_get_user_entries() {
        let blacklist = create_test_blacklist();

        // Add tokens for different users
        blacklist.revoke(
            "token-1".to_string(),
            "user-a".to_string(),
            "usera".to_string(),
            Utc::now() + Duration::hours(24),
            RevocationReason::Logout,
        ).unwrap();

        blacklist.revoke(
            "token-2".to_string(),
            "user-b".to_string(),
            "userb".to_string(),
            Utc::now() + Duration::hours(24),
            RevocationReason::AdminRevoke,
        ).unwrap();

        blacklist.revoke(
            "token-3".to_string(),
            "user-a".to_string(),
            "usera".to_string(),
            Utc::now() + Duration::hours(24),
            RevocationReason::PasswordChanged,
        ).unwrap();

        let user_a_entries = blacklist.get_user_entries("user-a");
        assert_eq!(user_a_entries.len(), 2);

        let user_b_entries = blacklist.get_user_entries("user-b");
        assert_eq!(user_b_entries.len(), 1);
    }
}
