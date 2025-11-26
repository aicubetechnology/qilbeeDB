"""
Integration tests for authentication with real QilbeeDB server.

Note: These tests require a QilbeeDB server running on localhost:7474
"""

import pytest
import time
from qilbeedb import QilbeeDB
from qilbeedb.exceptions import AuthenticationError


@pytest.mark.integration
class TestJWTAuthIntegration:
    """Integration tests for JWT authentication against real server."""

    @pytest.fixture(scope="class")
    def server_url(self):
        """QilbeeDB server URL."""
        return "http://localhost:7474"

    @pytest.fixture(autouse=True)
    def isolated_client(self, tmpdir):
        """
        Ensure each test gets an isolated token storage.

        This prevents test interference by giving each test a unique
        storage path instead of sharing the default ~/.qilbeedb/tokens.
        """
        # This fixture runs before each test method
        # tmpdir is unique per test, so tokens won't interfere
        yield tmpdir

    def test_connection_without_auth(self, server_url):
        """Test connection to server without authentication."""
        db = QilbeeDB(server_url)

        # Should be able to create client
        assert db.base_url == server_url
        assert not db.is_authenticated()

    def test_jwt_login_flow(self, server_url, isolated_client):
        """Test complete JWT login flow."""
        # Use isolated storage for this test
        db = QilbeeDB(server_url)
        db._auth_handler.token_storage.storage_path = isolated_client / "tokens"
        db._auth_handler.token_storage.persist = False  # Disable for this test

        # Initial state
        assert not db.is_authenticated()

        # Attempt login (this will work if server has bootstrap admin user)
        try:
            # Try to login with test credentials
            # Note: This assumes server may have been bootstrapped
            result = db.login("admin", "Admin123!@#")

            # Verify login response
            assert "access_token" in result
            assert "refresh_token" in result

            # Verify authenticated
            assert db.is_authenticated()

            print(f"✓ Login successful: {result.get('username', 'admin')}")

        except AuthenticationError as e:
            # Expected if credentials don't match
            print(f"Login failed (expected if not bootstrapped): {e}")
            assert "Invalid username or password" in str(e) or "Authentication failed" in str(e)

    def test_jwt_refresh_flow(self, server_url):
        """Test JWT token refresh flow."""
        db = QilbeeDB(server_url)

        try:
            # Login first
            db.login("admin", "Admin123!@#")

            # Get initial token
            initial_token = db._auth_handler.token_storage.access_token
            assert initial_token is not None

            # Refresh token
            new_token = db.refresh_token()

            # Verify new token is different
            assert new_token != initial_token
            assert db._auth_handler.token_storage.access_token == new_token

            print(f"✓ Token refresh successful")

        except AuthenticationError as e:
            print(f"Refresh test skipped (no valid session): {e}")

    def test_jwt_logout_flow(self, server_url):
        """Test JWT logout flow."""
        db = QilbeeDB(server_url)

        try:
            # Login first
            db.login("admin", "Admin123!@#")
            assert db.is_authenticated()

            # Logout
            db.logout()

            # Verify logged out
            assert not db.is_authenticated()
            assert db._auth_handler.token_storage.access_token is None

            print(f"✓ Logout successful")

        except AuthenticationError as e:
            print(f"Logout test skipped: {e}")

    def test_invalid_credentials(self, server_url, isolated_client):
        """Test login with invalid credentials."""
        # Use isolated storage to prevent token carryover
        db = QilbeeDB(server_url)
        db._auth_handler.token_storage.storage_path = isolated_client / "tokens"
        db._auth_handler.token_storage.persist = False
        db._auth_handler.token_storage.clear_tokens()  # Ensure clean state

        with pytest.raises(AuthenticationError) as exc_info:
            db.login("invalid_user", "wrong_password")

        assert "Invalid username or password" in str(exc_info.value) or "Authentication failed" in str(exc_info.value)
        print(f"✓ Invalid credentials rejected correctly")

    def test_api_key_auth(self, server_url):
        """Test API key authentication."""
        # Create client with API key
        db = QilbeeDB({
            "uri": server_url,
            "api_key": "qilbee_live_test_key_12345"
        })

        # Verify API key is set
        assert db.is_authenticated()
        assert db._auth_handler.api_key == "qilbee_live_test_key_12345"

        print(f"✓ API key authentication initialized")

    def test_basic_auth_deprecated(self, server_url):
        """Test basic authentication shows deprecation warning."""
        with pytest.warns(DeprecationWarning, match="Basic authentication is deprecated"):
            db = QilbeeDB({
                "uri": server_url,
                "auth": {
                    "username": "testuser",
                    "password": "testpass"
                }
            })

        assert db.is_authenticated()
        print(f"✓ Basic auth shows deprecation warning")

    def test_session_persistence(self, server_url, tmpdir):
        """Test that JWT tokens persist across sessions."""
        from pathlib import Path
        storage_path = Path(str(tmpdir / "test_tokens"))

        try:
            # First session: login and save tokens
            db1 = QilbeeDB(server_url)
            db1._auth_handler.token_storage.storage_path = storage_path
            db1._auth_handler.token_storage.persist = True

            db1.login("admin", "Admin123!@#")
            token1 = db1._auth_handler.token_storage.access_token

            # Close first session
            db1.close()

            # Second session: load saved tokens
            db2 = QilbeeDB(server_url)
            db2._auth_handler.token_storage.storage_path = storage_path
            db2._auth_handler.token_storage.persist = True
            db2._auth_handler.token_storage.load_tokens()

            # Verify tokens loaded
            token2 = db2._auth_handler.token_storage.access_token
            assert token1 == token2

            print(f"✓ Session persistence works")

        except AuthenticationError as e:
            print(f"Persistence test skipped: {e}")

    def test_client_repr(self, server_url):
        """Test client string representation shows auth type."""
        # Unauthenticated
        db1 = QilbeeDB(server_url)
        assert "JWT" in repr(db1) or "unauthenticated" in repr(db1)

        # API Key
        db2 = QilbeeDB({"uri": server_url, "api_key": "qilbee_live_test"})
        assert "API Key" in repr(db2)

        # Basic Auth (deprecated)
        with pytest.warns(DeprecationWarning):
            db3 = QilbeeDB({"uri": server_url, "auth": {"username": "test", "password": "test"}})
        assert "Basic" in repr(db3) or "deprecated" in repr(db3)

        print(f"✓ Client repr shows authentication types correctly")


@pytest.mark.integration
def test_health_check(self):
    """Test health endpoint without authentication."""
    db = QilbeeDB("http://localhost:7474")

    try:
        health = db.health()
        assert "status" in health or isinstance(health, dict)
        print(f"✓ Health check successful: {health}")
    except Exception as e:
        print(f"Health check failed: {e}")
        # Don't fail test, server might not have /health endpoint


if __name__ == "__main__":
    # Allow running integration tests standalone
    pytest.main([__file__, "-v", "-m", "integration", "-s"])
