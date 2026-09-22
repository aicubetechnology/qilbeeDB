"""Read-only cgroup v2 evidence for retrieval trials; unavailable is never zero."""

import json
import subprocess
import time

FILES = ("cpu.stat", "cpu.max", "memory.current", "memory.peak", "memory.max",
         "memory.events", "io.stat")


def nonnegative(value):
    number = int(value)
    if number < 0:
        raise ValueError("Negative resource counter")
    return number


def counters(raw):
    result = {}
    for line in raw.splitlines():
        key, value = line.split()
        if key in result:
            raise ValueError("Duplicate resource counter")
        result[key] = nonnegative(value)
    return result


def parse_metrics(raw):
    """Keep independently available metrics when optional controllers are absent."""
    parsed, unavailable = {}, []
    for name in FILES:
        try:
            value = raw[name]
            if value is None:
                raise ValueError("Unavailable controller")
            if name in ("cpu.stat", "memory.events"):
                parsed[name] = counters(value)
            elif name == "cpu.max":
                quota, period = value.split()
                period = nonnegative(period)
                if period == 0:
                    raise ValueError("Zero CPU period")
                parsed[name] = {"quota_usec": None if quota == "max" else nonnegative(quota),
                                "period_usec": period}
            elif name == "io.stat":
                devices = {}
                for line in value.splitlines():
                    device, *pairs = line.split()
                    major, minor = device.split(":")
                    nonnegative(major); nonnegative(minor)
                    if device in devices:
                        raise ValueError("Duplicate device")
                    devices[device] = counters("\n".join(pair.replace("=", " ", 1) for pair in pairs))
                parsed[name] = devices
            else:
                parsed[name] = None if name == "memory.max" and value.strip() == "max" else nonnegative(value.strip())
        except (KeyError, TypeError, ValueError):
            unavailable.append(name)
    return parsed, unavailable


def identity(container):
    try:
        result = subprocess.run(
            ["docker", "inspect", "--format", '{{json .Id}} {{json .State.StartedAt}}', container],
            capture_output=True, text=True, timeout=10, check=True,
        )
        decoder = json.JSONDecoder()
        identifier, end = decoder.raw_decode(result.stdout)
        timestamp = json.loads(result.stdout[end:].strip())
        if not identifier or not timestamp:
            raise ValueError("Missing container identity")
        return {"id": identifier, "started_at": timestamp}
    except (OSError, ValueError, subprocess.SubprocessError):
        return None


def resource_delta(before, after, field="cpu_usage_usec"):
    if not before or not after or not before.get("available") or not after.get("available"):
        return None
    if not before.get("container_identity") or before["container_identity"] != after.get("container_identity"):
        return None
    start, end = before.get(field), after.get(field)
    if type(start) is not int or type(end) is not int or end < start:
        return None
    return end - start


def container_resources(container):
    if not container:
        return {"available": False, "reason": "No container supplied"}
    started = time.monotonic_ns()
    initial_identity = identity(container)
    raw = {}
    for name in FILES:
        try:
            result = subprocess.run(
                ["docker", "exec", container, "cat", "/sys/fs/cgroup/" + name],
                capture_output=True, text=True, timeout=10, check=True,
            )
            raw[name] = result.stdout.strip()
        except (OSError, subprocess.SubprocessError):
            raw[name] = None
    metrics, unavailable = parse_metrics(raw)
    final_identity = identity(container)
    stable = initial_identity is not None and initial_identity == final_identity
    available = stable and "usage_usec" in metrics.get("cpu.stat", {})
    return {
        "available": available,
        "container_identity": final_identity,
        "stable_during_sampling": stable,
        "cpu_usage_usec": metrics.get("cpu.stat", {}).get("usage_usec"),
        "memory_current_bytes": metrics.get("memory.current"),
        "memory_peak_since_container_start_bytes": metrics.get("memory.peak"),
        "cgroup_v2": metrics,
        "unavailable_metrics": unavailable,
        "sampling_elapsed_ns": time.monotonic_ns() - started,
        "scope": "Entire container; samples are sequential, not atomic. Empty io.stat is not proof of zero physical disk activity.",
    }
