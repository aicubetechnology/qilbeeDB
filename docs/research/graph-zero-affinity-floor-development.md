# Zero-floor graph affinity: development result

Removing the positive affinity floor did not improve this development comparison.
The candidate's mean nDCG@10 decreased by 0.001495, with two query-level wins,
three losses and 25 ties. Judged recall was unchanged. The configuration was
rejected and is not a selectable public ranking profile.

## Comparison

Thirty previously observed MuSiQue development questions used one actual database
import containing 2,311 documents and 3,791 revision-bound relations. External
multilingual E5-small vectors had 384 dimensions. The original document-derived
graph was preserved; no added mutual-cosine relations were used. Every graph
request computed both scoring variants over the same admitted neighborhood and
embedding bindings in the same server snapshot. The ordinary response retained
`typed_path_strength_v1`; private server traces recorded the candidate's paths
and complete ranking. An unchanged hybrid-v1 arm also ran.

The baseline path multiplier is `0.5 + 0.5 * max(0, cosine)`, with 0.5 for a
missing usable embedding. The candidate uses `max(0, cosine)`, with zero for a
missing usable embedding. Both retain four anchors, hop decay 0.5, the original
relation weights, base contribution `0.5 / (2 + base_rank)` and graph contribution
`0.5 * strongest_path_strength`. No parameter sweep was performed. The change
can suppress an entire path through a missing embedding; it is not equivalent
to simply changing a final fusion weight.

| Method | Mean nDCG@10 | Mean judged Recall@10 |
| --- | ---: | ---: |
| Published strength profile | 0.616307 | 0.680556 |
| Private zero-floor candidate | 0.614812 | 0.680556 |

The fixed development rule rejected a regression in mean nDCG or judged recall.
The candidate failed that rule. This rejects this configuration on this corpus,
not every affinity-based method. No confidence interval or significance claim
was specified for this small screening result.

## Evidence and limitations

The server source began at `d6503f21934e4042006f114ac79f4ed8a917cac7`; private
instrumentation changed the candidate parameters and recorded both server-side
rankings without changing the published response. The
[machine-readable report](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/graph-affinity-development-report/benchmarks/retrieval/graph-zero-affinity-floor-development-report.json)
contains per-query rankings, metrics, exact input digests and the frozen protocol
digest. It supports inspection of the observations; it is not a packaged runner
for reproducing the private instrumentation.

The actual run completed 60 HTTP responses, plus import and current-source and
relation readback. History fences remained unchanged. Artifact checks verified
candidate membership, directed path endpoints, source and relation revisions,
anchors, missing-affinity declarations, path products and final server sorting.
An initial analysis rejected an omitted optional `episode_type` versus its
explicit null representation; normalizing exactly that default resolved the
analysis mismatch without modifying runtime evidence.

These questions are observed development data, not independent confirmation.
Judgments identify supporting documents; other documents are unjudged, so recall
is judged recall. Compare methods within this import: newly generated record IDs
can change ties across experiments. No latency, production-capacity or downstream
agent-task claim follows. Broader lifecycle/fault qualification and depth-zero
acceptance were not completed for this rejected candidate.

The separate [reserved graph comparison](twowiki-captured1536-reserved-results.md)
provides evidence on a different corpus and is not pooled with this screening
result. Existing public ranking profiles and defaults remain unchanged.
