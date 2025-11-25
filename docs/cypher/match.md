# MATCH

The MATCH clause is used to search for patterns in the graph.

## Basic Node Match

Match all nodes with a label:

```cypher
MATCH (n:User)
RETURN n
```

Match nodes with specific properties:

```cypher
MATCH (u:User {name: 'Alice'})
RETURN u
```

## Multiple Labels

Match nodes with multiple labels:

```cypher
MATCH (p:Person:Employee)
RETURN p
```

## Relationship Patterns

### Outgoing Relationships

```cypher
MATCH (u:User)-[:KNOWS]->(f:User)
RETURN u, f
```

### Incoming Relationships

```cypher
MATCH (u:User)<-[:FOLLOWS]-(f:User)
RETURN u, f
```

### Undirected Relationships

```cypher
MATCH (u:User)-[:CONNECTED]-(f:User)
RETURN u, f
```

### Any Relationship Type

```cypher
MATCH (u:User)-[r]->(other)
RETURN u, type(r), other
```

## Variable-Length Paths

Match paths of variable length:

```cypher
-- Friends up to 3 hops away
MATCH (u:User {name: 'Alice'})-[:KNOWS*1..3]->(friend)
RETURN DISTINCT friend.name
```

Shortest path:

```cypher
MATCH path = shortestPath((a:User {name: 'Alice'})-[:KNOWS*]-(b:User {name: 'Bob'}))
RETURN path
```

## Multiple Patterns

Match multiple patterns:

```cypher
MATCH (a:User)-[:KNOWS]->(b:User),
      (b)-[:WORKS_AT]->(c:Company)
WHERE a.name = 'Alice'
RETURN a, b, c
```

## Optional Matching

Match patterns that may not exist:

```cypher
MATCH (u:User)
OPTIONAL MATCH (u)-[:KNOWS]->(friend)
RETURN u, friend
```

## Pattern Comprehension

```cypher
MATCH (u:User)
RETURN u.name, 
       [(u)-[:KNOWS]->(f) | f.name] as friends
```

## Common Patterns

### Triangle Pattern

```cypher
-- Find triangles (mutual connections)
MATCH (a:User)-[:KNOWS]->(b:User)-[:KNOWS]->(c:User)-[:KNOWS]->(a)
WHERE a.id < b.id AND b.id < c.id
RETURN a, b, c
```

### Star Pattern

```cypher
-- Find users with many connections
MATCH (center:User)-[:KNOWS]->(other:User)
WITH center, count(other) as connections
WHERE connections > 10
RETURN center, connections
ORDER BY connections DESC
```

### Chain Pattern

```cypher
-- Find recommendation chain
MATCH (u:User {name: 'Alice'})-[:KNOWS]->(f)-[:KNOWS]->(fof)
WHERE NOT (u)-[:KNOWS]->(fof)
RETURN DISTINCT fof.name
```

## Performance Tips

1. **Start with Most Selective Pattern**
   ```cypher
   -- Good: Start with specific node
   MATCH (u:User {email: 'alice@example.com'})-[:KNOWS]->(f)
   RETURN f
   
   -- Bad: Start with broad pattern
   MATCH (f:User)<-[:KNOWS]-(u:User {email: 'alice@example.com'})
   RETURN f
   ```

2. **Use Labels**
   ```cypher
   -- Good: Label narrows search
   MATCH (u:User)
   
   -- Bad: Scans all nodes
   MATCH (u)
   ```

3. **Limit Variable-Length Paths**
   ```cypher
   -- Good: Bounded search
   MATCH (u)-[:KNOWS*1..3]->(f)
   
   -- Bad: Unbounded search
   MATCH (u)-[:KNOWS*]->(f)
   ```

## Next Steps

- Add filters with [WHERE](where.md)
- Select output with [RETURN](return.md)
- Create data with [CREATE](create.md)
- Read [Cypher Introduction](introduction.md)
