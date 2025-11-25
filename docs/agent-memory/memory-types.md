# Memory Types

QilbeeDB implements four distinct memory types inspired by human cognitive architecture: episodic, semantic, procedural, and factual memory. Each type serves a different purpose and is optimized for specific use cases.

## Overview

```
┌──────────────────────────────────────────────┐
│  Memory Types in QilbeeDB                     │
├──────────────────────────────────────────────┤
│  Episodic    │ Specific experiences & events │
│  Semantic    │ General facts & concepts      │
│  Procedural  │ Skills & how-to knowledge     │
│  Factual     │ Static facts & reference data │
└──────────────────────────────────────────────┘
```

## 1. Episodic Memory

**Definition**: Memory of specific experiences and events with temporal and contextual information.

**Characteristics**:
- Time-stamped events
- Rich contextual details
- Personal to the agent
- Subject to decay and forgetting

**Storage Model**: Episode nodes with temporal relationships

**Use Cases**:
- Conversation history
- User interaction tracking
- Decision audit trails
- Learning from experience

**Example**:

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

db = QilbeeDB("http://localhost:7474")
memory = db.agent_memory('customer_bot')

# Store conversation episodes (episodic memory)
memory.store_episode(Episode.conversation(
    'customer_bot',
    'I need help with my order #12345',
    'Let me look that up. Your order shipped yesterday.'
))

# Retrieve recent episodes
recent_interactions = memory.get_recent_episodes(20)
```

## 2. Semantic Memory

**Definition**: General knowledge and concepts independent of personal experience.

**Characteristics**:
- Context-free facts
- Conceptual relationships
- Shared knowledge
- Persistent and stable

**Storage Model**: Concept nodes with semantic relationships (IS_A, HAS_PROPERTY, RELATED_TO)

**Use Cases**:
- Knowledge graphs
- Domain models
- Ontologies
- Concept hierarchies

**Example**:

```python
# Create semantic knowledge
graph = db.graph('knowledge_base')

# Concepts
python = graph.create_node(
    ['Concept', 'ProgrammingLanguage'],
    {'name': 'Python', 'paradigm': 'multi-paradigm'}
)

web_dev = graph.create_node(
    ['Concept', 'Domain'],
    {'name': 'Web Development'}
)

# Semantic relationships
graph.create_relationship(
    python,
    'USED_FOR',
    web_dev,
    {'popularity': 0.9}
)

# Query semantic knowledge
results = graph.query("""
    MATCH (lang:ProgrammingLanguage)-[:USED_FOR]->(domain:Domain)
    WHERE domain.name = 'Web Development'
    RETURN lang.name
""")
```

## 3. Procedural Memory

**Definition**: Knowledge of how to perform tasks and execute procedures.

**Characteristics**:
- Step-by-step instructions
- Temporal ordering
- Often implicit (hard to verbalize)
- Improved through practice

**Storage Model**: Action sequence nodes with ordered relationships (THEN, BEFORE, AFTER)

**Use Cases**:
- Workflow automation
- Process documentation
- Task planning
- Skill acquisition

**Example**:

```python
# Define a procedure for password reset
graph = db.graph('procedures')

# Steps
step1 = graph.create_node(
    ['Step', 'Procedure'],
    {
        'procedure': 'password_reset',
        'step_number': 1,
        'action': 'Verify user email',
        'description': 'Send verification code to registered email'
    }
)

step2 = graph.create_node(
    ['Step', 'Procedure'],
    {
        'procedure': 'password_reset',
        'step_number': 2,
        'action': 'Validate code',
        'description': 'Check if code matches and is not expired'
    }
)

step3 = graph.create_node(
    ['Step', 'Procedure'],
    {
        'procedure': 'password_reset',
        'step_number': 3,
        'action': 'Update password',
        'description': 'Hash and store new password securely'
    }
)

# Temporal ordering
graph.create_relationship(step1, 'THEN', step2, {'order': 1})
graph.create_relationship(step2, 'THEN', step3, {'order': 2})

# Execute procedure by querying steps
procedure_steps = graph.query("""
    MATCH path = (start:Step {procedure: 'password_reset', step_number: 1})
                 -[:THEN*]->(end:Step)
    RETURN nodes(path) as steps
""")
```

## 4. Factual Memory

**Definition**: Static, objective facts and reference data that rarely change.

**Characteristics**:
- Unchanging truths
- High reliability
- No temporal context needed
- Maximum relevance (never forgotten)

**Storage Model**: Fact nodes with high relevance scores and no decay

**Use Cases**:
- Configuration data
- Reference tables
- Constants and definitions
- Business rules

**Example**:

```python
# Store factual information
facts_graph = db.graph('facts')

# Company information (factual)
facts_graph.create_node(
    ['Fact', 'CompanyInfo'],
    {
        'key': 'support_email',
        'value': 'support@example.com',
        'category': 'contact'
    }
)

facts_graph.create_node(
    ['Fact', 'CompanyInfo'],
    {
        'key': 'business_hours',
        'value': 'Monday-Friday 9:00-17:00 EST',
        'category': 'schedule'
    }
)

facts_graph.create_node(
    ['Fact', 'Policy'],
    {
        'key': 'return_period',
        'value': '30 days',
        'category': 'returns'
    }
)

# Query facts
contact_info = facts_graph.query("""
    MATCH (f:Fact)
    WHERE f.category = 'contact'
    RETURN f.key, f.value
""")
```

## Memory Type Comparison

| Aspect | Episodic | Semantic | Procedural | Factual |
|--------|----------|----------|------------|---------|
| **Content** | Specific events | General knowledge | How-to steps | Static facts |
| **Context** | Rich temporal context | Context-free | Sequential context | No context |
| **Decay** | Yes, based on relevance | Slow decay | No decay | No decay |
| **Retrieval** | By time & relevance | By concept | By procedure name | By key |
| **Volatility** | High | Medium | Low | Very low |
| **Example** | "User called at 2pm" | "Python is a language" | "How to reset password" | "Support: help@co.com" |

## Choosing the Right Memory Type

### Use Episodic Memory When:
- Recording agent interactions
- Tracking user sessions
- Building conversation history
- Creating audit trails
- Learning from experiences

### Use Semantic Memory When:
- Building knowledge graphs
- Defining domain models
- Creating ontologies
- Representing concepts and relationships
- Sharing knowledge between agents

### Use Procedural Memory When:
- Automating workflows
- Documenting processes
- Planning multi-step tasks
- Teaching agents new skills
- Optimizing repeated operations

### Use Factual Memory When:
- Storing configuration
- Maintaining reference data
- Defining business rules
- Recording constants
- Preserving critical information

## Hybrid Approaches

Often, the most effective agent memory systems combine multiple memory types:

### Example: Customer Support Bot

```python
db = QilbeeDB("http://localhost:7474")

# Episodic: Track customer conversations
support_memory = db.agent_memory('support_bot')
support_memory.store_episode(Episode.conversation(
    'support_bot',
    'How do I return an item?',
    'You can return items within 30 days...'
))

# Semantic: Knowledge about products
kb_graph = db.graph('knowledge_base')
product = kb_graph.create_node(
    ['Product', 'Concept'],
    {'name': 'Laptop Pro', 'category': 'Electronics'}
)

# Procedural: Return process
proc_graph = db.graph('procedures')
# ... define return steps with THEN relationships

# Factual: Company policies
facts = db.graph('facts')
facts.create_node(
    ['Fact', 'Policy'],
    {'key': 'return_period', 'value': '30 days'}
)
```

## Memory Type Integration

### Cross-Memory Queries

Query across memory types for comprehensive context:

```python
# Find all information about password reset

# Episodic: Recent password reset requests
recent_requests = support_memory.get_recent_episodes(100)
password_requests = [ep for ep in recent_requests
                     if 'password' in str(ep.content).lower()]

# Semantic: Password security concepts
security_concepts = kb_graph.query("""
    MATCH (c:Concept)
    WHERE c.name CONTAINS 'password'
    RETURN c
""")

# Procedural: Password reset steps
reset_procedure = proc_graph.query("""
    MATCH (s:Step {procedure: 'password_reset'})
    RETURN s ORDER BY s.step_number
""")

# Factual: Password policy
password_policy = facts.query("""
    MATCH (f:Fact)
    WHERE f.category = 'security' AND f.key CONTAINS 'password'
    RETURN f
""")
```

## Best Practices

### 1. Separate Concerns

Keep memory types in separate graphs or clearly labeled:

```python
# Good: Clear separation
episodic_memory = db.agent_memory('agent')
semantic_graph = db.graph('knowledge')
procedures = db.graph('workflows')
facts = db.graph('reference_data')

# Bad: Mixed in one graph
mixed = db.graph('everything')  # Hard to manage
```

### 2. Set Appropriate Decay

Different memory types should have different decay rates:

```python
# Episodic: Natural decay
# (automatic based on relevance)

# Semantic: Slow decay
semantic_node.set('relevance', 0.8)

# Procedural: No decay
procedural_node.set('relevance', 1.0)

# Factual: Never forget
factual_node.set('relevance', 1.0)
factual_node.set('protected', True)
```

### 3. Link Related Memories

Create relationships across memory types:

```python
# Link episodic experience to semantic concept
episode_node = graph.get_node(episode_id)
concept_node = kb_graph.get_node(concept_id)

graph.create_relationship(
    episode_node,
    'RELATES_TO',
    concept_node
)
```

### 4. Regular Maintenance

Different memory types need different maintenance:

```python
# Episodic: Regular consolidation and forgetting
# (automatic by QilbeeDB)

# Semantic: Periodic review and updates
# Update concepts as domain evolves

# Procedural: Version control
# Keep procedure history

# Factual: Validation and verification
# Ensure facts remain current
```

## Next Steps

- Learn about [Memory Consolidation](consolidation.md)
- Configure [Forgetting Policies](forgetting.md)
- Monitor [Memory Statistics](statistics.md)
- Review [AI Agent Use Cases](../use-cases/ai-agents.md)

## Related Documentation

- [Episodes](episodes.md) - Episode structure and usage
- [Agent Memory Overview](overview.md) - High-level architecture
- [Bi-Temporal Model](../architecture/bi-temporal.md) - Temporal tracking details
