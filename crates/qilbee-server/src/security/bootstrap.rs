//! Secure bootstrap for initial QilbeeDB deployment
//!
//! Handles first-time setup including initial admin user creation

use super::{UserService, Role};
use qilbee_core::Result;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::fs;
use tracing::{info, warn};
use serde::{Deserialize, Serialize};

/// Bootstrap state file name
const BOOTSTRAP_STATE_FILE: &str = ".qilbee_bootstrap";

/// Minimum password requirements
const MIN_PASSWORD_LENGTH: usize = 12;
const PASSWORD_REQUIREMENTS: &str = r#"
Password Requirements:
- Minimum 12 characters
- At least one uppercase letter
- At least one lowercase letter
- At least one number
- At least one special character (!@#$%^&*()_+-=[]{}|;:,.<>?)
"#;

/// Bootstrap state tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootstrapState {
    /// Whether initial bootstrap is complete
    pub is_bootstrapped: bool,
    /// Admin username created
    pub admin_username: String,
    /// Bootstrap timestamp
    pub bootstrapped_at: chrono::DateTime<chrono::Utc>,
}

impl BootstrapState {
    /// Create new incomplete bootstrap state
    pub fn new() -> Self {
        Self {
            is_bootstrapped: false,
            admin_username: String::new(),
            bootstrapped_at: chrono::Utc::now(),
        }
    }

    /// Mark as complete
    pub fn complete(admin_username: String) -> Self {
        Self {
            is_bootstrapped: true,
            admin_username,
            bootstrapped_at: chrono::Utc::now(),
        }
    }

    /// Load from file
    pub fn load(data_dir: &Path) -> Result<Option<Self>> {
        let path = data_dir.join(BOOTSTRAP_STATE_FILE);
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| qilbee_core::Error::Internal(format!("Failed to read bootstrap state: {}", e)))?;

        let state: BootstrapState = serde_json::from_str(&content)
            .map_err(|e| qilbee_core::Error::Internal(format!("Failed to parse bootstrap state: {}", e)))?;

        Ok(Some(state))
    }

    /// Save to file
    pub fn save(&self, data_dir: &Path) -> Result<()> {
        let path = data_dir.join(BOOTSTRAP_STATE_FILE);
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| qilbee_core::Error::Internal(format!("Failed to serialize bootstrap state: {}", e)))?;

        fs::write(&path, content)
            .map_err(|e| qilbee_core::Error::Internal(format!("Failed to write bootstrap state: {}", e)))?;

        Ok(())
    }
}

impl Default for BootstrapState {
    fn default() -> Self {
        Self::new()
    }
}

/// Bootstrap service for initial setup
pub struct BootstrapService {
    data_dir: PathBuf,
    user_service: std::sync::Arc<UserService>,
}

impl BootstrapService {
    /// Create new bootstrap service
    pub fn new(data_dir: PathBuf, user_service: std::sync::Arc<UserService>) -> Self {
        Self {
            data_dir,
            user_service,
        }
    }

    /// Check if bootstrap is required
    pub fn is_bootstrap_required(&self) -> Result<bool> {
        match BootstrapState::load(&self.data_dir)? {
            Some(state) => Ok(!state.is_bootstrapped),
            None => Ok(true), // No state file means bootstrap is required
        }
    }

    /// Run interactive bootstrap process
    pub fn run_interactive(&self) -> Result<BootstrapState> {
        println!("\n╔════════════════════════════════════════════════════════════╗");
        println!("║     QilbeeDB First-Time Setup - Initial Admin Account     ║");
        println!("╚════════════════════════════════════════════════════════════╝\n");

        println!("This appears to be a fresh QilbeeDB installation.");
        println!("Let's create your initial administrator account.\n");

        // Get admin username
        let username = loop {
            print!("Enter admin username (or press Enter for 'admin'): ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            let username = input.trim();

            let username = if username.is_empty() {
                "admin".to_string()
            } else {
                username.to_string()
            };

            if username.len() < 3 {
                println!("❌ Username must be at least 3 characters long.\n");
                continue;
            }

            if !username.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
                println!("❌ Username can only contain letters, numbers, underscores, and hyphens.\n");
                continue;
            }

            break username;
        };

        // Get admin email
        let email = loop {
            print!("Enter admin email: ");
            io::stdout().flush().unwrap();

            let mut input = String::new();
            io::stdin().read_line(&mut input).unwrap();
            let email = input.trim().to_string();

            if email.is_empty() {
                println!("❌ Email is required.\n");
                continue;
            }

            if !email.contains('@') || !email.contains('.') {
                println!("❌ Please enter a valid email address.\n");
                continue;
            }

            break email;
        };

        // Get admin password
        println!("\n{}", PASSWORD_REQUIREMENTS);

        let password = loop {
            let password = rpassword::prompt_password("Enter admin password: ").unwrap();

            if password.len() < MIN_PASSWORD_LENGTH {
                println!("❌ Password must be at least {} characters long.\n", MIN_PASSWORD_LENGTH);
                continue;
            }

            if !validate_password_strength(&password) {
                println!("❌ Password does not meet complexity requirements.\n");
                continue;
            }

            let confirm = rpassword::prompt_password("Confirm admin password: ").unwrap();

            if password != confirm {
                println!("❌ Passwords do not match.\n");
                continue;
            }

            break password;
        };

        // Create admin user
        info!("Creating admin user: {}", username);
        let mut user = self.user_service.create_user(username.clone(), email, &password)?;

        // Add admin role
        user.add_role(Role::Admin);
        self.user_service.update_user(user)?;

        // Save bootstrap state
        let state = BootstrapState::complete(username.clone());
        state.save(&self.data_dir)?;

        println!("\n✅ Admin account created successfully!");
        println!("   Username: {}", username);
        println!("   Role: Administrator\n");

        info!("Bootstrap completed successfully");

        Ok(state)
    }

    /// Run non-interactive bootstrap with environment variables
    pub fn run_from_env(&self) -> Result<BootstrapState> {
        info!("Running non-interactive bootstrap from environment variables");

        let username = std::env::var("QILBEEDB_ADMIN_USERNAME")
            .unwrap_or_else(|_| "admin".to_string());

        let email = std::env::var("QILBEEDB_ADMIN_EMAIL")
            .map_err(|_| qilbee_core::Error::Configuration(
                "QILBEEDB_ADMIN_EMAIL environment variable is required for non-interactive bootstrap".to_string()
            ))?;

        let password = std::env::var("QILBEEDB_ADMIN_PASSWORD")
            .map_err(|_| qilbee_core::Error::Configuration(
                "QILBEEDB_ADMIN_PASSWORD environment variable is required for non-interactive bootstrap".to_string()
            ))?;

        // Validate password strength
        if password.len() < MIN_PASSWORD_LENGTH {
            return Err(qilbee_core::Error::Configuration(
                format!("Password must be at least {} characters long", MIN_PASSWORD_LENGTH)
            ));
        }

        if !validate_password_strength(&password) {
            return Err(qilbee_core::Error::Configuration(
                "Password does not meet complexity requirements".to_string()
            ));
        }

        // Create admin user
        info!("Creating admin user: {}", username);
        let mut user = self.user_service.create_user(username.clone(), email, &password)?;

        // Add admin role
        user.add_role(Role::Admin);
        self.user_service.update_user(user)?;

        // Save bootstrap state
        let state = BootstrapState::complete(username.clone());
        state.save(&self.data_dir)?;

        info!("Non-interactive bootstrap completed successfully");

        Ok(state)
    }

    /// Run bootstrap automatically (interactive if TTY, env vars otherwise)
    pub fn run_auto(&self) -> Result<BootstrapState> {
        // Check if already bootstrapped
        if let Some(state) = BootstrapState::load(&self.data_dir)? {
            if state.is_bootstrapped {
                info!("System already bootstrapped with admin user: {}", state.admin_username);
                return Ok(state);
            }
        }

        // Check if we're in an interactive terminal
        if atty::is(atty::Stream::Stdin) && atty::is(atty::Stream::Stdout) {
            // Interactive mode
            self.run_interactive()
        } else {
            // Non-interactive mode (Docker, systemd, etc.)
            warn!("No TTY detected, using environment variables for bootstrap");
            self.run_from_env()
        }
    }
}

/// Validate password strength
fn validate_password_strength(password: &str) -> bool {
    let has_uppercase = password.chars().any(|c| c.is_uppercase());
    let has_lowercase = password.chars().any(|c| c.is_lowercase());
    let has_digit = password.chars().any(|c| c.is_numeric());
    let has_special = password.chars().any(|c| "!@#$%^&*()_+-=[]{}|;:,.<>?".contains(c));

    has_uppercase && has_lowercase && has_digit && has_special
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_password_validation() {
        // Valid passwords
        assert!(validate_password_strength("SecurePass123!"));
        assert!(validate_password_strength("MyP@ssw0rd2024"));
        assert!(validate_password_strength("C0mplex!tyRul3s"));

        // Invalid passwords
        assert!(!validate_password_strength("weakpassword"));       // No uppercase, digit, special
        assert!(!validate_password_strength("ALLUPPERCASE123!"));    // No lowercase
        assert!(!validate_password_strength("alllowercase123!"));    // No uppercase
        assert!(!validate_password_strength("NoDigitsHere!"));       // No digit
        assert!(!validate_password_strength("NoSpecialChar123"));    // No special char
    }

    #[test]
    fn test_bootstrap_state() {
        let state = BootstrapState::complete("admin".to_string());
        assert!(state.is_bootstrapped);
        assert_eq!(state.admin_username, "admin");
    }
}
