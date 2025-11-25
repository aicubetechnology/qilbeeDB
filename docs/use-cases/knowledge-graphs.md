# Knowledge Graphs

Knowledge graphs represent interconnected concepts, entities, and their semantic relationships. QilbeeDB's multi-label nodes and flexible schema make it ideal for knowledge representation.

## Building Knowledge Graphs

### Create Concepts

```python
from qilbeedb import QilbeeDB

db = QilbeeDB("http://localhost:7474")
kg = db.graph("knowledge_base")

# Programming language concept
python = kg.create_node(['Concept', 'ProgrammingLanguage'], {
    'name': 'Python',
    'paradigm': 'multi-paradigm',
    'year': 1991,
    'creator': 'Guido van Rossum'
})

# Domain concept  
web_dev = kg.create_node(['Concept', 'Domain'], {
    'name': 'Web Development',
    'category': 'software engineering'
})

# Semantic relationship
kg.create_relationship(python, 'USED_FOR', web_dev, {
    'popularity': 0.95,
    'since': 2000
})
```

### Ontologies and Taxonomies

```python
# Create hierarchy
programming = kg.create_node(['Category'], {'name': 'Programming'})
languages = kg.create_node(['Category'], {'name': 'Programming Languages'})
python_lang = kg.create_node(['Language'], {'name': 'Python'})

kg.create_relationship(languages, 'SUBCATEGORY_OF', programming)
kg.create_relationship(python_lang, 'INSTANCE_OF', languages)
```

### Query Knowledge

```python
# Find all uses of a technology
results = kg.query("""
    MATCH (tech:ProgrammingLanguage)-[:USED_FOR]->(domain:Domain)
    WHERE tech.name = $tech_name
    RETURN domain.name, domain.category
""", {"tech_name": "Python"})
```

### Semantic Search

```python
# Find related concepts
results = kg.query("""
    MATCH (concept:Concept)-[r]-(related:Concept)
    WHERE concept.name = $name
    RETURN related.name, type(r) as relationship_type
""", {"name": "Python"})
```

## Next Steps

- Explore [Graph Operations](../graph-operations/nodes.md)  
- Learn [Cypher](../cypher/introduction.md)
- Use the [Python SDK](../client-libraries/python.md)
