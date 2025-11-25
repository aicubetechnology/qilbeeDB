# Bolt Protocol

QilbeeDB supports the Bolt protocol for binary client-server communication, compatible with Neo4j drivers.

## Connection

```python
from neo4j import GraphDatabase

driver = GraphDatabase.driver(
    "bolt://localhost:7687",
    auth=("username", "password")
)

with driver.session() as session:
    result = session.run("MATCH (u:User) RETURN u.name")
    for record in result:
        print(record["u.name"])
```

## Features

- Binary protocol for efficient communication
- Connection pooling
- Transaction support
- Streaming results
- Compatible with Neo4j drivers

## Supported Drivers

- **Python**: neo4j-driver
- **JavaScript**: neo4j-driver
- **Java**: neo4j-java-driver
- **Go**: neo4j-go-driver
- **.NET**: neo4j-dotnet-driver

## Example Usage

### Python

```python
from neo4j import GraphDatabase

driver = GraphDatabase.driver("bolt://localhost:7687")

with driver.session() as session:
    # Create node
    result = session.run(
        "CREATE (u:User {name: $name, age: $age}) RETURN u",
        name="Alice", age=28
    )
    
    # Query
    result = session.run(
        "MATCH (u:User) WHERE u.age > $min_age RETURN u.name",
        min_age=25
    )
    
    for record in result:
        print(record["u.name"])
```

### JavaScript

```javascript
const neo4j = require('neo4j-driver');

const driver = neo4j.driver(
  'bolt://localhost:7687',
  neo4j.auth.basic('username', 'password')
);

const session = driver.session();

session
  .run('MATCH (u:User) WHERE u.age > $minAge RETURN u.name', { minAge: 25 })
  .then(result => {
    result.records.forEach(record => {
      console.log(record.get('u.name'));
    });
  })
  .finally(() => session.close());
```

## Configuration

```toml
[bolt]
enabled = true
address = "0.0.0.0:7687"
max_connections = 1000
```

## Next Steps

- Learn about [HTTP API](http-api.md)
- Explore [Graph API](graph-api.md)
- Use the [Python SDK](../client-libraries/python.md)
