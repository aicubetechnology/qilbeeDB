//! Password validation and policy enforcement
//!
//! Provides enterprise-grade password complexity validation.

use qilbee_core::Result;
use serde::{Deserialize, Serialize};

/// Minimum password length requirement
pub const MIN_PASSWORD_LENGTH: usize = 12;

/// Special characters allowed in passwords
pub const SPECIAL_CHARS: &str = "!@#$%^&*()_+-=[]{}|;:,.<>?";

/// Password requirements description
pub const PASSWORD_REQUIREMENTS: &str = r#"Password must contain:
- Minimum 12 characters
- At least one uppercase letter
- At least one lowercase letter
- At least one number
- At least one special character (!@#$%^&*()_+-=[]{}|;:,.<>?)"#;

/// Password policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordPolicy {
    /// Minimum password length
    pub min_length: usize,
    /// Require uppercase letter
    pub require_uppercase: bool,
    /// Require lowercase letter
    pub require_lowercase: bool,
    /// Require digit
    pub require_digit: bool,
    /// Require special character
    pub require_special: bool,
    /// Special characters allowed
    pub special_chars: String,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self {
            min_length: MIN_PASSWORD_LENGTH,
            require_uppercase: true,
            require_lowercase: true,
            require_digit: true,
            require_special: true,
            special_chars: SPECIAL_CHARS.to_string(),
        }
    }
}

/// Result of password validation with details about failures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PasswordValidationResult {
    /// Whether the password is valid
    pub is_valid: bool,
    /// List of validation errors
    pub errors: Vec<String>,
}

impl PasswordValidationResult {
    pub fn valid() -> Self {
        Self {
            is_valid: true,
            errors: Vec::new(),
        }
    }

    pub fn invalid(errors: Vec<String>) -> Self {
        Self {
            is_valid: false,
            errors,
        }
    }
}

/// Validate password against the default policy
pub fn validate_password(password: &str) -> Result<()> {
    validate_password_with_policy(password, &PasswordPolicy::default())
}

/// Validate password against a custom policy
pub fn validate_password_with_policy(password: &str, policy: &PasswordPolicy) -> Result<()> {
    let result = check_password_strength(password, policy);

    if result.is_valid {
        Ok(())
    } else {
        Err(qilbee_core::Error::WeakPassword(result.errors.join("; ")))
    }
}

/// Check password strength and return detailed results
pub fn check_password_strength(password: &str, policy: &PasswordPolicy) -> PasswordValidationResult {
    let mut errors = Vec::new();

    // Check length
    if password.len() < policy.min_length {
        errors.push(format!(
            "Password must be at least {} characters long (current: {})",
            policy.min_length,
            password.len()
        ));
    }

    // Check uppercase
    if policy.require_uppercase && !password.chars().any(|c| c.is_uppercase()) {
        errors.push("Password must contain at least one uppercase letter".to_string());
    }

    // Check lowercase
    if policy.require_lowercase && !password.chars().any(|c| c.is_lowercase()) {
        errors.push("Password must contain at least one lowercase letter".to_string());
    }

    // Check digit
    if policy.require_digit && !password.chars().any(|c| c.is_numeric()) {
        errors.push("Password must contain at least one number".to_string());
    }

    // Check special character
    if policy.require_special && !password.chars().any(|c| policy.special_chars.contains(c)) {
        errors.push(format!(
            "Password must contain at least one special character ({})",
            policy.special_chars
        ));
    }

    if errors.is_empty() {
        PasswordValidationResult::valid()
    } else {
        PasswordValidationResult::invalid(errors)
    }
}

/// Simple boolean check for password strength (backward compatible)
pub fn is_password_strong(password: &str) -> bool {
    let policy = PasswordPolicy::default();
    check_password_strength(password, &policy).is_valid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_passwords() {
        // Valid passwords
        assert!(validate_password("SecurePass123!").is_ok());
        assert!(validate_password("MyP@ssw0rd2024").is_ok());
        assert!(validate_password("C0mplex!tyRul3s").is_ok());
        assert!(validate_password("VeryStr0ng&Secure").is_ok());
    }

    #[test]
    fn test_too_short() {
        let result = validate_password("Short1!");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("at least 12 characters"));
    }

    #[test]
    fn test_missing_uppercase() {
        let result = validate_password("alllowercase123!");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("uppercase letter"));
    }

    #[test]
    fn test_missing_lowercase() {
        let result = validate_password("ALLUPPERCASE123!");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("lowercase letter"));
    }

    #[test]
    fn test_missing_digit() {
        let result = validate_password("NoDigitsHere!!!!");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("number"));
    }

    #[test]
    fn test_missing_special() {
        let result = validate_password("NoSpecialChar123");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("special character"));
    }

    #[test]
    fn test_multiple_errors() {
        let result = validate_password("weak");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        // Should have multiple errors
        assert!(err.contains("at least 12 characters"));
        assert!(err.contains("uppercase"));
        assert!(err.contains("number"));
        assert!(err.contains("special"));
    }

    #[test]
    fn test_is_password_strong() {
        assert!(is_password_strong("SecurePass123!"));
        assert!(!is_password_strong("weak"));
    }

    #[test]
    fn test_custom_policy() {
        let lenient_policy = PasswordPolicy {
            min_length: 8,
            require_uppercase: false,
            require_lowercase: true,
            require_digit: true,
            require_special: false,
            special_chars: String::new(),
        };

        // This would fail default policy but pass lenient policy
        assert!(validate_password_with_policy("simple123", &lenient_policy).is_ok());
        assert!(validate_password("simple123").is_err());
    }
}
