# Secure a QilbeeDB container deployment

Use a supported QilbeeDB image, persistent storage and credentials with only the
permissions your application needs. A healthy process does not establish that an
installation is secure or that its backups can be restored.

## Choose and update an image

Pin production deployments to an immutable image digest. Record the version and
review its release notes before upgrading. Scan the exact image you intend to
run, including its operating-system packages, and assess findings against your
organization's security policy. Recheck images as vendor advisories change.

Do not treat passing API tests as proof that system libraries are patched. If a
finding affects your deployment, apply an available supported update or mitigation
and repeat your integration and recovery checks. Retain package metadata so your
scanner can identify installed dependencies.

The service runtime omits a shell and package manager. Update it by rebuilding or
selecting a supported image, rather than installing packages in a running
container. Removing utilities reduces dependencies but does not prove the
remaining components have no vulnerabilities. Keep external diagnostic and
backup tools in your security inventory too.

## Verify application dependencies

An operating-system package scan may not identify Rust dependencies compiled
into the server. For a self-hosted build, retain the exact source revision and
`Cargo.lock`, build with `--locked`, and examine both the image and its application
dependencies. Match findings to the target operating system and enabled features:
a dependency selected on Linux may differ from one selected on macOS.

Use a supported release containing the required fixes rather than replacing
libraries inside a running container. Validate the rebuilt artifact's
authentication, authorized reads and writes, and recovery behavior before
adoption. A clean lockfile scan alone does not prove that an image is safe.

Managed-platform customers should use the platform's supported release and
security-update guidance; they do not need access to its host or build tools.
Keep client credentials scoped and update self-managed clients as required by
their own dependency advisories.

## Limit runtime privileges

Run the server as a non-root user. Keep the root filesystem read-only, drop
unneeded Linux capabilities and disable privilege escalation. Store persistent
data on the volume mounted at `/data`; use temporary storage for `/tmp`.

Expose the API only to intended clients. Network-facing deployments need TLS,
authentication and scoped authorization. Keep operator keys outside images,
source control and application logs. Separate global administration from routine
application credentials, and revoke credentials that are no longer needed.

## Understand the health check

The native `qilbeedb health-check` command checks the local HTTP service at
`127.0.0.1:7474`. It requires a healthy response with the expected server version,
uses timeouts, bounds the response size and refuses redirects.

Use health checks for process availability. Verify authenticated operations and
storage recovery separately; a successful probe does not prove that a particular
credential can read or write its intended scope.

## Verify recovery before upgrading

Keep an application-consistent backup and test restoration to separate storage.
Check the target release's storage compatibility before opening existing data
with a new binary. Do not reopen an upgraded data directory with an older binary
unless that downgrade is explicitly supported.

After writes resume, restoring an earlier backup can discard newer changes.
Reconcile those writes before any rollback. See [backup and recovery](../operations/backup.md)
and the [release notes](../releases/0.14.0.md) for applicable limits.
