# RETURN

The RETURN clause specifies what data to return from a query.

## Basic Returns

### Return Nodes

```cypher
MATCH (u:User)
RETURN u
```

### Return Properties

```cypher
MATCH (u:User)
RETURN u.name, u.age
```

### Return All Properties

```cypher
MATCH (u:User)
RETURN u.*
```

## Aliases

Use aliases for clarity:

```cypher
MATCH (u:User)
RETURN u.name AS userName, u.age AS userAge
```

## Expressions

### Arithmetic

```cypher
MATCH (u:User)
RETURN u.name, u.age, u.age + 10 AS ageInTenYears
```

### String Operations

```cypher
MATCH (u:User)
RETURN u.firstName + ' ' + u.lastName AS fullName
```

### Conditional Expressions

```cypher
MATCH (u:User)
RETURN u.name,
       CASE
         WHEN u.age < 18 THEN 'Minor'
         WHEN u.age < 65 THEN 'Adult'
         ELSE 'Senior'
       END AS ageGroup
```

## Aggregations

### COUNT

```cypher
MATCH (u:User)
RETURN count(u) AS totalUsers
```

### SUM, AVG, MIN, MAX

```cypher
MATCH (u:User)
RETURN avg(u.age) AS avgAge,
       min(u.age) AS minAge,
       max(u.age) AS maxAge
```

### COLLECT

```cypher
MATCH (u:User)-[:KNOWS]->(f:User)
RETURN u.name, collect(f.name) AS friends
```

### COUNT DISTINCT

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
RETURN count(DISTINCT u) AS activeUsers
```

## Grouping

Aggregations automatically group by non-aggregated fields:

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
RETURN u.name, count(p) AS postCount
ORDER BY postCount DESC
```

Multiple grouping fields:

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
RETURN u.city, u.country, count(p) AS posts
```

## DISTINCT

Return unique results:

```cypher
MATCH (u:User)-[:KNOWS]->(f:User)-[:KNOWS]->(fof:User)
RETURN DISTINCT fof.name
```

## Relationships

### Return Relationships

```cypher
MATCH (u:User)-[r:KNOWS]->(f:User)
RETURN u, r, f
```

### Relationship Properties

```cypher
MATCH (u:User)-[r:KNOWS]->(f:User)
RETURN u.name, f.name, r.since, r.strength
```

### Relationship Type

```cypher
MATCH (u:User)-[r]->(other)
RETURN u.name, type(r), other.name
```

## Paths

### Return Paths

```cypher
MATCH path = (u:User {name: 'Alice'})-[:KNOWS*1..3]->(f)
RETURN path
```

### Path Length

```cypher
MATCH path = (u:User {name: 'Alice'})-[:KNOWS*]->(f:User {name: 'Bob'})
RETURN length(path) AS pathLength
```

### Nodes in Path

```cypher
MATCH path = (u:User)-[:KNOWS*1..3]->(f)
RETURN nodes(path) AS pathNodes
```

## Computed Values

### Counts and Sizes

```cypher
MATCH (u:User)
RETURN u.name, 
       size((u)-[:KNOWS]->()) AS friendCount
```

### String Functions

```cypher
MATCH (u:User)
RETURN toLower(u.name) AS lowerName,
       toUpper(u.email) AS upperEmail,
       substring(u.name, 0, 1) AS initial
```

### Date/Time Functions

```cypher
MATCH (p:Post)
RETURN p.title,
       p.created,
       datetime() - p.created AS age
```

## Limiting Results

Combine with LIMIT:

```cypher
MATCH (u:User)
RETURN u.name, u.age
ORDER BY u.age DESC
LIMIT 10
```

## Maps and Objects

### Return as Map

```cypher
MATCH (u:User)
RETURN {
  name: u.name,
  age: u.age,
  email: u.email
} AS userInfo
```

### Collect Maps

```cypher
MATCH (u:User)-[:KNOWS]->(f:User)
RETURN u.name, 
       collect({name: f.name, age: f.age}) AS friends
```

## Pattern Comprehensions

```cypher
MATCH (u:User)
RETURN u.name,
       [(u)-[:KNOWS]->(f) | f.name] AS friendNames,
       [(u)-[:POSTED]->(p) | {title: p.title, date: p.created}] AS posts
```

## Conditional Returns

### CASE Expression

```cypher
MATCH (u:User)
RETURN u.name,
       CASE u.subscription
         WHEN 'premium' THEN 'Premium User'
         WHEN 'basic' THEN 'Basic User'
         ELSE 'Free User'
       END AS userType
```

### CASE with Conditions

```cypher
MATCH (u:User)
RETURN u.name,
       CASE
         WHEN u.age < 18 THEN 'Youth discount'
         WHEN u.age >= 65 THEN 'Senior discount'
         ELSE 'Regular price'
       END AS pricing
```

## Complex Aggregations

### Multiple Aggregations

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
RETURN u.name,
       count(p) AS posts,
       avg(p.likes) AS avgLikes,
       max(p.created) AS lastPost
```

### Nested Aggregations

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
WITH u, count(p) AS postCount
WHERE postCount > 10
RETURN u.name, postCount
ORDER BY postCount DESC
```

## Performance Tips

1. **Return Only What You Need**
   ```cypher
   -- Good: Return specific properties
   RETURN u.name, u.age
   
   -- Bad: Return entire nodes when not needed
   RETURN u
   ```

2. **Use DISTINCT Wisely**
   ```cypher
   -- DISTINCT can be expensive on large result sets
   RETURN DISTINCT u.city
   ```

3. **Limit Early**
   ```cypher
   -- Good: Filter before aggregation
   MATCH (u:User)
   WHERE u.active = true
   RETURN count(u)
   ```

4. **Project Early with WITH**
   ```cypher
   -- Good: Reduce data early
   MATCH (u:User)-[:POSTED]->(p:Post)
   WITH u, count(p) AS posts
   WHERE posts > 10
   RETURN u.name, posts
   ```

## Common Patterns

### Top N per Group

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
WITH u, p
ORDER BY p.created DESC
RETURN u.name, collect(p.title)[0..5] AS recentPosts
```

### Pivot Data

```cypher
MATCH (u:User)-[:LIVES_IN]->(c:City)
RETURN c.name AS city,
       count(u) AS population
ORDER BY population DESC
```

### Running Totals

```cypher
MATCH (p:Post)
WITH p
ORDER BY p.created
RETURN p.title,
       p.created,
       sum(p.views) OVER (ORDER BY p.created) AS cumulativeViews
```

## Next Steps

- Match patterns with [MATCH](match.md)
- Filter results with [WHERE](where.md)
- Sort results with [ORDER BY](orderby.md)
- Limit results with [LIMIT](limit.md)
- Read [Cypher Introduction](introduction.md)
