# AI Agents

QilbeeDB is purpose-built for AI agent systems, providing native memory management capabilities that enable agents to maintain context, learn from interactions, and evolve over time.

## Why QilbeeDB for AI Agents?

Traditional databases treat all data uniformly, but AI agents need memory systems that work like human cognition:

- **Episodic Memory** - Remember specific interactions and experiences
- **Semantic Memory** - Build and maintain knowledge graphs
- **Procedural Memory** - Learn and optimize workflows
- **Temporal Context** - Understand when events occurred and how they relate

QilbeeDB provides all of this natively, eliminating the need to build custom memory management on top of traditional databases.

## Common AI Agent Patterns

### 1. Customer Support Bot

Track customer interactions, learn from conversations, and provide contextual support.

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

db = QilbeeDB("http://localhost:7474")
memory = db.agent_memory('support_bot')

# Store customer interaction
episode = Episode.conversation(
    'support_bot',
    user_input='My order #12345 hasn't arrived',
    agent_response='Let me check that for you. I see your order was shipped 3 days ago and should arrive tomorrow.'
)
memory.store_episode(episode)

# Retrieve conversation history
recent = memory.get_recent_episodes(10)
for ep in recent:
    print(f"User: {ep.content.get('user_input')}")
    print(f"Bot: {ep.content.get('agent_response')}")
```

### 2. Personal Assistant

Maintain user preferences, track tasks, and provide personalized recommendations.

```python
# Track user preferences in graph
graph = db.graph('user_preferences')

user = graph.create_node(['User'], {
    'user_id': 'U123',
    'name': 'Alice',
    'timezone': 'America/Los_Angeles'
})

preference = graph.create_node(['Preference'], {
    'category': 'communication',
    'value': 'email',
    'priority': 1
})

graph.create_relationship(user, 'PREFERS', preference)

# Store episodic interactions
memory = db.agent_memory('personal_assistant')
memory.store_episode(Episode.action(
    'personal_assistant',
    'Scheduled meeting for Alice at 2pm PST',
    'Meeting added to calendar, reminder set for 1:45pm'
))
```

### 3. Autonomous Agent

Monitor environment, make decisions, and track outcomes.

```python
# Observation-Decision-Action cycle
memory = db.agent_memory('autonomous_agent')

# Observe
observation = Episode.observation(
    'autonomous_agent',
    'Server CPU at 95%, response time degraded to 2s'
)
memory.store_episode(observation)

# Decide and Act
action = Episode.action(
    'autonomous_agent',
    action='Scaled from 4 to 8 servers',
    result='CPU normalized to 45%, response time improved to 200ms'
)
memory.store_episode(action)

# Learn from outcomes
stats = memory.get_statistics()
print(f"Total actions taken: {stats.total_episodes}")
```

### 4. Research Assistant

Build knowledge graphs and track research progress.

```python
# Knowledge graph
kg = db.graph('research_knowledge')

# Create paper nodes
paper1 = kg.create_node(['Paper'], {
    'title': 'Attention Is All You Need',
    'year': 2017,
    'authors': 'Vaswani et al.'
})

paper2 = kg.create_node(['Paper'], {
    'title': 'BERT: Pre-training of Deep Bidirectional Transformers',
    'year': 2018,
    'authors': 'Devlin et al.'
})

# Link related papers
kg.create_relationship(paper2, 'CITES', paper1)
kg.create_relationship(paper2, 'BUILDS_ON', paper1, {
    'concept': 'transformer architecture'
})

# Track research sessions
memory = db.agent_memory('research_assistant')
memory.store_episode(Episode.conversation(
    'research_assistant',
    'Find papers about transformer architectures',
    'Found 45 papers. The seminal work is "Attention Is All You Need" (2017).'
))
```

## Multi-Agent Systems

QilbeeDB excels at managing multiple agents with separate memory spaces:

```python
# Sales agent
sales_memory = db.agent_memory('sales_agent')
sales_memory.store_episode(Episode.conversation(
    'sales_agent',
    'Tell me about enterprise pricing',
    'Our enterprise plan starts at $999/month...'
))

# Support agent
support_memory = db.agent_memory('support_agent')
support_memory.store_episode(Episode.conversation(
    'support_agent',
    'How do I reset my password?',
    'Click Forgot Password on the login page...'
))

# Shared knowledge graph
shared_kg = db.graph('company_knowledge')
product = shared_kg.create_node(['Product'], {
    'name': 'Enterprise Plan',
    'price': 999,
    'features': ['24/7 support', 'Custom SLA', 'Dedicated account manager']
})
```

## Memory Consolidation

QilbeeDB automatically consolidates important memories:

```python
# High-value interactions consolidate to long-term memory
important_episode = Episode.conversation(
    'agent',
    'Critical system alert: Database connection lost',
    'Initiated failover to backup database, system restored in 30 seconds'
)
important_episode.relevance = 1.0  # Maximum importance
memory.store_episode(important_episode)

# Low-value interactions decay over time
routine = Episode.observation('agent', 'Health check: All systems normal')
routine.relevance = 0.3  # Lower importance
memory.store_episode(routine)
```

## Best Practices

### 1. Separate Concerns

Use different memory spaces for different agent capabilities:

```python
# Conversation memory
conversation_memory = db.agent_memory('agent_conversations')

# Decision memory
decision_memory = db.agent_memory('agent_decisions')

# Learning memory
learning_memory = db.agent_memory('agent_learning')
```

### 2. Track Context

Store rich context in episodes:

```python
episode = Episode.conversation(
    'support_bot',
    user_input='I need help',
    agent_response='What can I help you with?'
)

# Add context through graph relationships
user_node = graph.get_node(user_id)
episode_node = graph.get_node(episode_id)
graph.create_relationship(episode_node, 'INVOLVES', user_node)
```

### 3. Monitor Memory Health

```python
stats = memory.get_statistics()

if stats.total_episodes > 10000:
    # Trigger consolidation
    print("Memory consolidation recommended")

if stats.avg_relevance < 0.5:
    # Review forgetting policy
    print("Low average relevance - check forgetting thresholds")
```

### 4. Leverage Temporal Queries

```python
# Find recent high-priority interactions
recent_important = [
    ep for ep in memory.get_recent_episodes(100)
    if ep.relevance > 0.8
]

# Time-based analysis
from datetime import datetime, timedelta
day_ago = datetime.now() - timedelta(days=1)

recent_day = [
    ep for ep in memory.get_recent_episodes(1000)
    if ep.event_time >= day_ago
]
```

## Performance Considerations

### Memory Sizing

```python
# Configure memory parameters
# In config.toml:
# [memory]
# consolidation_interval_hours = 24
# relevance_decay_days = 30
# min_relevance_threshold = 0.1
```

### Query Optimization

```python
# Use limits for large result sets
recent = memory.get_recent_episodes(limit=20)

# Use graph queries for complex relationships
result = graph.query("""
    MATCH (agent:Agent)-[:HAS_EPISODE]->(ep:Episode)
    WHERE ep.relevance > $threshold
    RETURN ep
    ORDER BY ep.event_time DESC
    LIMIT 10
""", {"threshold": 0.7})
```

## Integration Patterns

### With LLMs

```python
from qilbeedb import QilbeeDB
import openai

db = QilbeeDB("http://localhost:7474")
memory = db.agent_memory('llm_agent')

def chat_with_context(user_input):
    # Get conversation history
    history = memory.get_recent_episodes(5)

    # Build context
    context = "\n".join([
        f"User: {ep.content.get('user_input')}\nAssistant: {ep.content.get('agent_response')}"
        for ep in history
    ])

    # Call LLM with context
    response = openai.ChatCompletion.create(
        model="gpt-4",
        messages=[
            {"role": "system", "content": f"Previous conversation:\n{context}"},
            {"role": "user", "content": user_input}
        ]
    )

    # Store interaction
    memory.store_episode(Episode.conversation(
        'llm_agent',
        user_input,
        response.choices[0].message.content
    ))

    return response.choices[0].message.content
```

### With Vector Databases

```python
# Use QilbeeDB for structured memory, vector DB for semantic search
from qilbeedb import QilbeeDB
import pinecone

db = QilbeeDB("http://localhost:7474")
memory = db.agent_memory('hybrid_agent')

# Store structured episode in QilbeeDB
episode = Episode.conversation('agent', user_input, response)
episode_id = memory.store_episode(episode)

# Store embedding in vector DB for semantic search
pinecone.upsert([(episode_id, embedding, {"text": user_input})])

# Later: semantic search in vector DB, retrieve full context from QilbeeDB
similar_ids = pinecone.query(query_embedding, top_k=5)
for ep_id in similar_ids:
    full_episode = memory.get_episode(ep_id)
```

## Next Steps

- Deep dive into [Agent Memory](../agent-memory/overview.md)
- Learn about [Episodes](../agent-memory/episodes.md)
- Understand [Memory Types](../agent-memory/memory-types.md)
- Explore [Multi-Agent Systems](multi-agent.md)
- Read the [Python SDK](../client-libraries/python.md)
