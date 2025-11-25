"""
Agent memory management.
"""

from typing import Optional, Dict, Any, List, TYPE_CHECKING
from datetime import datetime
from urllib.parse import urljoin

from .exceptions import MemoryError

if TYPE_CHECKING:
    from .client import QilbeeDB


class MemoryConfig:
    """Configuration for agent memory."""

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
    """Represents an episodic memory."""

    def __init__(
        self,
        agent_id: str,
        episode_type: str,
        content: Dict[str, Any],
        event_time: Optional[int] = None,
        metadata: Optional[Dict[str, Any]] = None
    ):
        self.agent_id = agent_id
        self.episode_type = episode_type
        self.content = content
        self.event_time = event_time or int(datetime.utcnow().timestamp())
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
    """Agent memory management."""

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
        """Store an episode."""
        try:
            response = self.client.session.post(
                urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes"),
                json=episode.to_dict(),
                timeout=self.client.timeout
            )

            if response.status_code == 500:
                raise MemoryError("Failed to store episode")

            response.raise_for_status()
            data = response.json()
            return data["episodeId"]
        except Exception as e:
            if isinstance(e, MemoryError):
                raise
            raise MemoryError(f"Failed to store episode: {e}")

    def get_episode(self, episode_id: str) -> Optional[Episode]:
        """Get an episode by ID."""
        try:
            response = self.client.session.get(
                urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes/{episode_id}"),
                timeout=self.client.timeout
            )

            if response.status_code == 404:
                return None

            response.raise_for_status()
            data = response.json()

            return Episode(
                agent_id=data["agentId"],
                episode_type=data["episodeType"],
                content=data["content"],
                event_time=data["eventTime"]
            )
        except:
            return None

    def get_recent_episodes(self, limit: int = 10) -> List[Episode]:
        """Get recent episodes."""
        response = self.client.session.get(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes/recent"),
            params={"limit": limit},
            timeout=self.client.timeout
        )
        response.raise_for_status()
        data = response.json()

        episodes = []
        for ep_data in data.get("episodes", []):
            episodes.append(Episode(
                agent_id=ep_data["agentId"],
                episode_type=ep_data["episodeType"],
                content=ep_data["content"],
                event_time=ep_data["eventTime"]
            ))
        return episodes

    def search_episodes(self, query: str, limit: int = 10) -> List[Episode]:
        """Search episodes."""
        response = self.client.session.post(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/episodes/search"),
            json={"query": query, "limit": limit},
            timeout=self.client.timeout
        )
        response.raise_for_status()
        data = response.json()

        episodes = []
        for ep_data in data.get("episodes", []):
            episodes.append(Episode(
                agent_id=ep_data["agentId"],
                episode_type=ep_data["episodeType"],
                content=ep_data["content"],
                event_time=ep_data["eventTime"]
            ))
        return episodes

    def get_statistics(self) -> MemoryStatistics:
        """Get memory statistics."""
        response = self.client.session.get(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/statistics"),
            timeout=self.client.timeout
        )
        response.raise_for_status()
        data = response.json()

        return MemoryStatistics(
            total_episodes=data["totalEpisodes"],
            oldest_episode=data.get("oldestEpisode"),
            newest_episode=data.get("newestEpisode"),
            avg_relevance=data["avgRelevance"]
        )

    def consolidate(self) -> int:
        """Consolidate memories."""
        response = self.client.session.post(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/consolidate"),
            timeout=self.client.timeout
        )
        response.raise_for_status()
        data = response.json()
        return data["consolidated"]

    def forget(self, min_relevance: float = 0.1) -> int:
        """Forget low-relevance memories."""
        response = self.client.session.post(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}/forget"),
            json={"minRelevance": min_relevance},
            timeout=self.client.timeout
        )
        response.raise_for_status()
        data = response.json()
        return data["forgotten"]

    def clear(self) -> bool:
        """Clear all memories."""
        response = self.client.session.delete(
            urljoin(self.client.base_url, f"/memory/{self.agent_id}"),
            timeout=self.client.timeout
        )
        return response.status_code == 200
