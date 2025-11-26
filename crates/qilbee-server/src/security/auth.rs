//! Authentication service

use crate::security::{User, UserService, TokenService};
use super::token::AuthToken;
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
    sessions: Arc<RwLock<HashMap<String, Session>>>,
    login_attempts: Arc<RwLock<HashMap<String, LoginAttempt>>>,
    config: AuthConfig,
}

impl AuthService {
    /// Create new authentication service
    pub fn new(
        user_service: Arc<UserService>,
        token_service: Arc<TokenService>,
        config: AuthConfig,
    ) -> Self {
        Self {
            user_service,
            token_service,
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
    use crate::security::{Role, UserId};

    #[test]
    fn test_login_success() {
        let user_service = Arc::new(UserService::new());
        let token_service = Arc::new(TokenService::new("test_secret".to_string()));
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
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
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
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
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
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
        let mut config = AuthConfig::default();
        config.max_login_attempts = 3;
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
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
        let auth_service = AuthService::new(
            user_service.clone(),
            token_service,
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
}
