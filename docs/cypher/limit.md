# LIMIT

The LIMIT clause restricts the number of rows returned by a query.

## Basic Limit

```cypher
MATCH (u:User)
RETURN u.name
LIMIT 10
```

## Limit with ORDER BY

Get top N results:

```cypher
MATCH (p:Post)
RETURN p.title, p.views
ORDER BY p.views DESC
LIMIT 10
```

## Limit with Parameters

```cypher
MATCH (u:User)
RETURN u
LIMIT $maxResults
```

Python example:
```python
graph.query("""
    MATCH (u:User)
    RETURN u
    LIMIT $limit
""", {'limit': 10})
```

## SKIP and LIMIT (Pagination)

### Basic Pagination

```cypher
// Page 1 (skip 0, take 10)
MATCH (u:User)
RETURN u
ORDER BY u.name
SKIP 0
LIMIT 10

// Page 2 (skip 10, take 10)
MATCH (u:User)
RETURN u
ORDER BY u.name
SKIP 10
LIMIT 10

// Page 3 (skip 20, take 10)
MATCH (u:User)
RETURN u
ORDER BY u.name
SKIP 20
LIMIT 10
```

### Parameterized Pagination

```cypher
MATCH (u:User)
RETURN u
ORDER BY u.created DESC
SKIP $skip
LIMIT $limit
```

Python example:
```python
def get_users_page(page, page_size):
    skip = (page - 1) * page_size
    return graph.query("""
        MATCH (u:User)
        RETURN u
        ORDER BY u.created DESC
        SKIP $skip
        LIMIT $limit
    """, {'skip': skip, 'limit': page_size})

# Get page 3 with 20 items per page
users = get_users_page(page=3, page_size=20)
```

## Limit per Group

### Top N per Category

```cypher
MATCH (p:Post)-[:IN_CATEGORY]->(c:Category)
WITH c, p
ORDER BY p.likes DESC
RETURN c.name, collect(p.title)[0..5] AS topPosts
```

### Latest N per User

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
WITH u, p
ORDER BY p.created DESC
RETURN u.name, collect(p.title)[0..10] AS recentPosts
```

## Limit with Aggregations

```cypher
MATCH (u:User)-[:POSTED]->(p:Post)
RETURN u.name, count(p) AS posts
ORDER BY posts DESC
LIMIT 20
```

## Random Sample

Get random subset:

```cypher
MATCH (u:User)
RETURN u
ORDER BY rand()
LIMIT 100
```

## Limit Early for Performance

```cypher
// Good: Limit early
MATCH (u:User)
WHERE u.active = true
RETURN u
ORDER BY u.created DESC
LIMIT 100

// Then do expensive operations on limited set
WITH u
MATCH (u)-[:POSTED]->(p:Post)
RETURN u, count(p)
```

## Common Patterns

### Top 10 Most Popular

```cypher
MATCH (p:Post)
RETURN p.title, p.likes
ORDER BY p.likes DESC
LIMIT 10
```

### Latest 20 Posts

```cypher
MATCH (p:Post)
RETURN p.title, p.created
ORDER BY p.created DESC
LIMIT 20
```

### First 50 Users

```cypher
MATCH (u:User)
RETURN u.name
ORDER BY u.created ASC
LIMIT 50
```

### Sample 100 Random Items

```cypher
MATCH (n:Product)
RETURN n
ORDER BY rand()
LIMIT 100
```

### Top 5 per Category

```cypher
MATCH (p:Product)-[:IN_CATEGORY]->(c:Category)
WITH c, p
ORDER BY p.sales DESC
WITH c, collect(p)[0..5] AS topProducts
RETURN c.name, topProducts
```

## Infinite Scroll Implementation

```python
def get_feed(last_seen_id=None, limit=20):
    if last_seen_id:
        # Get posts after last seen
        query = """
            MATCH (p:Post)
            WHERE id(p) < $last_id
            RETURN p
            ORDER BY id(p) DESC
            LIMIT $limit
        """
        params = {'last_id': last_seen_id, 'limit': limit}
    else:
        # Get initial posts
        query = """
            MATCH (p:Post)
            RETURN p
            ORDER BY id(p) DESC
            LIMIT $limit
        """
        params = {'limit': limit}
    
    return graph.query(query, params)
```

## Cursor-Based Pagination

More efficient than SKIP for large offsets:

```cypher
// First page
MATCH (p:Post)
RETURN p
ORDER BY p.created DESC, id(p) DESC
LIMIT 20

// Next page (using last item as cursor)
MATCH (p:Post)
WHERE p.created < $lastCreated 
   OR (p.created = $lastCreated AND id(p) < $lastId)
RETURN p
ORDER BY p.created DESC, id(p) DESC
LIMIT 20
```

## LIMIT with DISTINCT

```cypher
MATCH (u:User)-[:KNOWS]-(friend)-[:KNOWS]-(fof)
RETURN DISTINCT fof.name
LIMIT 50
```

## Limit Subquery Results

```cypher
MATCH (u:User)
RETURN u.name,
  [(u)-[:POSTED]->(p:Post) | p.title][0..5] AS recentPosts
```

## Performance Tips

1. **Always Use with ORDER BY**
   ```cypher
   // Deterministic results
   ORDER BY u.created DESC
   LIMIT 10
   
   // Non-deterministic (random 10)
   LIMIT 10
   ```

2. **Use Cursor Pagination for Large Datasets**
   ```cypher
   // Good: Cursor-based
   WHERE p.created < $cursor
   LIMIT 20
   
   // Bad: SKIP for large offsets
   SKIP 100000
   LIMIT 20
   ```

3. **Limit Early in Query**
   ```cypher
   // Good: Limit before expensive operations
   MATCH (u:User)
   ORDER BY u.created DESC
   LIMIT 100
   WITH u
   MATCH (u)-[:POSTED]->(p)
   RETURN u, count(p)
   ```

4. **Use Indexes for ORDER BY + LIMIT**
   ```cypher
   CREATE INDEX ON :Post(created)
   
   // Fast with index
   MATCH (p:Post)
   ORDER BY p.created DESC
   LIMIT 10
   ```

## Count Total with Pagination

```cypher
MATCH (u:User)
WITH count(u) AS total, collect(u) AS users
RETURN total, 
       users[0..10] AS page
```

Or separate queries:
```python
# Get total count
total = graph.query("MATCH (u:User) RETURN count(u) AS total")[0]['total']

# Get page
page = graph.query("""
    MATCH (u:User)
    RETURN u
    SKIP $skip LIMIT $limit
""", {'skip': 0, 'limit': 10})

print(f"Showing 10 of {total} users")
```

## Next Steps

- Sort results with [ORDER BY](orderby.md)
- Filter with [WHERE](where.md)
- Return data with [RETURN](return.md)
- Read [Cypher Introduction](introduction.md)
