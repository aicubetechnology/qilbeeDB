# Cypher Functions

QilbeeDB supports a wide range of built-in functions for data manipulation and analysis.

## String Functions

### toLower / toUpper

```cypher
RETURN toLower("HELLO") AS lower  // "hello"
RETURN toUpper("hello") AS upper  // "HELLO"
```

### trim / lTrim / rTrim

```cypher
RETURN trim("  hello  ") AS trimmed      // "hello"
RETURN lTrim("  hello  ") AS leftTrim    // "hello  "
RETURN rTrim("  hello  ") AS rightTrim   // "  hello"
```

### substring

```cypher
RETURN substring("hello world", 0, 5) AS sub  // "hello"
RETURN substring("hello world", 6) AS sub     // "world"
```

### replace

```cypher
RETURN replace("hello world", "world", "there") AS result
// "hello there"
```

### split

```cypher
RETURN split("a,b,c", ",") AS parts  // ["a", "b", "c"]
```

### size (String Length)

```cypher
RETURN size("hello") AS length  // 5
```

### String Concatenation

```cypher
RETURN "hello" + " " + "world" AS greeting  // "hello world"
```

## Numeric Functions

### abs

```cypher
RETURN abs(-5) AS absolute  // 5
```

### round / floor / ceil

```cypher
RETURN round(3.7) AS rounded   // 4
RETURN floor(3.7) AS floored   // 3
RETURN ceil(3.2) AS ceiled     // 4
```

### sqrt / power

```cypher
RETURN sqrt(16) AS root        // 4
RETURN power(2, 8) AS result   // 256
```

### sign

```cypher
RETURN sign(-5) AS s   // -1
RETURN sign(0) AS s    // 0
RETURN sign(5) AS s    // 1
```

### rand

```cypher
RETURN rand() AS random  // Random float between 0 and 1
```

## Aggregation Functions

### count

```cypher
MATCH (u:User)
RETURN count(u) AS totalUsers

// Count distinct
MATCH (u:User)
RETURN count(DISTINCT u.city) AS cities
```

### sum

```cypher
MATCH (p:Product)
RETURN sum(p.price) AS totalValue
```

### avg

```cypher
MATCH (u:User)
RETURN avg(u.age) AS averageAge
```

### min / max

```cypher
MATCH (u:User)
RETURN min(u.age) AS youngest, max(u.age) AS oldest
```

### collect

```cypher
MATCH (u:User)-[:KNOWS]->(f:User)
RETURN u.name, collect(f.name) AS friends
```

### percentile

```cypher
MATCH (u:User)
RETURN percentileDisc(u.age, 0.5) AS medianAge
RETURN percentileCont(u.age, 0.95) AS p95Age
```

## List Functions

### size (List Length)

```cypher
RETURN size([1, 2, 3]) AS length  // 3
```

### head / last

```cypher
RETURN head([1, 2, 3]) AS first  // 1
RETURN last([1, 2, 3]) AS last   // 3
```

### tail

```cypher
RETURN tail([1, 2, 3]) AS rest  // [2, 3]
```

### range

```cypher
RETURN range(1, 10) AS numbers      // [1, 2, 3, ..., 10]
RETURN range(1, 10, 2) AS evens     // [1, 3, 5, 7, 9]
```

### reverse

```cypher
RETURN reverse([1, 2, 3]) AS reversed  // [3, 2, 1]
```

### List Comprehension

```cypher
MATCH (u:User)-[:KNOWS]->(f)
RETURN [friend IN collect(f) | friend.name] AS friendNames
```

## Date/Time Functions

### datetime

```cypher
RETURN datetime() AS now
RETURN datetime("2024-01-15") AS date
```

### date

```cypher
RETURN date() AS today
RETURN date("2024-01-15") AS specificDate
```

### time

```cypher
RETURN time() AS currentTime
RETURN time("14:30:00") AS specificTime
```

### duration

```cypher
RETURN duration("P1Y2M3D") AS period      // 1 year, 2 months, 3 days
RETURN duration("PT1H30M") AS timespan    // 1 hour, 30 minutes
```

### Date Arithmetic

```cypher
RETURN datetime() + duration("P7D") AS nextWeek
RETURN datetime() - duration("P1M") AS lastMonth
```

### Extract Components

```cypher
WITH datetime() AS now
RETURN now.year, now.month, now.day,
       now.hour, now.minute, now.second
```

## Type Conversion Functions

### toInteger

```cypher
RETURN toInteger("42") AS num      // 42
RETURN toInteger(42.7) AS num      // 42
```

### toFloat

```cypher
RETURN toFloat("3.14") AS num      // 3.14
RETURN toFloat(42) AS num          // 42.0
```

### toString

```cypher
RETURN toString(42) AS str         // "42"
RETURN toString(3.14) AS str       // "3.14"
```

### toBoolean

```cypher
RETURN toBoolean("true") AS bool   // true
RETURN toBoolean("false") AS bool  // false
```

## Predicate Functions

### exists

```cypher
MATCH (u:User)
WHERE exists(u.email)
RETURN u
```

### coalesce

```cypher
MATCH (u:User)
RETURN coalesce(u.nickname, u.name, "Anonymous") AS displayName
```

### CASE

```cypher
MATCH (u:User)
RETURN u.name,
  CASE
    WHEN u.age < 18 THEN "Minor"
    WHEN u.age < 65 THEN "Adult"
    ELSE "Senior"
  END AS ageGroup
```

## Path Functions

### length (Path Length)

```cypher
MATCH path = (a:User)-[:KNOWS*]-(b:User)
RETURN length(path) AS hops
```

### nodes

```cypher
MATCH path = (a)-[:KNOWS*1..3]-(b)
RETURN nodes(path) AS pathNodes
```

### relationships

```cypher
MATCH path = (a)-[:KNOWS*1..3]-(b)
RETURN relationships(path) AS pathRels
```

### shortestPath

```cypher
MATCH path = shortestPath((a:User {name: 'Alice'})-[:KNOWS*]-(b:User {name: 'Bob'}))
RETURN length(path) AS distance
```

## Node/Relationship Functions

### id

```cypher
MATCH (u:User)
RETURN id(u) AS nodeId
```

### labels

```cypher
MATCH (n)
RETURN labels(n) AS nodeLabels
```

### type (Relationship Type)

```cypher
MATCH ()-[r]->()
RETURN type(r) AS relType
```

### properties

```cypher
MATCH (u:User {name: 'Alice'})
RETURN properties(u) AS allProps
```

### keys

```cypher
MATCH (u:User {name: 'Alice'})
RETURN keys(u) AS propertyNames
```

## Spatial Functions (if supported)

### point

```cypher
RETURN point({latitude: 37.7749, longitude: -122.4194}) AS location
```

### distance

```cypher
WITH point({latitude: 37.7749, longitude: -122.4194}) AS sf,
     point({latitude: 40.7128, longitude: -74.0060}) AS nyc
RETURN distance(sf, nyc) AS distanceMeters
```

## User-Defined Functions

### randomUUID

```cypher
CREATE (u:User {id: randomUUID(), name: 'Alice'})
RETURN u
```

## Common Function Combinations

### Full Name

```cypher
MATCH (u:User)
RETURN u.firstName + ' ' + u.lastName AS fullName
```

### Email Normalization

```cypher
MATCH (u:User)
RETURN toLower(trim(u.email)) AS normalizedEmail
```

### Age from Birthday

```cypher
MATCH (u:User)
RETURN u.name,
  duration.between(u.birthDate, date()).years AS age
```

### Default Values

```cypher
MATCH (u:User)
RETURN u.name,
  coalesce(u.phone, "N/A") AS phone
```

### Time Ago

```cypher
MATCH (p:Post)
WITH p, duration.between(p.created, datetime()) AS elapsed
RETURN p.title,
  CASE
    WHEN elapsed.days = 0 THEN "Today"
    WHEN elapsed.days = 1 THEN "Yesterday"
    WHEN elapsed.days < 7 THEN elapsed.days + " days ago"
    WHEN elapsed.days < 30 THEN (elapsed.days / 7) + " weeks ago"
    ELSE (elapsed.days / 30) + " months ago"
  END AS timeAgo
```

### Search Score

```cypher
MATCH (p:Post)
WHERE toLower(p.title) CONTAINS toLower($query)
   OR toLower(p.content) CONTAINS toLower($query)
RETURN p,
  (CASE WHEN toLower(p.title) CONTAINS toLower($query) THEN 10 ELSE 0 END +
   CASE WHEN toLower(p.content) CONTAINS toLower($query) THEN 5 ELSE 0 END +
   p.likes * 0.1) AS score
ORDER BY score DESC
```

## Performance Tips

1. **Use Functions on Indexed Properties Carefully**
   ```cypher
   // Bad: Function prevents index use
   WHERE toLower(u.email) = 'alice@example.com'
   
   // Good: Direct comparison
   WHERE u.email = 'alice@example.com'
   ```

2. **Aggregate Early**
   ```cypher
   // Good: Aggregate before expensive operations
   MATCH (u:User)-[:POSTED]->(p:Post)
   WITH u, count(p) AS posts
   WHERE posts > 10
   RETURN u
   ```

3. **Use coalesce for Defaults**
   ```cypher
   SET u.credits = coalesce(u.credits, 0) + 10
   ```

## Next Steps

- Learn [MATCH](match.md) clause
- Explore [WHERE](where.md) filtering
- Use [RETURN](return.md) clause
- Read [Cypher Introduction](introduction.md)
