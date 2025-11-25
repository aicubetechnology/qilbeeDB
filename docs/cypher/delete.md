# DELETE

The DELETE clause is used to delete nodes and relationships from the graph.

## Delete Nodes

### Basic Node Deletion

```cypher
MATCH (u:User {name: 'Alice'})
DELETE u
```

### Delete Multiple Nodes

```cypher
MATCH (u:User)
WHERE u.active = false
DELETE u
```

## Delete Relationships

### Delete Specific Relationship

```cypher
MATCH (a:User)-[r:KNOWS]->(b:User)
WHERE a.name = 'Alice' AND b.name = 'Bob'
DELETE r
```

### Delete All Relationships of a Type

```cypher
MATCH ()-[r:TEMPORARY]->()
DELETE r
```

## DETACH DELETE

Delete node and all its relationships:

```cypher
MATCH (u:User {name: 'Alice'})
DETACH DELETE u
```

This is equivalent to:

```cypher
MATCH (u:User {name: 'Alice'})
OPTIONAL MATCH (u)-[r]-()
DELETE r, u
```

## Delete with Conditions

### Delete Old Data

```cypher
MATCH (p:Post)
WHERE p.created < datetime() - duration('P365D')
DETACH DELETE p
```

### Delete Inactive Users

```cypher
MATCH (u:User)
WHERE u.lastLogin < datetime() - duration('P90D')
  AND u.posts = 0
DETACH DELETE u
```

## Delete Patterns

### Delete Relationship Chain

```cypher
MATCH (a:User)-[r1:KNOWS]->(b:User)-[r2:KNOWS]->(c:User)
WHERE a.name = 'Alice' AND c.name = 'Charlie'
DELETE r1, r2
```

### Delete Subgraph

```cypher
MATCH (u:User {name: 'Alice'})-[r*0..2]-(connected)
DETACH DELETE u, connected
```

## Delete All Data

### Delete All Nodes and Relationships

```cypher
MATCH (n)
DETACH DELETE n
```

### Delete by Label

```cypher
MATCH (n:TempData)
DETACH DELETE n
```

## Conditional Delete

### Delete with CASE

```cypher
MATCH (u:User)
WITH u, 
     CASE 
       WHEN u.lastLogin < datetime() - duration('P365D') THEN true
       ELSE false 
     END AS shouldDelete
WHERE shouldDelete = true
DETACH DELETE u
```

## Delete and Return

Return deleted elements:

```cypher
MATCH (u:User {name: 'Alice'})
WITH u, u.name AS deletedName
DETACH DELETE u
RETURN deletedName
```

## Batch Delete

### Delete in Batches

```cypher
CALL {
  MATCH (u:User)
  WHERE u.inactive = true
  WITH u LIMIT 1000
  DETACH DELETE u
  RETURN count(u) AS deleted
}
RETURN deleted
```

### Delete with Progress Tracking

```cypher
MATCH (n:OldData)
WITH count(n) AS total
MATCH (n:OldData)
WITH n, total
LIMIT 1000
DETACH DELETE n
RETURN count(n) AS deleted, total
```

## Delete Orphaned Nodes

Delete nodes with no relationships:

```cypher
MATCH (n:User)
WHERE NOT (n)-[]-()
DELETE n
```

## Delete Duplicate Relationships

```cypher
MATCH (a:User)-[r:KNOWS]->(b:User)
WITH a, b, collect(r) AS rels
WHERE size(rels) > 1
UNWIND rels[1..] AS duplicateRel
DELETE duplicateRel
```

## Common Patterns

### Soft Delete (Mark as Deleted)

Instead of deleting, mark as deleted:

```cypher
MATCH (u:User {name: 'Alice'})
SET u.deleted = true, u.deletedAt = datetime()
RETURN u
```

Query active users:

```cypher
MATCH (u:User)
WHERE u.deleted IS NULL OR u.deleted = false
RETURN u
```

### Archive Before Delete

```cypher
MATCH (u:User {name: 'Alice'})
CREATE (a:ArchivedUser)
SET a = properties(u)
SET a.archivedAt = datetime()
WITH u
DETACH DELETE u
```

### Delete Cascade

Delete user and all related content:

```cypher
MATCH (u:User {name: 'Alice'})
OPTIONAL MATCH (u)-[:POSTED]->(p:Post)
OPTIONAL MATCH (u)-[:COMMENTED]->(c:Comment)
DETACH DELETE u, p, c
```

### Cleanup After Delete

```cypher
// Delete user
MATCH (u:User {name: 'Alice'})
DETACH DELETE u

// Delete orphaned data
MATCH (p:Post)
WHERE NOT ()-[:POSTED]->(p)
DELETE p
```

## Performance Tips

1. **Delete in Batches for Large Datasets**
   ```cypher
   // Good: Batch delete
   MATCH (n:OldData)
   WITH n LIMIT 10000
   DETACH DELETE n
   
   // Bad: Delete all at once (can lock database)
   MATCH (n:OldData)
   DETACH DELETE n
   ```

2. **Use DETACH DELETE for Nodes**
   ```cypher
   // Good: One operation
   DETACH DELETE u
   
   // Bad: Two operations
   MATCH (u)-[r]-()
   DELETE r
   DELETE u
   ```

3. **Index Properties Used in DELETE Queries**
   ```cypher
   CREATE INDEX ON :User(lastLogin)
   
   // Fast delete with index
   MATCH (u:User)
   WHERE u.lastLogin < $cutoffDate
   DETACH DELETE u
   ```

4. **Consider Soft Delete for Audit Trail**
   ```cypher
   // Keeps history
   SET u.deleted = true
   
   // Loses history
   DELETE u
   ```

## Safety Tips

1. **Always Use WHERE Clause**
   ```cypher
   // Dangerous: Deletes everything
   MATCH (n)
   DETACH DELETE n
   
   // Safe: Specific condition
   MATCH (n:TempData)
   WHERE n.created < datetime() - duration('P1D')
   DETACH DELETE n
   ```

2. **Test with COUNT First**
   ```cypher
   // Check what will be deleted
   MATCH (u:User)
   WHERE u.inactive = true
   RETURN count(u)
   
   // Then delete
   MATCH (u:User)
   WHERE u.inactive = true
   DETACH DELETE u
   ```

3. **Use Transactions**
   ```python
   with graph.transaction() as tx:
       tx.execute("MATCH (u:User {name: $name}) DETACH DELETE u", 
                  {'name': 'Alice'})
       # Rolls back on error
   ```

4. **Backup Before Large Deletes**
   ```bash
   # Create backup
   curl -X POST http://localhost:7474/admin/snapshot
   
   # Then perform delete
   ```

## Error Handling

### Constraint Violations

```cypher
// Can't delete node with relationships without DETACH
MATCH (u:User {name: 'Alice'})
DELETE u
// Error: Cannot delete node with relationships

// Use DETACH DELETE
MATCH (u:User {name: 'Alice'})
DETACH DELETE u
// Success
```

## Next Steps

- Create data with [CREATE](create.md)
- Update data with [SET](set.md)
- Match patterns with [MATCH](match.md)
- Read [Cypher Introduction](introduction.md)
