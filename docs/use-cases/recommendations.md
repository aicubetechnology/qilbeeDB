# Recommendation Systems

Graph databases excel at recommendation engines by leveraging relationship patterns and collaborative filtering. QilbeeDB provides fast traversals for real-time recommendations.

## Collaborative Filtering

### Track User Behavior

```python
from qilbeedb import QilbeeDB

db = QilbeeDB("http://localhost:7474")
rec = db.graph("recommendations")

# Users and products
user = rec.create_node(['Customer'], {'user_id': 'U001', 'name': 'Alice'})
product = rec.create_node(['Product'], {'product_id': 'P123', 'name': 'Laptop'})

# Interactions
rec.create_relationship(user, 'PURCHASED', product, {
    'date': '2024-01-15',
    'rating': 5,
    'price': 1299.99
})
```

### Generate Recommendations

```python
# Find products purchased by similar users
results = rec.query("""
    MATCH (u:Customer {user_id: $user_id})-[:PURCHASED]->(p:Product)
          <-[:PURCHASED]-(similar:Customer)-[:PURCHASED]->(rec:Product)
    WHERE NOT (u)-[:PURCHASED]->(rec)
    RETURN rec.name, rec.product_id, COUNT(similar) as score
    ORDER BY score DESC
    LIMIT 10
""", {"user_id": "U001"})
```

### Content-Based Filtering

```python
# Find similar products by category
results = rec.query("""
    MATCH (p:Product {product_id: $product_id})-[:IN_CATEGORY]->(cat:Category)
          <-[:IN_CATEGORY]-(similar:Product)
    WHERE p.product_id <> similar.product_id
    RETURN similar.name, similar.product_id
    LIMIT 5
""", {"product_id": "P123"})
```

## Next Steps

- Learn [Graph Operations](../graph-operations/nodes.md)
- Master [Cypher Queries](../cypher/introduction.md)
- Use the [Python SDK](../client-libraries/python.md)
