# Generate evaluation embeddings outside the database

Use an external process to turn a frozen text corpus into a model-bound retrieval
fixture. QilbeeDB accepts the vectors and their full identity through the memory
API. It does not download models, run inference or receive provider credentials.
Applications may use any compatible embedding provider; this optional reference
pipeline is one reproducible evaluation example, not a required client dependency.

## Reference encoder

The example uses Microsoft's [multilingual E5 research](https://arxiv.org/abs/2402.05672)
and the model publisher's [multilingual-e5-small artifacts](https://huggingface.co/intfloat/multilingual-e5-small/tree/614241f622f53c4eeff9890bdc4f31cfecc418b3).
The upstream model is MIT licensed. The script pins the original FP32 ONNX graph
and tokenizer by repository commit and SHA-256, then runs
[ONNX Runtime](https://onnxruntime.ai/docs/api/python/api_summary.html) on CPU in a
separate process. No hosted inference service or paid provider is called.

| Property | Reference setting |
| --- | --- |
| Model | `intfloat/multilingual-e5-small` |
| Repository commit | `614241f622f53c4eeff9890bdc4f31cfecc418b3` |
| Dimensions | 384 |
| Document / query prefixes | `passage: ` / `query: `, including the trailing space |
| Pooling | Attention-mask mean over token embeddings, then float32 L2 normalization |
| Input length | At most 512 tokens including prefix and special tokens; default rejects longer input; explicit `--overlength truncate` records right truncation |
| Runtime | CPU provider, two intra-operation threads, one inter-operation thread, full graph optimization |
| Database model-space revision | Repository commit plus a SHA-256 of artifacts, preprocessing and runtime identity |

The full pipeline identity is recorded in `embedding_provenance`. A different
artifact, tokenizer, runtime version or preprocessing policy produces a different
space revision. Never assume embeddings from another export or provider are
interchangeable because the base model name is the same. Exact vector reuse is the
reproducibility boundary; independently generated floating-point vectors can vary.

## Prepare a fixture

Use Python 3.12 or later. Create an isolated environment and install the optional dependencies from
`benchmarks/retrieval/embedding-requirements.txt`. They are not installed in the
server image or imported by the standard-library retrieval evaluator.

Download `onnx/model.onnx` and `onnx/tokenizer.json` from the pinned upstream
commit into a local directory, named `model.onnx` and `tokenizer.json`. The combined
files occupy about 487 MB. The generation script performs no downloads and checks
both hashes before opening an inference session:

```text
model.onnx     ca456c06b3a9505ddfd9131408916dd79290368331e7d76bb621f1cba6bc8665
tokenizer.json 0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39
```

```bash
python3 -m venv /secure/path/embedding-venv
/secure/path/embedding-venv/bin/pip install \
  -r benchmarks/retrieval/embedding-requirements.txt
/secure/path/embedding-venv/bin/python scripts/build_embedding_fixture.py \
  --source benchmarks/retrieval/memory-text-v1.json \
  --model-dir /secure/path/e5-artifacts \
  --output /secure/path/e5-fixture.json \
  --measurements /secure/path/e5-generation.json
```

Use distinct new output paths. Existing files are not overwritten. The source
contains fictional memories, explicit 0–3 judgments, categories and development/test
membership. Generation preserves those fields and validates every output vector.
It does not inspect relevance grades to generate embeddings. For longer sources,
define and freeze a chunking policy and its judgments before running this example;
it will not silently truncate text and retain misleading labels.

For a fixed document-level benchmark, `--overlength truncate` explicitly selects
right truncation to 512 tokens, including special tokens. Full source text remains
in the corpus for lexical retrieval; only the embedding input is shortened. The
policy changes the model-space revision. Generation evidence records original and
retained token counts and a truncation flag for every input. Report truncated-source
counts and this lexical/dense representation difference with any relevance result.

The bundled `benchmarks/retrieval/e5-memory-fixture.json` freezes actual vectors
from this pipeline. It derives from the already exposed synthetic contract corpus.
Its test queries are **previously inspected**, so results are diagnostic regression
evidence, not new held-out relevance qualification. A real encoder does not make a
small authored corpus representative or independently judged.

## Keep generation and retrieval measurements separate

The generation JSON records each input's tokens and elapsed time for tokenization,
CPU inference, pooling and normalization. It uses serial, single-input requests;
no initial inference is discarded as warmup. Asset download, verification, session
loading and output writes are excluded. Timing is kept outside the frozen fixture
so different runtime measurements do not change the corpus identity.

Run the [retrieval evaluator](retrieval-evaluation.md) against the generated
fixture, using the same vectors for cosine and hybrid requests. Pin a development
plan before choosing ranking parameters. Retain generation evidence alongside the
retrieval report, matched by the canonical fixture hash. Retrieval and HTTP latency
do not include embedding generation. Summing separate percentile values is not a
measured end-to-end latency distribution; measure a complete client request cycle
separately when making that claim. Local hardware and energy costs remain unknown,
even when there is no hosted API charge. No agent-task outcome is inferred.
