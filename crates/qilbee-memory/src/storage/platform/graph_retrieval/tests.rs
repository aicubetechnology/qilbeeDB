use super::super::semantic_tests::{actor, attach, create, input, open, space};
use super::*;
use tempfile::TempDir;
#[path = "tests_boundaries.rs"]
mod boundaries;
#[path = "tests_strength.rs"]
mod strength;

fn query(text: &str) -> GraphRetrievalQuery {
    serde_json::from_value(serde_json::json!({
        "ranking_version":"typed_path_balanced_v1", "seed":{"mode":"lexical","text":text}, "limit":10,
        "expansion":{"direction":"outgoing","max_depth":8,"node_limit":128,"edge_limit":256,"scan_limit":1024,"embedding_bytes_limit":8388608}
    })).unwrap()
}
fn link(
    db: &RocksDbMemoryStorage,
    a: &CommandReceipt,
    b: &CommandReceipt,
    kind: MemoryRelationKind,
    key: &str,
) -> MemoryRelationReceipt {
    db.apply_memory_relation_command(
        "scope",
        &actor(),
        &MemoryRelationCommand {
            contract_version: 1,
            idempotency_key: key.into(),
            operation: MemoryRelationOperation::Assert {
                relation: MemoryRelationInput {
                    evidence_sources: Vec::new(),
                    source: MemorySourceRef {
                        record_id: a.record_id,
                        revision: a.revision,
                    },
                    target: MemorySourceRef {
                        record_id: b.record_id,
                        revision: b.revision,
                    },
                    kind,
                    provenance: RelationProvenance {
                        origin: RelationOrigin::ToolObservation,
                        method: "fixture".into(),
                        method_revision: "v1".into(),
                        evidence_ref: "trace://synthetic-graph-search".into(),
                        model: None,
                    },
                    valid_from_millis: None,
                    valid_until_millis: None,
                },
            },
        },
    )
    .unwrap()
}
fn change(db: &RocksDbMemoryStorage, operation: MemoryOperation, key: &str) -> CommandReceipt {
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: key.into(),
            operation,
        },
    )
    .unwrap()
}
fn ids(page: &GraphRetrievalPage) -> BTreeSet<Uuid> {
    page.hits.iter().map(|h| h.record.record_id).collect()
}

#[test]
fn paths_retrieve_nonlexical_memories_with_exact_proofs_and_all_profile_score_bounds() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let b = create(&db, "scope", "distant evidence");
    let relation = link(&db, &a, &b, MemoryRelationKind::SameEntity, "link");
    for version in GraphRankingVersion::ALL {
        let mut q = query("anchor");
        q.ranking_version = version;
        let result = db.search_memory_graph("scope", &q).unwrap();
        assert!(result.coverage.complete);
        assert_eq!(result.seed.lexical_matches, Some(1));
        assert_eq!(ids(&result), [a.record_id, b.record_id].into());
        assert_eq!(result.hits[0].score, 1.0 / 3.0);
        assert!(
            result
                .hits
                .iter()
                .all(|h| h.score <= result.ranking.maximum_score)
        );
        let neighbor = result
            .hits
            .iter()
            .find(|h| h.record.record_id == b.record_id)
            .unwrap();
        assert!(neighbor.base.is_none());
        assert!(neighbor.affinity.is_none());
        let path = neighbor.graph.as_ref().unwrap();
        assert_eq!(path.anchor.record_id, a.record_id);
        assert_eq!(path.steps.len(), 1);
        assert_eq!(path.steps[0].relation_id, relation.relation_id);
        assert_eq!(path.steps[0].relation_revision, 1);
        assert_eq!(path.steps[0].to.revision, 1);
        assert_eq!(result.relations.len(), 1);
        assert!(result.ranking.experimental);
    }
}

#[test]
fn direction_and_intent_select_distinct_evidence_without_rewriting_claims() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let b = create(&db, "scope", "entity");
    let c = create(&db, "scope", "contradiction");
    let mut earlier = input("earlier");
    earlier.event_time_millis -= 1;
    let d = change(&db, MemoryOperation::Create { record: earlier }, "earlier");
    link(&db, &a, &b, MemoryRelationKind::SameEntity, "ab");
    link(&db, &a, &c, MemoryRelationKind::Contradicts, "ac");
    link(&db, &d, &a, MemoryRelationKind::TemporalBefore, "da");
    let mut q = query("anchor");
    q.ranking_version = GraphRankingVersion::TypedPathEntityV1;
    assert_eq!(
        ids(&db.search_memory_graph("scope", &q).unwrap()),
        [a.record_id, b.record_id].into()
    );
    q.ranking_version = GraphRankingVersion::TypedPathEvidenceV1;
    let result = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(result.hits[1].record.record_id, c.record_id);
    assert!(
        result
            .relations
            .iter()
            .any(|r| r.input.kind == MemoryRelationKind::Contradicts)
    );
    q.ranking_version = GraphRankingVersion::TypedPathTemporalV1;
    q.expansion.direction = TypedGraphDirection::Incoming;
    let result = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(ids(&result), [a.record_id, d.record_id].into());
    assert_eq!(
        result.hits[1].graph.as_ref().unwrap().steps[0].direction,
        GraphStepDirection::Incoming
    );
}

#[test]
fn duplicate_assertions_and_cycles_cannot_inflate_rank_or_path_strength() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let b = create(&db, "scope", "neighbor");
    let c = create(&db, "scope", "leaf");
    link(&db, &a, &b, MemoryRelationKind::Supports, "ab");
    link(&db, &b, &c, MemoryRelationKind::Supports, "bc");
    let before = db.search_memory_graph("scope", &query("anchor")).unwrap();
    for i in 0..12 {
        link(
            &db,
            &a,
            &b,
            MemoryRelationKind::Supports,
            &format!("copy-{i}"),
        );
    }
    link(&db, &c, &a, MemoryRelationKind::Supports, "cycle");
    let after = db.search_memory_graph("scope", &query("anchor")).unwrap();
    assert!(before.coverage.complete && after.coverage.complete);
    let scores = |p: &GraphRetrievalPage| {
        p.hits
            .iter()
            .map(|h| {
                (
                    h.record.record_id,
                    h.score,
                    h.graph.as_ref().unwrap().strength,
                    h.graph.as_ref().unwrap().steps.len(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(scores(&before), scores(&after));
    assert_eq!(after.relations.len(), 2);
}

#[test]
fn payload_filters_apply_before_expansion_and_cannot_bridge_through_excluded_records() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let mut excluded = input("excluded intermediary");
    excluded.tags = vec!["outside".into()];
    let b = change(
        &db,
        MemoryOperation::Create { record: excluded },
        "excluded",
    );
    let c = create(&db, "scope", "eligible leaf");
    link(&db, &a, &b, MemoryRelationKind::SameEntity, "ab");
    link(&db, &b, &c, MemoryRelationKind::SameEntity, "bc");
    let mut q = query("anchor");
    q.tag = Some("knowledge".into());
    let result = db.search_memory_graph("scope", &q).unwrap();
    assert!(result.coverage.complete);
    assert_eq!(ids(&result), [a.record_id].into());
    assert_eq!(result.graph_coverage.records_examined, 2);
    assert!(result.relations.is_empty());
    assert!(
        db.search_memory_graph("other-scope", &q)
            .unwrap()
            .hits
            .is_empty()
    );
}

#[test]
fn one_snapshot_preserves_revision_paths_while_live_updates_and_deletions_invalidate_them() {
    for version in GraphRankingVersion::ALL {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let a = create(&db, "scope", "anchor");
        let b = create(&db, "scope", "neighbor");
        link(&db, &a, &b, MemoryRelationKind::SameEntity, "ab");
        let snapshot = db.memory_snapshot();
        let mut q = query("anchor");
        q.ranking_version = version;
        let before = snapshot.search_graph("scope", &q).unwrap();
        let update = change(
            &db,
            MemoryOperation::Update {
                record_id: b.record_id,
                expected_revision: 1,
                record: input("replaced"),
            },
            "update",
        );
        let old = snapshot.search_graph("scope", &q).unwrap();
        assert_eq!(
            serde_json::to_value(&before).unwrap(),
            serde_json::to_value(old).unwrap()
        );
        assert_eq!(
            ids(&db.search_memory_graph("scope", &q).unwrap()),
            [a.record_id].into()
        );
        link(
            &db,
            &a,
            &update,
            MemoryRelationKind::SameEntity,
            "updated-link",
        );
        assert_eq!(db.search_memory_graph("scope", &q).unwrap().hits.len(), 2);
        change(
            &db,
            MemoryOperation::Delete {
                record_id: b.record_id,
                expected_revision: 2,
            },
            "delete",
        );
        assert_eq!(
            ids(&db.search_memory_graph("scope", &q).unwrap()),
            [a.record_id].into()
        );
    }
}

#[test]
fn semantic_and_hybrid_seeds_preserve_raw_scores_and_report_missing_external_embeddings() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let b = create(&db, "scope", "neighbor");
    db.apply_memory_embedding(
        "scope",
        &actor(),
        &attach(a.record_id, "vector", vec![1.0, 0.0, 0.0]),
    )
    .unwrap();
    link(&db, &a, &b, MemoryRelationKind::SameEntity, "ab");
    for hybrid in [false, true] {
        let mut q = query("anchor");
        q.seed = if hybrid {
            GraphSeedQuery::Hybrid {
                text: "anchor".into(),
                space: space(),
                vector: vec![1.0, 0.0, 0.0],
                ranking_version: HybridRankingVersion::WeightedRrfV2,
                min_score: -1.0,
            }
        } else {
            GraphSeedQuery::Semantic {
                space: space(),
                vector: vec![1.0, 0.0, 0.0],
                min_score: -1.0,
            }
        };
        let result = db.search_memory_graph("scope", &q).unwrap();
        assert_eq!(ids(&result), [a.record_id, b.record_id].into());
        assert!(!result.coverage.embeddings_complete);
        assert!(matches!(
            result.seed.embedding_coverage,
            Some(EmbeddingCoverage::Partial)
        ));
        assert_eq!(result.affinity_work.reused_scores, 1);
        assert_eq!(result.affinity_work.missing_bindings, 1);
        assert_eq!(result.hits[0].affinity.as_ref().unwrap().cosine, 1.0);
        let graph = result.hits[1].graph.as_ref().unwrap();
        assert_eq!(graph.missing_affinity.len(), 1);
        assert_eq!(graph.strength, 1.0 / 12.0);
    }
    let mut wrong = space();
    wrong.revision = "another-space".into();
    let mut q = query("anchor");
    q.seed = GraphSeedQuery::Semantic {
        space: wrong,
        vector: vec![1.0, 0.0, 0.0],
        min_score: -1.0,
    };
    let empty = db.search_memory_graph("scope", &q).unwrap();
    assert!(empty.hits.is_empty());
    assert!(!empty.coverage.complete);
    assert!(matches!(
        empty.seed.embedding_coverage,
        Some(EmbeddingCoverage::Missing)
    ));
}

#[test]
fn traversal_and_source_cuts_are_explicit_and_affinity_byte_exhaustion_returns_no_page() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let b = create(&db, "scope", "neighbor");
    link(&db, &a, &b, MemoryRelationKind::SameEntity, "ab");
    let mut q = query("anchor");
    q.expansion.node_limit = 1;
    let page = db.search_memory_graph("scope", &q).unwrap();
    assert!(!page.coverage.complete);
    assert!(!page.coverage.graph_complete);
    assert!(
        page.graph_coverage
            .stop_reasons
            .contains(&TypedGraphStopReason::NodeLimit)
    );
    q = query("anchor");
    q.scan_limit = 1;
    assert!(
        !db.search_memory_graph("scope", &q)
            .unwrap()
            .coverage
            .source_complete
    );
    q.scan_bytes_limit = 1;
    assert!(matches!(
        db.search_memory_graph("scope", &q),
        Err(Error::ValidationError(_))
    ));
    db.apply_memory_embedding(
        "scope",
        &actor(),
        &attach(a.record_id, "a", vec![1.0, 0.0, 0.0]),
    )
    .unwrap();
    db.apply_memory_embedding(
        "scope",
        &actor(),
        &attach(b.record_id, "b", vec![0.0, 1.0, 0.0]),
    )
    .unwrap();
    q = query("anchor");
    q.seed = GraphSeedQuery::Semantic {
        space: space(),
        vector: vec![1.0, 0.0, 0.0],
        min_score: 0.9,
    };
    q.expansion.embedding_bytes_limit = 1;
    assert!(matches!(
        db.search_memory_graph("scope", &q),
        Err(Error::ValidationError(_))
    ));
}

#[test]
fn returned_payload_count_and_proofs_respect_the_requested_context_budget() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let b = create(&db, "scope", "neighbor");
    link(&db, &a, &b, MemoryRelationKind::Supports, "ab");
    let mut q = query("anchor");
    q.limit = 1;
    let page = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.candidates_ranked, 2);
    assert!(page.relations.is_empty());
    assert_eq!(page.hits[0].record.record_id, a.record_id);
    let empty = db
        .search_memory_graph("scope", &query("absenttoken"))
        .unwrap();
    assert!(empty.hits.is_empty());
    assert!(empty.coverage.complete);
    assert!(empty.graph_roots.is_empty());
}

#[test]
fn best_channel_admits_graph_only_evidence_without_summing_duplicate_signals() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let mut anchors = Vec::new();
    for index in 0..10 {
        anchors.push(create(&db, "scope", &format!("anchor {index}")));
    }
    anchors.sort_by_key(|r| r.record_id);
    let distant = create(&db, "scope", "distant evidence");
    link(
        &db,
        &anchors[0],
        &distant,
        MemoryRelationKind::SameEntity,
        "discovery",
    );
    let mut q = query("anchor");
    q.ranking_version = GraphRankingVersion::TypedPathBasePreservingV1;
    let old = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(old.hits.len(), 10);
    assert!(!ids(&old).contains(&distant.record_id));
    q.ranking_version = GraphRankingVersion::TypedPathBestChannelV1;
    let page = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(page.hits.len(), 10);
    let hit = page
        .hits
        .iter()
        .find(|h| h.record.record_id == distant.record_id)
        .unwrap();
    assert!(hit.base.is_none());
    assert_eq!(hit.graph.as_ref().unwrap().rank, 5);
    assert_eq!(hit.score, 1.0 / 7.0);
    let first = &page.hits[0];
    assert_eq!(first.base.as_ref().unwrap().contribution, 1.0 / 3.0);
    assert_eq!(first.graph.as_ref().unwrap().contribution, 1.0 / 3.0);
    assert_eq!(first.score, 1.0 / 3.0);
    for pair in page.hits.windows(2) {
        assert!(pair[0].score >= pair[1].score);
        if pair[0].score == pair[1].score {
            assert!(pair[0].record.record_id < pair[1].record.record_id);
        }
    }
    assert!(
        db.search_memory_graph("another-scope", &q)
            .unwrap()
            .hits
            .is_empty()
    );
}
