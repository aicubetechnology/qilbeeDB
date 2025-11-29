"""
Agent memory management.

This module provides a Python interface to QilbeeDB's agent memory system,
which supports bi-temporal memory management for AI agents.

Memory Persistence:
    QilbeeDB provides enterprise-grade memory persistence using RocksDB on the
    server side. Memories are automatically persisted with:
    - Write-ahead logging (WAL) for durability
    - Configurable compression (LZ4)
    - Automatic recovery on server restart

    Persistence is configured server-side and is transparent to the SDK client.
    All episodes stored via this SDK are automatically persisted to disk.

Memory Types:
    - Episodic: Specific events and interactions
    - Semantic: General knowledge and concepts
    - Procedural: How-to knowledge and workflows
    - Factual: User preferences and persistent facts

Example:
    >>> db = QilbeeDB("http://localhost:7474")
    >>> db.login("admin", "password")
    >>> memory = db.agent_memory("my-agent")
    >>> episode = Episode.conversation("my-agent", "Hello", "Hi there!")
    >>> episode_id = memory.store_episode(episode)
    >>> # Episode is now persisted - survives server restart
"""

from typing import Optional, Dict, Any, List, TYPE_CHECKING
from datetime import datetime, timezone
from urllib.parse import urljoin

from .exceptions import MemoryError, AuthenticationError

if TYPE_CHECKING:
    from .client import QilbeeDB


class MemoryConfig:
    """
    Configuration for agent memory.

    These settings control client-side behavior for memory operations.
    Server-side persistence settings (WAL, compression, etc.) are configured
    when starting the QilbeeDB server.

    Attributes:
        max_episodes: Maximum episodes to store (soft limit, managed by server)
        min_relevance: Minimum relevance score for episode retrieval
        auto_consolidate: Whether to auto-consolidate old episodes
        auto_forget: Whether to auto-forget low-relevance episodes
        consolidation_threshold: Episode count threshold for consolidation
        episodic_retention_days: Days to retain episodic memories
    """

    def __init__(
        self,
        max_episodes: int = 10000,
        min_relevance: float = 0.1,
        auto_consolidate: bool = False,
        auto_forget: bool = False,
        consolidation_threshold: int = 5000,
        episodic_retention_days: int = 30
    ):
        self.max_episodes = max_episodes
        self.min_relevance = min_relevance
        self.auto_consolidate = auto_consolidate
        self.auto_forget = auto_forget
        self.consolidation_threshold = consolidation_threshold
        self.episodic_retention_days = episodic_retention_days


class MemoryStatistics:
    """Memory statistics."""

    def __init__(
        self,
        total_episodes: int,
        oldest_episode: Optional[int],
        newest_episode: Optional[int],
        avg_relevance: float
    ):
        self.total_episodes = total_episodes
        self.oldest_episode = oldest_episode
        self.newest_episode = newest_episode
        self.avg_relevance = avg_relevance


class Episode:
    """
    Represents an episodic memory.

    Episodes are automatically persisted to RocksDB on the server.
    Each episode has a unique ID assigned by the server upon storage.

    Attributes:
        id: Unique episode identifier (assigned by server on store)
        agent_id: The agent this episode belongs to
        episode_type: Type of episode (conversation, observation, action, etc.)
        content: Episode content as a dictionary
        event_time: Unix timestamp of when the event occurred
        metadata: Additional metadata dictionary

    Example:
        >>> episode = Episode.conversation("my-agent", "Hello", "Hi there!")
        >>> episode_id = memory.store_episode(episode)
        >>> # Episode is now persisted with the returned ID
    """

    def __init__(
        self,
        agent_id: str,
        episode_type: str,
        content: Dict[str, Any],
        event_time: Optional[int] = None,
        metadata: Optional[Dict[str, Any]] = None,
        episode_id: Optional[str] = None
    ):
        self.id = episode_id
        self.agent_id = agent_id
        self.episode_type = episode_type
        self.content = content
        self.event_time = event_time or int(datetime.now(timezone.utc).timestamp())
        self.metadata = metadata or {}

    @staticmethod
    def conversation(
        agent_id: str,
        user_input: str,
        agent_response: str,
        metadata: Optional[Dict[str, Any]] = None
    ) -> "Episode":
        """Create a conversation episode."""
        return Episode(
            agent_id=agent_id,
            episode_type="conversation",
            content={
                "user_input": user_input,
                "agent_response": agent_response
            },
            metadata=metadata
        )

    @staticmethod
    def observation(
        agent_id: str,
        observation: str,
        metadata: Optional[Dict[str, Any]] = None
    ) -> "Episode":
        """Create an observation episode."""
        return Episode(
            agent_id=agent_id,
            episode_type="observation",
            content={"observation": observation},
            metadata=metadata
        )

    @staticmethod
    def action(
        agent_id: str,
        action: str,
        result: str,
        metadata: Optional[Dict[str, Any]] = None
    ) -> "Episode":
        """Create an action episode."""
        return Episode(
            agent_id=agent_id,
            episode_type="action",
            content={"action": action, "result": result},
            metadata=metadata
        )

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary."""
        return {
            "agentId": self.agent_id,
            "episodeType": self.episode_type,
            "content": self.content,
            "eventTime": self.event_time,
            "metadata": self.metadata
        }


class AgentMemory:
    """
    Agent memory management.

    Provides a Python interface to QilbeeDB's agent memory system.
    All memories stored via this class are automatically persisted to
    RocksDB on the server and survive server restarts.

    Features:
        - Episodic memory storage with automatic persistence
        - Memory search and retrieval
        - Memory consolidation and forgetting
        - Full CRUD operations on episodes

    Example:
        >>> db = QilbeeDB("http://localhost:7474")
        >>> db.login("admin", "password")
        >>> memory = db.agent_memory("my-agent")
        >>>
        >>> # Store an episode (automatically persisted)
        >>> episode = Episode.conversation("my-agent", "Hello", "Hi there!")
        >>> episode_id = memory.store_episode(episode)
        >>>
        >>> # Retrieve the episode
        >>> retrieved = memory.get_episode(episode_id)
        >>>
        >>> # Get memory statistics
        >>> stats = memory.get_statistics()
        >>> print(f"Total episodes: {stats.total_episodes}")

    Note:
        All memory operations require authentication. Make sure to call
        `db.login()` or configure API key authentication before using
        memory operations.
    """

    def __init__(
        self,
        agent_id: str,
        client: "QilbeeDB",
        config: Optional[MemoryConfig] = None
    ):
        self.agent_id = agent_id
        self.client = client
        self.config = config or MemoryConfig()

    def store_episode(self, episode: Episode) -> str:
        """
        Store an episode.

        The episode is persisted to RocksDB on the server and survives
        server restarts. A unique episode ID is assigned by the server.

        Args:
            episode: The Episode object to store

        Returns:
            The unique episode ID assigned by the server

        Raises:
            MemoryError: If the episode could not be stored
            AuthenticationError: If not authenticated

        Example:
            >>> episode = Episode.conversation("my-agent", "Hello", "Hi!")
            >>> episode_id = memory.store_episode(episode)
            >>> print(f"Stored episode: {episode_id}")
        """
        try:
            response = self.client.session.post(
                urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes"),
                json=episode.to_dict(),
                timeout=self.client.timeout
            )

            if response.status_code == 401:
                raise AuthenticationError("Authentication required for memory operations")

            if response.status_code == 500:
                raise MemoryError("Failed to store episode")

            response.raise_for_status()
            data = response.json()
            return data["episodeId"]
        except (MemoryError, AuthenticationError):
            raise
        except Exception as e:
            raise MemoryError(f"Failed to store episode: {e}")

    def get_episode(self, episode_id: str) -> Optional[Episode]:
        """
        Get an episode by ID.

        This retrieves a persisted episode from the server. Episodes are stored
        in RocksDB and survive server restarts.

        Args:
            episode_id: The unique episode identifier

        Returns:
            Episode if found, None otherwise

        Raises:
            AuthenticationError: If not authenticated
        """
        try:
            response = self.client.session.get(
                urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes/{episode_id}"),
                timeout=self.client.timeout
            )

            if response.status_code == 401:
                raise AuthenticationError("Authentication required for memory operations")

            if response.status_code == 404:
                return None

            response.raise_for_status()
            data = response.json()

            return Episode(
                agent_id=data["agentId"],
                episode_type=data["episodeType"],
                content=data["content"],
                event_time=data["eventTime"],
                metadata=data.get("metadata", {}),
                episode_id=data.get("episodeId")
            )
        except AuthenticationError:
            raise
        except:
            return None

    def get_recent_episodes(self, limit: int = 10) -> List[Episode]:
        """
        Get recent episodes.

        Retrieves the most recent episodes for this agent, ordered by
        event time descending (newest first).

        Args:
            limit: Maximum number of episodes to return (default: 10)

        Returns:
            List of Episode objects, ordered by event time (newest first)

        Raises:
            AuthenticationError: If not authenticated
        """
        response = self.client.session.get(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes/recent"),
            params={"limit": limit},
            timeout=self.client.timeout
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication required for memory operations")

        response.raise_for_status()
        data = response.json()

        episodes = []
        for ep_data in data.get("episodes", []):
            episodes.append(Episode(
                agent_id=ep_data["agentId"],
                episode_type=ep_data["episodeType"],
                content=ep_data["content"],
                event_time=ep_data["eventTime"],
                metadata=ep_data.get("metadata", {}),
                episode_id=ep_data.get("episodeId")
            ))
        return episodes

    def search_episodes(self, query: str, limit: int = 10) -> List[Episode]:
        """
        Search episodes by content.

        Searches episode content for the given query string. Currently uses
        substring matching; semantic/vector search is planned for future releases.

        Args:
            query: Search query string to match against episode content
            limit: Maximum number of episodes to return (default: 10)

        Returns:
            List of Episode objects matching the query

        Raises:
            AuthenticationError: If not authenticated
        """
        response = self.client.session.post(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes/search"),
            json={"query": query, "limit": limit},
            timeout=self.client.timeout
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication required for memory operations")

        response.raise_for_status()
        data = response.json()

        episodes = []
        for ep_data in data.get("episodes", []):
            episodes.append(Episode(
                agent_id=ep_data["agentId"],
                episode_type=ep_data["episodeType"],
                content=ep_data["content"],
                event_time=ep_data["eventTime"],
                metadata=ep_data.get("metadata", {}),
                episode_id=ep_data.get("episodeId")
            ))
        return episodes

    def delete_episode(self, episode_id: str) -> bool:
        """
        Delete an episode by ID.

        Permanently removes the episode from persistent storage.

        Args:
            episode_id: The unique episode identifier

        Returns:
            True if the episode was deleted, False if not found

        Raises:
            AuthenticationError: If not authenticated
            MemoryError: If deletion failed
        """
        try:
            response = self.client.session.delete(
                urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes/{episode_id}"),
                timeout=self.client.timeout
            )

            if response.status_code == 401:
                raise AuthenticationError("Authentication required for memory operations")

            if response.status_code == 404:
                return False

            if response.status_code == 500:
                raise MemoryError("Failed to delete episode")

            return response.status_code == 200 or response.status_code == 204
        except (MemoryError, AuthenticationError):
            raise
        except Exception as e:
            raise MemoryError(f"Failed to delete episode: {e}")

    def get_statistics(self) -> MemoryStatistics:
        """
        Get memory statistics for this agent.

        Returns aggregate statistics about the agent's episodic memory,
        including total episode count and average relevance scores.

        Returns:
            MemoryStatistics object containing memory metrics

        Raises:
            AuthenticationError: If not authenticated
        """
        response = self.client.session.get(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/statistics"),
            timeout=self.client.timeout
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication required for memory operations")

        response.raise_for_status()
        data = response.json()

        return MemoryStatistics(
            total_episodes=data["totalEpisodes"],
            oldest_episode=data.get("oldestEpisode"),
            newest_episode=data.get("newestEpisode"),
            avg_relevance=data["avgRelevance"]
        )

    def consolidate(self) -> int:
        """
        Consolidate episodic memories.

        Triggers memory consolidation, which summarizes and merges similar
        episodes to reduce memory usage while preserving key information.

        Returns:
            Number of episodes consolidated

        Raises:
            AuthenticationError: If not authenticated

        Note:
            Full consolidation with LLM integration is planned for future releases.
        """
        response = self.client.session.post(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/consolidate"),
            timeout=self.client.timeout
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication required for memory operations")

        response.raise_for_status()
        data = response.json()
        return data["consolidated"]

    def forget(self, min_relevance: float = 0.1) -> int:
        """
        Forget low-relevance memories.

        Removes episodes with relevance scores below the threshold.
        This helps manage memory size by removing less important memories.

        Args:
            min_relevance: Minimum relevance score to keep (0.0-1.0, default: 0.1)

        Returns:
            Number of episodes forgotten (deleted)

        Raises:
            AuthenticationError: If not authenticated
        """
        response = self.client.session.post(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/forget"),
            json={"minRelevance": min_relevance},
            timeout=self.client.timeout
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication required for memory operations")

        response.raise_for_status()
        data = response.json()
        return data["forgotten"]

    def clear(self) -> bool:
        """
        Clear all memories for this agent.

        Permanently deletes all episodes from persistent storage.
        Use with caution - this operation cannot be undone.

        Returns:
            True if memories were cleared successfully

        Raises:
            AuthenticationError: If not authenticated
        """
        response = self.client.session.delete(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}"),
            timeout=self.client.timeout
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication required for memory operations")

        return response.status_code == 200

    def semantic_search(
        self,
        query: str,
        limit: int = 10,
        min_score: Optional[float] = None
    ) -> List["SemanticSearchResult"]:
        """
        Search episodes using semantic/vector similarity.

        Uses vector embeddings to find episodes with similar meaning to the query,
        even when exact keywords don't match. This enables finding conceptually
        related memories.

        Args:
            query: The search query text (will be embedded for comparison)
            limit: Maximum number of results to return (default: 10)
            min_score: Minimum similarity score (0.0-1.0) to include in results

        Returns:
            List of SemanticSearchResult objects, ordered by similarity (highest first)

        Raises:
            AuthenticationError: If not authenticated
            MemoryError: If semantic search is not enabled on the server

        Example:
            >>> # Find episodes about "machine learning" even if they use
            >>> # different terminology like "AI", "neural networks", etc.
            >>> results = memory.semantic_search("machine learning concepts")
            >>> for result in results:
            ...     print(f"Score: {result.score:.2f}, Episode: {result.episode.id}")
        """
        request_body = {"query": query, "limit": limit}
        if min_score is not None:
            request_body["minScore"] = min_score

        response = self.client.session.post(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes/semantic-search"),
            json=request_body,
            timeout=self.client.timeout
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication required for memory operations")

        if response.status_code == 501:
            raise MemoryError("Semantic search is not enabled on this server")

        response.raise_for_status()
        data = response.json()

        results = []
        for result_data in data.get("results", []):
            ep_data = result_data.get("episode", {})
            episode = Episode(
                agent_id=ep_data.get("agentId", self.agent_id),
                episode_type=ep_data.get("episodeType", ""),
                content=ep_data.get("content", {}),
                event_time=ep_data.get("eventTime"),
                metadata=ep_data.get("metadata", {}),
                episode_id=ep_data.get("episodeId")
            )
            results.append(SemanticSearchResult(
                episode=episode,
                score=result_data.get("score", 0.0)
            ))
        return results

    def hybrid_search(
        self,
        query: str,
        limit: int = 10,
        semantic_weight: float = 0.5
    ) -> List["HybridSearchResult"]:
        """
        Search episodes using combined keyword and semantic search.

        Combines traditional keyword matching with semantic similarity for
        best-of-both-worlds search. The semantic_weight parameter controls
        the balance between keyword and semantic scores.

        Args:
            query: The search query text
            limit: Maximum number of results to return (default: 10)
            semantic_weight: Weight for semantic score vs keyword score (0.0-1.0)
                           0.0 = keyword only, 1.0 = semantic only, 0.5 = balanced

        Returns:
            List of HybridSearchResult objects with combined scoring

        Raises:
            AuthenticationError: If not authenticated
            MemoryError: If hybrid search fails

        Example:
            >>> # Search with balanced keyword and semantic matching
            >>> results = memory.hybrid_search("Python programming", semantic_weight=0.5)
            >>> for result in results:
            ...     print(f"Combined: {result.score:.2f}, Keyword: {result.keyword_score}, Semantic: {result.semantic_score}")
        """
        request_body = {
            "query": query,
            "limit": limit,
            "semanticWeight": semantic_weight
        }

        response = self.client.session.post(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes/hybrid-search"),
            json=request_body,
            timeout=self.client.timeout
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication required for memory operations")

        if response.status_code == 501:
            raise MemoryError("Hybrid search is not enabled on this server")

        response.raise_for_status()
        data = response.json()

        results = []
        for result_data in data.get("results", []):
            ep_data = result_data.get("episode", {})
            episode = Episode(
                agent_id=ep_data.get("agentId", self.agent_id),
                episode_type=ep_data.get("episodeType", ""),
                content=ep_data.get("content", {}),
                event_time=ep_data.get("eventTime"),
                metadata=ep_data.get("metadata", {}),
                episode_id=ep_data.get("episodeId")
            )
            results.append(HybridSearchResult(
                episode=episode,
                score=result_data.get("score", 0.0),
                keyword_score=result_data.get("keywordScore"),
                semantic_score=result_data.get("semanticScore")
            ))
        return results

    def find_similar_episodes(
        self,
        episode_id: str,
        limit: int = 10
    ) -> List["SemanticSearchResult"]:
        """
        Find episodes similar to a given episode.

        Uses the embedding of the specified episode to find other episodes
        with similar content/meaning.

        Args:
            episode_id: The ID of the episode to find similar episodes for
            limit: Maximum number of similar episodes to return (default: 10)

        Returns:
            List of SemanticSearchResult objects (excludes the source episode)

        Raises:
            AuthenticationError: If not authenticated
            MemoryError: If the episode is not found or semantic search is not enabled

        Example:
            >>> # Find episodes similar to a specific conversation
            >>> similar = memory.find_similar_episodes("episode-123", limit=5)
            >>> for result in similar:
            ...     print(f"Similar episode: {result.episode.id} (score: {result.score:.2f})")
        """
        response = self.client.session.get(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes/{episode_id}/similar"),
            params={"limit": limit},
            timeout=self.client.timeout
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication required for memory operations")

        if response.status_code == 404:
            raise MemoryError(f"Episode {episode_id} not found")

        if response.status_code == 501:
            raise MemoryError("Semantic search is not enabled on this server")

        response.raise_for_status()
        data = response.json()

        results = []
        for result_data in data.get("results", []):
            ep_data = result_data.get("episode", {})
            episode = Episode(
                agent_id=ep_data.get("agentId", self.agent_id),
                episode_type=ep_data.get("episodeType", ""),
                content=ep_data.get("content", {}),
                event_time=ep_data.get("eventTime"),
                metadata=ep_data.get("metadata", {}),
                episode_id=ep_data.get("episodeId")
            )
            results.append(SemanticSearchResult(
                episode=episode,
                score=result_data.get("score", 0.0)
            ))
        return results

    def get_semantic_search_status(self) -> Dict[str, Any]:
        """
        Get the status of semantic search for this agent.

        Returns information about whether semantic search is enabled,
        the embedding model configuration, and index statistics.

        Returns:
            Dictionary containing:
                - enabled: Whether semantic search is available
                - model: Embedding model name (if enabled)
                - dimensions: Embedding vector dimensions (if enabled)
                - indexed_episodes: Number of episodes in the vector index

        Raises:
            AuthenticationError: If not authenticated
        """
        response = self.client.session.get(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/semantic-search/status"),
            timeout=self.client.timeout
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication required for memory operations")

        response.raise_for_status()
        return response.json()


class SemanticSearchResult:
    """
    Result from a semantic search operation.

    Contains the matched episode along with its similarity score.

    Attributes:
        episode: The matched Episode object
        score: Similarity score (0.0 to 1.0, higher is more similar)
    """

    def __init__(self, episode: Episode, score: float):
        self.episode = episode
        self.score = score

    def __repr__(self) -> str:
        return f"SemanticSearchResult(episode_id={self.episode.id}, score={self.score:.4f})"


class HybridSearchResult:
    """
    Result from a hybrid search operation.

    Contains the matched episode along with combined and component scores.

    Attributes:
        episode: The matched Episode object
        score: Combined score (weighted average of keyword and semantic scores)
        keyword_score: Score from keyword/text matching (may be None)
        semantic_score: Score from semantic similarity (may be None)
    """

    def __init__(
        self,
        episode: Episode,
        score: float,
        keyword_score: Optional[float] = None,
        semantic_score: Optional[float] = None
    ):
        self.episode = episode
        self.score = score
        self.keyword_score = keyword_score
        self.semantic_score = semantic_score

    def __repr__(self) -> str:
        return f"HybridSearchResult(episode_id={self.episode.id}, score={self.score:.4f})"
