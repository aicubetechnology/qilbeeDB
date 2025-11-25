# CREATE

The CREATE clause is used to create nodes and relationships in the graph.

## Create Nodes

### Basic Node Creation

```cypher
CREATE (n:User)
RETURN n
```

### Node with Properties

```cypher
CREATE (u:User {name: 'Alice', age: 28, email: 'alice@example.com'})
RETURN u
```

### Multiple Labels

```cypher
CREATE (p:Person:Employee {name: 'Bob', employeeId: '12345'})
RETURN p
```

### Multiple Nodes

```cypher
CREATE (u1:User {name: 'Alice'}),
       (u2:User {name: 'Bob'}),
       (u3:User {name: 'Charlie'})
RETURN u1, u2, u3
```

## Create Relationships

### Basic Relationship

```cypher
MATCH (a:User {name: 'Alice'}),
      (b:User {name: 'Bob'})
CREATE (a)-[:KNOWS]->(b)
RETURN a, b
```

### Relationship with Properties

```cypher
MATCH (a:User {name: 'Alice'}),
      (b:User {name: 'Bob'})
CREATE (a)-[:KNOWS {since: '2020-01-15', strength: 0.8}]->(b)
RETURN a, b
```

### Bidirectional Relationships

```cypher
MATCH (a:User {name: 'Alice'}),
      (b:User {name: 'Bob'})
CREATE (a)-[:KNOWS]->(b),
       (b)-[:KNOWS]->(a)
```

## Create Nodes and Relationships Together

### Create Path

```cypher
CREATE (a:User {name: 'Alice'})-[:KNOWS]->(b:User {name: 'Bob'})
RETURN a, b
```

### Create Complex Pattern

```cypher
CREATE (u:User {name: 'Alice'})-[:POSTED]->(p:Post {title: 'Hello World'}),
       (u)-[:LIVES_IN]->(c:City {name: 'New York'})
RETURN u, p, c
```

### Create Chain

```cypher
CREATE (a:User {name: 'Alice'})-[:KNOWS]->(b:User {name: 'Bob'})-[:KNOWS]->(c:User {name: 'Charlie'})
RETURN a, b, c
```

## Create with RETURN

Return created elements:

```cypher
CREATE (u:User {name: 'Alice', created: datetime()})
RETURN u.name, u.created, id(u) AS userId
```

## Create from Parameters

Use parameters for dynamic data:

```cypher
CREATE (u:User {
  name: $name,
  age: $age,
  email: $email
})
RETURN u
```

Python example:
```python
graph.query("""
    CREATE (u:User {name: $name, age: $age})
    RETURN u
""", {
    'name': 'Alice',
    'age': 28
})
```

## Bulk Creation

### Create Multiple Nodes with UNWIND

```cypher
UNWIND [
  {name: 'Alice', age: 28},
  {name: 'Bob', age: 32},
  {name: 'Charlie', age: 25}
] AS userData
CREATE (u:User {name: userData.name, age: userData.age})
RETURN count(u) AS created
```

### Batch Create from List

```cypher
WITH [
  ['Alice', 'Bob'],
  ['Bob', 'Charlie'],
  ['Alice', 'Charlie']
] AS friendships
UNWIND friendships AS friendship
MATCH (a:User {name: friendship[0]}),
      (b:User {name: friendship[1]})
CREATE (a)-[:KNOWS]->(b)
```

## CREATE vs MERGE

CREATE always creates new elements:

```cypher
-- Creates duplicate even if exists
CREATE (u:User {name: 'Alice'})
```

Use MERGE to avoid duplicates:

```cypher
-- Only creates if doesn't exist
MERGE (u:User {name: 'Alice'})
```

## Create with Match

Create based on existing data:

```cypher
MATCH (u:User)
WHERE u.age > 25
CREATE (u)-[:MEMBER_OF]->(g:Group {name: 'Adults'})
```

## Set Timestamps on Creation

```cypher
CREATE (u:User {
  name: 'Alice',
  created: datetime(),
  updated: datetime()
})
RETURN u
```

## Create Unique IDs

```cypher
CREATE (u:User {
  id: randomUUID(),
  name: 'Alice'
})
RETURN u
```

## Common Patterns

### Create User Profile

```cypher
CREATE (u:User {name: 'Alice'})-[:HAS_PROFILE]->(p:Profile {
  bio: 'Software Engineer',
  location: 'San Francisco',
  website: 'https://alice.dev'
})
RETURN u, p
```

### Create Social Connection

```cypher
MATCH (a:User {name: 'Alice'}),
      (b:User {name: 'Bob'})
CREATE (a)-[r:KNOWS {
  since: datetime(),
  type: 'friend',
  strength: 1.0
}]->(b)
RETURN a, r, b
```

### Create Hierarchical Data

```cypher
CREATE (company:Company {name: 'TechCorp'})
CREATE (dept1:Department {name: 'Engineering'})-[:PART_OF]->(company)
CREATE (dept2:Department {name: 'Sales'})-[:PART_OF]->(company)
CREATE (emp1:Employee {name: 'Alice'})-[:WORKS_IN]->(dept1)
CREATE (emp2:Employee {name: 'Bob'})-[:WORKS_IN]->(dept1)
RETURN company, dept1, dept2, emp1, emp2
```

### Create Time-Series Events

```cypher
UNWIND range(1, 10) AS i
CREATE (e:Event {
  id: i,
  timestamp: datetime() + duration({days: i}),
  type: 'measurement',
  value: rand() * 100
})
```

## Performance Tips

1. **Batch Creates**
   ```cypher
   -- Good: Single query with UNWIND
   UNWIND $users AS user
   CREATE (u:User {name: user.name, age: user.age})
   
   -- Bad: Multiple individual CREATE queries
   CREATE (u:User {name: 'Alice', age: 28})
   CREATE (u:User {name: 'Bob', age: 32})
   ```

2. **Use Transactions**
   ```python
   with graph.transaction() as tx:
       for user in users:
           tx.execute("CREATE (u:User {name: $name})", {'name': user})
   ```

3. **Set Indexes Before Bulk Loads**
   ```cypher
   CREATE INDEX ON :User(email)
   -- Then bulk create users
   ```

4. **Avoid Creating Duplicates**
   ```cypher
   -- Check first with MERGE
   MERGE (u:User {email: $email})
   ON CREATE SET u.name = $name, u.created = datetime()
   ON MATCH SET u.updated = datetime()
   ```

## Error Handling

### Avoid Constraint Violations

```cypher
-- Create constraint
CREATE CONSTRAINT ON (u:User) ASSERT u.email IS UNIQUE

-- This will fail if email exists
CREATE (u:User {email: 'alice@example.com'})

-- Use MERGE to handle existing data
MERGE (u:User {email: 'alice@example.com'})
ON CREATE SET u.name = 'Alice', u.created = datetime()
```

## Next Steps

- Update data with [SET](set.md)
- Delete data with [DELETE](delete.md)
- Match patterns with [MATCH](match.md)
- Read [Cypher Introduction](introduction.md)
