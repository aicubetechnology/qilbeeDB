#!/usr/bin/env python3
"""Measure exact retrieval coverage under controlled update/delete history in an isolated scope.

Vectors and documents are synthetic capacity fixtures, not relevance judgments.
Credentials are read from files and never copied into reports.
"""
import argparse
import concurrent.futures
import hashlib
import json
import math
import os
import statistics
import subprocess
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path


class HistoryBenchmark:
    def __init__(self, url, token, scope, output, documents=5183, samples=20, workers=4, resource_container=None):
        self.url, self.token, self.scope = url.rstrip("/"), token, scope
        self.output, self.documents, self.samples, self.workers = Path(output), documents, samples, workers
        self.vector = [round(math.sin(n * 0.137 + 0.3), 7) for n in range(1536)]
        self.space = dict(provider="synthetic", model="history-coverage", revision="v1", dimensions=1536)
        self.rows, self.slots, self.results = {}, {}, []
        self.resource_container = resource_container
        self.intent_lock = threading.Lock()

    def request(self, route, body=None):
        raw = json.dumps(body, separators=(",", ":")).encode() if body is not None else None
        request = urllib.request.Request(self.url + route, data=raw, headers={
            "Authorization": "Bearer " + self.token, "Content-Type": "application/json"})
        if route in ["/api/v1/memory/commands", "/api/v1/memory/embeddings"]:
            # Keep exact commands for reconciliation; never store bearer credentials.
            intent = json.dumps(dict(route=route, body=body), separators=(",", ":")) + "\n"
            with self.intent_lock:
                with open(self.output / "write-intents.jsonl", "a", opener=lambda path,flags: os.open(path,flags,0o600)) as log:
                    log.write(intent); log.flush(); os.fsync(log.fileno())
        started = time.perf_counter()
        with urllib.request.urlopen(request, timeout=120) as response:
            wire = response.read()
            return json.loads(wire), dict(response.headers), (time.perf_counter() - started) * 1000, len(wire)

    def resources(self):
        if self.resource_container is None:
            return None
        if not self.resource_container.startswith("qilbeedb-history-"):
            raise ValueError("Resource collection requires an isolated qilbeedb-history-* container")
        lines = subprocess.check_output(["docker", "exec", self.resource_container, "sh", "-c",
            "getconf CLK_TCK; cat /proc/1/stat /proc/1/status"], text=True, timeout=10).splitlines()
        ticks = int(lines[0]); process = lines[1].rsplit(")", 1)[1].split()
        status = dict(line.split(":", 1) for line in lines[2:] if ":" in line)
        return dict(process_cpu_seconds=(int(process[11])+int(process[12]))/ticks,
                    rss_kib=int(status["VmRSS"].split()[0]),
                    lifetime_peak_rss_kib=int(status["VmHWM"].split()[0]))

    def record(self, number, cycle, tag="corpus"):
        return dict(episode_type="Observation", event_time_millis=1700000000000 + cycle,
                    content=dict(primary=f"historycoverage corpus document {number} revision {cycle}. " +
                                 "Synthetic source text for coverage and read-work measurement. " * 12),
                    tags=[tag], metadata={"fixture": "history-coverage-v1", "slot": number})

    def command(self, key, operation):
        return self.request("/api/v1/memory/commands", dict(contract_version=1, scope=self.scope,
                            idempotency_key=key, operation=operation))[0]["receipt"]

    def create(self, number, cycle, tag="corpus"):
        receipt = self.command(f"create-{cycle}-{tag}-{number}", dict(type="create", record=self.record(number, cycle, tag)))
        return receipt["record_id"], dict(slot=number, revision=receipt["revision"], embedding_revision=None,
                                          live=True, tag=tag, born_cycle=cycle)

    def embed(self, identifier, row, cycle):
        self.request("/api/v1/memory/embeddings", dict(contract_version=1, scope=self.scope,
                     idempotency_key=f"embedding-{identifier}-{row['revision']}-{cycle}", record_id=identifier,
                     record_revision=row["revision"], space=self.space, vector=self.vector))
        row["embedding_revision"] = row["revision"]
        return identifier, row

    def parallel(self, function, values):
        with concurrent.futures.ThreadPoolExecutor(max_workers=self.workers) as pool:
            return list(pool.map(function, values))

    def save(self, name):
        state = dict(schema_version=1, scope=self.scope, space=self.space,
                     vector=self.vector, rows=self.rows, slots=self.slots, results=self.results)
        (self.output / (name + "-state.json")).write_text(json.dumps(state, indent=2) + "\n")
        (self.output / "report.json").write_text(json.dumps(dict(schema_version=1,
            fixture="synthetic-history-coverage-v1", relevance_evidence=False,
            documents=self.documents, dimensions=1536, candidate_budget=10000,
            lexical_hybrid_byte_budget=134217728, result_limit=10, request_concurrency=1,
            mutation_concurrency=self.workers, warmup_requests=2, measured_requests=self.samples,
            timing="Server retrieval time; HTTP separately. External embedding generation excluded.",
            cache="Warm repeated queries; no cache eviction. Other workstation activity is not controlled.",
            vector_sha256=hashlib.sha256(json.dumps(self.vector).encode()).hexdigest(),
            results=self.results), indent=2) + "\n")

    def measure(self, label):
        health = self.request("/health")[0]
        stage = dict(stage=label, server_version=health["version"],
                     current_corpus=sum(r["live"] and r["tag"] == "corpus" for r in self.rows.values()),
                     tombstones=sum(not r["live"] for r in self.rows.values()),
                     outside_tag=sum(r["live"] and r["tag"] != "corpus" for r in self.rows.values()), methods={})
        for mode, route in [("lexical", "/api/v1/memory/search/lexical"),
                            ("semantic", "/api/v1/memory/search"),
                            ("hybrid", "/api/v1/memory/search/hybrid")]:
            query = dict(limit=10, scan_limit=10000, tag="corpus")
            body = dict(contract_version=1, scope=self.scope, query=query)
            if mode != "semantic":
                body["mode"] = mode
                query.update(text="historycoverage", scan_bytes_limit=134217728)
            if mode != "lexical": query.update(space=self.space, vector=self.vector, min_score=-1)
            if mode == "hybrid": query["ranking_version"] = "weighted_rrf_v1"
            before_resources = self.resources()
            retrieval, http, sizes = [], [], []
            pages = []
            for sample in range(2 + self.samples):
                response, headers, elapsed, size = self.request(route, body)
                page = dict(response["page"])
                normalized_headers = {key.lower(): value for key,value in headers.items()}
                for field in ["candidate_selection_version", "candidate_index_bytes", "scanned_records", "scanned_bytes"]:
                    value = normalized_headers.get("x-qilbee-" + field.replace("_", "-"))
                    if value is not None:
                        page[field] = value if field.endswith("version") else int(value)
                hit_ids = [hit["record"]["record_id"] for hit in page["hits"]]
                assert len(hit_ids) == len(set(hit_ids)), "Duplicate result IDs"
                for hit in page["hits"]:
                    row = self.rows[hit["record"]["record_id"]]
                    assert row["live"] and row["tag"] == "corpus", "Unavailable or foreign result"
                    assert hit["record"]["revision"] == row["revision"], "Stale source revision"
                    if mode == "semantic":
                        assert abs(hit["score"] - 1.0) < 1e-12, "Exact cosine changed"
                if sample < 2: continue
                if mode == "semantic":
                    micros = int({k.lower():v for k,v in headers.items()}["x-qilbee-retrieval-micros"])
                else: micros = response["timing"]["retrieval_micros"]
                retrieval.append(micros / 1000); http.append(elapsed); sizes.append(size)
                pages.append({k:v for k,v in page.items() if k != "hits"})
            assert all(page == pages[0] for page in pages), "Coverage changed without fixture mutations"
            after_resources = self.resources()
            page = pages[0]
            endpoint = page.get("next_after")
            prefix = [(key, row) for key, row in self.rows.items() if endpoint is None or key <= endpoint]
            # Legacy scans use UUID order. These are controlled-fixture categories,
            # not instrumentation of an arbitrary production namespace.
            distribution = dict(represents_scanned_candidates="candidate_selection_version" not in page,
                total_keys=len(prefix), unavailable=sum(not r["live"] for _,r in prefix),
                outside_tag=sum(r["live"] and r["tag"] != "corpus" for _,r in prefix),
                stale_live_embeddings=sum(r["live"] and r["embedding_revision"] != r["revision"] for _,r in prefix),
                expected_current_eligible=sum(r["live"] and r["tag"] == "corpus" and
                    (mode == "lexical" or r["embedding_revision"] == r["revision"]) for _,r in prefix))
            def timing(values):
                ordered = sorted(values)
                return dict(p50=statistics.median(values), p95=ordered[math.ceil(.95*len(values))-1],
                            minimum=ordered[0], maximum=ordered[-1], samples=len(values))
            resource_work = None if before_resources is None else dict(
                process_cpu_seconds=after_resources["process_cpu_seconds"]-before_resources["process_cpu_seconds"],
                queries_including_warmup=2+self.samples, before=before_resources, after=after_resources,
                limitation="CPU for the PID 1 server over the query loop; RSS sampled at boundaries, lifetime peak is not a per-method peak.")
            stage["methods"][mode] = dict(server_resources=resource_work, page=page, retrieval_ms=timing(retrieval), http_ms=timing(http),
                response_bytes=dict(minimum=min(sizes), maximum=max(sizes)),
                controlled_fixture_uuid_prefix=distribution)
        self.results.append(stage)
        self.save(label)
        print(json.dumps(dict(stage=label, corpus=stage["current_corpus"], tombstones=stage["tombstones"],
            methods={mode:dict(exhaustive=result["page"]["exhaustive"],
                corpus=result["page"].get("corpus_records", result["page"].get("matched_records")),
                scanned=result["page"].get("scanned_records",result["page"].get("scanned_embeddings")),
                bytes=result["page"].get("scanned_bytes")) for mode,result in stage["methods"].items()})), flush=True)

    def run(self, cycles=3, outside_tag=0):
        self.output.mkdir(parents=True, exist_ok=False, mode=0o700)
        initial = self.request("/api/v1/memory/query", dict(contract_version=1, scope=self.scope,
            filter=dict(limit=1, scan_limit=1)))[0]["page"]
        if initial["records"] or initial["scanned_records"] or initial["next_after"]:
            raise ValueError("The dedicated scope must be empty, including tombstones")
        previous = 0
        for target in sorted({min(1024, self.documents), min(2592, self.documents), self.documents}):
            created = self.parallel(lambda number:self.create(number, 0), range(previous, target))
            for identifier, row in created:
                self.rows[identifier] = row; self.slots[row["slot"]] = identifier
            self.parallel(lambda pair:self.embed(*pair, 0), created)
            self.measure("initial" if target == self.documents else f"size-{target}")
            previous = target
        for cycle in range(1, cycles+1):
            current = [(identifier,self.rows[identifier]) for identifier in self.slots.values()]
            def update(pair):
                identifier,row=pair
                receipt=self.command(f"update-{cycle}-{row['slot']}", dict(type="update",record_id=identifier,
                    expected_revision=row["revision"], record=self.record(row["slot"],cycle)))
                row["revision"]=receipt["revision"]
                self.embed(identifier,row,cycle)
            self.parallel(update,current)
            self.measure(f"cycle-{cycle}-updated")
            def replace(pair):
                identifier,row=pair
                receipt=self.command(f"delete-{cycle}-{row['slot']}",dict(type="delete",record_id=identifier,
                    expected_revision=row["revision"]))
                row["revision"]=receipt["revision"];row["live"]=False
                new_id,new_row=self.create(row["slot"],cycle)
                self.embed(new_id,new_row,cycle)
                return new_id,new_row
            for identifier,row in self.parallel(replace,current):
                self.rows[identifier]=row;self.slots[row["slot"]]=identifier
            self.measure(f"cycle-{cycle}-replaced")
        if outside_tag:
            for identifier,row in self.parallel(lambda n:self.create(n,cycles+1,"background"),range(outside_tag)):
                self.rows[identifier]=row
                self.embed(identifier,row,cycles+1)
            self.measure("outside-tag-background")


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url",required=True)
    parser.add_argument("--credential-file",type=Path,required=True)
    parser.add_argument("--scope-file",type=Path,required=True)
    parser.add_argument("--output",type=Path,required=True)
    parser.add_argument("--documents",type=int,default=5183)
    parser.add_argument("--cycles",type=int,default=3)
    parser.add_argument("--samples",type=int,default=20)
    parser.add_argument("--workers",type=int,default=4)
    parser.add_argument("--outside-tag",type=int,default=0)
    parser.add_argument("--resource-container", help="Optional isolated Docker container with the server as PID 1")
    parser.add_argument("--allow-isolated-mutations",action="store_true")
    args=parser.parse_args()
    if not args.allow_isolated_mutations: parser.error("Explicit isolated-fixture mutation permission is required")
    if min(args.documents,args.samples,args.workers)<1 or not 0<=args.cycles<=3 or args.outside_tag<0:
        parser.error("Invalid fixture size or cycle bounds")
    token=json.loads(args.credential_file.read_text())["secret"]
    scope=json.loads(args.scope_file.read_text())
    if not scope["project_id"].startswith("history-benchmark-"):
        parser.error("Use a dedicated history-benchmark-* project scope")
    HistoryBenchmark(args.url,token,scope,args.output,args.documents,args.samples,args.workers,args.resource_container).run(args.cycles,args.outside_tag)


if __name__=="__main__": main()
