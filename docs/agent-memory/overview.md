# Agent Memory Overview

QilbeeDB's native agent memory system is what sets it apart from traditional graph databases. It provides sophisticated memory management capabilities specifically designed for AI agents, enabling them to maintain context, learn from interactions, and evolve over time.

## What is Agent Memory?

Agent memory in QilbeeDB is a bi-temporal memory management system that allows AI agents to:

- **Store episodic experiences** - Conversations, observations, and actions
- **Track temporal context** - When events occurred and when they were recorded
- **Automatically consolidate** - Move from short-term to long-term memory
- **Intelligently forget** - Prune low-relevance memories based on decay
- **Query historical context** - Retrieve relevant past experiences

## Why Agent Memory Matters

Traditional databases treat all data equally, but AI agents need memory that works like human memory:

1. **Recent events are more accessible** - Short-term memory is quickly retrievable
2. **Important memories persist** - High-relevance experiences consolidate to long-term memory
3. **Irrelevant details fade** - Low-relevance memories decay and are forgotten
4. **Context matters** - Temporal relationships help agents understand cause and effect

## Memory Architecture

QilbeeDB implements a sophisticated memory architecture with four distinct layers:

```
┌─────────────────────────────────────────┐
│     Application Layer                    │
│   (AI Agent / Application Code)          │
└─────────────────────────────────────────┘
           ↓ Episodes
┌─────────────────────────────────────────┐
│     Memory API                           │
│   Store, Retrieve, Consolidate, Forget   │
└─────────────────────────────────────────┘
           ↓
┌─────────────────────────────────────────┐
│     Memory Types                         │
│   Episodic | Semantic | Procedural       │
└─────────────────────────────────────────┘
           ↓
┌─────────────────────────────────────────┐
│     Bi-Temporal Storage                  │
│   Event Time + Transaction Time          │
└─────────────────────────────────────────┘
           ↓
┌─────────────────────────────────────────┐
│     Graph Storage                        │
│   Nodes, Relationships, Properties       │
└─────────────────────────────────────────┘
```

## Core Concepts

### Episodes

An **episode** is a discrete unit of agent experience. QilbeeDB supports three types of episodes:

1. **Conversation Episodes** - User interactions and agent responses
   ```python
   Episode.conversation(
       agent_id='customer_bot',
       user_input='How do I reset my password?',
       agent_response='Click the "Forgot Password" link...'
   )
   ```

2. **Observation Episodes** - Environmental observations and perceptions
   ```python
   Episode.observation(
       agent_id='analytics_bot',
       observation='Detected traffic spike: 300% increase'
   )
   ```

3. **Action Episodes** - Agent actions and their outcomes
   ```python
   Episode.action(
       agent_id='automation_bot',
       action='Scaled servers from 2 to 8 instances',
       result='Latency reduced from 800ms to 120ms'
   )
   ```

### Bi-Temporal Tracking

Every episode is tracked with **two timestamps**:

- **Event Time** - When the event actually occurred in the real world
- **Transaction Time** - When the event was recorded in the database

This enables:
- **Time-travel queries** - "What did the agent know at time T?"
- **Audit trails** - Complete history of all changes
- **Late-arriving data** - Handle events recorded out of order
- **Temporal analysis** - Understand how agent knowledge evolved

### Relevance Scoring

Each episode has a **relevance score** (0.0 to 1.0) that determines:

- How likely it is to be retrieved
- When it should be consolidated to long-term memory
- When it should be forgotten

Relevance scores naturally decay over time unless reinforced by:
- Repeated access (frequently retrieved memories stay relevant)
- Explicit boosting (mark important memories)
- Contextual connections (related to other high-relevance memories)

### Memory Consolidation

Similar to human memory, QilbeeDB automatically consolidates memories:

```
┌─────────────────┐
│  Short-Term     │  Recent, high-access memories
│  Memory         │  Fast retrieval, volatile
└────────┬────────┘
         │ Consolidation
         ↓ (relevance threshold)
┌─────────────────┐
│  Long-Term      │  Consolidated, important memories
│  Memory         │  Persistent, indexed
└─────────────────┘
```

Consolidation happens based on:
- **Time elapsed** - Configurable consolidation interval
- **Access patterns** - Frequently accessed memories consolidate faster
- **Relevance threshold** - Only memories above threshold are consolidated

### Active Forgetting

To prevent memory bloat, QilbeeDB implements **active forgetting**:

- Memories with relevance below threshold are pruned
- Configurable decay rate controls forgetting speed
- Important memories can be protected from forgetting
- Forgetting frees resources for new experiences

## Memory Types

QilbeeDB supports four types of agent memory:

### 1. Episodic Memory

**What**: Specific experiences and events
**Example**: "User Alice asked about password reset on Nov 24"
**Storage**: Individual episode nodes with temporal relationships

```python
memory.store_episode(Episode.conversation(
    'support_bot',
    'I forgot my password',
    'Let me help you reset it'
))
```

### 2. Semantic Memory

**What**: General facts and knowledge
**Example**: "Password reset emails expire after 24 hours"
**Storage**: Concept nodes with semantic relationships

```python
# Stored as graph relationships
graph.create_relationship(
    password_reset_concept,
    'HAS_PROPERTY',
    expiration_concept,
    {'duration': '24 hours'}
)
```

### 3. Procedural Memory

**What**: How to perform tasks
**Example**: "To reset password: verify email → send token → update password"
**Storage**: Action sequences with temporal ordering

```python
# Stored as linked action nodes
graph.create_relationship(
    verify_email_step,
    'THEN',
    send_token_step,
    {'order': 1}
)
```

### 4. Factual Memory

**What**: Static facts and reference data
**Example**: "Support email is support@example.com"
**Storage**: Property nodes with high-relevance scores

```python
graph.create_node(
    ['Fact', 'ContactInfo'],
    {'type': 'support_email', 'value': 'support@example.com'}
)
```

## Quick Start

### Basic Usage

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

# Connect and get memory manager
db = QilbeeDB("http://localhost:7474")
memory = db.agent_memory('my_agent')

# Store episode
episode = Episode.conversation(
    'my_agent',
    'What is QilbeeDB?',
    'QilbeeDB is a graph database with agent memory...'
)
memory.store_episode(episode)

# Retrieve recent episodes
recent = memory.get_recent_episodes(10)
for ep in recent:
    print(f"{ep.event_time}: {ep.content}")

# Get statistics
stats = memory.get_statistics()
print(f"Total episodes: {stats.total_episodes}")
print(f"Average relevance: {stats.avg_relevance}")
```

## Use Cases

### Customer Support Bot

Track customer interactions and learn from support patterns:

```python
memory = db.agent_memory('support_bot')

# Store each interaction
for interaction in customer_interactions:
    ep = Episode.conversation(
        'support_bot',
        interaction.user_message,
        interaction.bot_response
    )
    memory.store_episode(ep)

# Retrieve similar past cases
similar_cases = memory.get_recent_episodes(limit=5)
```

### Autonomous Agent

Track observations, decisions, and outcomes:

```python
memory = db.agent_memory('autonomous_agent')

# Observe environment
memory.store_episode(Episode.observation(
    'autonomous_agent',
    'Battery level at 20%'
))

# Take action
memory.store_episode(Episode.action(
    'autonomous_agent',
    'Initiated charging routine',
    'Docked at charging station'
))
```

### Multi-Agent System

Each agent maintains its own memory space:

```python
# Sales agent memory
sales_memory = db.agent_memory('sales_agent')
sales_memory.store_episode(Episode.conversation(
    'sales_agent',
    'Interested in enterprise plan',
    'Let me show you our features...'
))

# Support agent memory
support_memory = db.agent_memory('support_agent')
support_memory.store_episode(Episode.conversation(
    'support_agent',
    'How do I upgrade?',
    'You can upgrade from your dashboard...'
))
```

## Benefits

### For AI Applications

- **Context Awareness** - Agents remember past interactions
- **Learning Over Time** - Knowledge accumulates and consolidates
- **Efficient Memory Use** - Automatic forgetting prevents bloat
- **Temporal Reasoning** - Understand cause and effect relationships

### For Developers

- **Simple API** - Easy to integrate into existing applications
- **Automatic Management** - Consolidation and forgetting handled automatically
- **Flexible Storage** - Store any type of agent experience
- **Rich Queries** - Powerful temporal and graph queries

### For Operations

- **Auditability** - Complete history with bi-temporal tracking
- **Scalability** - Efficient storage with automatic pruning
- **Observability** - Built-in statistics and metrics
- **Reliability** - ACID transactions and WAL support
- **Durability** - RocksDB-backed persistence with automatic recovery

## Next Steps

- Learn about [Episodes](episodes.md) in detail
- Understand [Memory Types](memory-types.md)
- Configure [Persistence](persistence.md) and storage
- Explore [Consolidation](consolidation.md) mechanisms
- Configure [Forgetting](forgetting.md) policies
- Review [Statistics](statistics.md) and monitoring

## Further Reading

- [Bi-Temporal Model](../architecture/bi-temporal.md) - Deep dive into temporal tracking
- [Memory Engine](../architecture/memory-engine.md) - Architecture details
- [AI Agent Use Cases](../use-cases/ai-agents.md) - Real-world examples
- [Memory API Reference](../api/memory-api.md) - Complete API documentation
