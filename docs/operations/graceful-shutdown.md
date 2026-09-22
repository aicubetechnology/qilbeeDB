# Stop the server without losing track of accepted work

Status: **available in the released 0.14.0 image**, from revision
`06a77efe8b7be8c5940b4c28d918dda72d23f40d`. Earlier 0.13 builds and preliminary
0.14 candidates do not acquire these guarantees from their version string alone.
Pin the qualified image digest when deploying.

## Shutdown sequence

On Unix, SIGINT and SIGTERM initiate the same shutdown sequence. Other supported
platforms use the Ctrl+C signal. The server closes HTTP admission, waits for
accepted connections and requests to finish, waits for tracked blocking work,
and then flushes the graph database. Only a completed sequence reports successful
stop. Platform memory and identity operations retain their synchronous write
contracts; this feature does not replace their persistence rules.

A disconnected client does not necessarily cancel a database operation. Blocking
work carries its own lifetime ticket, including authentication, administration,
platform memory operations and the legacy HTTP memory adapter. It remains tracked
when the request awaiting its result is cancelled. A successful stop means that
tracked work finished; it does not mean every client received its response.

HTTP/1 keep-alive does not admit a subsequent request after graceful connection
shutdown. New connections are refused once the listener closes. There is no new
administrative shutdown endpoint or application error envelope.

## Configure the supervisor deliberately

The server does not silently abort work after an internal timeout. Configure the
container or service manager's stop deadline for your workload and operational
policy. A slow client or operation can prolong drainage. If the supervisor forces
termination, treat the result as an interrupted shutdown, not a clean stop.

For Docker, set `QILBEE_STOP_TIMEOUT_SECONDS` to the operator-approved deadline
before running:

```bash
docker stop --timeout "${QILBEE_STOP_TIMEOUT_SECONDS:?Set the approved stop deadline}" qilbeedb
docker inspect --format '{{.State.Status}} {{.State.ExitCode}} {{.State.OOMKilled}}' qilbeedb
```

Inspect the exact deployed image, process exit status and shutdown logs. A
successful Docker stop command by itself does not establish that the application
drained successfully. The application's successful-stop message follows drainage
and flush; failed HTTP tasks, panicking request/worker tasks and flush failures
must not produce that message. Normal operation errors returned to a client are
not equivalent to a server-task panic. Signal-listener or shutdown failure causes
the server entry point to exit unsuccessfully.

Before a consistent offline backup or migration, confirm the process has exited,
no other process can write the volume, and the operator's admission controls
remain in effect. A momentarily idle worker count or a healthy response before
shutdown does not establish quiescence. Storage background work and callers using
an embedded database directly are outside the HTTP admission boundary.

## Reconcile uncertain writes

Connection closure, timeout and refusal cannot prove whether an earlier accepted
write committed. Retain its operation identity, authorized scope, expected
revision and original payload. Inspect or replay the exact operation only where
that endpoint defines an idempotent contract. A changed payload under the same
identity must remain a conflict; creating a new identity can duplicate effects.

After forced termination, use the existing recovery procedure and the compatible
binary. Do not restore a previous backup over acknowledged later writes without
reconciliation. This feature does not add a universal retry contract to endpoints
that do not already have one.

## Embedded server lifecycle

`Server::stop()` waits for completion. Cancelling the task awaiting it preserves
the draining lifecycle; another `stop()` call can continue waiting. `start()`
rejects attempts to restart while drainage or a failed lifecycle remains.
A flush failure leaves admission closed and allows a subsequent stop to retry the
flush. A failed HTTP task or panicked tracked work prevents a clean-stop result;
operator recovery is required. Dropping a server value is not equivalent to
awaiting successful shutdown. These guarantees belong to `Server`; serving a
returned router independently with `axum::serve` requires the caller to implement
its own lifecycle and worker tracking.

Maintainers must route blocking handler work through the shared tracking wrapper.
Detached tasks started outside that wrapper are not covered by its accounting.
Adding a new background execution mechanism therefore requires lifecycle design
and tests; it must not rely on request cancellation to stop a writer.

## Validation boundaries

Real TCP tests cover a blocked writer, admission closure, a subsequent HTTP/1
request on an existing connection, client disconnection, cancelled stop waiters,
restart prevention and data reopen. A controlled flush failure and HTTP-task
failure verify unsuccessful stop behavior. Worker tests cover cancellation and
panic. Child processes exercise SIGINT and SIGTERM. A real authorized memory
command is also tested with its committed response held back, termination during
drainage, and replay after restart using the original idempotency key.

The workspace suite passes, including HTTP response-schema checks. These tests
do not establish production
rollout readiness, every supervisor configuration, host power-loss resilience or
HTTP/2 interoperability. A production window and recovery checks remain separate.
