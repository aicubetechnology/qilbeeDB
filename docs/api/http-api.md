# HTTP REST API

QilbeeDB provides a JSON-based HTTP REST API for all database operations.

## Base URL

```
http://localhost:7474
```

## Health Check

```bash
GET /health
```

Response:
```json
{
  "status": "healthy",
  "version": "0.1.0"
}
```

## List Graphs

```bash
GET /graphs
```

Response:
```json
{
  "graphs": ["social_network", "knowledge_base"]
}
```

## Execute Query

```bash
POST /graphs/{graph_name}/query
Content-Type: application/json

{
  "cypher": "MATCH (n:User) WHERE n.age > $min_age RETURN n",
  "parameters": {
    "min_age": 25
  }
}
```

Response:
```json
{
  "results": [
    {"n.name": "Alice", "n.age": 28},
    {"n.age": 32, "n.name": "Bob"}
  ],
  "stats": {
    "nodesScanned": 150,
    "executionTimeMs": 12
  }
}
```

## Create Node

```bash
POST /graphs/{graph_name}/nodes
Content-Type: application/json

{
  "labels": ["User", "Person"],
  "properties": {
    "name": "Alice",
    "age": 28
  }
}
```

## Create Relationship

```bash
POST /graphs/{graph_name}/relationships
Content-Type: application/json

{
  "startNode": 123,
  "type": "KNOWS",
  "endNode": 456,
  "properties": {
    "since": "2023-01-15"
  }
}
```

## Authentication

```bash
# Basic Auth
curl -u username:password http://localhost:7474/graphs

# Token Auth
curl -H "Authorization: Bearer <token>" http://localhost:7474/graphs
```

## Next Steps

- Learn about [Bolt Protocol](bolt-protocol.md)
- Explore [Graph API](graph-api.md)
- Use the [Python SDK](../client-libraries/python.md)
