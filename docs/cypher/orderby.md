# ORDER BY

The ORDER BY clause sorts query results.

## Basic Sorting

### Ascending Order (Default)

```cypher
MATCH (u:User)
RETURN u.name, u.age
ORDER BY u.age
```

### Descending Order

```cypher
MATCH (u:User)
RETURN u.name, u.age
ORDER BY u.age DESC
```

### Explicit Ascending

```cypher
MATCH (u:User)
RETURN u.name, u.age
ORDER BY u.age ASC
```

## Multiple Sort Columns

```cypher
MATCH (u:User)
RETURN u.city, u.name, u.age
ORDER BY u.city, u.age DESC, u.name
```

This sorts by:
1. City (ascending)
2. Then age (descending) within each city
3. Then name (ascending) for same city and age

## Sort by Expression

### Computed Values

```cypher
MATCH (u:User)
RETURN u.firstName, u.lastName
ORDER BY u.firstName + ' ' + u.lastName
```

### Function Results

```cypher
MATCH (u:User)
RETURN u.name, u.email
ORDER BY toLower(u.email)
```

### Aggregations

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
RETURN u.name, count(p) AS posts
ORDER BY posts DESC
```

## Sort by Relationship Properties

```cypher
MATCH (a:User)-[r:KNOWS]->(b:User)
WHERE a.name = 'Alice'
RETURN b.name, r.since
ORDER BY r.since DESC
```

## Sort with Aliases

```cypher
MATCH (u:User)
RETURN u.name AS userName, u.age AS userAge
ORDER BY userAge DESC, userName
```

## Null Handling

NULL values sort to the end by default:

```cypher
MATCH (u:User)
RETURN u.name, u.age
ORDER BY u.age
// NULLs appear last
```

Descending order:

```cypher
MATCH (u:User)
RETURN u.name, u.age
ORDER BY u.age DESC
// NULLs appear first
```

## Sort by Multiple Properties

### Primary and Secondary Sort

```cypher
MATCH (p:Post)
RETURN p.title, p.likes, p.created
ORDER BY p.likes DESC, p.created DESC
```

### Mixed Ascending/Descending

```cypher
MATCH (u:User)
RETURN u.country, u.city, u.name
ORDER BY u.country ASC, u.city DESC, u.name ASC
```

## Sort with LIMIT

```cypher
MATCH (p:Post)
RETURN p.title, p.likes
ORDER BY p.likes DESC
LIMIT 10
```

## Sort Aggregated Results

### Sort by Count

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
RETURN u.name, count(p) AS postCount
ORDER BY postCount DESC
```

### Sort by Average

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
RETURN u.name, avg(p.likes) AS avgLikes
ORDER BY avgLikes DESC
```

### Sort by Multiple Aggregations

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
RETURN u.name, 
       count(p) AS posts,
       avg(p.likes) AS avgLikes
ORDER BY posts DESC, avgLikes DESC
```

## Case-Insensitive Sorting

```cypher
MATCH (u:User)
RETURN u.name
ORDER BY toLower(u.name)
```

## Sort by Date/Time

### Recent First

```cypher
MATCH (p:Post)
RETURN p.title, p.created
ORDER BY p.created DESC
```

### Oldest First

```cypher
MATCH (u:User)
RETURN u.name, u.joinedDate
ORDER BY u.joinedDate ASC
```

## Sort by String Length

```cypher
MATCH (p:Post)
RETURN p.title
ORDER BY size(p.title) DESC
```

## Sort by Array Size

```cypher
MATCH (u:User)
RETURN u.name, size(u.tags) AS tagCount
ORDER BY tagCount DESC
```

## Common Patterns

### Top N Items

```cypher
MATCH (p:Post)
RETURN p.title, p.views
ORDER BY p.views DESC
LIMIT 10
```

### Bottom N Items

```cypher
MATCH (p:Product)
RETURN p.name, p.sales
ORDER BY p.sales ASC
LIMIT 5
```

### Most Active Users

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
RETURN u.name, count(p) AS posts
ORDER BY posts DESC
LIMIT 20
```

### Recent Activity

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
RETURN u.name, p.title, p.created
ORDER BY p.created DESC
LIMIT 50
```

### Alphabetical List

```cypher
MATCH (u:User)
RETURN u.name
ORDER BY u.name ASC
```

### Leaderboard

```cypher
MATCH (u:User)
RETURN u.name, u.score
ORDER BY u.score DESC, u.name ASC
```

### Timeline

```cypher
MATCH (e:Event)
RETURN e.description, e.timestamp
ORDER BY e.timestamp ASC
```

## Performance Tips

1. **Create Indexes on Sorted Properties**
   ```cypher
   CREATE INDEX ON :User(age)
   
   // Fast sorting with index
   MATCH (u:User)
   RETURN u
   ORDER BY u.age
   ```

2. **Limit Before Sorting When Possible**
   ```cypher
   // If you only need top 10, use LIMIT
   ORDER BY u.age DESC
   LIMIT 10
   ```

3. **Sort Early in Query**
   ```cypher
   // Good: Sort before expensive operations
   MATCH (u:User)
   ORDER BY u.created DESC
   LIMIT 100
   WITH u
   // ... more operations
   
   // Bad: Sort after expensive operations
   MATCH (u:User)
   // ... expensive operations
   ORDER BY u.created DESC
   ```

4. **Avoid Sorting Large Result Sets**
   ```cypher
   // Good: Filter then sort
   MATCH (u:User)
   WHERE u.active = true
   RETURN u
   ORDER BY u.name
   
   // Bad: Sort then filter
   MATCH (u:User)
   ORDER BY u.name
   WHERE u.active = true
   RETURN u
   ```

## Sorting NULL Values

Control NULL placement:

```cypher
// NULLs last (default ascending)
ORDER BY u.age ASC

// NULLs first (default descending)  
ORDER BY u.age DESC

// Force NULLs to end
ORDER BY 
  CASE WHEN u.age IS NULL THEN 1 ELSE 0 END,
  u.age
```

## Random Order

```cypher
MATCH (u:User)
RETURN u
ORDER BY rand()
LIMIT 10
```

## Next Steps

- Limit results with [LIMIT](limit.md)
- Filter with [WHERE](where.md)
- Return data with [RETURN](return.md)
- Read [Cypher Introduction](introduction.md)
