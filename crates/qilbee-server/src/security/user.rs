//! User management and storage

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use chrono::{DateTime, Utc};
use uuid::Uuid;
use qilbee_core::Result;
use super::rbac::Role;

/// Unique user identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

impl UserId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for UserId {
    fn default() -> Self {
        Self::new()
    }
}

/// User account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub id: UserId,
    pub username: String,
    pub email: String,
    #[serde(skip_serializing)]
    pub password_hash: String,
    pub roles: Vec<Role>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
    pub metadata: HashMap<String, String>,
}

impl User {
    /// Create a new user with hashed password
    pub fn new(username: String, email: String, password: &str) -> Result<Self> {
        let password_hash = hash_password(password)?;

        Ok(Self {
            id: UserId::new(),
            username,
            email,
            password_hash,
            roles: vec![Role::Read], // Default role
            is_active: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_login: None,
            metadata: HashMap::new(),
        })
    }

    /// Verify password
    pub fn verify_password(&self, password: &str) -> Result<bool> {
        verify_password(password, &self.password_hash)
    }

    /// Update password
    pub fn update_password(&mut self, new_password: &str) -> Result<()> {
        self.password_hash = hash_password(new_password)?;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Add role
    pub fn add_role(&mut self, role: Role) {
        if !self.roles.contains(&role) {
            self.roles.push(role);
            self.updated_at = Utc::now();
        }
    }

    /// Remove role
    pub fn remove_role(&mut self, role: &Role) {
        self.roles.retain(|r| r != role);
        self.updated_at = Utc::now();
    }

    /// Check if user has role
    pub fn has_role(&self, role: &Role) -> bool {
        self.roles.contains(role)
    }

    /// Record login
    pub fn record_login(&mut self) {
        self.last_login = Some(Utc::now());
        self.updated_at = Utc::now();
    }
}

/// Hash password using Argon2
fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();

    let password_hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| qilbee_core::Error::Internal(format!("Failed to hash password: {}", e)))?
        .to_string();

    Ok(password_hash)
}

/// Verify password against hash
fn verify_password(password: &str, hash: &str) -> Result<bool> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| qilbee_core::Error::Internal(format!("Invalid password hash: {}", e)))?;

    let argon2 = Argon2::default();

    Ok(argon2
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

/// User service for managing users
pub struct UserService {
    users: Arc<RwLock<HashMap<UserId, User>>>,
    username_index: Arc<RwLock<HashMap<String, UserId>>>,
    email_index: Arc<RwLock<HashMap<String, UserId>>>,
}

impl UserService {
    /// Create new user service
    pub fn new() -> Self {
        Self {
            users: Arc::new(RwLock::new(HashMap::new())),
            username_index: Arc::new(RwLock::new(HashMap::new())),
            email_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new user
    pub fn create_user(&self, username: String, email: String, password: &str) -> Result<User> {
        // Check if username exists
        if self.username_index.read().unwrap().contains_key(&username) {
            return Err(qilbee_core::Error::Internal("Username already exists".to_string()));
        }

        // Check if email exists
        if self.email_index.read().unwrap().contains_key(&email) {
            return Err(qilbee_core::Error::Internal("Email already exists".to_string()));
        }

        let user = User::new(username.clone(), email.clone(), password)?;
        let user_id = user.id;

        // Store user
        self.users.write().unwrap().insert(user_id, user.clone());
        self.username_index.write().unwrap().insert(username, user_id);
        self.email_index.write().unwrap().insert(email, user_id);

        Ok(user)
    }

    /// Get user by ID
    pub fn get_user(&self, user_id: &UserId) -> Option<User> {
        self.users.read().unwrap().get(user_id).cloned()
    }

    /// Get user by username
    pub fn get_user_by_username(&self, username: &str) -> Option<User> {
        let user_id = self.username_index.read().unwrap().get(username).copied()?;
        self.get_user(&user_id)
    }

    /// Get user by email
    pub fn get_user_by_email(&self, email: &str) -> Option<User> {
        let user_id = self.email_index.read().unwrap().get(email).copied()?;
        self.get_user(&user_id)
    }

    /// Update user
    pub fn update_user(&self, user: User) -> Result<()> {
        let user_id = user.id;
        self.users.write().unwrap().insert(user_id, user);
        Ok(())
    }

    /// Delete user
    pub fn delete_user(&self, user_id: &UserId) -> Result<()> {
        if let Some(user) = self.users.write().unwrap().remove(user_id) {
            self.username_index.write().unwrap().remove(&user.username);
            self.email_index.write().unwrap().remove(&user.email);
        }
        Ok(())
    }

    /// Authenticate user
    pub fn authenticate(&self, username: &str, password: &str) -> Result<User> {
        let user = self
            .get_user_by_username(username)
            .ok_or_else(|| qilbee_core::Error::Internal("Invalid credentials".to_string()))?;

        if !user.is_active {
            return Err(qilbee_core::Error::Internal("User account is disabled".to_string()));
        }

        if !user.verify_password(password)? {
            return Err(qilbee_core::Error::Internal("Invalid credentials".to_string()));
        }

        // Record login
        let mut updated_user = user;
        updated_user.record_login();
        self.update_user(updated_user.clone())?;

        Ok(updated_user)
    }

    /// List all users
    pub fn list_users(&self) -> Vec<User> {
        self.users.read().unwrap().values().cloned().collect()
    }

    /// Create default admin user
    pub fn create_default_admin(&self, password: &str) -> Result<User> {
        let mut user = self.create_user(
            "admin".to_string(),
            "admin@qilbeedb.io".to_string(),
            password,
        )?;

        user.add_role(Role::Admin);
        self.update_user(user.clone())?;

        Ok(user)
    }
}

impl Default for UserService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_hashing() {
        let password = "secure_password_123";
        let hash = hash_password(password).unwrap();
        assert!(verify_password(password, &hash).unwrap());
        assert!(!verify_password("wrong_password", &hash).unwrap());
    }

    #[test]
    fn test_user_creation() {
        let user = User::new(
            "testuser".to_string(),
            "test@example.com".to_string(),
            "password123",
        ).unwrap();

        assert_eq!(user.username, "testuser");
        assert!(user.verify_password("password123").unwrap());
        assert!(!user.verify_password("wrongpassword").unwrap());
    }

    #[test]
    fn test_user_service() {
        let service = UserService::new();

        let user = service
            .create_user("alice".to_string(), "alice@example.com".to_string(), "password123")
            .unwrap();

        assert_eq!(user.username, "alice");

        let retrieved = service.get_user_by_username("alice").unwrap();
        assert_eq!(retrieved.id, user.id);

        let authed = service.authenticate("alice", "password123").unwrap();
        assert_eq!(authed.id, user.id);
    }
}
