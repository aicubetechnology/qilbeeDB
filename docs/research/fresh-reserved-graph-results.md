# Fresh reserved graph retrieval results

The optional `typed_path_base_preserving_v1` profile did not demonstrate a
relevance improvement over hybrid v2 in this reserved comparison. Its mean
nDCG@10 difference is -0.000038, with exploratory 95% paired-query bootstrap
interval [-0.005216, +0.005141]: 20 wins, 20 losses and 160 ties. The profile
remains experimental and is not admitted as the default.

## Frozen comparison

The [protocol](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/fresh-graph-reserved-evaluation/benchmarks/retrieval/graph-base-preserving-reserved-v1.json),
[selection manifest](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/fresh-graph-reserved-evaluation/benchmarks/retrieval/musique-fresh-reserved-selection-v1.json)
and [verified report](https://github.com/aicubetechnology/qilbeeDB/blob/aicube/fresh-graph-reserved-evaluation/benchmarks/retrieval/musique-fresh-reserved-report.json)
cover 200 MuSiQue-Ans public development questions, 3,488 documents and 5,912
document-derived relations. External multilingual E5-small embeddings have 384
dimensions. Nine methods ran three repetitions, producing 5,400 measured HTTP
requests and 1,800 query/method rows without reported failures. Thirty previous
training questions remain in the fixture for warmup; they are not measured here.

The native optimized engine was frozen at
`f47d6fe9d021a3bdf10df175e34e889eb18cf77c`; evaluation tools were frozen at
`f3cc41383b31def5cc5be9d493581333855c21bf`. Later engine optimizations were not
substituted during the run. All methods used the same database import, revisions,
query vectors, response limit, scope and declared budgets. Current sources,
relations, history fences, score/path contracts, seed equivalence and the full
measurement matrix passed verification. The frozen exporter independently
recomputed metrics and aggregates. An additional measurement-integrity check
was applied after completion; it was not part of the frozen runner.

## Results

| Method | nDCG@10 | Judged Recall@10 | All supports | No labeled support | Retrieval p95 (ms) |
| --- | ---: | ---: | ---: | ---: | ---: |
| BM25 | 0.4913 | 0.5221 | 33/200 | 14/200 | 84.05 |
| Exact cosine | 0.5083 | 0.5254 | 39/200 | 13/200 | 227.05 |
| Hybrid v1 | 0.5488 | 0.5854 | 43/200 | 5/200 | 280.17 |
| Hybrid v2 | 0.5436 | 0.5854 | 48/200 | 5/200 | 274.39 |
| Balanced graph, hybrid seeds | 0.5389 | 0.5713 | 59/200 | 21/200 | 333.17 |
| Entity graph, hybrid seeds | 0.5376 | 0.5700 | 59/200 | 21/200 | 364.05 |
| Balanced graph, lexical seeds | 0.5011 | 0.5425 | 48/200 | 25/200 | 138.31 |
| Hybrid seeds, depth zero | 0.5436 | 0.5854 | 48/200 | 5/200 | 277.17 |
| Base-preserving graph | 0.5436 | 0.5833 | 49/200 | 6/200 | 350.39 |

The primary recall difference against hybrid v2 is -0.002083, with exploratory
interval [-0.014583, +0.010417]. Complete-support retrieval improves by one
question, while the number of questions without a labeled support increases
from five to six. Against hybrid v1, the candidate's nDCG difference is -0.005211
with interval [-0.022464, +0.012684]. Neither comparison establishes superiority.

The balanced graph profile retrieves all supports more often but also returns
no labeled support in 21 cases, versus five for hybrid v2. This tradeoff must not
be concealed by a single average. Depth-zero results preserve hybrid-v2 ordering.

## Regressions and coverage

The candidate's largest nDCG losses against hybrid v2 include
`2hop__38828_89399` and `2hop__17827_54024` (each -0.177239), followed by
`3hop1__68981_91191_156667` (-0.135652). The largest gain is
`2hop__6744_6731` (+0.193426). These now serve as regression cases, not unseen
queries for further tuning. Category averages and all rankings are in the report.

Candidate nDCG falls slightly in two-hop, `3hop2` and `4hop3` categories and rises
slightly in `3hop1` and `4hop1`. No uniform gain is demonstrated. The candidate
and balanced hybrid graph report traversal cuts on 192/200 questions; all hybrid
methods report candidate cuts. A completed source scan is not exhaustive ranking
or graph exploration. These budgets describe the experiment, not tenant policy.

## Post-publication diagnosis and next hypothesis

A separate, post-hoc comparison of the recorded top-ten lists finds membership
changes on 83/200 queries: 101 query-document entries enter and 101 leave.
Six entering entries have labeled support and seven departing entries have
labeled support. Unjudged entries are not established irrelevant. Examination
of the retained path evidence shows that all 101 promoted entries already have
a base-channel contribution; 61 use a one-hop proof and 40 a two-hop proof.
Thus, in this run, top-ten turnover promotes lower-ranked base candidates rather
than discovering results outside the retained base channel. This is diagnostic
attribution, not a new held-out evaluation or a proof of the cause of every loss.

A label-aware reachability audit finds 114 support entries missing from the
hybrid-v2 top ten but reachable within two hops, of which six were recovered.
This ignores traversal budgets and is an oracle diagnostic, not an achievable
retrieval estimate. A replay restricted to this fully eligible fixture matches
all 200 observed traversal work counters, stop reasons and candidate counts.
Of the 108 unrecovered reachable entries, 86 occur in the bounded neighborhood
and 22 do not. More exploration alone therefore cannot explain or correct most
of these misses; the replay does not establish general storage/security or
scoring equivalence.

There is also a structural limitation of this profile. With four anchors, any
graph-only result has graph rank at least five: hop decay bounds a non-anchor's
strength below the first three anchors and at most equal to the fourth, whose
zero-hop path wins the tie. Its fusion contribution is therefore at most
`0.25 / (2 + 5) = 1/28`. If ten base candidates exist, each of their base
contributions is at least `0.75 / (2 + 10) = 1/16`, even before graph evidence.
Thus a graph-only result cannot enter the top ten under these conditions.
Revising exploration without revising this fusion constraint cannot enable that
form of graph discovery. This argument is specific to this profile and limit;
it does not apply to every graph profile or sparse base result set.

[MAGMA](https://arxiv.org/html/2601.03236v2) motivates a distinct hypothesis:
query-conditioned traversal can prioritize useful relational paths instead of
only changing final fusion weights. QilbeeDB's current implementation selects a
bounded neighborhood before affinity-based path scoring. Changing that ordering
would require a new, immutable server policy with explicit work accounting and
an independent evaluation. The paper's results do not validate that change in
QilbeeDB. Query-intent extraction and business interpretation remain application
responsibilities; a shared extension requires an explicit ownership agreement.

Do not select new weights or report new confirmation on this already observed
cohort. Use it for regression analysis, qualify candidate policies on development,
then freeze a new comparison and assess downstream effects separately.

## Cost and evidence limits

Observed candidate retrieval p95 is 350.39 ms versus hybrid v2's 274.39 ms,
about 28% higher. This shared ARM64 host also ran compilation and container
qualification during portions of the campaign. The shuffled schedule reduces
simple order effects but does not remove contention; these timings are neither
production capacity estimates nor a controlled hardware speed comparison.
External embedding-plus-HTTP values reuse measured embedding generation times;
they are estimates, not simultaneous end-to-end observations. One document was
truncated at the encoder's 512-token limit; no query was truncated.

The explicit selection excludes previously used query IDs, normalized question
texts and component IDs. It does not prove independence within the new cohort:
82 component IDs recur across reserved queries, forming 110 connected query
groups, with a largest group of 14. The frozen interval resamples individual
queries, not these groups. Its uncertainty is exploratory and may understate
dependence; no multiplicity correction or confirmatory superiority claim is made.

The corpus is public development data, not a hidden test or proof of exclusion
from model training. Judgments label original supporting paragraphs; other
union-corpus documents are unjudged, so recall is judged recall. The selection
importer verifies explicit frozen IDs and disjointness but does not replay the
original hash-selection procedure. New imports can change UUID tie-breaks;
compare methods within the same frozen import. These results are separate from
previous development results and do not demonstrate agent-task gains, token
savings or continual-learning improvements.

Future ranking choices require development evidence and a new frozen comparison.
The current reserved cohort becomes regression data after publication. Downstream
agent evaluation retains a separate model, task, tool, cost and acceptance protocol.
