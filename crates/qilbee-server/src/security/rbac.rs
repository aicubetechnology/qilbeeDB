//! Role-Based Access Control (RBAC) system

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

/// Permissions in the system
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    // Graph operations
    GraphCreate,
    GraphRead,
    GraphUpdate,
    GraphDelete,
    GraphExecuteQuery,

    // Node operations
    NodeCreate,
    NodeRead,
    NodeUpdate,
    NodeDelete,

    // Relationship operations
    RelationshipCreate,
    RelationshipRead,
    RelationshipUpdate,
    RelationshipDelete,

    // Memory operations
    MemoryCreate,
    MemoryRead,
    MemoryUpdate,
    MemoryDelete,
    MemorySearch,
    MemoryConsolidate,
    MemoryForget,

    // Agent operations
    AgentCreate,
    AgentRead,
    AgentManage,

    // Admin operations
    UserCreate,
    UserRead,
    UserUpdate,
    UserDelete,
    UserManageRoles,

    // System operations
    SystemMonitor,
    SystemConfigure,
    SystemBackup,
    SystemRestore,

    // Audit operations
    AuditRead,
    AuditManage,
}

/// Predefined roles with associated permissions
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    /// Administrator - full system access
    Admin,

    /// Developer - read/write data operations
    Developer,

    /// Data Scientist - read and query access
    DataScientist,

    /// Agent - AI agent with specific permissions
    Agent,

    /// Read-only user
    Read,

    /// Custom role
    Custom(String),
}

impl Role {
    /// Get permissions for this role
    pub fn permissions(&self) -> HashSet<Permission> {
        match self {
            Role::Admin => HashSet::from([
                // All permissions
                Permission::GraphCreate,
                Permission::GraphRead,
                Permission::GraphUpdate,
                Permission::GraphDelete,
                Permission::GraphExecuteQuery,
                Permission::NodeCreate,
                Permission::NodeRead,
                Permission::NodeUpdate,
                Permission::NodeDelete,
                Permission::RelationshipCreate,
                Permission::RelationshipRead,
                Permission::RelationshipUpdate,
                Permission::RelationshipDelete,
                Permission::MemoryCreate,
                Permission::MemoryRead,
                Permission::MemoryUpdate,
                Permission::MemoryDelete,
                Permission::MemorySearch,
                Permission::MemoryConsolidate,
                Permission::MemoryForget,
                Permission::AgentCreate,
                Permission::AgentRead,
                Permission::AgentManage,
                Permission::UserCreate,
                Permission::UserRead,
                Permission::UserUpdate,
                Permission::UserDelete,
                Permission::UserManageRoles,
                Permission::SystemMonitor,
                Permission::SystemConfigure,
                Permission::SystemBackup,
                Permission::SystemRestore,
                Permission::AuditRead,
                Permission::AuditManage,
            ]),

            Role::Developer => HashSet::from([
                Permission::GraphCreate,
                Permission::GraphRead,
                Permission::GraphUpdate,
                Permission::GraphDelete,
                Permission::GraphExecuteQuery,
                Permission::NodeCreate,
                Permission::NodeRead,
                Permission::NodeUpdate,
                Permission::NodeDelete,
                Permission::RelationshipCreate,
                Permission::RelationshipRead,
                Permission::RelationshipUpdate,
                Permission::RelationshipDelete,
                Permission::MemoryCreate,
                Permission::MemoryRead,
                Permission::MemoryUpdate,
                Permission::MemoryDelete,
                Permission::MemorySearch,
                Permission::MemoryConsolidate,
                Permission::MemoryForget,
                Permission::AgentRead,
                Permission::SystemMonitor,
            ]),

            Role::DataScientist => HashSet::from([
                Permission::GraphRead,
                Permission::GraphExecuteQuery,
                Permission::NodeRead,
                Permission::RelationshipRead,
                Permission::MemoryRead,
                Permission::MemorySearch,
                Permission::AgentRead,
                Permission::SystemMonitor,
            ]),

            Role::Agent => HashSet::from([
                Permission::GraphRead,
                Permission::GraphExecuteQuery,
                Permission::NodeRead,
                Permission::RelationshipRead,
                Permission::MemoryCreate,
                Permission::MemoryRead,
                Permission::MemorySearch,
                Permission::MemoryConsolidate,
                Permission::AgentRead,
            ]),

            Role::Read => HashSet::from([
                Permission::GraphRead,
                Permission::NodeRead,
                Permission::RelationshipRead,
                Permission::MemoryRead,
                Permission::AgentRead,
            ]),

            Role::Custom(_) => HashSet::new(),
        }
    }
}

/// RBAC service for managing roles and permissions
pub struct RbacService {
    custom_roles: Arc<RwLock<HashMap<String, HashSet<Permission>>>>,
}

impl RbacService {
    /// Create new RBAC service
    pub fn new() -> Self {
        Self {
            custom_roles: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Check if roles have a specific permission
    pub fn has_permission(&self, roles: &[Role], permission: &Permission) -> bool {
        for role in roles {
            match role {
                Role::Custom(name) => {
                    if let Some(perms) = self.custom_roles.read().unwrap().get(name) {
                        if perms.contains(permission) {
                            return true;
                        }
                    }
                }
                _ => {
                    if role.permissions().contains(permission) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if roles have any of the specified permissions
    pub fn has_any_permission(&self, roles: &[Role], permissions: &[Permission]) -> bool {
        permissions
            .iter()
            .any(|perm| self.has_permission(roles, perm))
    }

    /// Check if roles have all of the specified permissions
    pub fn has_all_permissions(&self, roles: &[Role], permissions: &[Permission]) -> bool {
        permissions
            .iter()
            .all(|perm| self.has_permission(roles, perm))
    }

    /// Create a custom role
    pub fn create_custom_role(&self, name: String, permissions: HashSet<Permission>) {
        self.custom_roles.write().unwrap().insert(name, permissions);
    }

    /// Get permissions for a custom role
    pub fn get_custom_role_permissions(&self, name: &str) -> Option<HashSet<Permission>> {
        self.custom_roles.read().unwrap().get(name).cloned()
    }

    /// Delete a custom role
    pub fn delete_custom_role(&self, name: &str) {
        self.custom_roles.write().unwrap().remove(name);
    }

    /// Get all permissions for roles
    pub fn get_all_permissions(&self, roles: &[Role]) -> HashSet<Permission> {
        let mut all_perms = HashSet::new();
        for role in roles {
            match role {
                Role::Custom(name) => {
                    if let Some(perms) = self.custom_roles.read().unwrap().get(name) {
                        all_perms.extend(perms.clone());
                    }
                }
                _ => {
                    all_perms.extend(role.permissions());
                }
            }
        }
        all_perms
    }
}

impl Default for RbacService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_permissions() {
        let admin_perms = Role::Admin.permissions();
        assert!(admin_perms.contains(&Permission::UserCreate));
        assert!(admin_perms.contains(&Permission::GraphDelete));

        let read_perms = Role::Read.permissions();
        assert!(read_perms.contains(&Permission::GraphRead));
        assert!(!read_perms.contains(&Permission::GraphCreate));
    }

    #[test]
    fn test_rbac_service() {
        let rbac = RbacService::new();

        let roles = vec![Role::Developer];
        assert!(rbac.has_permission(&roles, &Permission::GraphCreate));
        assert!(!rbac.has_permission(&roles, &Permission::UserCreate));

        let admin_roles = vec![Role::Admin];
        assert!(rbac.has_permission(&admin_roles, &Permission::UserCreate));
    }

    #[test]
    fn test_custom_roles() {
        let rbac = RbacService::new();

        let mut perms = HashSet::new();
        perms.insert(Permission::GraphRead);
        perms.insert(Permission::NodeRead);

        rbac.create_custom_role("viewer".to_string(), perms);

        let roles = vec![Role::Custom("viewer".to_string())];
        assert!(rbac.has_permission(&roles, &Permission::GraphRead));
        assert!(!rbac.has_permission(&roles, &Permission::GraphCreate));
    }
}
