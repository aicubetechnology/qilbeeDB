"""
QilbeeDB authentication module.

Supports JWT tokens and API keys for authentication.
"""

import os
import json
import time
import warnings
from pathlib import Path
from typing import Optional, Dict, Any, Tuple
from datetime import datetime, timedelta

import jwt
from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.backends import default_backend


class TokenStorage:
    """
    Manages JWT token storage with automatic expiration handling.

    Tokens can be stored in memory or persisted to disk for session persistence.
    """

    def __init__(self, persist: bool = True, storage_path: Optional[str] = None):
        """
        Initialize token storage.

        Args:
            persist: Whether to persist tokens to disk
            storage_path: Custom path for token storage (default: ~/.qilbeedb/tokens)
        """
        self.persist = persist
        self.access_token: Optional[str] = None
        self.refresh_token: Optional[str] = None
        self.token_expiry: Optional[datetime] = None
        self.refresh_expiry: Optional[datetime] = None

        if persist:
            if storage_path:
                self.storage_path = Path(storage_path)
            else:
                self.storage_path = Path.home() / ".qilbeedb" / "tokens"
            self.storage_path.parent.mkdir(parents=True, exist_ok=True)

    def save_tokens(self, access_token: str, refresh_token: str,
                   access_expiry: Optional[datetime] = None,
                   refresh_expiry: Optional[datetime] = None) -> None:
        """
        Save JWT tokens.

        Args:
            access_token: JWT access token
            refresh_token: JWT refresh token
            access_expiry: Access token expiration time
            refresh_expiry: Refresh token expiration time
        """
        self.access_token = access_token
        self.refresh_token = refresh_token
        self.token_expiry = access_expiry
        self.refresh_expiry = refresh_expiry

        if self.persist:
            token_data = {
                "access_token": access_token,
                "refresh_token": refresh_token,
                "access_expiry": access_expiry.isoformat() if access_expiry else None,
                "refresh_expiry": refresh_expiry.isoformat() if refresh_expiry else None,
            }

            # Write with restricted permissions (owner read/write only)
            self.storage_path.write_text(json.dumps(token_data))
            os.chmod(self.storage_path, 0o600)

    def load_tokens(self) -> Tuple[Optional[str], Optional[str]]:
        """
        Load stored tokens.

        Returns:
            Tuple of (access_token, refresh_token)
        """
        if not self.persist or not self.storage_path.exists():
            return None, None

        try:
            token_data = json.loads(self.storage_path.read_text())

            self.access_token = token_data.get("access_token")
            self.refresh_token = token_data.get("refresh_token")

            if token_data.get("access_expiry"):
                self.token_expiry = datetime.fromisoformat(token_data["access_expiry"])

            if token_data.get("refresh_expiry"):
                self.refresh_expiry = datetime.fromisoformat(token_data["refresh_expiry"])

            return self.access_token, self.refresh_token
        except (json.JSONDecodeError, KeyError, ValueError):
            # Invalid token file, ignore it
            return None, None

    def clear_tokens(self) -> None:
        """Clear all stored tokens."""
        self.access_token = None
        self.refresh_token = None
        self.token_expiry = None
        self.refresh_expiry = None

        if self.persist and self.storage_path.exists():
            self.storage_path.unlink()

    def is_access_token_valid(self, buffer_seconds: int = 60) -> bool:
        """
        Check if access token is valid and not expired.

        Args:
            buffer_seconds: Seconds before expiry to consider token invalid

        Returns:
            True if token is valid
        """
        if not self.access_token or not self.token_expiry:
            return False

        # Check if token expires within buffer time
        expiry_with_buffer = self.token_expiry - timedelta(seconds=buffer_seconds)
        return datetime.now() < expiry_with_buffer

    def is_refresh_token_valid(self, buffer_seconds: int = 300) -> bool:
        """
        Check if refresh token is valid and not expired.

        Args:
            buffer_seconds: Seconds before expiry to consider token invalid

        Returns:
            True if token is valid
        """
        if not self.refresh_token or not self.refresh_expiry:
            return False

        expiry_with_buffer = self.refresh_expiry - timedelta(seconds=buffer_seconds)
        return datetime.now() < expiry_with_buffer

    def get_access_token(self) -> Optional[str]:
        """
        Get current access token if valid.

        Returns:
            Access token or None if expired/invalid
        """
        if self.is_access_token_valid():
            return self.access_token
        return None

    def get_refresh_token(self) -> Optional[str]:
        """
        Get current refresh token if valid.

        Returns:
            Refresh token or None if expired/invalid
        """
        if self.is_refresh_token_valid():
            return self.refresh_token
        return None


class JWTAuth:
    """
    JWT-based authentication handler.

    Manages login, logout, token refresh, and automatic token renewal.
    """

    def __init__(self, base_url: str, session, timeout: int = 30,
                 verify_ssl: bool = True, persist_tokens: bool = True):
        """
        Initialize JWT authentication.

        Args:
            base_url: QilbeeDB server base URL
            session: Requests session object
            timeout: Request timeout in seconds
            verify_ssl: Whether to verify SSL certificates
            persist_tokens: Whether to persist tokens to disk
        """
        self.base_url = base_url
        self.session = session
        self.timeout = timeout
        self.verify_ssl = verify_ssl
        self.token_storage = TokenStorage(persist=persist_tokens)
        self.username: Optional[str] = None

        # Try to load existing tokens
        self.token_storage.load_tokens()

    def login(self, username: str, password: str) -> Dict[str, Any]:
        """
        Authenticate with username and password.

        Args:
            username: User's username
            password: User's password

        Returns:
            Login response with user info

        Raises:
            AuthenticationError: If login fails
        """
        from .exceptions import AuthenticationError

        try:
            response = self.session.post(
                f"{self.base_url}/api/v1/auth/login",
                json={"username": username, "password": password},
                timeout=self.timeout,
                verify=self.verify_ssl
            )

            if response.status_code == 401:
                raise AuthenticationError("Invalid username or password")

            response.raise_for_status()
            data = response.json()

            # Extract tokens
            access_token = data.get("access_token")
            refresh_token = data.get("refresh_token") or ""  # Use empty string if None

            if not access_token:
                raise AuthenticationError("Invalid response from server: missing access token")

            # Calculate expiry times
            access_expiry = datetime.now() + timedelta(seconds=data.get("expires_in", 3600))
            refresh_expiry = datetime.now() + timedelta(seconds=data.get("refresh_expires_in", 86400))

            # Save tokens
            self.token_storage.save_tokens(
                access_token, refresh_token if refresh_token else "",
                access_expiry, refresh_expiry
            )

            self.username = username

            # Set authorization header
            self.session.headers["Authorization"] = f"Bearer {access_token}"

            return data

        except Exception as e:
            if isinstance(e, AuthenticationError):
                raise
            raise AuthenticationError(f"Login failed: {e}")

    def logout(self) -> None:
        """
        Logout and clear all tokens.

        This invalidates the current session and removes stored tokens.
        """
        try:
            # Try to call logout endpoint
            if self.token_storage.get_access_token():
                self.session.post(
                    f"{self.base_url}/api/v1/auth/logout",
                    timeout=self.timeout,
                    verify=self.verify_ssl
                )
        except Exception:
            # Ignore logout errors, we're clearing tokens anyway
            pass
        finally:
            # Clear tokens and headers
            self.token_storage.clear_tokens()
            self.session.headers.pop("Authorization", None)
            self.username = None

    def refresh_access_token(self) -> str:
        """
        Refresh the access token using the refresh token.

        Returns:
            New access token

        Raises:
            AuthenticationError: If refresh fails
        """
        from .exceptions import AuthenticationError

        refresh_token = self.token_storage.get_refresh_token()
        if not refresh_token:
            raise AuthenticationError("No valid refresh token available")

        try:
            response = self.session.post(
                f"{self.base_url}/api/v1/auth/refresh",
                json={"refresh_token": refresh_token},
                timeout=self.timeout,
                verify=self.verify_ssl
            )

            if response.status_code == 401:
                # Refresh token expired, need to login again
                self.token_storage.clear_tokens()
                raise AuthenticationError("Refresh token expired, please login again")

            response.raise_for_status()
            data = response.json()

            access_token = data.get("access_token")
            if not access_token:
                raise AuthenticationError("Invalid response from server: missing access token")

            # Update access token (keep existing refresh token)
            access_expiry = datetime.now() + timedelta(seconds=data.get("expires_in", 3600))

            self.token_storage.save_tokens(
                access_token,
                refresh_token,
                access_expiry,
                self.token_storage.refresh_expiry
            )

            # Update authorization header
            self.session.headers["Authorization"] = f"Bearer {access_token}"

            return access_token

        except Exception as e:
            if isinstance(e, AuthenticationError):
                raise
            raise AuthenticationError(f"Token refresh failed: {e}")

    def ensure_valid_token(self) -> str:
        """
        Ensure we have a valid access token, refreshing if needed.

        Returns:
            Valid access token

        Raises:
            AuthenticationError: If unable to get valid token
        """
        from .exceptions import AuthenticationError

        # Check if current token is valid
        access_token = self.token_storage.get_access_token()
        if access_token:
            return access_token

        # Try to refresh
        if self.token_storage.get_refresh_token():
            return self.refresh_access_token()

        raise AuthenticationError("No valid authentication, please login")

    def is_authenticated(self) -> bool:
        """
        Check if user is currently authenticated.

        Returns:
            True if authenticated with valid token
        """
        try:
            self.ensure_valid_token()
            return True
        except Exception:
            return False


class APIKeyAuth:
    """
    API Key-based authentication handler.

    Simpler authentication method using static API keys.
    """

    def __init__(self, api_key: str, session):
        """
        Initialize API key authentication.

        Args:
            api_key: QilbeeDB API key (starts with 'qilbee_live_')
            session: Requests session object
        """
        if not api_key.startswith("qilbee_live_"):
            warnings.warn(
                "API key should start with 'qilbee_live_'. "
                "Make sure you're using a valid QilbeeDB API key.",
                UserWarning
            )

        self.api_key = api_key
        self.session = session

        # Set API key header
        self.session.headers["X-API-Key"] = api_key

    def is_authenticated(self) -> bool:
        """
        Check if API key is set.

        Returns:
            True if API key is configured
        """
        return bool(self.api_key)

    def logout(self) -> None:
        """Remove API key from session."""
        self.session.headers.pop("X-API-Key", None)
        self.api_key = None


class BasicAuth:
    """
    HTTP Basic Authentication handler (deprecated).

    This method is deprecated and will be removed in a future version.
    Please use JWT or API key authentication instead.
    """

    def __init__(self, username: str, password: str, session):
        """
        Initialize basic authentication.

        Args:
            username: Username
            password: Password
            session: Requests session object
        """
        warnings.warn(
            "Basic authentication is deprecated and will be removed in version 1.0. "
            "Please use JWT authentication (login/logout) or API key authentication instead.",
            DeprecationWarning,
            stacklevel=2
        )

        self.username = username
        self.password = password
        self.session = session
        self.session.auth = (username, password)

    def is_authenticated(self) -> bool:
        """
        Check if credentials are set.

        Returns:
            True if credentials are configured
        """
        return bool(self.username and self.password)

    def logout(self) -> None:
        """Remove credentials from session."""
        self.session.auth = None
        self.username = None
        self.password = None
