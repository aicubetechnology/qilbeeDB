"""Small stdlib transport: no redirect, proxy, credential persistence or automatic retry."""

import http.client
import json
import math
import re
from urllib.parse import urlsplit
from ._consumer_wire import ConsumerAPIError, ConsumerProtocolError, ConsumerTransportError, require


def _unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate_response_field")
        result[key] = value
    return result


def _invalid_constant(_):
    raise ConsumerProtocolError("nonfinite_response_number")


def _bounded_integer(value):
    require(len(value.lstrip("-")) <= 20, "response_integer_out_of_range")
    result = int(value)
    require(-(2**63) <= result <= 2**64 - 1, "response_integer_out_of_range")
    return result


def _finite_float(value):
    result = float(value)
    require(math.isfinite(result), "nonfinite_response_number")
    return result


class ConsumerHTTP:
    def __init__(self, base_url, timeout, max_response_bytes):
        url = urlsplit(base_url)
        if (
            url.scheme not in ("http", "https")
            or not url.hostname
            or url.username is not None
            or url.password is not None
            or url.query
            or url.fragment
        ):
            raise ValueError(
                "Expected an HTTP(S) server URL without credentials, query or fragment"
            )
        if (
            not isinstance(timeout, (int, float))
            or isinstance(timeout, bool)
            or not math.isfinite(timeout)
            or timeout <= 0
        ):
            raise ValueError("timeout must be a positive finite socket timeout")
        if (
            type(max_response_bytes) is not int
            or not 1024 <= max_response_bytes <= 64 * 1024 * 1024
        ):
            raise ValueError("max_response_bytes must be between 1024 and 67108864")
        self._url = url
        self._timeout = timeout
        self._max_bytes = max_response_bytes

    def request(self, token, path, body=None, *, mutation=False):
        connection_type = (
            http.client.HTTPSConnection
            if self._url.scheme == "https"
            else http.client.HTTPConnection
        )
        connection = connection_type(self._url.hostname, self._url.port, timeout=self._timeout)
        uncertain = "unknown" if mutation else "not_attempted"
        try:
            data = (
                None
                if body is None
                else json.dumps(body, allow_nan=False, separators=(",", ":")).encode("utf-8")
            )
            require(data is None or len(data) <= 65536, "request_too_large")
            connection.request(
                "GET" if body is None else "POST",
                self._url.path.rstrip("/") + path,
                body=data,
                headers={
                    "Authorization": "Bearer " + token,
                    "Accept": "application/json",
                    "Content-Type": "application/json",
                    "Accept-Encoding": "identity",
                },
            )
            response = connection.getresponse()
            if 300 <= response.status < 400:
                raise ConsumerProtocolError(
                    "redirect_rejected", status=response.status, checkpoint_outcome=uncertain
                )
            headers = response.getheaders()
            for name in (
                "content-type",
                "content-length",
                "content-encoding",
                "cache-control",
                "transfer-encoding",
            ):
                require(
                    sum(k.lower() == name for k, _ in headers) <= 1, "duplicate_response_header"
                )
            require(
                response.getheader("Content-Encoding", "identity").lower() == "identity",
                "encoded_response_rejected",
            )
            require(
                response.getheader("Content-Type", "").split(";")[0].strip().lower()
                == "application/json",
                "non_json_response",
            )
            require(
                response.getheader("Cache-Control") == "no-store", "cacheable_response_rejected"
            )
            length = response.getheader("Content-Length")
            if length is not None:
                require(
                    length.isascii() and length.isdigit() and int(length) <= self._max_bytes,
                    "response_too_large",
                )
                require(
                    response.getheader("Transfer-Encoding") is None, "ambiguous_response_length"
                )
            raw = response.read(self._max_bytes + 1)
            require(len(raw) <= self._max_bytes, "response_too_large")
            if length is not None:
                require(len(raw) == int(length), "truncated_response")
            try:
                value = json.loads(
                    raw.decode("utf-8"),
                    object_pairs_hook=_unique_object,
                    parse_constant=_invalid_constant,
                    parse_int=_bounded_integer,
                    parse_float=_finite_float,
                )
            except (ValueError, UnicodeError, RecursionError):
                raise ConsumerProtocolError("invalid_json_response") from None
            if response.status != 200:
                require(
                    isinstance(value, dict)
                    and type(value.get("contract_version")) is int
                    and value["contract_version"] == 1
                )
                error = value.get("error")
                require(isinstance(error, dict) and isinstance(error.get("code"), str))
                require(re.fullmatch(r"[a-z][a-z0-9_]{0,127}", error["code"]) is not None)
                outcome = (
                    ("rejected" if 400 <= response.status < 500 else "unknown")
                    if mutation
                    else "not_attempted"
                )
                raise ConsumerAPIError(
                    error["code"], status=response.status, checkpoint_outcome=outcome
                )
            return value
        except ConsumerProtocolError as error:
            error.checkpoint_outcome = uncertain
            raise
        except (OSError, http.client.HTTPException) as error:
            raise ConsumerTransportError(
                "transport_failure", checkpoint_outcome=uncertain
            ) from None
        finally:
            connection.close()
