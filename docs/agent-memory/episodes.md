# Episodes

Episodes are the fundamental units of agent experience in QilbeeDB. An episode represents a discrete event or interaction that an agent experiences, observes, or performs.

## Episode Types

QilbeeDB supports three types of episodes, each designed for different aspects of agent operation:

### 1. Conversation Episodes

**Purpose**: Store interactive dialogues between agents and users or other agents.

**Structure**:
- Agent ID
- User input (what was said to the agent)
- Agent response (what the agent said back)
- Event timestamp
- Relevance score

**Use Cases**:
- Customer support interactions
- Chatbot conversations
- Multi-agent communication
- User feedback collection

**Example**:

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

db = QilbeeDB("http://localhost:7474")
memory = db.agent_memory('customer_support_bot')

# Store a conversation episode
episode = Episode.conversation(
    agent_id='customer_support_bot',
    user_input='How do I reset my password?',
    agent_response='You can reset your password by clicking the "Forgot Password" link on the login page. You\'ll receive an email with reset instructions.'
)

episode_id = memory.store_episode(episode)
print(f"Stored episode: {episode_id}")
```

### 2. Observation Episodes

**Purpose**: Record environmental observations, sensor readings, or system state changes.

**Structure**:
- Agent ID
- Observation description
- Event timestamp
- Relevance score

**Use Cases**:
- Environmental monitoring
- System metrics tracking
- Anomaly detection
- Pattern recognition

**Example**:

```python
# Store system observation
observation = Episode.observation(
    agent_id='monitoring_agent',
    observation='CPU utilization reached 95% for 5 consecutive minutes'
)

memory.store_episode(observation)

# Store multiple observations
observations = [
    'Database query latency increased to 800ms',
    'Disk I/O wait time at 45%',
    'Memory usage at 78%'
]

for obs in observations:
    memory.store_episode(Episode.observation('monitoring_agent', obs))
```

### 3. Action Episodes

**Purpose**: Record actions taken by the agent and their outcomes.

**Structure**:
- Agent ID
- Action description (what was done)
- Result description (what happened)
- Event timestamp
- Relevance score

**Use Cases**:
- Decision tracking
- Automation workflows
- Performance analysis
- Cause-effect learning

**Example**:

```python
# Store action and result
action = Episode.action(
    agent_id='automation_agent',
    action='Scaled application servers from 4 to 12 instances',
    result='Average response time decreased from 850ms to 120ms, CPU utilization normalized to 45%'
)

memory.store_episode(action)

# Track decision chain
decisions = [
    ('Detected traffic spike', 'Initiated auto-scaling procedure'),
    ('Scaled to 8 instances', 'Latency reduced to 300ms'),
    ('Scaled to 12 instances', 'Latency reduced to 120ms'),
    ('Traffic normalized', 'Scaled back to 6 instances')
]

for action, result in decisions:
    memory.store_episode(Episode.action('automation_agent', action, result))
```

## Episode Properties

Every episode in QilbeeDB has the following properties:

### Core Properties

```python
class Episode:
    agent_id: str          # Which agent owns this episode
    episode_type: str      # 'conversation', 'observation', or 'action'
    content: dict          # Type-specific content
    event_time: datetime   # When the event occurred
    transaction_time: datetime  # When it was recorded
    relevance: float       # Relevance score (0.0 to 1.0)
```

### Bi-Temporal Timestamps

**Event Time** - When the event actually occurred:
```python
episode.event_time  # datetime object
```

**Transaction Time** - When the episode was recorded:
```python
episode.transaction_time  # datetime object
```

This enables:
- Late-arriving data handling
- Time-travel queries
- Audit trails
- Temporal analysis

### Relevance Score

The relevance score determines:
- Retrieval priority (higher scores retrieved first)
- Consolidation eligibility (high scores consolidate to long-term memory)
- Forgetting threshold (low scores are pruned)

```python
# Manually set relevance
episode.relevance = 0.9  # High importance

# Or let the system calculate it
# Default relevance based on recency and access patterns
```

## Storing Episodes

### Basic Storage

```python
from qilbeedb import QilbeeDB
from qilbeedb.memory import Episode

db = QilbeeDB("http://localhost:7474")
memory = db.agent_memory('my_agent')

# Store conversation
conv_id = memory.store_episode(Episode.conversation(
    'my_agent',
    'What is the weather today?',
    'The weather is sunny with a high of 75°F.'
))

# Store observation
obs_id = memory.store_episode(Episode.observation(
    'my_agent',
    'Temperature sensor reading: 72°F, humidity: 45%'
))

# Store action
action_id = memory.store_episode(Episode.action(
    'my_agent',
    'Adjusted thermostat to 70°F',
    'Room temperature stabilized at 70°F after 15 minutes'
))
```

### Batch Storage

For multiple episodes:

```python
# Prepare episodes
episodes = [
    Episode.conversation('chat_bot', 'Hello', 'Hi! How can I help?'),
    Episode.conversation('chat_bot', 'What are your hours?', 'We\'re open 9-5 weekdays'),
    Episode.observation('chat_bot', 'User session started'),
]

# Store in batch
episode_ids = []
for episode in episodes:
    episode_id = memory.store_episode(episode)
    episode_ids.append(episode_id)

print(f"Stored {len(episode_ids)} episodes")
```

### Custom Event Time

By default, event time is set to current time. You can specify custom event time:

```python
from datetime import datetime, timedelta

# Event that occurred 1 hour ago
past_time = datetime.now() - timedelta(hours=1)

episode = Episode.conversation(
    'my_agent',
    'Previous question',
    'Previous answer'
)
episode.event_time = past_time

memory.store_episode(episode)
```

## Retrieving Episodes

### Get Recent Episodes

```python
# Get 10 most recent episodes
recent = memory.get_recent_episodes(10)

for episode in recent:
    print(f"Type: {episode.episode_type}")
    print(f"Time: {episode.event_time}")
    print(f"Content: {episode.content}")
    print(f"Relevance: {episode.relevance}")
    print("---")
```

### Filter by Type

```python
# Get only conversation episodes
conversations = [ep for ep in memory.get_recent_episodes(100)
                 if ep.episode_type == 'conversation']

# Get only observations
observations = [ep for ep in memory.get_recent_episodes(100)
                if ep.episode_type == 'observation']
```

### Filter by Time Range

```python
from datetime import datetime, timedelta

# Get episodes from last 24 hours
day_ago = datetime.now() - timedelta(days=1)
recent_episodes = [ep for ep in memory.get_recent_episodes(1000)
                   if ep.event_time >= day_ago]
```

### Filter by Relevance

```python
# Get high-relevance episodes only
important = [ep for ep in memory.get_recent_episodes(100)
             if ep.relevance >= 0.7]
```

## Working with Episode Content

### Conversation Content

```python
conv = Episode.conversation('agent', 'input', 'response')

# Access content
print(conv.content['user_input'])      # 'input'
print(conv.content['agent_response'])  # 'response'
```

### Observation Content

```python
obs = Episode.observation('agent', 'Something observed')

# Access content
print(obs.content['observation'])  # 'Something observed'
```

### Action Content

```python
action = Episode.action('agent', 'Action taken', 'Result achieved')

# Access content
print(action.content['action'])  # 'Action taken'
print(action.content['result'])  # 'Result achieved'
```

## Advanced Usage

### Contextual Episodes

Link episodes to graph entities:

```python
# Create a user node
user = graph.create_node(['User'], {'name': 'Alice', 'user_id': 'U123'})

# Store conversation with user context
episode = Episode.conversation(
    'support_bot',
    'I need help with order #456',
    'Let me check order #456 for you.'
)

# Store episode
episode_id = memory.store_episode(episode)

# Link episode to user in graph
episode_node = graph.get_node(episode_id)
graph.create_relationship(user, 'HAD_CONVERSATION', episode_node)
```

### Episode Chains

Track sequences of related episodes:

```python
# Problem-solving sequence
episodes = [
    Episode.observation('agent', 'User reports login issue'),
    Episode.action('agent', 'Checked user account status', 'Account is active'),
    Episode.action('agent', 'Checked recent login attempts', 'Found failed attempts from new IP'),
    Episode.action('agent', 'Sent verification email', 'User confirmed identity'),
    Episode.action('agent', 'Whitelisted new IP address', 'User can now log in'),
]

# Store and link episodes
prev_id = None
for episode in episodes:
    episode_id = memory.store_episode(episode)

    if prev_id:
        # Link to previous episode
        prev_node = graph.get_node(prev_id)
        curr_node = graph.get_node(episode_id)
        graph.create_relationship(prev_node, 'FOLLOWED_BY', curr_node)

    prev_id = episode_id
```

### Episode Analytics

```python
# Analyze conversation patterns
recent = memory.get_recent_episodes(1000)

conversations = [ep for ep in recent if ep.episode_type == 'conversation']
observations = [ep for ep in recent if ep.episode_type == 'observation']
actions = [ep for ep in recent if ep.episode_type == 'action']

print(f"Conversations: {len(conversations)}")
print(f"Observations: {len(observations)}")
print(f"Actions: {len(actions)}")

# Calculate average relevance by type
avg_conv_relevance = sum(ep.relevance for ep in conversations) / len(conversations)
avg_obs_relevance = sum(ep.relevance for ep in observations) / len(observations)
avg_action_relevance = sum(ep.relevance for ep in actions) / len(actions)

print(f"Avg conversation relevance: {avg_conv_relevance:.2f}")
print(f"Avg observation relevance: {avg_obs_relevance:.2f}")
print(f"Avg action relevance: {avg_action_relevance:.2f}")
```

## Best Practices

### 1. Choose the Right Episode Type

- **Conversation**: Use for any interactive exchange
- **Observation**: Use for passive information gathering
- **Action**: Use for agent decisions and their outcomes

### 2. Provide Meaningful Content

```python
# Good: Specific and informative
Episode.observation(
    'sensor_agent',
    'Temperature: 72°F, Humidity: 45%, Pressure: 1013 hPa at sensor_living_room'
)

# Bad: Too vague
Episode.observation('sensor_agent', 'Reading received')
```

### 3. Set Appropriate Relevance

```python
# Critical system event
critical = Episode.observation('monitor', 'CRITICAL: Database connection lost')
critical.relevance = 1.0  # Maximum importance

# Routine status check
routine = Episode.observation('monitor', 'Routine health check: OK')
routine.relevance = 0.3  # Lower importance
```

### 4. Use Batch Operations

```python
# Efficient: Batch processing
episodes = [create_episode(data) for data in batch_data]
for ep in episodes:
    memory.store_episode(ep)

# Less efficient: Individual operations
for data in batch_data:
    episode = create_episode(data)
    memory.store_episode(episode)
```

### 5. Link Related Episodes

Use graph relationships to connect related episodes for richer context:

```python
# Store primary episode
primary = memory.store_episode(Episode.observation('agent', 'Issue detected'))

# Store follow-up
followup = memory.store_episode(Episode.action('agent', 'Issue resolved', 'System stable'))

# Link them
primary_node = graph.get_node(primary)
followup_node = graph.get_node(followup)
graph.create_relationship(primary_node, 'RESOLVED_BY', followup_node)
```

## Next Steps

- Learn about [Memory Types](memory-types.md) and how episodes are organized
- Understand [Consolidation](consolidation.md) from short-term to long-term memory
- Configure [Forgetting](forgetting.md) policies for memory management
- Review [Statistics](statistics.md) for memory monitoring

## Related Documentation

- [Agent Memory Overview](overview.md)
- [Memory API Reference](../api/memory-api.md)
- [AI Agent Use Cases](../use-cases/ai-agents.md)
- [Bi-Temporal Model](../architecture/bi-temporal.md)
