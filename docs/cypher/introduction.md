# Cypher Query Language

Cypher is a declarative graph query language that allows for expressive and efficient querying of graph data.

## Overview

QilbeeDB supports OpenCypher, the industry-standard query language for graph databases. Cypher uses ASCII-art syntax to make queries easy to read and write, using patterns to describe graph structures.

## Why Cypher?

- **Declarative**: Describe what you want, not how to get it
- **Pattern-based**: Use ASCII-art patterns to match graph structures
- **Expressive**: Complex queries are readable and concise
- **Standard**: OpenCypher is widely adopted across graph databases

## Graph Data Model

Cypher works with two fundamental elements:

### Nodes

Nodes represent entities in your graph:

```cypher
(u:User {name: 'Alice', age: 28})
```

- `u` - Variable name
- `User` - Label
- `{name: 'Alice', age: 28}` - Properties

### Relationships

Relationships connect nodes:

```cypher
(alice:User)-[:KNOWS {since: '2020-01-15'}]->(bob:User)
```

- `[:KNOWS]` - Relationship type
- `->` - Direction
- `{since: '2020-01-15'}` - Properties

## Basic Query Structure

A typical Cypher query follows this pattern:

```cypher
MATCH (pattern)
WHERE (conditions)
RETURN (results)
```

### Example Query

```cypher
MATCH (u:User)-[:KNOWS]->(friend:User)
WHERE u.name = 'Alice' AND friend.age > 25
RETURN friend.name, friend.age
ORDER BY friend.age DESC
LIMIT 10
```

This query:
1. **MATCH**: Finds users named Alice and their friends
2. **WHERE**: Filters friends older than 25
3. **RETURN**: Returns friend names and ages
4. **ORDER BY**: Sorts by age descending
5. **LIMIT**: Returns only top 10 results

## Common Clauses

### MATCH - Pattern Matching

Find nodes and relationships:

```cypher
MATCH (u:User)
MATCH (u:User)-[:KNOWS]->(f:User)
MATCH (u:User)-[:KNOWS*1..3]->(f)  -- Variable-length path
```

[Learn more about MATCH](match.md)

### WHERE - Filtering

Filter matched patterns:

```cypher
WHERE u.age > 25
WHERE u.name STARTS WITH 'A'
WHERE u.email =~ '.*@example.com'
```

[Learn more about WHERE](where.md)

### RETURN - Output

Select what to return:

```cypher
RETURN u.name, u.age
RETURN count(u) AS totalUsers
RETURN u.name, collect(f.name) AS friends
```

[Learn more about RETURN](return.md)

### CREATE - Insert Data

Create nodes and relationships:

```cypher
CREATE (u:User {name: 'Alice', age: 28})
CREATE (a)-[:KNOWS]->(b)
```

[Learn more about CREATE](create.md)

### SET - Update Data

Update properties:

```cypher
SET u.age = 29
SET u += {city: 'New York', updated: datetime()}
```

[Learn more about SET](set.md)

### DELETE - Remove Data

Delete nodes and relationships:

```cypher
DELETE r
DETACH DELETE u  -- Delete node and its relationships
```

[Learn more about DELETE](delete.md)

### ORDER BY - Sorting

Sort results:

```cypher
ORDER BY u.age DESC
ORDER BY u.city, u.name
```

[Learn more about ORDER BY](orderby.md)

### LIMIT - Restrict Results

Limit number of results:

```cypher
LIMIT 10
SKIP 20 LIMIT 10  -- Pagination
```

[Learn more about LIMIT](limit.md)

## Complete Example

Let's build a social network:

### Create Data

```cypher
-- Create users
CREATE (alice:User {name: 'Alice', age: 28, city: 'San Francisco'})
CREATE (bob:User {name: 'Bob', age: 32, city: 'New York'})
CREATE (charlie:User {name: 'Charlie', age: 25, city: 'San Francisco'})

-- Create friendships
CREATE (alice)-[:KNOWS {since: '2020-01-15'}]->(bob)
CREATE (alice)-[:KNOWS {since: '2021-03-20'}]->(charlie)
CREATE (bob)-[:KNOWS {since: '2020-06-10'}]->(charlie)
```

### Query Data

Find mutual friends:

```cypher
MATCH (a:User {name: 'Alice'})-[:KNOWS]->(mutual:User)<-[:KNOWS]-(b:User)
WHERE a <> b
RETURN DISTINCT b.name AS mutualFriend
```

Find friends in same city:

```cypher
MATCH (u:User {name: 'Alice'})-[:KNOWS]->(friend:User)
WHERE u.city = friend.city
RETURN friend.name, friend.city
```

Count friends by city:

```cypher
MATCH (u:User)-[:KNOWS]->(friend:User)
RETURN friend.city, count(*) AS friendsCount
ORDER BY friendsCount DESC
```

### Update Data

Update user information:

```cypher
MATCH (u:User {name: 'Alice'})
SET u.age = 29, u.updated = datetime()
RETURN u
```

### Delete Data

Remove a friendship:

```cypher
MATCH (a:User {name: 'Alice'})-[r:KNOWS]->(b:User {name: 'Bob'})
DELETE r
```

## Patterns

### Simple Pattern

```cypher
(a)-[:KNOWS]->(b)
```

### Variable-Length Pattern

```cypher
(a)-[:KNOWS*1..3]->(b)  -- 1 to 3 hops
```

### Multiple Patterns

```cypher
MATCH (a)-[:KNOWS]->(b)-[:WORKS_AT]->(c)
```

### Shortest Path

```cypher
MATCH path = shortestPath((a:User)-[:KNOWS*]-(b:User))
WHERE a.name = 'Alice' AND b.name = 'Charlie'
RETURN length(path)
```

## Working with Properties

### Property Access

```cypher
RETURN u.name, u.age
```

### Property Existence

```cypher
WHERE exists(u.email)
```

### Property Update

```cypher
SET u.age = u.age + 1
```

## Aggregations

Cypher supports powerful aggregations:

```cypher
-- Count
RETURN count(u) AS totalUsers

-- Sum
RETURN sum(p.price) AS totalValue

-- Average
RETURN avg(u.age) AS averageAge

-- Collect
RETURN u.name, collect(f.name) AS friends

-- Min/Max
RETURN min(u.age), max(u.age)
```

## Parameters

Use parameters for dynamic queries:

```cypher
MATCH (u:User)
WHERE u.age > $minAge AND u.city = $city
RETURN u
```

Python example:
```python
graph.query("""
    MATCH (u:User)
    WHERE u.age > $minAge
    RETURN u
""", {'minAge': 25})
```

## Best Practices

1. **Use Parameters**
   - Improves security (prevents injection)
   - Enables query plan caching
   - Makes queries reusable

2. **Create Indexes**
   ```cypher
   CREATE INDEX ON :User(email)
   ```

3. **Use EXPLAIN**
   ```cypher
   EXPLAIN MATCH (u:User) WHERE u.age > 25 RETURN u
   ```

4. **Limit Results**
   - Always use LIMIT for exploration
   - Prevents accidentally loading huge datasets

5. **Use Specific Patterns**
   ```cypher
   -- Good: Specific pattern
   MATCH (u:User {email: 'alice@example.com'})

   -- Bad: Broad scan
   MATCH (u)
   WHERE u.email = 'alice@example.com'
   ```

## Common Operations

### Create Node

```cypher
CREATE (u:User {name: 'Alice', age: 28})
RETURN u
```

### Find Node

```cypher
MATCH (u:User {name: 'Alice'})
RETURN u
```

### Update Node

```cypher
MATCH (u:User {name: 'Alice'})
SET u.age = 29
RETURN u
```

### Delete Node

```cypher
MATCH (u:User {name: 'Alice'})
DETACH DELETE u
```

### Create Relationship

```cypher
MATCH (a:User {name: 'Alice'}),
      (b:User {name: 'Bob'})
CREATE (a)-[:KNOWS]->(b)
```

### Find Relationships

```cypher
MATCH (a:User)-[r:KNOWS]->(b:User)
WHERE a.name = 'Alice'
RETURN a, r, b
```

## Functions

Cypher includes many built-in functions:

### String Functions
```cypher
toLower(s), toUpper(s), trim(s), substring(s, start, length)
```

### Numeric Functions
```cypher
abs(n), round(n), sqrt(n), rand()
```

### Aggregation Functions
```cypher
count(), sum(), avg(), min(), max(), collect()
```

### Date/Time Functions
```cypher
datetime(), date(), duration()
```

[See all functions](functions.md)

## Resources

- [MATCH Clause](match.md) - Pattern matching
- [WHERE Clause](where.md) - Filtering
- [RETURN Clause](return.md) - Output selection
- [CREATE Clause](create.md) - Data insertion
- [SET Clause](set.md) - Data updates
- [DELETE Clause](delete.md) - Data deletion
- [ORDER BY Clause](orderby.md) - Sorting
- [LIMIT Clause](limit.md) - Result limiting
- [Functions](functions.md) - Built-in functions

## Next Steps

1. Try the [Quick Start Guide](../getting-started/quickstart.md)
2. Learn about [Graph Operations](../graph-operations/nodes.md)
3. Explore the [Python SDK](../client-libraries/python.md)
4. Read about [Query Optimization](../architecture/query-engine.md)
