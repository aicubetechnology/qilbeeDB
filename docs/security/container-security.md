# Container security qualification

An image release needs both functional qualification and a current assessment of
its runtime dependencies. Pin deployments to an immutable image digest and retain
the Git revision, package inventory, scan timestamp and acceptance evidence.

## Runtime update for the first AWS deployment

The initial 0.11.0 candidate passed its API and recovery tests, but Amazon ECR's
scan reported four critical and fourteen high findings in its Debian Bookworm
runtime source packages. Updating the Bookworm package index still offered
OpenSSL `3.0.20-1~deb12u2` and Perl `5.36.0-7+deb12u3`.

Debian's security tracker identifies fixed Trixie packages for
[CVE-2026-75803](https://security-tracker.debian.org/tracker/CVE-2026-75803),
[CVE-2026-13221](https://security-tracker.debian.org/tracker/CVE-2026-13221),
[CVE-2026-57433](https://security-tracker.debian.org/tracker/CVE-2026-57433)
and [CVE-2026-12087](https://security-tracker.debian.org/tracker/CVE-2026-12087).
The runtime therefore moves to the official Debian Trixie image, pinned at
`sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a`,
and applies available package upgrades during the build.

The Rust build stage remains pinned to Rust 1.93.1 on Bookworm. The server binary
must run correctly with the newer runtime libraries; that compatibility is
qualified through the complete disposable-container acceptance suite, including
credential lifecycle and crash recovery. The update does not change API scores,
retrieval defaults or embedding generation.

## Built-in health probe

The container no longer installs curl or its optional protocol libraries solely for health checks. `qilbeedb health-check` connects only to `127.0.0.1:7474`, bounds its response to 8 KiB, uses socket timeouts, refuses redirects and requires the current server version with a healthy status. Docker applies a three-second overall probe timeout. The probe validates process health, not scoped write readiness or storage recovery.

The first Trixie scan also flagged curl. Removing this unused general-purpose client reduces the runtime dependency surface; package metadata remains intact for scanning.

## Release evidence

Record the candidate's exact image digest and package versions after building.
Run the integration and recovery tests against that image, then review its scan
before deploying it to a customer-facing endpoint. An older qualified image must
not silently substitute for the candidate if a build or scan fails.

Scanners can report source-package findings for optional components or versions
that are not reachable through this service. Investigate such findings using the
maintainer's advisory, the installed binary package inventory and the actual
runtime configuration. Keep the finding and assessment visible; do not remove
package metadata or suppress a finding merely to obtain a clean dashboard.

This process does not establish that the software has no vulnerabilities. A
current scan is a time-bounded observation, and functional acceptance is separate
from independent penetration testing. Reassess future releases and rebuild when
maintainer security updates become available.
