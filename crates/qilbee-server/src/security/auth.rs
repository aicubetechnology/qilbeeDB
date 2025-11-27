//! Authentication service

use crate::security::{User, UserService, TokenService};
use super::token::AuthToken;
use super::token_blacklist::{TokenBlacklist, RevocationReason};
use qilbee_core::Result;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use std::collections::HashMap;
use chrono::{DateTime, Utc, Duration};
use uuid::Uuid;

/// Authentication credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}

/// Session information
#[derive(Debug, Clone)]
pub struct Session {
    pub user_id: String,
    pub username: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
}

impl Session {
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    pub fn is_inactive(&self, inactive_timeout_mins: i64) -> bool {
        Utc::now() > self.last_activity + Duration::minutes(inactive_timeout_mins)
    }

    pub fn refresh(&mut self) {
        self.last_activity = Utc::now();
    }
}

/// Login attempt tracking for rate limiting
#[derive(Debug, Clone)]
struct LoginAttempt {
    username: String,
    attempts: u32,
    first_attempt: DateTime<Utc>,
    locked_until: Option<DateTime<Utc>>,
}

impl LoginAttempt {
    fn new(username: String) -> Self {
        Self {
            username,
            attempts: 1,
            first_attempt: Utc::now(),
            locked_until: None,
        }
    }

    fn increment(&mut self) {
        self.attempts += 1;
    }

    fn is_locked(&self) -> bool {
        if let Some(locked_until) = self.locked_until {
            Utc::now() < locked_until
        } else {
            false
        }
    }

    fn lock(&mut self, duration_mins: i64) {
        self.locked_until = Some(Utc::now() + Duration::minutes(duration_mins));
    }

    fn reset(&mut self) {
        self.attempts = 0;
        self.first_attempt = Utc::now();
        self.locked_until = None;
    }
}

/// Authentication service configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Session duration in seconds (default: 24 hours)
    pub session_duration_secs: i64,
    /// Inactive timeout in minutes (default: 30 minutes)
    pub inactive_timeout_mins: i64,
    /// Maximum login attempts before lockout (default: 5)
    pub max_login_attempts: u32,
    /// Account lockout duration in minutes (default: 15)
    pub lockout_duration_mins: i64,
    /// Login attempt window in minutes (default: 15)
    pub attempt_window_mins: i64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            session_duration_secs: 86400,        // 24 hours
            inactive_timeout_mins: 30,           // 30 minutes
            max_login_attempts: 5,               // 5 attempts
            lockout_duration_mins: 15,           // 15 minutes
            attempt_window_mins: 15,             // 15 minutes
        }
    }
}

/// Main authentication service
pub struct AuthService {
    user_service: Arc<UserService>,
    token_service: Arc<TokenService>,
    token_blacklist: Arc<TokenBlacklist>,
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    login_attempts: Arc<RwLock<HashMap<String, LoginAttempt>>>,
    config: AuthConfig,
}

impl AuthService {
    /// Create new authentication service
    pub fn new(
        user_service: Arc<UserService>,
        token_service: Arc<TokenService>,
        token_blacklist: Arc<TokenBlacklist>,
        config: AuthConfig,
    ) -> Self {
        Self {
            user_service,
            token_service,
            token_blacklist,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            login_attempts: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Authenticate user with credentials and return JWT token
    pub fn login(&self, credentials: Credentials) -> Result<AuthToken> {
        // Check if account is locked
        if self.is_account_locked(&credentials.username) {
            return Err(qilbee_core::Error::Internal(
                "Account temporarily locked due to too many failed login attempts".to_string(),
            ));
        }

        // Authenticate user
        let user = match self.user_service.authenticate(&credentials.username, &credentials.password) {
            Ok(user) => {
                // Reset login attempts on successful authentication
                self.reset_login_attempts(&credentials.username);
                user
            }
            Err(e) => {
                // Record failed login attempt
                self.record_login_attempt(&credentials.username);
                return Err(e);
            }
        };

        // Generate JWT token
        let token = self.token_service.generate_jwt(
            user.id.clone(),
            user.username.clone(),
            user.roles.clone(),
        )?;

        // Create session
        let session = Session {
            user_id: user.id.0.to_string(),
            username: user.username.clone(),
            created_at: Utc::now(),
            expires_at: Utc::now() + Duration::seconds(self.config.session_duration_secs),
            last_activity: Utc::now(),
        };

        self.sessions.write().unwrap().insert(user.id.0.to_string(), session);

        Ok(token)
    }

    /// Validate JWT token and return user
    pub fn validate_token(&self, token: &str) -> Result<User> {
        // Validate JWT and extract claims
        let claims = self.token_service.validate_jwt(token)?;

        // Check if token is blacklisted
        if self.token_blacklist.is_revoked(&claims.jti) {
            return Err(qilbee_core::Error::Internal("Token has been revoked".to_string()));
        }

        // Check if token was invalidated by a "revoke all" operation
        let token_issued_at = DateTime::from_timestamp(claims.iat as i64, 0)
            .ok_or_else(|| qilbee_core::Error::Internal("Invalid token issue time".to_string()))?;
        if self.token_blacklist.is_invalidated_by_revoke_all(&claims.sub, token_issued_at) {
            return Err(qilbee_core::Error::Internal("Token has been invalidated".to_string()));
        }

        // Check if session exists and is valid
        let mut sessions = self.sessions.write().unwrap();
        if let Some(session) = sessions.get_mut(&claims.sub) {
            if session.is_expired() {
                sessions.remove(&claims.sub);
                return Err(qilbee_core::Error::Internal("Session expired".to_string()));
            }

            if session.is_inactive(self.config.inactive_timeout_mins) {
                sessions.remove(&claims.sub);
                return Err(qilbee_core::Error::Internal("Session inactive".to_string()));
            }

            // Refresh session activity
            session.refresh();
        }

        // Get user from user service
        let user_id = Uuid::parse_str(&claims.sub)
            .map_err(|e| qilbee_core::Error::Internal(format!("Invalid user ID in token: {}", e)))?;

        self.user_service.get_user(&super::UserId(user_id))
            .ok_or_else(|| qilbee_core::Error::Internal("User not found".to_string()))
    }

    /// Validate JWT token and return claims (for revocation)
    pub fn validate_token_claims(&self, token: &str) -> Result<super::token::Claims> {
        self.token_service.validate_jwt(token)
    }

    /// Validate API key and return user
    pub fn validate_api_key(&self, api_key: &str) -> Result<User> {
        let user_id = self.token_service.validate_api_key(api_key)?;
        self.user_service.get_user(&user_id)
            .ok_or_else(|| qilbee_core::Error::Internal("User not found".to_string()))
    }

    /// Logout user by invalidating session
    pub fn logout(&self, user_id: &str) -> Result<()> {
        self.sessions.write().unwrap().remove(user_id);
        Ok(())
    }

    /// Revoke a specific token
    ///
    /// The token will no longer be valid for authentication.
    pub fn revoke_token(
        &self,
        token_id: String,
        user_id: String,
        username: String,
        expires_at: DateTime<Utc>,
        reason: RevocationReason,
    ) -> Result<()> {
        self.token_blacklist.revoke(token_id, user_id, username, expires_at, reason)
    }

    /// Revoke all tokens for a user
    ///
    /// All tokens issued before this call will be invalidated.
    pub fn revoke_all_user_tokens(
        &self,
        user_id: &str,
        username: &str,
        reason: RevocationReason,
    ) -> Result<usize> {
        // Also remove the session
        self.sessions.write().unwrap().remove(user_id);
        self.token_blacklist.revoke_all_for_user(user_id, username, reason)
    }

    /// Get the token blacklist (for HTTP endpoints)
    pub fn token_blacklist(&self) -> &Arc<TokenBlacklist> {
        &self.token_blacklist
    }

    /// Cleanup expired blacklist entries
    pub fn cleanup_blacklist(&self) -> usize {
        self.token_blacklist.cleanup_expired()
    }

    /// Refresh JWT token
    pub fn refresh_token(&self, token: &str) -> Result<AuthToken> {
        // Validate current token
        let user = self.validate_token(token)?;

        // Generate new token
        self.token_service.generate_jwt(
            user.id.clone(),
            user.username.clone(),
            user.roles.clone(),
        )
    }

    /// Check if account is locked due to failed login attempts
    fn is_account_locked(&self, username: &str) -> bool {
        let attempts = self.login_attempts.read().unwrap();
        if let Some(attempt) = attempts.get(username) {
            attempt.is_locked()
        } else {
            false
        }
    }

    /// Record failed login attempt
    fn record_login_attempt(&self, username: &str) {
        let mut attempts = self.login_attempts.write().unwrap();

        let should_lock = if let Some(attempt) = attempts.get_mut(username) {
            // Check if attempt is within the window
            let window_start = Utc::now() - Duration::minutes(self.config.attempt_window_mins);
            if attempt.first_attempt < window_start {
                // Reset if outside window
                attempt.reset();
                attempt.increment();
                false
            } else {
                attempt.increment();
                attempt.attempts >= self.config.max_login_attempts
            }
        } else {
            // First attempt
            attempts.insert(username.to_string(), LoginAttempt::new(username.to_string()));
            false
        };

        // Lock account if max attempts reached
        if should_lock {
            if let Some(attempt) = attempts.get_mut(username) {
                attempt.lock(self.config.lockout_duration_mins);
            }
        }
    }

    /// Reset login attempts for user
    fn reset_login_attempts(&self, username: &str) {
        self.login_attempts.write().unwrap().remove(username);
    }

    /// Clean up expired sessions (should be called periodically)
    pub fn cleanup_sessions(&self) {
        let mut sessions = self.sessions.write().unwrap();
        sessions.retain(|_, session| {
            !session.is_expired() && !session.is_inactive(self.config.inactive_timeout_mins)
        });
    }

    /// Clean up old login attempts (should be called periodically)
    pub fn cleanup_login_attempts(&self) {
        let mut attempts = self.login_attempts.write().unwrap();
        let window_start = Utc::now() - Duration::minutes(self.config.attempt_window_mins);
        attempts.retain(|_, attempt| {
            // Keep if locked or within window
            attempt.is_locked() || attempt.first_attempt > window_start
        });
    }

    /// Get active session count
    pub fn active_session_count(&self) -> usize {
        self.sessions.read().unwrap().len()
    }

    /// Get user's session if exists
    pub fn get_session(&self, user_id: &str) -> Option<Session> {
        self.sessions.read().unwrap().get(user_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::{Role, UserId, BlacklistConfig};

    fn create_test_blacklist() -> Arc<TokenBlacklist> {
        Arc::new(TokenBlacklist::new(BlacklistConfig::default()))
    }

    #[test]
    fn test_login_success() {
        let user_service = Arc::new(UserService::new());
        let token_service = Arc::new(TokenService::new("test_secret".to_string()));
        let blacklist = create_test_blacklist();
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
            blacklist,
            AuthConfig::default(),
        );

        // Create test user
        let user = user_service
            .create_user("testuser".to_string(), "test@example.com".to_string(), "password123")
            .unwrap();

        // Login
        let credentials = Credentials {
            username: "testuser".to_string(),
            password: "password123".to_string(),
        };

        let token = auth_service.login(credentials).unwrap();
        assert_eq!(token.token_type, "Bearer");
        assert!(token.expires_in > 0);
    }

    #[test]
    fn test_login_failure() {
        let user_service = Arc::new(UserService::new());
        let token_service = Arc::new(TokenService::new("test_secret".to_string()));
        let blacklist = create_test_blacklist();
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
            blacklist,
            AuthConfig::default(),
        );

        // Create test user
        user_service
            .create_user("testuser".to_string(), "test@example.com".to_string(), "password123")
            .unwrap();

        // Login with wrong password
        let credentials = Credentials {
            username: "testuser".to_string(),
            password: "wrongpassword".to_string(),
        };

        let result = auth_service.login(credentials);
        assert!(result.is_err());
    }

    #[test]
    fn test_token_validation() {
        let user_service = Arc::new(UserService::new());
        let token_service = Arc::new(TokenService::new("test_secret".to_string()));
        let blacklist = create_test_blacklist();
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
            blacklist,
            AuthConfig::default(),
        );

        // Create test user
        user_service
            .create_user("testuser".to_string(), "test@example.com".to_string(), "password123")
            .unwrap();

        // Login
        let credentials = Credentials {
            username: "testuser".to_string(),
            password: "password123".to_string(),
        };

        let token = auth_service.login(credentials).unwrap();

        // Validate token
        let user = auth_service.validate_token(&token.access_token).unwrap();
        assert_eq!(user.username, "testuser");
    }

    #[test]
    fn test_rate_limiting() {
        let user_service = Arc::new(UserService::new());
        let token_service = Arc::new(TokenService::new("test_secret".to_string()));
        let blacklist = create_test_blacklist();
        let mut config = AuthConfig::default();
        config.max_login_attempts = 3;
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
            blacklist,
            config,
        );

        // Create test user
        user_service
            .create_user("testuser".to_string(), "test@example.com".to_string(), "password123")
            .unwrap();

        // Attempt login with wrong password multiple times
        for _ in 0..3 {
            let credentials = Credentials {
                username: "testuser".to_string(),
                password: "wrongpassword".to_string(),
            };
            let _ = auth_service.login(credentials);
        }

        // Next attempt should be locked
        let credentials = Credentials {
            username: "testuser".to_string(),
            password: "password123".to_string(), // Even correct password
        };

        let result = auth_service.login(credentials);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("locked"));
    }

    #[test]
    fn test_logout() {
        let user_service = Arc::new(UserService::new());
        let token_service = Arc::new(TokenService::new("test_secret".to_string()));
        let blacklist = create_test_blacklist();
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
            blacklist,
            AuthConfig::default(),
        );

        // Create test user
        let user = user_service
            .create_user("testuser".to_string(), "test@example.com".to_string(), "password123")
            .unwrap();

        // Login
        let credentials = Credentials {
            username: "testuser".to_string(),
            password: "password123".to_string(),
        };

        auth_service.login(credentials).unwrap();

        // Verify session exists
        assert_eq!(auth_service.active_session_count(), 1);

        // Logout
        auth_service.logout(&user.id.0.to_string()).unwrap();

        // Verify session is removed
        assert_eq!(auth_service.active_session_count(), 0);
    }

    #[test]
    fn test_token_revocation() {
        let user_service = Arc::new(UserService::new());
        let token_service = Arc::new(TokenService::new("test_secret".to_string()));
        let blacklist = create_test_blacklist();
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
            blacklist,
            AuthConfig::default(),
        );

        // Create test user
        user_service
            .create_user("testuser".to_string(), "test@example.com".to_string(), "password123")
            .unwrap();

        // Login
        let credentials = Credentials {
            username: "testuser".to_string(),
            password: "password123".to_string(),
        };

        let token = auth_service.login(credentials).unwrap();

        // Token should be valid
        let user = auth_service.validate_token(&token.access_token).unwrap();
        assert_eq!(user.username, "testuser");

        // Get claims for revocation
        let claims = auth_service.validate_token_claims(&token.access_token).unwrap();
        let expires_at = DateTime::from_timestamp(claims.exp as i64, 0).unwrap();

        // Revoke the token
        auth_service.revoke_token(
            claims.jti.clone(),
            claims.sub.clone(),
            claims.username.clone(),
            expires_at,
            RevocationReason::Logout,
        ).unwrap();

        // Token should now be invalid
        let result = auth_service.validate_token(&token.access_token);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("revoked"));
    }

    #[test]
    fn test_revoke_all_user_tokens() {
        let user_service = Arc::new(UserService::new());
        let token_service = Arc::new(TokenService::new("test_secret".to_string()));
        let blacklist = create_test_blacklist();
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
            blacklist,
            AuthConfig::default(),
        );

        // Create test user
        let created_user = user_service
            .create_user("testuser".to_string(), "test@example.com".to_string(), "password123")
            .unwrap();

        // Login
        let credentials = Credentials {
            username: "testuser".to_string(),
            password: "password123".to_string(),
        };

        let token = auth_service.login(credentials).unwrap();

        // Token should be valid
        let user = auth_service.validate_token(&token.access_token).unwrap();
        assert_eq!(user.username, "testuser");

        // Revoke all tokens for the user
        auth_service.revoke_all_user_tokens(
            &created_user.id.0.to_string(),
            &created_user.username,
            RevocationReason::AdminRevoke,
        ).unwrap();

        // Token should now be invalid
        let result = auth_service.validate_token(&token.access_token);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("invalidated"));
    }
}
