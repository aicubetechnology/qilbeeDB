///! Admin-configurable rate limiting implementation using token bucket algorithm.
///!
///! This module provides enterprise-grade rate limiting for protecting
///! QilbeeDB API endpoints from abuse with admin-defined policies.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use uuid::Uuid;
use chrono::{DateTime, Utc};

/// Rate limit policy ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PolicyId(pub Uuid);

impl PolicyId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for PolicyId {
    fn default() -> Self {
        Self::new()
    }
}

/// Endpoint types that can have rate limiting applied
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EndpointType {
    /// Login endpoint
    Login,
    /// API key creation
    ApiKeyCreation,
    /// General API endpoints
    GeneralApi,
    /// User management endpoints
    UserManagement,
    /// Memory operations (store, search, consolidate, forget)
    MemoryOperations,
    /// Memory clear operation (destructive - stricter limits)
    MemoryClear,
    /// Custom endpoint pattern (regex or path)
    Custom(String),
}

impl EndpointType {
    /// Get default policy name
    pub fn default_name(&self) -> String {
        match self {
            EndpointType::Login => "login-limit".to_string(),
            EndpointType::ApiKeyCreation => "api-key-creation-limit".to_string(),
            EndpointType::GeneralApi => "general-api-limit".to_string(),
            EndpointType::UserManagement => "user-management-limit".to_string(),
            EndpointType::MemoryOperations => "memory-operations-limit".to_string(),
            EndpointType::MemoryClear => "memory-clear-limit".to_string(),
            EndpointType::Custom(pattern) => format!("custom-{}", pattern),
        }
    }
}

/// Rate limit policy configuration (admin-defined)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitPolicy {
    pub id: PolicyId,
    pub name: String,
    pub endpoint_type: EndpointType,
    /// Maximum number of requests allowed
    pub max_requests: u32,
    /// Time window in seconds
    pub window_secs: u64,
    /// Whether this policy is enabled
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub created_by: String,  // User ID who created this policy
}

impl RateLimitPolicy {
    /// Create default login policy
    pub fn default_login() -> Self {
        Self {
            id: PolicyId::new(),
            name: "Default Login Rate Limit".to_string(),
            endpoint_type: EndpointType::Login,
            max_requests: 10000,
            window_secs: 60,  // 10000 requests per minute - reasonable for testing while preventing abuse
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: "system".to_string(),
        }
    }

    /// Create default API key creation policy
    pub fn default_api_key_creation() -> Self {
        Self {
            id: PolicyId::new(),
            name: "Default API Key Creation Rate Limit".to_string(),
            endpoint_type: EndpointType::ApiKeyCreation,
            max_requests: 100,
            window_secs: 3600,  // 100 requests per hour - reasonable for key management
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: "system".to_string(),
        }
    }

    /// Create default general API policy
    pub fn default_general_api() -> Self {
        Self {
            id: PolicyId::new(),
            name: "Default General API Rate Limit".to_string(),
            endpoint_type: EndpointType::GeneralApi,
            max_requests: 100000,
            window_secs: 3600,  // 100k requests per hour - enterprise AI agent workload
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: "system".to_string(),
        }
    }

    /// Create default user management policy
    pub fn default_user_management() -> Self {
        Self {
            id: PolicyId::new(),
            name: "Default User Management Rate Limit".to_string(),
            endpoint_type: EndpointType::UserManagement,
            max_requests: 2000,
            window_secs: 60,  // 2000 requests per minute - reasonable for testing while preventing abuse
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: "system".to_string(),
        }
    }

    /// Create default memory operations policy
    pub fn default_memory_operations() -> Self {
        Self {
            id: PolicyId::new(),
            name: "Default Memory Operations Rate Limit".to_string(),
            endpoint_type: EndpointType::MemoryOperations,
            max_requests: 200000,
            window_secs: 60,  // 200k requests per minute - high throughput for AI agent memory operations
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: "system".to_string(),
        }
    }

    /// Create default memory clear policy (stricter for destructive operations)
    pub fn default_memory_clear() -> Self {
        Self {
            id: PolicyId::new(),
            name: "Default Memory Clear Rate Limit".to_string(),
            endpoint_type: EndpointType::MemoryClear,
            max_requests: 10000,
            window_secs: 3600,  // 10000 clears per hour - strict limit for destructive operation
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: "system".to_string(),
        }
    }

    /// Get Duration from window_secs
    pub fn window_duration(&self) -> Duration {
        Duration::from_secs(self.window_secs)
    }
}

/// Token bucket for rate limiting a single identifier
#[derive(Debug)]
struct TokenBucket {
    /// Current number of tokens
    tokens: f64,
    /// Maximum number of tokens (capacity)
    capacity: f64,
    /// Rate of token refill per second
    refill_rate: f64,
    /// Last time tokens were refilled
    last_refill: Instant,
}

impl TokenBucket {
    fn new(policy: &RateLimitPolicy) -> Self {
        let capacity = policy.max_requests as f64;
        let refill_rate = capacity / policy.window_secs as f64;

        Self {
            tokens: capacity,
            capacity,
            refill_rate,
            last_refill: Instant::now(),
        }
    }

    /// Attempt to consume a token. Returns true if successful.
    fn try_consume(&mut self) -> bool {
        self.refill();

        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// Refill tokens based on elapsed time
    fn refill(&mut self) {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();

        let new_tokens = self.tokens + (elapsed * self.refill_rate);
        self.tokens = new_tokens.min(self.capacity);
        self.last_refill = now;
    }

    /// Get remaining tokens
    fn remaining(&mut self) -> u32 {
        self.refill();
        self.tokens.floor() as u32
    }

    /// Get time until next token becomes available (in seconds)
    fn time_until_reset(&mut self) -> u64 {
        self.refill();

        if self.tokens >= self.capacity {
            return 0;
        }

        let tokens_needed = 1.0 - self.tokens;
        (tokens_needed / self.refill_rate).ceil() as u64
    }

    /// Check if this bucket's configuration matches the given policy.
    /// Returns false if the policy has changed and the bucket needs to be recreated.
    fn matches_policy(&self, policy: &RateLimitPolicy) -> bool {
        let expected_capacity = policy.max_requests as f64;
        let expected_refill_rate = expected_capacity / policy.window_secs as f64;

        // Use small epsilon for floating point comparison
        (self.capacity - expected_capacity).abs() < 0.01
            && (self.refill_rate - expected_refill_rate).abs() < 0.0001
    }
}

/// Rate limiter key (can be IP or user ID)
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum RateLimitKey {
    Ip(String),
    UserId(String),
}

impl RateLimitKey {
    pub fn from_ip(ip: impl Into<String>) -> Self {
        Self::Ip(ip.into())
    }

    pub fn from_user_id(user_id: impl Into<String>) -> Self {
        Self::UserId(user_id.into())
    }
}

/// Rate limit result
#[derive(Debug, Clone)]
pub struct RateLimitInfo {
    pub allowed: bool,
    pub limit: u32,
    pub remaining: u32,
    pub reset: u64,
}

/// Service for managing rate limit policies
#[derive(Clone)]
pub struct RateLimitService {
    /// Stored policies
    policies: Arc<RwLock<HashMap<PolicyId, RateLimitPolicy>>>,
    /// Active rate limiters per endpoint type
    limiters: Arc<RwLock<HashMap<EndpointType, Arc<RwLock<HashMap<RateLimitKey, TokenBucket>>>>>>,
}

impl RateLimitService {
    /// Create a new rate limit service with default policies
    pub fn new() -> Self {
        let mut policies = HashMap::new();

        // Add default policies
        let login_policy = RateLimitPolicy::default_login();
        let api_key_policy = RateLimitPolicy::default_api_key_creation();
        let general_policy = RateLimitPolicy::default_general_api();
        let user_management_policy = RateLimitPolicy::default_user_management();
        let memory_operations_policy = RateLimitPolicy::default_memory_operations();
        let memory_clear_policy = RateLimitPolicy::default_memory_clear();

        policies.insert(login_policy.id, login_policy);
        policies.insert(api_key_policy.id, api_key_policy);
        policies.insert(general_policy.id, general_policy);
        policies.insert(user_management_policy.id, user_management_policy);
        policies.insert(memory_operations_policy.id, memory_operations_policy);
        policies.insert(memory_clear_policy.id, memory_clear_policy);

        Self {
            policies: Arc::new(RwLock::new(policies)),
            limiters: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a new rate limit policy (admin only)
    pub fn create_policy(&self, mut policy: RateLimitPolicy) -> PolicyId {
        policy.id = PolicyId::new();
        policy.created_at = Utc::now();
        policy.updated_at = Utc::now();

        let id = policy.id;
        let mut policies = self.policies.write().unwrap();
        policies.insert(id, policy);

        id
    }

    /// Update an existing policy (admin only)
    pub fn update_policy(&self, id: PolicyId, mut policy: RateLimitPolicy) -> Option<()> {
        let mut policies = self.policies.write().unwrap();

        if let Some(existing) = policies.get_mut(&id) {
            policy.id = id;
            policy.created_at = existing.created_at;
            policy.updated_at = Utc::now();
            *existing = policy;
            Some(())
        } else {
            None
        }
    }

    /// Delete a policy (admin only)
    pub fn delete_policy(&self, id: PolicyId) -> Option<RateLimitPolicy> {
        let mut policies = self.policies.write().unwrap();
        policies.remove(&id)
    }

    /// Get a policy by ID
    pub fn get_policy(&self, id: PolicyId) -> Option<RateLimitPolicy> {
        let policies = self.policies.read().unwrap();
        policies.get(&id).cloned()
    }

    /// List all policies
    pub fn list_policies(&self) -> Vec<RateLimitPolicy> {
        let policies = self.policies.read().unwrap();
        policies.values().cloned().collect()
    }

    /// Get policy for an endpoint type
    pub fn get_policy_for_endpoint(&self, endpoint_type: &EndpointType) -> Option<RateLimitPolicy> {
        let policies = self.policies.read().unwrap();
        // Get all matching enabled policies and return the most recently created one
        // This allows custom policies to override default policies
        policies
            .values()
            .filter(|p| &p.endpoint_type == endpoint_type && p.enabled)
            .max_by_key(|p| p.created_at)
            .cloned()
    }

    /// Check if a request is allowed for the given endpoint and key
    pub fn check(&self, endpoint_type: EndpointType, key: RateLimitKey) -> RateLimitInfo {
        // Get policy for this endpoint
        let policy = match self.get_policy_for_endpoint(&endpoint_type) {
            Some(p) if p.enabled => p,
            _ => {
                // No policy or disabled - allow request
                return RateLimitInfo {
                    allowed: true,
                    limit: u32::MAX,
                    remaining: u32::MAX,
                    reset: 0,
                };
            }
        };

        // Get or create limiter for this endpoint
        let mut limiters = self.limiters.write().unwrap();
        let endpoint_limiters = limiters
            .entry(endpoint_type)
            .or_insert_with(|| Arc::new(RwLock::new(HashMap::new())));

        let mut buckets = endpoint_limiters.write().unwrap();

        // Get existing bucket or create new one
        let bucket = buckets
            .entry(key)
            .or_insert_with(|| TokenBucket::new(&policy));

        // Check if bucket needs to be recreated due to policy change
        // This ensures that when an admin updates a policy, existing buckets
        // are reset to use the new limits instead of the old cached values
        if !bucket.matches_policy(&policy) {
            *bucket = TokenBucket::new(&policy);
        }

        let allowed = bucket.try_consume();
        let remaining = bucket.remaining();
        let reset = bucket.time_until_reset();

        RateLimitInfo {
            allowed,
            limit: policy.max_requests,
            remaining,
            reset,
        }
    }

    /// Clean up old buckets that are at full capacity
    /// This prevents memory leaks from one-time requests
    pub fn cleanup(&self) {
        let limiters = self.limiters.read().unwrap();

        for endpoint_limiters in limiters.values() {
            let mut buckets = endpoint_limiters.write().unwrap();
            buckets.retain(|_, bucket| bucket.tokens < bucket.capacity);
        }
    }
}

impl Default for RateLimitService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_policy_creation() {
        let service = RateLimitService::new();

        let policy = RateLimitPolicy {
            id: PolicyId::new(),
            name: "Test Policy".to_string(),
            endpoint_type: EndpointType::Custom("/api/test".to_string()),
            max_requests: 10,
            window_secs: 60,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: "admin".to_string(),
        };

        let id = service.create_policy(policy.clone());
        let retrieved = service.get_policy(id).unwrap();

        assert_eq!(retrieved.name, "Test Policy");
        assert_eq!(retrieved.max_requests, 10);
    }

    #[test]
    fn test_rate_limiting() {
        let service = RateLimitService::new();
        let key = RateLimitKey::from_ip("127.0.0.1");

        // Create a custom policy with low limit for testing (5 requests per minute)
        let test_policy = RateLimitPolicy {
            id: PolicyId::new(),
            name: "Test Login Limit".to_string(),
            endpoint_type: EndpointType::Login,
            max_requests: 5,
            window_secs: 60,
            enabled: true,
            created_at: Utc::now() + chrono::Duration::seconds(1), // Make it newer than default
            updated_at: Utc::now(),
            created_by: "test".to_string(),
        };
        service.create_policy(test_policy);

        // Test endpoint should now have limit of 5 per minute
        for i in 0..5 {
            let info = service.check(EndpointType::Login, key.clone());
            assert!(info.allowed, "Request {} should be allowed", i + 1);
        }

        // 6th request should fail
        let info = service.check(EndpointType::Login, key.clone());
        assert!(!info.allowed, "6th request should be denied");
        assert_eq!(info.remaining, 0);
    }

    #[test]
    fn test_policy_change_resets_bucket() {
        let service = RateLimitService::new();
        let key = RateLimitKey::from_ip("192.168.1.100");

        // Create initial policy with 10 requests
        let initial_policy = RateLimitPolicy {
            id: PolicyId::new(),
            name: "Initial Policy".to_string(),
            endpoint_type: EndpointType::Custom("/test/change".to_string()),
            max_requests: 10,
            window_secs: 60,
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            created_by: "test".to_string(),
        };
        service.create_policy(initial_policy);

        // Use 5 tokens
        for i in 0..5 {
            let info = service.check(EndpointType::Custom("/test/change".to_string()), key.clone());
            assert!(info.allowed, "Request {} should be allowed", i + 1);
            assert_eq!(info.limit, 10);
        }

        // Should have 5 remaining
        let info = service.check(EndpointType::Custom("/test/change".to_string()), key.clone());
        assert!(info.allowed);
        assert_eq!(info.remaining, 4); // 10 - 6 = 4

        // Now create a NEW policy with only 3 requests (should reset bucket)
        let new_policy = RateLimitPolicy {
            id: PolicyId::new(),
            name: "Stricter Policy".to_string(),
            endpoint_type: EndpointType::Custom("/test/change".to_string()),
            max_requests: 3,
            window_secs: 60,
            enabled: true,
            created_at: Utc::now() + chrono::Duration::seconds(2), // Make it newer
            updated_at: Utc::now(),
            created_by: "test".to_string(),
        };
        service.create_policy(new_policy);

        // First request after policy change should reset bucket and use new limit
        let info = service.check(EndpointType::Custom("/test/change".to_string()), key.clone());
        assert!(info.allowed);
        assert_eq!(info.limit, 3); // New policy limit
        assert_eq!(info.remaining, 2); // Fresh bucket: 3 - 1 = 2

        // Use remaining 2 tokens
        let info = service.check(EndpointType::Custom("/test/change".to_string()), key.clone());
        assert!(info.allowed);
        let info = service.check(EndpointType::Custom("/test/change".to_string()), key.clone());
        assert!(info.allowed);

        // 4th request should be denied (limit is 3)
        let info = service.check(EndpointType::Custom("/test/change".to_string()), key.clone());
        assert!(!info.allowed, "4th request should be denied with new 3-request limit");
    }

    #[test]
    fn test_disabled_policy() {
        let service = RateLimitService::new();

        // Create a disabled policy
        let mut policy = RateLimitPolicy::default_login();
        policy.enabled = false;
        policy.endpoint_type = EndpointType::Custom("/test".to_string());

        service.create_policy(policy);

        // Should allow unlimited requests since policy is disabled
        let key = RateLimitKey::from_ip("127.0.0.1");
        for _ in 0..100 {
            let info = service.check(
                EndpointType::Custom("/test".to_string()),
                key.clone()
            );
            assert!(info.allowed);
        }
    }
}
