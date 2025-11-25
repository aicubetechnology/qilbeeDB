"""
QilbeeDB client implementation.
"""

import requests
from typing import Optional, Dict, Any, List, Union
from urllib.parse import urljoin

from .graph import Graph
from .memory import AgentMemory, MemoryConfig
from .exceptions import ConnectionError, AuthenticationError


class QilbeeDB:
    """
    Main QilbeeDB client for connecting to the database.

    Example:
        >>> db = QilbeeDB("http://localhost:7474")
        >>> db = QilbeeDB({"uri": "http://localhost:7474", "auth": {"username": "neo4j", "password": "pass"}})
    """

    def __init__(self, uri_or_config: Union[str, Dict[str, Any]]):
        """
        Initialize QilbeeDB client.

        Args:
            uri_or_config: Either a URI string or a config dict with keys:
                - uri: Connection URI (http://)
                - auth: Dict with username and password
                - timeout: Request timeout in seconds (default: 30)
                - verify_ssl: Verify SSL certificates (default: True)
        """
        if isinstance(uri_or_config, str):
            self.base_url = uri_or_config
            self.auth = None
            self.timeout = 30
            self.verify_ssl = True
        else:
            self.base_url = uri_or_config.get("uri", "http://localhost:7474")
            auth_config = uri_or_config.get("auth")
            if auth_config:
                self.auth = (auth_config["username"], auth_config["password"])
            else:
                self.auth = None
            self.timeout = uri_or_config.get("timeout", 30)
            self.verify_ssl = uri_or_config.get("verify_ssl", True)

        self.session = requests.Session()
        if self.auth:
            self.session.auth = self.auth

    def health(self) -> Dict[str, Any]:
        """
        Get database health status.

        Returns:
            Health status dictionary

        Raises:
            ConnectionError: If connection fails
            AuthenticationError: If authentication fails
        """
        try:
            response = self.session.get(
                urljoin(self.base_url, "/health"),
                timeout=self.timeout,
                verify=self.verify_ssl
            )

            if response.status_code == 401:
                raise AuthenticationError("Authentication failed")

            response.raise_for_status()
            return response.json()
        except requests.exceptions.RequestException as e:
            if hasattr(e, 'response') and e.response is not None and e.response.status_code == 401:
                raise AuthenticationError("Authentication failed")
            raise ConnectionError(f"Failed to connect to QilbeeDB: {e}")

    def graph(self, name: str) -> Graph:
        """
        Get or create a graph by name.

        Args:
            name: Graph name

        Returns:
            Graph instance
        """
        return Graph(name, self)

    def list_graphs(self) -> List[str]:
        """
        List all graphs in the database.

        Returns:
            List of graph names
        """
        response = self.session.get(
            urljoin(self.base_url, "/graphs"),
            timeout=self.timeout,
            verify=self.verify_ssl
        )
        response.raise_for_status()
        return response.json().get("graphs", [])

    def create_graph(self, name: str) -> Graph:
        """
        Create a new graph.

        Args:
            name: Graph name

        Returns:
            Graph instance
        """
        response = self.session.post(
            urljoin(self.base_url, f"/graphs/{name}"),
            timeout=self.timeout,
            verify=self.verify_ssl
        )
        response.raise_for_status()
        return Graph(name, self)

    def delete_graph(self, name: str) -> bool:
        """
        Delete a graph and all its data.

        Args:
            name: Graph name

        Returns:
            True if deleted successfully
        """
        response = self.session.delete(
            urljoin(self.base_url, f"/graphs/{name}"),
            timeout=self.timeout,
            verify=self.verify_ssl
        )
        return response.status_code == 200

    def agent_memory(
        self,
        agent_id: str,
        config: Optional[MemoryConfig] = None
    ) -> AgentMemory:
        """
        Create or access agent memory.

        Args:
            agent_id: Unique agent identifier
            config: Memory configuration

        Returns:
            AgentMemory instance
        """
        return AgentMemory(agent_id, self, config)

    def close(self):
        """Close the database connection."""
        if self.session:
            self.session.close()

    def __enter__(self):
        """Context manager entry."""
        return self

    def __exit__(self, exc_type, exc_val, exc_tb):
        """Context manager exit."""
        self.close()

    def __repr__(self):
        return f"QilbeeDB(base_url='{self.base_url}')"
