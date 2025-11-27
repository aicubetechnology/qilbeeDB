"""
QilbeeDB client implementation.
"""

import requests
from typing import Optional, Dict, Any, List, Union
from urllib.parse import urljoin

from .graph import Graph
from .memory import AgentMemory, MemoryConfig
from .exceptions import ConnectionError, AuthenticationError
from .auth import JWTAuth, APIKeyAuth, BasicAuth


class QilbeeDB:
    """
    Main QilbeeDB client for connecting to the database.

    Example (JWT):
        >>> db = QilbeeDB("http://localhost:7474")
        >>> db.login("admin", "password")

    Example (API Key):
        >>> db = QilbeeDB({"uri": "http://localhost:7474", "api_key": "qilbee_live_abc123"})

    Example (Basic Auth - deprecated):
        >>> db = QilbeeDB({"uri": "http://localhost:7474", "auth": {"username": "neo4j", "password": "pass"}})
    """

    def __init__(self, uri_or_config: Union[str, Dict[str, Any]]):
        """
        Initialize QilbeeDB client.

        Args:
            uri_or_config: Either a URI string or a config dict with keys:
                - uri: Connection URI (http://)
                - api_key: API key for authentication (recommended)
                - auth: Dict with username and password (deprecated, use login() instead)
                - timeout: Request timeout in seconds (default: 30)
                - verify_ssl: Verify SSL certificates (default: True)
                - persist_tokens: Whether to persist JWT tokens (default: True)
        """
        if isinstance(uri_or_config, str):
            self.base_url = uri_or_config
            self.timeout = 30
            self.verify_ssl = True
            self.persist_tokens = True
            auth_config = None
            api_key = None
        else:
            self.base_url = uri_or_config.get("uri", "http://localhost:7474")
            self.timeout = uri_or_config.get("timeout", 30)
            self.verify_ssl = uri_or_config.get("verify_ssl", True)
            self.persist_tokens = uri_or_config.get("persist_tokens", True)
            auth_config = uri_or_config.get("auth")
            api_key = uri_or_config.get("api_key")

        self.session = requests.Session()
        self._auth_handler: Optional[Union[JWTAuth, APIKeyAuth, BasicAuth]] = None

        # Initialize authentication based on config
        if api_key:
            # API Key authentication (recommended)
            self._auth_handler = APIKeyAuth(api_key, self.session)
        elif auth_config:
            # Basic authentication (deprecated)
            username = auth_config.get("username")
            password = auth_config.get("password")
            if username and password:
                self._auth_handler = BasicAuth(username, password, self.session)
        else:
            # No authentication configured, will use JWT with login()
            self._auth_handler = JWTAuth(
                self.base_url,
                self.session,
                self.timeout,
                self.verify_ssl,
                self.persist_tokens
            )

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

    def login(self, username: str, password: str) -> Dict[str, Any]:
        """
        Login with username and password (JWT authentication).

        Args:
            username: User's username
            password: User's password

        Returns:
            Login response with user info and tokens

        Raises:
            AuthenticationError: If login fails
        """
        if not isinstance(self._auth_handler, JWTAuth):
            # Replace current auth handler with JWT
            self._auth_handler = JWTAuth(
                self.base_url,
                self.session,
                self.timeout,
                self.verify_ssl,
                self.persist_tokens
            )

        return self._auth_handler.login(username, password)

    def logout(self) -> None:
        """
        Logout and clear authentication.

        This clears JWT tokens or removes API key/basic auth from the session.
        """
        if self._auth_handler:
            self._auth_handler.logout()

    def is_authenticated(self) -> bool:
        """
        Check if client is currently authenticated.

        Returns:
            True if authenticated with valid credentials/token
        """
        if not self._auth_handler:
            return False
        return self._auth_handler.is_authenticated()

    def set_api_key(self, api_key: str) -> None:
        """
        Switch to API key authentication.

        This method allows you to change authentication method to API key
        after client initialization. Useful for switching from JWT to API key.

        Args:
            api_key: QilbeeDB API key (starts with 'qilbee_live_')

        Example:
            >>> db = QilbeeDB("http://localhost:7474")
            >>> db.login("admin", "password")
            >>> db.set_api_key("qilbee_live_abc123...")
        """
        # Clear any existing auth headers
        if self._auth_handler:
            self._auth_handler.logout()

        # Set up API key authentication
        self._auth_handler = APIKeyAuth(api_key, self.session)

    def refresh_token(self) -> str:
        """
        Manually refresh the JWT access token.

        Returns:
            New access token

        Raises:
            AuthenticationError: If not using JWT or refresh fails
        """
        if not isinstance(self._auth_handler, JWTAuth):
            raise AuthenticationError("Token refresh only available with JWT authentication")

        return self._auth_handler.refresh_access_token()

    # User Management Methods

    def create_user(self, username: str, password: str, email: Optional[str] = None,
                    roles: Optional[List[str]] = None) -> Dict[str, Any]:
        """
        Create a new user (admin only).

        Args:
            username: Username for the new user
            password: Password for the new user
            email: Email address for the new user (optional)
            roles: List of roles (Admin, Developer, DataScientist, Agent, Read)

        Returns:
            Created user information

        Raises:
            AuthenticationError: If not authenticated or not admin
        """
        payload = {
            "username": username,
            "password": password,
            "roles": roles or ["Read"]
        }
        if email:
            payload["email"] = email

        response = self.session.post(
            urljoin(self.base_url, "/api/v1/users"),
            json=payload,
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")

        response.raise_for_status()
        return response.json()

    def list_users(self) -> List[Dict[str, Any]]:
        """
        List all users (admin only).

        Returns:
            List of user information dictionaries

        Raises:
            AuthenticationError: If not authenticated or not admin
        """
        response = self.session.get(
            urljoin(self.base_url, "/api/v1/users"),
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")

        response.raise_for_status()
        return response.json().get("users", [])

    def get_user(self, user_id: str) -> Dict[str, Any]:
        """
        Get user by ID (admin only).

        Args:
            user_id: User UUID

        Returns:
            User information

        Raises:
            AuthenticationError: If not authenticated or not admin
        """
        response = self.session.get(
            urljoin(self.base_url, f"/api/v1/users/{user_id}"),
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")

        response.raise_for_status()
        return response.json()

    def update_user(self, user_id: str, password: Optional[str] = None,
                    roles: Optional[List[str]] = None) -> Dict[str, Any]:
        """
        Update user information (admin only).

        Args:
            user_id: User UUID
            password: New password (optional)
            roles: New roles list (optional)

        Returns:
            Updated user information

        Raises:
            AuthenticationError: If not authenticated or not admin
        """
        payload = {}
        if password:
            payload["password"] = password
        if roles:
            payload["roles"] = roles

        response = self.session.put(
            urljoin(self.base_url, f"/api/v1/users/{user_id}"),
            json=payload,
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")

        response.raise_for_status()
        return response.json()

    def update_user_roles(self, user_id: str, roles: List[str]) -> Dict[str, Any]:
        """
        Update user roles (admin only).

        This is a separate method because the server uses a dedicated endpoint
        for role updates at PUT /api/v1/users/{user_id}/roles.

        Args:
            user_id: User UUID
            roles: New roles list

        Returns:
            Updated user information

        Raises:
            AuthenticationError: If not authenticated or not admin
        """
        response = self.session.put(
            urljoin(self.base_url, f"/api/v1/users/{user_id}/roles"),
            json={"roles": roles},
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")

        response.raise_for_status()
        return response.json()

    def delete_user(self, user_id: str) -> bool:
        """
        Delete user (admin only).

        Args:
            user_id: User UUID

        Returns:
            True if deleted successfully

        Raises:
            AuthenticationError: If not authenticated or not admin
        """
        response = self.session.delete(
            urljoin(self.base_url, f"/api/v1/users/{user_id}"),
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")

        return response.status_code == 200

    # API Key Management Methods

    def create_api_key(self, name: str) -> Dict[str, Any]:
        """
        Create a new API key.

        Args:
            name: Name/description for the API key

        Returns:
            Dictionary with 'id', 'key', 'name', and 'created_at'

        Raises:
            AuthenticationError: If not authenticated
        """
        response = self.session.post(
            urljoin(self.base_url, "/api/v1/api-keys"),
            json={"name": name},
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed")

        response.raise_for_status()
        return response.json()

    def list_api_keys(self) -> List[Dict[str, Any]]:
        """
        List all API keys for the current user.

        Returns:
            List of API key information (without the actual key values)

        Raises:
            AuthenticationError: If not authenticated
        """
        response = self.session.get(
            urljoin(self.base_url, "/api/v1/api-keys"),
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed")

        response.raise_for_status()
        return response.json().get("api_keys", [])

    def delete_api_key(self, key_id: str) -> bool:
        """
        Delete an API key.

        Args:
            key_id: API key ID (UUID)

        Returns:
            True if deleted successfully

        Raises:
            AuthenticationError: If not authenticated
        """
        response = self.session.delete(
            urljoin(self.base_url, f"/api/v1/api-keys/{key_id}"),
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed")

        return response.status_code == 200

    # Rate Limit Policy Management Methods (Admin Only)

    def create_rate_limit_policy(
        self,
        name: str,
        endpoint_type: Union[str, Dict[str, str]],
        max_requests: int,
        window_secs: int,
        enabled: bool = True
    ) -> Dict[str, Any]:
        """
        Create a new rate limit policy (admin only).

        Args:
            name: Name/description for the policy
            endpoint_type: Endpoint type, either:
                - String: "Login", "ApiKeyCreation", "GeneralApi", "UserManagement"
                - Dict: {"Custom": "/api/path"} for custom endpoint pattern
            max_requests: Maximum number of requests allowed
            window_secs: Time window in seconds
            enabled: Whether the policy is enabled (default: True)

        Returns:
            Created rate limit policy information

        Raises:
            AuthenticationError: If not authenticated or not admin

        Example:
            >>> db.create_rate_limit_policy(
            ...     name="Test API Limit",
            ...     endpoint_type={"Custom": "/api/test"},
            ...     max_requests=100,
            ...     window_secs=3600,
            ...     enabled=True
            ... )
        """
        payload = {
            "name": name,
            "endpoint_type": endpoint_type,
            "max_requests": max_requests,
            "window_secs": window_secs,
            "enabled": enabled
        }

        response = self.session.post(
            urljoin(self.base_url, "/api/v1/rate-limits"),
            json=payload,
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")

        response.raise_for_status()
        return response.json()

    def list_rate_limit_policies(self) -> List[Dict[str, Any]]:
        """
        List all rate limit policies (admin only).

        Returns:
            List of rate limit policy information

        Raises:
            AuthenticationError: If not authenticated or not admin

        Example:
            >>> policies = db.list_rate_limit_policies()
            >>> for policy in policies:
            ...     print(f"{policy['name']}: {policy['max_requests']} req/{policy['window_secs']}s")
        """
        response = self.session.get(
            urljoin(self.base_url, "/api/v1/rate-limits"),
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")

        response.raise_for_status()
        return response.json().get("policies", [])

    def get_rate_limit_policy(self, policy_id: str) -> Dict[str, Any]:
        """
        Get rate limit policy by ID (admin only).

        Args:
            policy_id: Rate limit policy UUID

        Returns:
            Rate limit policy information

        Raises:
            AuthenticationError: If not authenticated or not admin

        Example:
            >>> policy = db.get_rate_limit_policy("1132c011-cd65-4583-b1cc-1ffe3444698c")
            >>> print(f"Policy: {policy['name']}, Limit: {policy['max_requests']}")
        """
        response = self.session.get(
            urljoin(self.base_url, f"/api/v1/rate-limits/{policy_id}"),
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")

        response.raise_for_status()
        return response.json()

    def update_rate_limit_policy(
        self,
        policy_id: str,
        name: Optional[str] = None,
        max_requests: Optional[int] = None,
        window_secs: Optional[int] = None,
        enabled: Optional[bool] = None
    ) -> Dict[str, Any]:
        """
        Update rate limit policy (admin only).

        Args:
            policy_id: Rate limit policy UUID
            name: New policy name (optional)
            max_requests: New max requests limit (optional)
            window_secs: New window size in seconds (optional)
            enabled: New enabled status (optional)

        Returns:
            Updated rate limit policy information

        Raises:
            AuthenticationError: If not authenticated or not admin

        Example:
            >>> updated = db.update_rate_limit_policy(
            ...     policy_id="1132c011-cd65-4583-b1cc-1ffe3444698c",
            ...     max_requests=200,
            ...     enabled=False
            ... )
        """
        payload = {}
        if name is not None:
            payload["name"] = name
        if max_requests is not None:
            payload["max_requests"] = max_requests
        if window_secs is not None:
            payload["window_secs"] = window_secs
        if enabled is not None:
            payload["enabled"] = enabled

        response = self.session.put(
            urljoin(self.base_url, f"/api/v1/rate-limits/{policy_id}"),
            json=payload,
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")

        response.raise_for_status()
        return response.json()

    def delete_rate_limit_policy(self, policy_id: str) -> bool:
        """
        Delete rate limit policy (admin only).

        Args:
            policy_id: Rate limit policy UUID

        Returns:
            True if deleted successfully

        Raises:
            AuthenticationError: If not authenticated or not admin

        Example:
            >>> db.delete_rate_limit_policy("1132c011-cd65-4583-b1cc-1ffe3444698c")
            True
        """
        response = self.session.delete(
            urljoin(self.base_url, f"/api/v1/rate-limits/{policy_id}"),
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")

        return response.status_code == 200

    # Audit Log Methods (Admin Only)

    def get_audit_logs(
        self,
        event_type: Optional[str] = None,
        user_id: Optional[str] = None,
        username: Optional[str] = None,
        result: Optional[str] = None,
        ip_address: Optional[str] = None,
        start_time: Optional[str] = None,
        end_time: Optional[str] = None,
        limit: int = 100
    ) -> Dict[str, Any]:
        """
        Query audit logs (admin only).

        Args:
            event_type: Filter by event type (e.g., 'login', 'login_failed', 'user_created')
            user_id: Filter by user ID
            username: Filter by username
            result: Filter by result ('success', 'failure', 'unauthorized', 'forbidden')
            ip_address: Filter by IP address
            start_time: Filter events after this ISO8601 timestamp
            end_time: Filter events before this ISO8601 timestamp
            limit: Maximum events to return (default: 100, max: 1000)

        Returns:
            Dictionary with 'events', 'count', and 'limit' keys

        Raises:
            AuthenticationError: If not authenticated or not admin

        Example:
            >>> logs = db.get_audit_logs(event_type="login_failed", limit=50)
            >>> print(f"Found {logs['count']} failed login attempts")
            >>> for event in logs['events']:
            ...     print(f"{event['timestamp']}: {event['username']} - {event['result']}")
        """
        params = {"limit": limit}
        if event_type:
            params["event_type"] = event_type
        if user_id:
            params["user_id"] = user_id
        if username:
            params["username"] = username
        if result:
            params["result"] = result
        if ip_address:
            params["ip_address"] = ip_address
        if start_time:
            params["start_time"] = start_time
        if end_time:
            params["end_time"] = end_time

        response = self.session.get(
            urljoin(self.base_url, "/api/v1/audit-logs"),
            params=params,
            timeout=self.timeout,
            verify=self.verify_ssl
        )

        if response.status_code == 401:
            raise AuthenticationError("Authentication failed or insufficient permissions")
        if response.status_code == 403:
            raise AuthenticationError("Admin role required to access audit logs")

        response.raise_for_status()
        return response.json()

    def get_failed_logins(self, limit: int = 100) -> List[Dict[str, Any]]:
        """
        Get failed login attempts (admin only).

        Convenience method for querying login_failed events.

        Args:
            limit: Maximum events to return (default: 100)

        Returns:
            List of failed login audit events

        Raises:
            AuthenticationError: If not authenticated or not admin

        Example:
            >>> failed = db.get_failed_logins(limit=10)
            >>> for event in failed:
            ...     print(f"{event['timestamp']}: {event.get('username', 'unknown')}")
        """
        result = self.get_audit_logs(event_type="login_failed", limit=limit)
        return result.get("events", [])

    def get_user_audit_events(self, username: str, limit: int = 100) -> List[Dict[str, Any]]:
        """
        Get audit events for a specific user (admin only).

        Args:
            username: Username to filter by
            limit: Maximum events to return (default: 100)

        Returns:
            List of audit events for the user

        Raises:
            AuthenticationError: If not authenticated or not admin

        Example:
            >>> events = db.get_user_audit_events("alice", limit=50)
            >>> for event in events:
            ...     print(f"{event['event_type']}: {event['result']}")
        """
        result = self.get_audit_logs(username=username, limit=limit)
        return result.get("events", [])

    def get_security_events(self, limit: int = 100) -> List[Dict[str, Any]]:
        """
        Get security-relevant events (admin only).

        Returns events with 'unauthorized' or 'forbidden' results,
        which indicate potential security issues.

        Args:
            limit: Maximum events to return (default: 100)

        Returns:
            List of security-relevant audit events

        Raises:
            AuthenticationError: If not authenticated or not admin

        Example:
            >>> security_events = db.get_security_events(limit=50)
            >>> for event in security_events:
            ...     print(f"{event['event_type']}: {event['ip_address']}")
        """
        # Get unauthorized events
        unauthorized = self.get_audit_logs(result="unauthorized", limit=limit)
        # Get forbidden events
        forbidden = self.get_audit_logs(result="forbidden", limit=limit)

        # Combine and sort by timestamp (most recent first)
        events = unauthorized.get("events", []) + forbidden.get("events", [])
        events.sort(key=lambda x: x.get("timestamp", ""), reverse=True)
        return events[:limit]

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
        auth_type = "unauthenticated"
        if self._auth_handler:
            if isinstance(self._auth_handler, JWTAuth):
                auth_type = "JWT"
            elif isinstance(self._auth_handler, APIKeyAuth):
                auth_type = "API Key"
            elif isinstance(self._auth_handler, BasicAuth):
                auth_type = "Basic (deprecated)"
        return f"QilbeeDB(base_url='{self.base_url}', auth='{auth_type}')"
