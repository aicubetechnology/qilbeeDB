# WHERE

The WHERE clause is used to filter the results of MATCH patterns.

## Basic Filtering

### Property Comparison

```cypher
MATCH (u:User)
WHERE u.age > 25
RETURN u
```

### Multiple Conditions

```cypher
MATCH (u:User)
WHERE u.age > 25 AND u.city = 'New York'
RETURN u
```

### OR Conditions

```cypher
MATCH (u:User)
WHERE u.age < 18 OR u.age > 65
RETURN u
```

## Comparison Operators

```cypher
-- Equality
WHERE u.name = 'Alice'

-- Inequality
WHERE u.age <> 30

-- Numeric comparison
WHERE u.age > 25
WHERE u.age >= 18
WHERE u.age < 65
WHERE u.age <= 100

-- String comparison
WHERE u.name > 'A'  -- Alphabetical
```

## String Matching

### Exact Match

```cypher
WHERE u.name = 'Alice'
```

### Case-Insensitive

```cypher
WHERE toLower(u.name) = 'alice'
```

### Contains

```cypher
WHERE u.name CONTAINS 'Ali'
```

### Starts With

```cypher
WHERE u.name STARTS WITH 'Al'
```

### Ends With

```cypher
WHERE u.name ENDS WITH 'ce'
```

### Regular Expressions

```cypher
WHERE u.email =~ '.*@example\\.com'
```

## NULL Checks

### IS NULL

```cypher
WHERE u.phone IS NULL
```

### IS NOT NULL

```cypher
WHERE u.email IS NOT NULL
```

## List Operations

### IN Operator

```cypher
WHERE u.city IN ['New York', 'Los Angeles', 'Chicago']
```

### List Membership

```cypher
WHERE 'Admin' IN u.roles
```

## Range Checks

```cypher
-- Age between 25 and 35
WHERE u.age >= 25 AND u.age <= 35

-- Using IN for discrete values
WHERE u.age IN [25, 26, 27, 28, 29, 30]
```

## Pattern Filters

### Relationship Existence

```cypher
MATCH (u:User)
WHERE (u)-[:KNOWS]->(:User {name: 'Alice'})
RETURN u
```

### NOT Pattern

```cypher
MATCH (u:User)
WHERE NOT (u)-[:KNOWS]->(:User {name: 'Bob'})
RETURN u
```

### Path Existence

```cypher
MATCH (u:User), (target:User {name: 'Alice'})
WHERE (u)-[:KNOWS*1..3]->(target)
RETURN u
```

## Boolean Logic

### NOT

```cypher
WHERE NOT u.age > 65
WHERE NOT (u)-[:KNOWS]->(:User)
```

### Complex Expressions

```cypher
WHERE (u.age > 25 AND u.city = 'NYC') OR (u.age < 18 AND u.student = true)
```

## Property Existence

```cypher
-- Check if property exists
WHERE EXISTS(u.email)

-- Check if property doesn't exist
WHERE NOT EXISTS(u.phone)
```

## Using Parameters

```cypher
MATCH (u:User)
WHERE u.age > $minAge AND u.city = $city
RETURN u
```

## Label Checking

```cypher
MATCH (n)
WHERE n:User OR n:Admin
RETURN n
```

## Filtering Relationships

```cypher
MATCH (u:User)-[r:KNOWS]->(f:User)
WHERE r.since > '2020-01-01'
RETURN u, f
```

## Common Patterns

### Active Users

```cypher
MATCH (u:User)
WHERE u.lastLogin > datetime() - duration('P7D')
  AND u.active = true
RETURN u
```

### Users with Friends

```cypher
MATCH (u:User)
WHERE EXISTS((u)-[:KNOWS]->(:User))
RETURN u
```

### Exclude Self-Loops

```cypher
MATCH (a:User)-[:KNOWS]->(b:User)
WHERE a <> b
RETURN a, b
```

### Time Range

```cypher
MATCH (p:Post)
WHERE p.created >= datetime('2024-01-01')
  AND p.created < datetime('2024-02-01')
RETURN p
```

## Performance Tips

1. **Use Indexed Properties**
   ```cypher
   -- Fast with index on email
   WHERE u.email = 'alice@example.com'
   ```

2. **Most Selective Filters First**
   ```cypher
   -- Good: Most selective first
   WHERE u.email = 'alice@example.com' AND u.active = true
   
   -- Less optimal
   WHERE u.active = true AND u.email = 'alice@example.com'
   ```

3. **Avoid Property Functions in Filters**
   ```cypher
   -- Good: Direct comparison
   WHERE u.email = 'ALICE@EXAMPLE.COM'
   
   -- Bad: Function prevents index use
   WHERE toUpper(u.email) = 'ALICE@EXAMPLE.COM'
   ```

4. **Use Parameters**
   ```cypher
   -- Good: Query plan cached
   WHERE u.age > $minAge
   
   -- Bad: Reparse every time
   WHERE u.age > 25
   ```

## Next Steps

- Match patterns with [MATCH](match.md)
- Select output with [RETURN](return.md)
- Sort results with [ORDER BY](orderby.md)
- Read [Cypher Introduction](introduction.md)
