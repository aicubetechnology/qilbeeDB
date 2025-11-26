//! JWT and API Key token management

use chrono::{DateTime, Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use rand::{distributions::Alphanumeric, Rng};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;
use qilbee_core::Result;
use super::user::UserId;
use super::rbac::Role;

/// JWT Claims
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,          // Subject (user_id)
    pub username: String,
    pub roles: Vec<Role>,
    pub exp: usize,          // Expiration time
    pub iat: usize,          // Issued at
    pub jti: String,         // JWT ID
}

/// Auth token
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthToken {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: u64,
    pub refresh_token: Option<String>,
}

/// API Key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKey {
    pub id: String,
    pub user_id: UserId,
    #[serde(skip_serializing)]
    pub key_hash: String,
    pub prefix: String,
    pub name: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub last_used: Option<DateTime<Utc>>,
    pub is_active: bool,
}

/// Token service
pub struct TokenService {
    jwt_secret: String,
    jwt_expiration_secs: u64,
    refresh_expiration_secs: u64,
    api_keys: Arc<RwLock<HashMap<String, ApiKey>>>,
}

impl TokenService {
    /// Create new token service
    pub fn new(jwt_secret: String) -> Self {
        Self {
            jwt_secret,
            jwt_expiration_secs: 86400,      // 24 hours
            refresh_expiration_secs: 2592000, // 30 days
            api_keys: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Generate JWT token
    pub fn generate_jwt(&self, user_id: UserId, username: String, roles: Vec<Role>) -> Result<AuthToken> {
        let now = Utc::now();
        let exp = now + Duration::seconds(self.jwt_expiration_secs as i64);

        let claims = Claims {
            sub: user_id.0.to_string(),
            username,
            roles,
            exp: exp.timestamp() as usize,
            iat: now.timestamp() as usize,
            jti: Uuid::new_v4().to_string(),
        };

        let token = encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.jwt_secret.as_bytes()),
        )
        .map_err(|e| qilbee_core::Error::Internal(format!("Failed to generate JWT: {}", e)))?;

        Ok(AuthToken {
            access_token: token,
            token_type: "Bearer".to_string(),
            expires_in: self.jwt_expiration_secs,
            refresh_token: None,
        })
    }

    /// Validate JWT token
    pub fn validate_jwt(&self, token: &str) -> Result<Claims> {
        let validation = Validation::default();
        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(self.jwt_secret.as_bytes()),
            &validation,
        )
        .map_err(|e| qilbee_core::Error::Internal(format!("Invalid JWT: {}", e)))?;

        Ok(token_data.claims)
    }

    /// Generate API key
    pub fn generate_api_key(&self, user_id: UserId, name: String) -> Result<(String, ApiKey)> {
        let key: String = rand::thread_rng()
            .sample_iter(&Alphanumeric)
            .take(32)
            .map(char::from)
            .collect();

        let prefix = "qilbee_live_";
        let full_key = format!("{}{}", prefix, key);

        let mut hasher = Sha256::new();
        hasher.update(full_key.as_bytes());
        let key_hash = format!("{:x}", hasher.finalize());

        let api_key = ApiKey {
            id: Uuid::new_v4().to_string(),
            user_id,
            key_hash: key_hash.clone(),
            prefix: prefix.to_string(),
            name,
            created_at: Utc::now(),
            expires_at: None,
            last_used: None,
            is_active: true,
        };

        self.api_keys.write().unwrap().insert(key_hash, api_key.clone());

        Ok((full_key, api_key))
    }

    /// Validate API key
    pub fn validate_api_key(&self, key: &str) -> Result<UserId> {
        let mut hasher = Sha256::new();
        hasher.update(key.as_bytes());
        let key_hash = format!("{:x}", hasher.finalize());

        let mut api_keys = self.api_keys.write().unwrap();
        let api_key = api_keys
            .get_mut(&key_hash)
            .ok_or_else(|| qilbee_core::Error::Internal("Invalid API key".to_string()))?;

        if !api_key.is_active {
            return Err(qilbee_core::Error::Internal("API key is inactive".to_string()));
        }

        if let Some(expires_at) = api_key.expires_at {
            if Utc::now() > expires_at {
                return Err(qilbee_core::Error::Internal("API key expired".to_string()));
            }
        }

        api_key.last_used = Some(Utc::now());

        Ok(api_key.user_id)
    }

    /// Revoke API key
    pub fn revoke_api_key(&self, key_hash: &str) -> Result<()> {
        self.api_keys.write().unwrap().remove(key_hash);
        Ok(())
    }

    /// List API keys for user
    pub fn list_api_keys(&self, user_id: &UserId) -> Vec<ApiKey> {
        self.api_keys
            .read()
            .unwrap()
            .values()
            .filter(|k| &k.user_id == user_id)
            .cloned()
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_jwt_generation_and_validation() {
        let service = TokenService::new("test_secret".to_string());
        let user_id = UserId::new();
        let roles = vec![Role::Developer];

        let token = service
            .generate_jwt(user_id, "testuser".to_string(), roles.clone())
            .unwrap();

        let claims = service.validate_jwt(&token.access_token).unwrap();
        assert_eq!(claims.username, "testuser");
        assert_eq!(claims.roles, roles);
    }

    #[test]
    fn test_api_key_generation_and_validation() {
        let service = TokenService::new("test_secret".to_string());
        let user_id = UserId::new();

        let (key, api_key) = service
            .generate_api_key(user_id, "Test Key".to_string())
            .unwrap();

        assert!(key.starts_with("qilbee_live_"));
        assert_eq!(api_key.name, "Test Key");

        let validated_user_id = service.validate_api_key(&key).unwrap();
        assert_eq!(validated_user_id, user_id);
    }
}
