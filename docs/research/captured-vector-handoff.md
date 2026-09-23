# Verify a captured-vector evaluation handoff

**Availability: source preview; offline evaluation tooling.** Use a frozen capture
when an external embedding provider does not expose an immutable weight revision.
The snapshot identifies the exact supplied inputs and vectors. It does not prove
that the provider used unchanged weights or can regenerate identical values.
Embedding generation and its credentials remain the application's responsibility.

## Pin evidence before evaluation

Retain the source corpus, original captured fixture, evaluation fixture, generation
plan, per-batch dispatch and response records, input/vector manifest, measurements
and handoff audit. Obtain the expected canonical fixture and manifest SHA-256
values from the agreed handoff. Do not recompute expected pins from an untrusted
replacement and treat the match as independent verification.

The manifest binds every document and query to its source identifier, exact UTF-8
text hash, vector JSON hash, expected float32 little-endian hash, batch position,
and position within its document or query group. Canonical JSON uses sorted keys,
compact separators, UTF-8, literal Unicode and no non-finite numbers or trailing
newline. The revision is `captured-` followed by the first 24 hexadecimal characters
of the manifest digest; retain the full digest for evidence verification.

For the supported capture format, request hashes cover the canonical object with
`model`, `dimensions`, `encoding_format: "float"`, and the ordered `input` array.
Response records contain parsed JSON. These hashes do not represent raw HTTP
bytes or provider-signed attestations.

```bash
python3 scripts/verify_captured_vectors.py \
  --directory /path/to/capture-bundle \
  --source /path/to/frozen-source.json \
  --fixture-sha256 "$EXPECTED_FIXTURE_SHA256" \
  --manifest-sha256 "$EXPECTED_MANIFEST_SHA256" \
  --output /path/to/new-verification-receipt.json
```

The verifier runs without a provider connection. It checks complete ordered source
coverage, unchanged source fields and vectors, model identity, finite nonzero
vectors, response indices, request hashes, dispatch chronology, successful HTTP
status and receipt summaries. It reconstructs the manifest from the preserved
records, including float32 conversion hashes. Token usage is checked per batch;
it is not divided among individual inputs. Unknown monetary cost remains `null`,
not zero. This format currently accepts complete successful captures with known
batch token usage and uncalculated monetary cost; other capture formats need an
explicit adapter and validation before use.

## Failure and recovery

Missing or inconsistent evidence stops verification. No provider call, retry,
vector repair, normalization or database import occurs. The output is created
exclusively after successful verification; an existing output is not overwritten.
Preserve failed evidence and resolve the discrepancy with its producer. An
uncertain generation outcome must not trigger automatic paid regeneration.

## Limits and next steps

A successful receipt is an offline integrity result. It does not authenticate the
producer's account, prove provider execution, establish retrieval quality or
verify database storage. The expected float32 hashes must still be compared with
actual database reads after import. Keep `float32_database_roundtrip_verified`
false until that separate check succeeds; do not rewrite the frozen manifest to
record later results, because doing so changes the captured identity.

Use captured vectors only in the isolated evaluation corpus. Do not assign this
revision to subsequently generated queries or mix newly generated vectors into
the frozen space. Provider weight revision remains unknown. This exception does
not relax the production model-space compatibility contract.

Freeze a new development protocol for the captured fixture before measuring
retrieval. Preserve query roles, graph construction, ranking versions and work
budgets; keep reserved results unopened until the development gate passes. A new
embedding fixture cannot inherit confirmation from a different embedding model
or dimension count. Record preparation costs separately from retrieval costs and
state any amortization rule before interpreting downstream savings.

## Verify persisted document values

**Availability: source preview; operator-owned offline laboratory only.** The
HTTP embedding receipt does not return the complete vector. To verify actual
stored values, first complete the handoff verification above, import documents
through the authenticated HTTP API using the frozen fixture, and preserve its
preparation state. Stop the laboratory server and wait for its process to exit.
Do not run this procedure against a live platform database.

```bash
cargo run --locked -p qilbee-memory --example verify_fixture_vectors -- \
  /path/to/closed-laboratory/agent-memory \
  /path/to/preparation-state.json \
  /path/to/vector-input-manifest.json \
  /path/to/new-readback-receipt.json
```

The example opens existing RocksDB column families read-only. It uses the
preparation state's company, scope and private subject to reconstruct storage
keys; this is an operator filesystem tool, not an authenticated application API.
Run it only with authorized access to the laboratory files. Its format is coupled
to the current storage implementation, not a stable external storage contract.

Every document must have a distinct stored record and a matching manifest entry.
The audit checks stored schema, namespace, record revision, complete embedding
receipt, model identity, dimensions and the storage vector digest. It decodes the
stored values as float32 and compares the hash of their little-endian bytes with
the manifest. It writes a separate receipt only after all documents pass and
refuses to overwrite an existing output. Missing records, changed expected
values, wrong private subjects or invalid vectors fail without creating a
successful receipt. Preserve the original manifest and the successful handoff
receipt; the readback tool does not replace those independent input pins.

A successful result verifies persisted document vectors after storage reopen.
It does not persist query vectors, execute retrieval, confirm current memory
eligibility, prove crash recovery or establish host power-loss durability.
Validate those guarantees separately. Keep the original capture's roundtrip flag
unchanged and associate the new readback receipt with the pinned fixture instead.
