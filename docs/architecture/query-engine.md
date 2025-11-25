# Query Engine

QilbeeDB's query engine implements OpenCypher with cost-based optimization and vectorized execution.

## Architecture

```
Cypher Query → Parser → Planner → Optimizer → Executor
```

## Query Processing

### 1. Parsing

Convert Cypher text into Abstract Syntax Tree (AST):

```cypher
MATCH (u:User {name: 'Alice'})-[:KNOWS]->(f)
RETURN f.name
```

### 2. Planning

Generate optimized execution plan with cost-based optimization:

```
Return[f.name]
  ↑
Expand[(u)-[:KNOWS]->(f)]
  ↑
IndexSeek[User.name = 'Alice']
```

### 3. Execution

Execute plan using physical operators:

- **NodeScan**: Scan all nodes with label
- **IndexSeek**: Use index for exact match
- **Filter**: Apply WHERE predicate
- **Expand**: Traverse relationships
- **Project**: Select output columns

## Optimization Techniques

### Index Selection

Automatically uses indexes when available:

```cypher
-- Create index
CREATE INDEX ON :User(email)

-- Fast lookup
MATCH (u:User {email: 'alice@example.com'}) RETURN u
```

### Predicate Pushdown

Move filters close to data source to reduce scanned data.

### Cardinality Estimation

Estimate result sizes for better planning.

### Join Reordering

Choose optimal join order based on estimated costs.

## Performance Tips

1. **Use Parameters**
   ```cypher
   MATCH (u:User) WHERE u.age > $age RETURN u
   ```

2. **Create Indexes**
   ```cypher
   CREATE INDEX ON :User(email)
   ```

3. **Limit Results**
   ```cypher
   MATCH (u:User) RETURN u LIMIT 100
   ```

4. **Use EXPLAIN**
   ```cypher
   EXPLAIN MATCH (u:User)-[:KNOWS*2..3]->(f) RETURN f
   ```

## Next Steps

- Understand [Storage Engine](storage.md)
- Learn about [Memory Engine](memory-engine.md)
- Explore [Cypher Language](../cypher/introduction.md)
