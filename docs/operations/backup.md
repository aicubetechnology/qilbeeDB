# Backup and recovery

Preserve a recoverable copy of the complete database and the deployment context
needed to open it. The current platform does not provide the previously described
`/admin/snapshot` or `/admin/snapshots` HTTP endpoints. Use a qualified operator
procedure for the deployed version.

## Create an offline backup

1. Coordinate the maintenance window and stop admitting new work. Account for
   every client and direct writer, including administrators and background jobs.
2. Stop the server and verify its exit status and logs. For versions containing
   the new lifecycle feature, follow [graceful shutdown](graceful-shutdown.md).
   A deadline or force-kill is not a successful drain.
3. Confirm no process can mutate the data volume. Copy or snapshot the complete
   data directory and its required deployment configuration. Preserve ownership,
   permissions and all database subdirectories. Do not copy selected live RocksDB
   files or assume that separate copies of active stores form one consistent set.
4. Record the deployed image digest, schema/storage versions, snapshot identity,
   capture time and integrity manifest. Keep the retained source unchanged.
5. Run [`qilbeedb verify-store`](store-verification.md) on the stopped source
   and on the copy with the same binary version, and keep both reports with the
   manifest. Matching family digests confirm a faithful copy; the learning
   store's knowledge index is checked against its ledgers.
6. Restore a copy to an isolated directory or volume and open it with the exact
   compatible binary. Verify authorized inventories, revisions, receipts and
   representative reads before considering the backup usable.

Cloud volume snapshot consistency and freeze/unfreeze handling belong to the
operator procedure. Snapshot completion alone does not verify restore semantics.
This guide does not introduce a cloud backup service or an online snapshot API.

## Restore or roll back

Restore into a separate location first. Verify the stopped copy with
`qilbeedb verify-store` before starting a server on it, then validate identity
and authorization, retained memory, graph state and dependent learning evidence. Confirm the intended
service version and actual data mounts before routing traffic to the restored
instance. Preserve the failed or newer data for reconciliation.

A backup from before an upgrade may omit later acknowledged writes. Restoring it
over the current directory can discard those writes; reconcile them before any
cutover. Never open a migrated storage format using an older incompatible binary.
Use the release's documented downgrade constraints and a verified pre-upgrade
copy rather than changing only the container image.

For an interrupted shutdown, inspect the recovered database and reconcile unknown
request outcomes using the original operation identities where supported. Do not
infer that all interrupted requests failed or that every returned response was
delivered to its client.

## Set and measure recovery objectives

The company owns retention, recovery-point and recovery-time objectives. Record
measured restore duration, the actual snapshot boundary, any unreconciled writes,
and validation failures. No fixed recovery-time or recovery-point guarantee is
established by the examples or by a successful health check.

Rehearse restoration with realistic data and permissions. Protect backup access
as database access, and include credentials/configuration needed for recovery in
the approved private operational storage. Keep deployment secrets out of source
repositories and user-facing evidence reports.
