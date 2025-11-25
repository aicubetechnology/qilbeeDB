# SET

The SET clause is used to update properties on nodes and relationships.

## Set Node Properties

### Set Single Property

```cypher
MATCH (u:User {name: 'Alice'})
SET u.age = 29
RETURN u
```

### Set Multiple Properties

```cypher
MATCH (u:User {name: 'Alice'})
SET u.age = 29, u.city = 'New York', u.updated = datetime()
RETURN u
```

### Set from Map

```cypher
MATCH (u:User {name: 'Alice'})
SET u = {name: 'Alice', age: 29, city: 'New York'}
RETURN u
```

**Warning**: This replaces ALL properties!

### Set with += (Add Properties)

```cypher
MATCH (u:User {name: 'Alice'})
SET u += {age: 29, city: 'New York'}
RETURN u
```

This preserves existing properties and adds/updates specified ones.

## Set Relationship Properties

```cypher
MATCH (a:User)-[r:KNOWS]->(b:User)
WHERE a.name = 'Alice' AND b.name = 'Bob'
SET r.strength = 0.9, r.updated = datetime()
RETURN r
```

## Add Labels

### Add Single Label

```cypher
MATCH (u:User {name: 'Alice'})
SET u:Premium
RETURN u
```

### Add Multiple Labels

```cypher
MATCH (u:User {name: 'Alice'})
SET u:Premium:Verified
RETURN u
```

## Set with Parameters

```cypher
MATCH (u:User {name: $name})
SET u.age = $age, u.city = $city
RETURN u
```

Python example:
```python
graph.query("""
    MATCH (u:User {name: $name})
    SET u.age = $age
    RETURN u
""", {
    'name': 'Alice',
    'age': 29
})
```

## Conditional Set

### Set Based on Condition

```cypher
MATCH (u:User)
SET u.status = CASE
  WHEN u.age < 18 THEN 'minor'
  WHEN u.age >= 65 THEN 'senior'
  ELSE 'adult'
END
```

### Set If Property Exists

```cypher
MATCH (u:User)
WHERE u.tempEmail IS NOT NULL
SET u.email = u.tempEmail
REMOVE u.tempEmail
```

## Increment/Decrement

### Increment Counter

```cypher
MATCH (p:Post {id: $postId})
SET p.views = p.views + 1
RETURN p.views
```

### Decrement Value

```cypher
MATCH (u:User {name: 'Alice'})
SET u.credits = u.credits - 10
RETURN u.credits
```

## Set from Expressions

### Computed Values

```cypher
MATCH (u:User)
SET u.fullName = u.firstName + ' ' + u.lastName
RETURN u
```

### String Operations

```cypher
MATCH (u:User)
SET u.email = toLower(u.email),
    u.username = trim(u.username)
RETURN u
```

### Date/Time Operations

```cypher
MATCH (u:User)
SET u.lastLogin = datetime(),
    u.sessionExpiry = datetime() + duration('PT1H')
RETURN u
```

## Set All Properties

Copy all properties from one node to another:

```cypher
MATCH (source:User {name: 'Alice'}),
      (target:User {name: 'Bob'})
SET target = properties(source)
RETURN target
```

## Set NULL (Remove Property)

```cypher
MATCH (u:User {name: 'Alice'})
SET u.tempData = NULL
RETURN u
```

Or use REMOVE:

```cypher
MATCH (u:User {name: 'Alice'})
REMOVE u.tempData
```

## Bulk Updates

### Update All Matching Nodes

```cypher
MATCH (u:User)
WHERE u.active = true
SET u.lastNotified = datetime()
RETURN count(u) AS updated
```

### Update with UNWIND

```cypher
UNWIND [
  {name: 'Alice', age: 29},
  {name: 'Bob', age: 33}
] AS update
MATCH (u:User {name: update.name})
SET u.age = update.age
```

## Common Patterns

### Update or Create Timestamp

```cypher
MATCH (u:User {name: 'Alice'})
SET u.updated = datetime()
SET u.created = coalesce(u.created, datetime())
RETURN u
```

### Increment View Counter

```cypher
MATCH (p:Post {id: $postId})
SET p.views = coalesce(p.views, 0) + 1,
    p.lastViewed = datetime()
RETURN p
```

### Toggle Boolean

```cypher
MATCH (u:User {name: 'Alice'})
SET u.active = NOT u.active
RETURN u.active
```

### Add to Array

```cypher
MATCH (u:User {name: 'Alice'})
SET u.tags = coalesce(u.tags, []) + ['premium']
RETURN u
```

### Update Based on Relationship

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
WITH u, count(p) AS postCount
SET u.totalPosts = postCount
RETURN u
```

### Normalize Data

```cypher
MATCH (u:User)
SET u.email = toLower(trim(u.email)),
    u.name = trim(u.name)
WHERE u.email IS NOT NULL
RETURN count(u) AS normalized
```

### Set Default Values

```cypher
MATCH (u:User)
WHERE u.role IS NULL
SET u.role = 'user'
RETURN count(u) AS updated
```

## SET with CREATE

```cypher
CREATE (u:User)
SET u.name = 'Alice',
    u.created = datetime(),
    u.id = randomUUID()
RETURN u
```

## SET with MERGE

```cypher
MERGE (u:User {email: $email})
ON CREATE SET 
  u.name = $name,
  u.created = datetime()
ON MATCH SET
  u.lastSeen = datetime()
RETURN u
```

## Performance Tips

1. **Batch Updates**
   ```cypher
   // Good: Single query updates many
   MATCH (u:User)
   WHERE u.active = true
   SET u.updated = datetime()
   
   // Bad: Many individual queries
   // Multiple SET queries for each user
   ```

2. **Use += for Partial Updates**
   ```cypher
   // Good: Only updates specified properties
   SET u += {age: 29}
   
   // Bad: Replaces all properties
   SET u = {age: 29}
   ```

3. **Index Updated Properties**
   ```cypher
   CREATE INDEX ON :User(lastLogin)
   
   // Fast queries on updated property
   MATCH (u:User)
   WHERE u.lastLogin > datetime() - duration('P7D')
   RETURN u
   ```

4. **Minimize Property Writes**
   ```cypher
   // Good: Only update if changed
   MATCH (u:User {name: 'Alice'})
   WHERE u.age <> 29
   SET u.age = 29
   
   // Bad: Always writes even if unchanged
   MATCH (u:User {name: 'Alice'})
   SET u.age = 29
   ```

## Common Mistakes

### Overwriting All Properties

```cypher
// WRONG: Removes all other properties
MATCH (u:User {name: 'Alice'})
SET u = {age: 29}  // name is now gone!

// CORRECT: Use +=
MATCH (u:User {name: 'Alice'})
SET u += {age: 29}  // name is preserved
```

### Not Handling NULL

```cypher
// WRONG: Error if credits is NULL
SET u.credits = u.credits + 10

// CORRECT: Handle NULL
SET u.credits = coalesce(u.credits, 0) + 10
```

## Next Steps

- Create data with [CREATE](create.md)
- Delete data with [DELETE](delete.md)
- Match patterns with [MATCH](match.md)
- Read [Cypher Introduction](introduction.md)
