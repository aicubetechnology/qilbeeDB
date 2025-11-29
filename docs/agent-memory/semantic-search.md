# Semantic Search

QilbeeDB's agent memory system includes powerful semantic search capabilities that enable AI agents to find relevant memories based on meaning, not just keywords. This document covers how to use semantic search, hybrid search, and similarity search features.

## Overview

Traditional keyword search looks for exact matches. Semantic search uses vector embeddings to find content that is conceptually similar, even when the exact words differ.

```
Keyword Search:   "machine learning" → finds "machine learning basics"
                                     → misses "AI training techniques"

Semantic Search:  "machine learning" → finds "machine learning basics"
                                     → finds "AI training techniques"
                                     → finds "neural network concepts"
```

## Features

### Semantic Search

Find episodes by conceptual similarity using vector embeddings:

```python
from qilbeedb import QilbeeDB

db = QilbeeDB("http://localhost:7474")
db.login("admin", "password")
memory = db.agent_memory("my-agent")

# Find episodes about machine learning concepts
results = memory.semantic_search("artificial intelligence research", limit=10)

for result in results:
    print(f"Score: {result.score:.2f}")
    print(f"Episode: {result.episode.content}")
```

### Hybrid Search

Combine keyword matching with semantic similarity for best-of-both-worlds search:

```python
# Balance keyword and semantic matching (50/50)
results = memory.hybrid_search("Python programming", semantic_weight=0.5)

for result in results:
    print(f"Combined score: {result.score:.2f}")
    print(f"Keyword score: {result.keyword_score}")
    print(f"Semantic score: {result.semantic_score}")
```

**Semantic Weight Options:**
- `0.0` - Keyword only (traditional search)
- `0.5` - Balanced (default)
- `1.0` - Semantic only

### Find Similar Episodes

Discover episodes similar to a specific episode:

```python
# Store some episodes
ep_id = memory.store_episode(Episode.conversation(
    "my-agent",
    "How do I learn Python?",
    "Start with the basics like variables and loops"
))

# Find similar conversations
similar = memory.find_similar_episodes(ep_id, limit=5)

for result in similar:
    print(f"Similar episode (score: {result.score:.2f}): {result.episode.id}")
```

### Check Semantic Search Status

Verify semantic search configuration:

```python
status = memory.get_semantic_search_status()

if status["enabled"]:
    print(f"Model: {status['model']}")
    print(f"Dimensions: {status['dimensions']}")
    print(f"Indexed episodes: {status['indexedEpisodes']}")
else:
    print("Semantic search not enabled")
```

## API Reference

### semantic_search()

```python
def semantic_search(
    query: str,
    limit: int = 10,
    min_score: Optional[float] = None
) -> List[SemanticSearchResult]
```

**Parameters:**
- `query`: Search text (will be embedded for comparison)
- `limit`: Maximum results to return (default: 10)
- `min_score`: Minimum similarity score (0.0-1.0)

**Returns:**
List of `SemanticSearchResult` objects containing:
- `episode`: The matched Episode
- `score`: Similarity score (0.0-1.0, higher is more similar)

### hybrid_search()

```python
def hybrid_search(
    query: str,
    limit: int = 10,
    semantic_weight: float = 0.5
) -> List[HybridSearchResult]
```

**Parameters:**
- `query`: Search text
- `limit`: Maximum results (default: 10)
- `semantic_weight`: Balance between semantic (1.0) and keyword (0.0) search

**Returns:**
List of `HybridSearchResult` objects containing:
- `episode`: The matched Episode
- `score`: Combined weighted score
- `keyword_score`: Score from keyword matching
- `semantic_score`: Score from semantic similarity

### find_similar_episodes()

```python
def find_similar_episodes(
    episode_id: str,
    limit: int = 10
) -> List[SemanticSearchResult]
```

**Parameters:**
- `episode_id`: ID of the source episode
- `limit`: Maximum similar episodes to return

**Returns:**
List of `SemanticSearchResult` objects (excludes the source episode)

### get_semantic_search_status()

```python
def get_semantic_search_status() -> Dict[str, Any]
```

**Returns:**
Dictionary containing:
- `enabled`: Whether semantic search is available
- `model`: Embedding model name (if enabled)
- `dimensions`: Vector dimensions (if enabled)
- `indexedEpisodes`: Number of indexed episodes

## Configuration

### Server-Side Setup

Semantic search requires the server to be configured with an embedding provider:

```toml
# config.toml
[memory.semantic_search]
enabled = true
provider = "openai"  # or "mock" for testing
model = "text-embedding-ada-002"
dimensions = 1536
auto_embed = true  # Automatically embed new episodes
```

### Supported Embedding Providers

| Provider | Model | Dimensions | Notes |
|----------|-------|------------|-------|
| OpenAI | text-embedding-ada-002 | 1536 | Production recommended |
| Mock | N/A | Configurable | Testing only |

## Use Cases

### Conversational Context

Find relevant past conversations to provide context:

```python
def get_conversation_context(user_query):
    """Get relevant past conversations for context."""
    results = memory.semantic_search(user_query, limit=3)

    context = []
    for result in results:
        if result.score > 0.7:  # High relevance only
            context.append({
                "user": result.episode.content.get("user_input"),
                "agent": result.episode.content.get("agent_response"),
                "relevance": result.score
            })
    return context
```

### Knowledge Retrieval

Find related knowledge across different terminology:

```python
# User asks about "ML" but knowledge uses "machine learning"
results = memory.semantic_search("ML training process")
# Will find episodes about "machine learning training", "model training", etc.
```

### Deduplication

Find potentially duplicate episodes:

```python
def check_for_duplicate(new_episode):
    """Check if a similar episode already exists."""
    similar = memory.find_similar_episodes(new_episode.id, limit=1)

    if similar and similar[0].score > 0.95:
        return True, similar[0].episode.id
    return False, None
```

### Topic Clustering

Group episodes by semantic similarity:

```python
def find_topic_cluster(topic):
    """Find all episodes related to a topic."""
    return memory.semantic_search(topic, limit=50, min_score=0.6)
```

## Best Practices

### 1. Choose Appropriate Limits

- Start with small limits (10-20) for user-facing searches
- Use larger limits (50-100) for background processing
- Set `min_score` to filter out low-relevance results

### 2. Tune Semantic Weight

For different use cases:
```python
# Precise keyword matching needed
results = memory.hybrid_search(query, semantic_weight=0.2)

# Conceptual similarity preferred
results = memory.hybrid_search(query, semantic_weight=0.8)

# Balanced (default)
results = memory.hybrid_search(query, semantic_weight=0.5)
```

### 3. Handle Missing Semantic Search

Always check if semantic search is enabled:
```python
try:
    results = memory.semantic_search(query)
except MemoryError as e:
    if "not enabled" in str(e):
        # Fall back to keyword search
        results = memory.search_episodes(query)
    else:
        raise
```

### 4. Index Management

For large datasets, consider:
- Regular index rebuilding for optimal performance
- Monitoring indexed episode count
- Removing old/irrelevant episodes from index

## Performance Considerations

### Vector Index (HNSW)

QilbeeDB uses HNSW (Hierarchical Navigable Small World) indexing:
- Approximate nearest neighbor search
- Sub-linear search time complexity
- Configurable trade-off between speed and accuracy

### Memory Usage

- Each episode embedding uses ~6KB (1536 dimensions * 4 bytes)
- 1 million episodes ≈ 6GB index memory
- Consider using `forget()` to manage memory usage

### Latency

Typical latencies:
- Semantic search: 10-50ms
- Hybrid search: 20-80ms
- Find similar: 10-40ms

## Error Handling

### Common Errors

```python
from qilbeedb.exceptions import MemoryError, AuthenticationError

try:
    results = memory.semantic_search(query)
except AuthenticationError:
    print("Please log in first")
except MemoryError as e:
    if "not enabled" in str(e):
        print("Semantic search not configured on server")
    else:
        print(f"Memory error: {e}")
```

## Next Steps

- [Episodes](episodes.md) - Learn about episode types
- [Memory Types](memory-types.md) - Understand different memory categories
- [Persistence](persistence.md) - Configure storage backend
- [Memory API](../api/memory-api.md) - Complete API reference
