"""
Graph operations and data structures.
"""

from typing import Any, Dict, List, Optional, TYPE_CHECKING, Union
from urllib.parse import urljoin

from .exceptions import NodeNotFoundError, RelationshipNotFoundError, QueryError

if TYPE_CHECKING:
    from .client import QilbeeDB


def _build_url(base: str, path: str) -> str:
    """Build URL safely handling mocks."""
    base_str = str(base).rstrip('/')
    path_str = str(path).lstrip('/')
    return f"{base_str}/{path_str}"


class Node:
    """Represents a graph node."""

    def __init__(
        self,
        labels: List[str],
        properties: Optional[Dict[str, Any]] = None,
        id: Optional[int] = None
    ):
        """
        Create a node.

        Args:
            labels: List of node labels
            properties: Node properties
            id: Node ID (assigned by database)
        """
        self.id = id
        self.labels = labels
        self.properties = properties or {}

    def get(self, key: str, default: Any = None) -> Any:
        """Get a property value."""
        return self.properties.get(key, default)

    def set(self, key: str, value: Any):
        """Set a property value."""
        self.properties[key] = value

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary."""
        return {
            "id": self.id,
            "labels": self.labels,
            "properties": self.properties
        }

    def __repr__(self):
        labels_str = ":".join(self.labels)
        return f"Node(id={self.id}, labels=[{labels_str}])"


class Relationship:
    """Represents a graph relationship."""

    def __init__(
        self,
        type: str,
        start_node: int,
        end_node: int,
        properties: Optional[Dict[str, Any]] = None,
        id: Optional[int] = None
    ):
        """
        Create a relationship.

        Args:
            type: Relationship type
            start_node: Start node ID
            end_node: End node ID
            properties: Relationship properties
            id: Relationship ID (assigned by database)
        """
        self.id = id
        self.type = type
        self.start_node = start_node
        self.end_node = end_node
        self.properties = properties or {}

    def get(self, key: str, default: Any = None) -> Any:
        """Get a property value."""
        return self.properties.get(key, default)

    def set(self, key: str, value: Any):
        """Set a property value."""
        self.properties[key] = value

    def to_dict(self) -> Dict[str, Any]:
        """Convert to dictionary."""
        return {
            "id": self.id,
            "type": self.type,
            "startNode": self.start_node,
            "endNode": self.end_node,
            "properties": self.properties
        }

    def __repr__(self):
        return f"Relationship(id={self.id}, type={self.type}, {self.start_node}->{self.end_node})"


class Graph:
    """Graph database operations."""

    def __init__(self, name: str, client: "QilbeeDB"):
        """
        Create a graph instance.

        Args:
            name: Graph name
            client: QilbeeDB client instance
        """
        self.name = name
        self.client = client

    def create_node(
        self,
        labels: List[str],
        properties: Optional[Dict[str, Any]] = None
    ) -> Node:
        """
        Create a new node.

        Args:
            labels: Node labels
            properties: Node properties

        Returns:
            Created node
        """
        response = self.client.session.post(
            urljoin(self.client.base_url, f"/graphs/{self.name}/nodes"),
            json={
                "labels": labels,
                "properties": properties or {}
            },
            timeout=self.client.timeout
        )
        response.raise_for_status()
        data = response.json()

        return Node(
            labels=data["labels"],
            properties=data["properties"],
            id=data["id"]
        )

    def get_node(self, node_id: int) -> Optional[Node]:
        """
        Get a node by ID.

        Args:
            node_id: Node ID

        Returns:
            Node if found, None otherwise

        Raises:
            NodeNotFoundError: If node not found
        """
        try:
            response = self.client.session.get(
                urljoin(self.client.base_url, f"/graphs/{self.name}/nodes/{node_id}"),
                timeout=self.client.timeout
            )

            if response.status_code == 404:
                raise NodeNotFoundError(f"Node {node_id} not found")

            response.raise_for_status()
            data = response.json()

            return Node(
                labels=data["labels"],
                properties=data["properties"],
                id=data["id"]
            )
        except Exception as e:
            if "404" in str(e) or (hasattr(e, 'response') and e.response and e.response.status_code == 404):
                raise NodeNotFoundError(f"Node {node_id} not found")
            raise

    def update_node(self, node: Node) -> Node:
        """
        Update a node.

        Args:
            node: Node to update

        Returns:
            Updated node
        """
        response = self.client.session.put(
            urljoin(self.client.base_url, f"/graphs/{self.name}/nodes/{node.id}"),
            json={
                "labels": node.labels,
                "properties": node.properties
            },
            timeout=self.client.timeout
        )
        response.raise_for_status()
        data = response.json()

        return Node(
            labels=data["labels"],
            properties=data["properties"],
            id=data["id"]
        )

    def delete_node(self, node_id: int) -> bool:
        """
        Delete a node.

        Args:
            node_id: Node ID

        Returns:
            True if deleted
        """
        response = self.client.session.delete(
            urljoin(self.client.base_url, f"/graphs/{self.name}/nodes/{node_id}"),
            timeout=self.client.timeout
        )
        return response.status_code == 200

    def create_relationship(
        self,
        from_node: Union[int, Node],
        rel_type: str,
        to_node: Union[int, Node],
        properties: Optional[Dict[str, Any]] = None
    ) -> Relationship:
        """
        Create a relationship.

        Args:
            from_node: Start node ID or Node
            rel_type: Relationship type
            to_node: End node ID or Node
            properties: Relationship properties

        Returns:
            Created relationship
        """
        from_id = from_node.id if isinstance(from_node, Node) else from_node
        to_id = to_node.id if isinstance(to_node, Node) else to_node

        response = self.client.session.post(
            urljoin(self.client.base_url, f"/graphs/{self.name}/relationships"),
            json={
                "startNode": from_id,
                "type": rel_type,
                "endNode": to_id,
                "properties": properties or {}
            },
            timeout=self.client.timeout
        )
        response.raise_for_status()
        data = response.json()

        return Relationship(
            type=data["type"],
            start_node=data["startNode"],
            end_node=data["endNode"],
            properties=data["properties"],
            id=data["id"]
        )

    def find_nodes(
        self,
        label: Optional[str] = None,
        properties: Optional[Dict[str, Any]] = None,
        limit: int = 100
    ) -> List[Node]:
        """
        Find nodes by label and/or properties.

        Args:
            label: Node label to filter by
            properties: Properties to filter by
            limit: Maximum number of nodes to return

        Returns:
            List of matching nodes
        """
        params = {"limit": limit}
        if label:
            params["label"] = label
        if properties:
            params["properties"] = properties

        response = self.client.session.get(
            urljoin(self.client.base_url, f"/graphs/{self.name}/nodes"),
            params=params,
            timeout=self.client.timeout
        )
        response.raise_for_status()
        data = response.json()

        nodes = []
        for node_data in data.get("nodes", []):
            nodes.append(Node(
                labels=node_data["labels"],
                properties=node_data["properties"],
                id=node_data["id"]
            ))
        return nodes

    def get_relationships(
        self,
        node: Union[int, Node],
        direction: str = "both"
    ) -> List[Relationship]:
        """
        Get relationships for a node.

        Args:
            node: Node ID or Node
            direction: Relationship direction (incoming, outgoing, both)

        Returns:
            List of relationships
        """
        node_id = node.id if isinstance(node, Node) else node

        response = self.client.session.get(
            urljoin(self.client.base_url, f"/graphs/{self.name}/nodes/{node_id}/relationships"),
            params={"direction": direction},
            timeout=self.client.timeout
        )
        response.raise_for_status()
        data = response.json()

        relationships = []
        for rel_data in data.get("relationships", []):
            relationships.append(Relationship(
                type=rel_data["type"],
                start_node=rel_data["startNode"],
                end_node=rel_data["endNode"],
                properties=rel_data["properties"],
                id=rel_data["id"]
            ))
        return relationships

    def query(
        self,
        cypher: str,
        parameters: Optional[Dict[str, Any]] = None
    ) -> "QueryResult":
        """
        Execute a Cypher query.

        Args:
            cypher: Cypher query string
            parameters: Query parameters

        Returns:
            Query result

        Raises:
            QueryError: If query execution fails
        """
        from .query import QueryResult, QueryStats

        try:
            response = self.client.session.post(
                urljoin(self.client.base_url, f"/graphs/{self.name}/query"),
                json={
                    "cypher": cypher,
                    "parameters": parameters or {}
                },
                timeout=self.client.timeout
            )

            if response.status_code == 400:
                error_data = response.json()
                raise QueryError(error_data.get("error", "Query execution failed"))

            response.raise_for_status()
            data = response.json()

            stats_data = data.get("stats", {})
            stats = QueryStats(
                nodes_created=stats_data.get("nodesCreated", 0),
                nodes_deleted=stats_data.get("nodesDeleted", 0),
                relationships_created=stats_data.get("relationshipsCreated", 0),
                relationships_deleted=stats_data.get("relationshipsDeleted", 0),
                execution_time_ms=stats_data.get("executionTimeMs", 0.0)
            )

            return QueryResult(data.get("results", []), stats)
        except Exception as e:
            if isinstance(e, QueryError):
                raise
            raise QueryError(f"Query execution failed: {e}")

    def __repr__(self):
        return f"Graph(name='{self.name}')"
