# Authorization (RBAC)

QilbeeDB uses Role-Based Access Control (RBAC) to manage user permissions. This provides fine-grained control over who can perform which operations.

## Overview

RBAC in QilbeeDB provides:

- **5 Predefined Roles** with different permission levels
- **30+ Granular Permissions** for all database operations
- **Custom Role Creation** for specific use cases
- **User-Role Assignment** for flexible access control

## Predefined Roles

### Read
Basic read-only access to the database.

**Permissions:**
- Read nodes, relationships, and properties
- Execute read-only queries
- View indexes

**Use Case:** Data consumers, reporting tools, analytics dashboards

### Developer
Full access to data operations but no user management.

**Permissions:**
- All Read permissions
- Create, update, delete nodes and relationships
- Modify properties
- Execute write queries
- Manage indexes

**Use Case:** Application development, data engineering

### Analyst
Read access plus advanced query capabilities.

**Permissions:**
- All Read permissions
- Execute complex queries
- View query plans
- Access statistics

**Use Case:** Data analysis, business intelligence

### Admin
Database administration with user management.

**Permissions:**
- All Developer permissions
- User management (create, update, delete users)
- Role assignment
- View audit logs
- System configuration

**Use Case:** Database administrators

### SuperAdmin
Full system control including security configuration.

**Permissions:**
- All Admin permissions
- Security configuration
- Backup and restore
- System maintenance
- Audit log management

**Use Case:** System administrators, DevOps

## Complete Permission List

### Node Operations
- `READ_NODES` - Read node data
- `CREATE_NODES` - Create new nodes
- `UPDATE_NODES` - Modify existing nodes
- `DELETE_NODES` - Remove nodes

### Relationship Operations
- `READ_RELATIONSHIPS` - Read relationship data
- `CREATE_RELATIONSHIPS` - Create new relationships
- `UPDATE_RELATIONSHIPS` - Modify relationships
- `DELETE_RELATIONSHIPS` - Remove relationships

### Property Operations
- `READ_PROPERTIES` - Read property values
- `UPDATE_PROPERTIES` - Modify property values

### Query Operations
- `EXECUTE_QUERY` - Run Cypher queries
- `EXECUTE_WRITE_QUERY` - Run write queries
- `VIEW_QUERY_PLAN` - See query execution plans

### Index Operations
- `READ_INDEXES` - View indexes
- `CREATE_INDEXES` - Create new indexes
- `DELETE_INDEXES` - Remove indexes

### User Management
- `CREATE_USERS` - Create new users
- `READ_USERS` - View user information
- `UPDATE_USERS` - Modify user details
- `DELETE_USERS` - Remove users

### Role Management
- `ASSIGN_ROLES` - Assign roles to users
- `CREATE_ROLES` - Create custom roles
- `DELETE_ROLES` - Remove custom roles

### System Operations
- `VIEW_AUDIT_LOGS` - Access audit logs
- `CONFIGURE_SECURITY` - Modify security settings
- `BACKUP_DATABASE` - Create backups
- `RESTORE_DATABASE` - Restore from backups
- `SYSTEM_MAINTENANCE` - Perform maintenance

### Agent Memory Operations
- `READ_EPISODES` - Read memory episodes
- `WRITE_EPISODES` - Create/modify episodes
- `DELETE_EPISODES` - Remove episodes
- `CONSOLIDATE_MEMORY` - Trigger consolidation
- `FORGET_MEMORY` - Initiate forgetting

## Managing Users and Roles

### Create User

```bash
curl -X POST http://localhost:7474/api/v1/users \
  -H "Authorization: Bearer admin-token" \
  -d '{
    "username": "alice",
    "email": "alice@company.com",
    "password": "SecureP@ssw0rd123"
  }'
```

### Assign Role

```bash
curl -X POST http://localhost:7474/api/v1/users/{user_id}/roles \
  -H "Authorization: Bearer admin-token" \
  -d '{
    "role": "Developer"
  }'
```

### Remove Role

```bash
curl -X DELETE http://localhost:7474/api/v1/users/{user_id}/roles/Developer \
  -H "Authorization: Bearer admin-token"
```

### List User Roles

```bash
curl -X GET http://localhost:7474/api/v1/users/{user_id}/roles \
  -H "Authorization: Bearer admin-token"
```

## Custom Roles

Create custom roles for specific use cases.

### Create Custom Role

```bash
curl -X POST http://localhost:7474/api/v1/roles \
  -H "Authorization: Bearer admin-token" \
  -d '{
    "name": "DataScientist",
    "permissions": [
      "READ_NODES",
      "READ_RELATIONSHIPS",
      "READ_PROPERTIES",
      "EXECUTE_QUERY",
      "VIEW_QUERY_PLAN",
      "READ_EPISODES"
    ]
  }'
```

### View Custom Role

```bash
curl -X GET http://localhost:7474/api/v1/roles/DataScientist \
  -H "Authorization: Bearer admin-token"
```

### Update Custom Role

```bash
curl -X PUT http://localhost:7474/api/v1/roles/DataScientist \
  -H "Authorization: Bearer admin-token" \
  -d '{
    "permissions": [
      "READ_NODES",
      "READ_RELATIONSHIPS",
      "READ_PROPERTIES",
      "EXECUTE_QUERY",
      "VIEW_QUERY_PLAN",
      "READ_EPISODES",
      "WRITE_EPISODES"
    ]
  }'
```

### Delete Custom Role

```bash
curl -X DELETE http://localhost:7474/api/v1/roles/DataScientist \
  -H "Authorization: Bearer admin-token"
```

## Permission Checking

QilbeeDB automatically checks permissions for every operation.

### Permission Denied Example

```bash
# User with Read role tries to create a node
curl -X POST http://localhost:7474/api/v1/nodes \
  -H "Authorization: Bearer read-user-token" \
  -d '{
    "labels": ["Person"],
    "properties": {"name": "Alice"}
  }'
```

**Response:**

```json
{
  "error": "Insufficient permissions",
  "message": "Required permission: CREATE_NODES",
  "user_roles": ["Read"],
  "required_permission": "CREATE_NODES"
}
```

## Best Practices

!!! tip "Principle of Least Privilege"
    Grant users the minimum permissions needed for their role:

    - Start with Read role
    - Add permissions incrementally
    - Use custom roles for specific needs
    - Review permissions regularly

!!! warning "Admin Access"
    Limit Admin and SuperAdmin roles:

    - Only assign to trusted personnel
    - Use separate accounts for admin tasks
    - Enable audit logging for admin operations
    - Require MFA for admin accounts (future feature)

!!! info "Role Design"
    Design roles based on job functions:

    - **Application Accounts**: Developer role
    - **BI Tools**: Analyst role
    - **DBAs**: Admin role
    - **Custom Needs**: Create specific custom roles

## Example Role Matrix

| Operation | Read | Developer | Analyst | Admin | SuperAdmin |
|-----------|------|-----------|---------|-------|------------|
| Read Data | ✓ | ✓ | ✓ | ✓ | ✓ |
| Write Data | ✗ | ✓ | ✗ | ✓ | ✓ |
| Complex Queries | ✗ | ✓ | ✓ | ✓ | ✓ |
| Manage Users | ✗ | ✗ | ✗ | ✓ | ✓ |
| View Audit Logs | ✗ | ✗ | ✗ | ✓ | ✓ |
| System Config | ✗ | ✗ | ✗ | ✗ | ✓ |

## Client Examples

### Python

```python
from qilbeedb import Client

client = Client("http://localhost:7474")
client.login("admin", "password")

# Create user with role
user = client.users.create(
    username="alice",
    email="alice@company.com",
    password="SecureP@ssw0rd123",
    roles=["Developer"]
)

# Add additional role
client.users.add_role(user.id, "Analyst")

# Remove role
client.users.remove_role(user.id, "Analyst")
```

### JavaScript/Node.js

```javascript
const { Client } = require('@qilbeedb/client');

const client = new Client('http://localhost:7474');
await client.login('admin', 'password');

// Create user with role
const user = await client.users.create({
  username: 'alice',
  email: 'alice@company.com',
  password: 'SecureP@ssw0rd123',
  roles: ['Developer']
});

// Add additional role
await client.users.addRole(user.id, 'Analyst');

// Remove role
await client.users.removeRole(user.id, 'Analyst');
```

## Troubleshooting

### Permission Denied

If users can't perform operations they should be able to:

1. Check user's current roles
2. Verify role has required permissions
3. Check if custom role was modified
4. Review audit logs for permission changes

### Role Assignment Fails

Common issues:

- User performing assignment lacks `ASSIGN_ROLES` permission
- Role name is misspelled
- Custom role was deleted
- User already has the role

## Next Steps

- [Authentication](authentication.md) - Configure auth methods
- [Audit Logging](audit.md) - Track permission usage
- [Security Overview](overview.md) - Complete security guide
