"""
Unit tests for authentication module.
"""

import pytest
import os
import json
import tempfile
from datetime import datetime, timedelta
from pathlib import Path
from unittest.mock import Mock, patch, MagicMock
import requests

from qilbeedb.auth import TokenStorage, JWTAuth, APIKeyAuth, BasicAuth
from qilbeedb.exceptions import AuthenticationError


class TestTokenStorage:
    """Tests for TokenStorage class."""

    def test_init_with_persist(self, tmpdir):
        """Test initialization with persistence enabled."""
        storage_path = str(tmpdir / "tokens")
        storage = TokenStorage(persist=True, storage_path=storage_path)

        assert storage.persist is True
        assert storage.storage_path == Path(storage_path)
        assert storage.access_token is None
        assert storage.refresh_token is None

    def test_init_without_persist(self):
        """Test initialization without persistence."""
        storage = TokenStorage(persist=False)

        assert storage.persist is False
        assert storage.access_token is None
        assert storage.refresh_token is None

    def test_save_tokens_in_memory(self):
        """Test saving tokens in memory only."""
        storage = TokenStorage(persist=False)
        access_expiry = datetime.now() + timedelta(hours=1)
        refresh_expiry = datetime.now() + timedelta(days=1)

        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            access_expiry,
            refresh_expiry
        )

        assert storage.access_token == "access_token_123"
        assert storage.refresh_token == "refresh_token_456"
        assert storage.token_expiry == access_expiry
        assert storage.refresh_expiry == refresh_expiry

    def test_save_tokens_to_disk(self, tmpdir):
        """Test saving tokens to disk with proper permissions."""
        storage_path = str(tmpdir / "tokens")
        storage = TokenStorage(persist=True, storage_path=storage_path)

        access_expiry = datetime.now() + timedelta(hours=1)
        refresh_expiry = datetime.now() + timedelta(days=1)

        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            access_expiry,
            refresh_expiry
        )

        # Check file exists
        assert Path(storage_path).exists()

        # Check file permissions (owner read/write only)
        file_mode = os.stat(storage_path).st_mode & 0o777
        assert file_mode == 0o600

        # Check file content
        with open(storage_path, 'r') as f:
            data = json.load(f)
            assert data["access_token"] == "access_token_123"
            assert data["refresh_token"] == "refresh_token_456"

    def test_load_tokens_from_disk(self, tmpdir):
        """Test loading tokens from disk."""
        storage_path = str(tmpdir / "tokens")
        storage = TokenStorage(persist=True, storage_path=storage_path)

        # Save tokens first
        access_expiry = datetime.now() + timedelta(hours=1)
        refresh_expiry = datetime.now() + timedelta(days=1)
        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            access_expiry,
            refresh_expiry
        )

        # Create new storage instance and load
        storage2 = TokenStorage(persist=True, storage_path=storage_path)
        access, refresh = storage2.load_tokens()

        assert access == "access_token_123"
        assert refresh == "refresh_token_456"
        assert storage2.token_expiry == access_expiry
        assert storage2.refresh_expiry == refresh_expiry

    def test_load_tokens_no_file(self, tmpdir):
        """Test loading tokens when file doesn't exist."""
        storage_path = str(tmpdir / "nonexistent")
        storage = TokenStorage(persist=True, storage_path=storage_path)

        access, refresh = storage.load_tokens()

        assert access is None
        assert refresh is None

    def test_load_tokens_invalid_json(self, tmpdir):
        """Test loading tokens with invalid JSON file."""
        storage_path = str(tmpdir / "invalid_tokens")
        Path(storage_path).write_text("invalid json{{{")

        storage = TokenStorage(persist=True, storage_path=storage_path)
        access, refresh = storage.load_tokens()

        assert access is None
        assert refresh is None

    def test_clear_tokens_in_memory(self):
        """Test clearing tokens from memory."""
        storage = TokenStorage(persist=False)
        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() + timedelta(days=1)
        )

        storage.clear_tokens()

        assert storage.access_token is None
        assert storage.refresh_token is None
        assert storage.token_expiry is None
        assert storage.refresh_expiry is None

    def test_clear_tokens_from_disk(self, tmpdir):
        """Test clearing tokens removes file from disk."""
        storage_path = str(tmpdir / "tokens")
        storage = TokenStorage(persist=True, storage_path=storage_path)

        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() + timedelta(days=1)
        )

        assert Path(storage_path).exists()

        storage.clear_tokens()

        assert not Path(storage_path).exists()
        assert storage.access_token is None

    def test_is_access_token_valid(self):
        """Test access token validity check."""
        storage = TokenStorage(persist=False)

        # No token
        assert not storage.is_access_token_valid()

        # Valid token (expires in 2 hours)
        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            datetime.now() + timedelta(hours=2),
            datetime.now() + timedelta(days=1)
        )
        assert storage.is_access_token_valid()

        # Expired token
        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            datetime.now() - timedelta(hours=1),
            datetime.now() + timedelta(days=1)
        )
        assert not storage.is_access_token_valid()

        # Token expiring within buffer (default 60 seconds)
        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            datetime.now() + timedelta(seconds=30),
            datetime.now() + timedelta(days=1)
        )
        assert not storage.is_access_token_valid()

    def test_is_refresh_token_valid(self):
        """Test refresh token validity check."""
        storage = TokenStorage(persist=False)

        # No token
        assert not storage.is_refresh_token_valid()

        # Valid token (expires in 2 days)
        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() + timedelta(days=2)
        )
        assert storage.is_refresh_token_valid()

        # Expired token
        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() - timedelta(days=1)
        )
        assert not storage.is_refresh_token_valid()

    def test_get_access_token(self):
        """Test getting valid access token."""
        storage = TokenStorage(persist=False)

        # No token
        assert storage.get_access_token() is None

        # Valid token
        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() + timedelta(days=1)
        )
        assert storage.get_access_token() == "access_token_123"

        # Expired token
        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            datetime.now() - timedelta(hours=1),
            datetime.now() + timedelta(days=1)
        )
        assert storage.get_access_token() is None

    def test_get_refresh_token(self):
        """Test getting valid refresh token."""
        storage = TokenStorage(persist=False)

        # No token
        assert storage.get_refresh_token() is None

        # Valid token
        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() + timedelta(days=1)
        )
        assert storage.get_refresh_token() == "refresh_token_456"

        # Expired token
        storage.save_tokens(
            "access_token_123",
            "refresh_token_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() - timedelta(days=1)
        )
        assert storage.get_refresh_token() is None


class TestJWTAuth:
    """Tests for JWTAuth class."""

    def test_init(self):
        """Test JWT auth initialization."""
        session = Mock()
        auth = JWTAuth(
            "http://localhost:7474",
            session,
            timeout=30,
            verify_ssl=True,
            persist_tokens=False
        )

        assert auth.base_url == "http://localhost:7474"
        assert auth.session == session
        assert auth.timeout == 30
        assert auth.verify_ssl is True
        assert auth.username is None

    def test_login_success(self):
        """Test successful login."""
        session = Mock()
        session.headers = {}
        mock_response = Mock()
        mock_response.status_code = 200
        mock_response.json.return_value = {
            "access_token": "access_123",
            "refresh_token": "refresh_456",
            "expires_in": 3600,
            "refresh_expires_in": 86400,
            "username": "testuser"
        }
        session.post.return_value = mock_response

        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)
        result = auth.login("testuser", "password123")

        # Verify API call
        session.post.assert_called_once()
        call_args = session.post.call_args
        assert call_args[0][0] == "http://localhost:7474/api/v1/auth/login"
        assert call_args[1]["json"] == {"username": "testuser", "password": "password123"}

        # Verify tokens saved
        assert auth.token_storage.access_token == "access_123"
        assert auth.token_storage.refresh_token == "refresh_456"
        assert auth.username == "testuser"

        # Verify Authorization header set
        assert session.headers["Authorization"] == "Bearer access_123"

    def test_login_invalid_credentials(self):
        """Test login with invalid credentials."""
        session = Mock()
        mock_response = Mock()
        mock_response.status_code = 401
        session.post.return_value = mock_response

        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)

        with pytest.raises(AuthenticationError) as exc_info:
            auth.login("baduser", "badpass")

        assert "Invalid username or password" in str(exc_info.value)

    def test_login_missing_tokens_in_response(self):
        """Test login with missing tokens in response."""
        session = Mock()
        mock_response = Mock()
        mock_response.status_code = 200
        mock_response.json.return_value = {"username": "testuser"}  # Missing tokens
        session.post.return_value = mock_response

        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)

        with pytest.raises(AuthenticationError) as exc_info:
            auth.login("testuser", "password")

        assert "missing access token" in str(exc_info.value)

    def test_logout(self):
        """Test logout clears tokens."""
        session = Mock()
        mock_response = Mock()
        mock_response.status_code = 200
        session.post.return_value = mock_response

        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)

        # Set up some tokens first
        auth.token_storage.save_tokens(
            "access_123",
            "refresh_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() + timedelta(days=1)
        )
        auth.username = "testuser"

        # Logout
        auth.logout()

        # Verify tokens cleared
        assert auth.token_storage.access_token is None
        assert auth.token_storage.refresh_token is None
        assert auth.username is None

    def test_refresh_access_token_success(self):
        """Test successful token refresh."""
        session = Mock()
        session.headers = {}
        mock_response = Mock()
        mock_response.status_code = 200
        mock_response.json.return_value = {
            "access_token": "new_access_789",
            "expires_in": 3600
        }
        session.post.return_value = mock_response

        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)

        # Set up initial tokens
        auth.token_storage.save_tokens(
            "old_access_123",
            "refresh_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() + timedelta(days=1)
        )

        # Refresh
        new_token = auth.refresh_access_token()

        # Verify API call
        session.post.assert_called_once()
        call_args = session.post.call_args
        assert call_args[0][0] == "http://localhost:7474/api/v1/auth/refresh"
        assert call_args[1]["json"] == {"refresh_token": "refresh_456"}

        # Verify new token saved
        assert new_token == "new_access_789"
        assert auth.token_storage.access_token == "new_access_789"
        assert auth.token_storage.refresh_token == "refresh_456"  # Unchanged

    def test_refresh_access_token_no_refresh_token(self):
        """Test refresh fails when no refresh token available."""
        session = Mock()
        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)

        with pytest.raises(AuthenticationError) as exc_info:
            auth.refresh_access_token()

        assert "No valid refresh token" in str(exc_info.value)

    def test_refresh_access_token_expired_refresh_token(self):
        """Test refresh fails when refresh token expired."""
        session = Mock()
        mock_response = Mock()
        mock_response.status_code = 401
        session.post.return_value = mock_response

        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)

        # Set up expired refresh token
        auth.token_storage.save_tokens(
            "access_123",
            "refresh_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() + timedelta(days=1)
        )

        with pytest.raises(AuthenticationError) as exc_info:
            auth.refresh_access_token()

        assert "please login again" in str(exc_info.value)

        # Verify tokens cleared
        assert auth.token_storage.access_token is None

    def test_ensure_valid_token_with_valid_token(self):
        """Test ensure_valid_token with valid token."""
        session = Mock()
        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)

        # Set up valid token
        auth.token_storage.save_tokens(
            "access_123",
            "refresh_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() + timedelta(days=1)
        )

        token = auth.ensure_valid_token()

        assert token == "access_123"

    def test_ensure_valid_token_refreshes_expired_token(self):
        """Test ensure_valid_token refreshes expired access token."""
        session = Mock()
        session.headers = {}
        mock_response = Mock()
        mock_response.status_code = 200
        mock_response.json.return_value = {
            "access_token": "new_access_789",
            "expires_in": 3600
        }
        session.post.return_value = mock_response

        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)

        # Set up expired access token but valid refresh token
        auth.token_storage.save_tokens(
            "expired_access",
            "refresh_456",
            datetime.now() - timedelta(hours=1),  # Expired
            datetime.now() + timedelta(days=1)
        )

        token = auth.ensure_valid_token()

        assert token == "new_access_789"
        session.post.assert_called_once()  # Refresh was called

    def test_ensure_valid_token_no_tokens(self):
        """Test ensure_valid_token fails when no tokens."""
        session = Mock()
        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)

        with pytest.raises(AuthenticationError) as exc_info:
            auth.ensure_valid_token()

        assert "please login" in str(exc_info.value)

    def test_is_authenticated_with_valid_token(self):
        """Test is_authenticated returns True with valid token."""
        session = Mock()
        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)

        # Set up valid token
        auth.token_storage.save_tokens(
            "access_123",
            "refresh_456",
            datetime.now() + timedelta(hours=1),
            datetime.now() + timedelta(days=1)
        )

        assert auth.is_authenticated() is True

    def test_is_authenticated_no_token(self):
        """Test is_authenticated returns False with no token."""
        session = Mock()
        auth = JWTAuth("http://localhost:7474", session, persist_tokens=False)

        assert auth.is_authenticated() is False


class TestAPIKeyAuth:
    """Tests for APIKeyAuth class."""

    def test_init_with_valid_key(self):
        """Test initialization with valid API key."""
        session = Mock()
        session.headers = {}
        auth = APIKeyAuth("qilbee_live_abc123", session)

        assert auth.api_key == "qilbee_live_abc123"
        assert auth.session == session

        # Verify header set
        assert session.headers["X-API-Key"] == "qilbee_live_abc123"

    def test_init_with_invalid_key_warns(self):
        """Test initialization with invalid API key shows warning."""
        session = Mock()
        session.headers = {}

        with pytest.warns(UserWarning, match="API key should start with 'qilbee_live_'"):
            auth = APIKeyAuth("invalid_key", session)

        assert auth.api_key == "invalid_key"

    def test_is_authenticated(self):
        """Test is_authenticated returns True when key is set."""
        session = Mock()
        session.headers = {}
        auth = APIKeyAuth("qilbee_live_abc123", session)

        assert auth.is_authenticated() is True

    def test_is_authenticated_no_key(self):
        """Test is_authenticated returns False when no key."""
        session = Mock()
        session.headers = {}
        auth = APIKeyAuth("qilbee_live_abc123", session)
        auth.api_key = None

        assert auth.is_authenticated() is False

    def test_logout(self):
        """Test logout removes API key."""
        session = Mock()
        session.headers = {}
        auth = APIKeyAuth("qilbee_live_abc123", session)

        auth.logout()

        assert auth.api_key is None


class TestBasicAuth:
    """Tests for BasicAuth class (deprecated)."""

    def test_init_shows_deprecation_warning(self):
        """Test initialization shows deprecation warning."""
        session = Mock()

        with pytest.warns(DeprecationWarning, match="Basic authentication is deprecated"):
            auth = BasicAuth("testuser", "password", session)

        assert auth.username == "testuser"
        assert auth.password == "password"
        assert session.auth == ("testuser", "password")

    def test_is_authenticated(self):
        """Test is_authenticated returns True when credentials set."""
        session = Mock()

        with pytest.warns(DeprecationWarning):
            auth = BasicAuth("testuser", "password", session)

        assert auth.is_authenticated() is True

    def test_is_authenticated_no_credentials(self):
        """Test is_authenticated returns False when no credentials."""
        session = Mock()

        with pytest.warns(DeprecationWarning):
            auth = BasicAuth("testuser", "password", session)

        auth.username = None

        assert auth.is_authenticated() is False

    def test_logout(self):
        """Test logout removes credentials."""
        session = Mock()

        with pytest.warns(DeprecationWarning):
            auth = BasicAuth("testuser", "password", session)

        auth.logout()

        assert auth.username is None
        assert auth.password is None
        assert session.auth is None
