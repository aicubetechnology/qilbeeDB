use super::*;

#[test]
fn graph_can_admit_a_memory_outside_the_capped_base_pool_without_hiding_the_cut() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let mut records = Vec::new();
    for n in 0..110 {
        records.push(create(&db, "scope", &format!("anchor {n}")));
    }
    let leaf = create(&db, "scope", "unmatched neighbor");
    let mut q = query("anchor");
    q.limit = 100;
    let base = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(base.seed.lexical_matches, Some(110));
    assert!(base.seed.base_candidates_truncated);
    let root = records
        .iter()
        .find(|r| r.record_id == base.seed.selected_anchors[0].record_id)
        .unwrap();
    link(
        &db,
        root,
        &leaf,
        MemoryRelationKind::Supports,
        "outside-pool",
    );
    let graph = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(graph.seed.base_candidates, 100);
    assert_eq!(graph.seed.selected_anchors.len(), 4);
    assert_eq!(graph.candidates_ranked, 101);
    assert_eq!(graph.hits.len(), 100);
    assert!(ids(&graph).contains(&leaf.record_id));
    assert!(!graph.coverage.candidates_complete);
    assert!(graph.coverage.source_complete);
    assert_eq!(
        graph
            .hits
            .iter()
            .find(|h| h.record.record_id == leaf.record_id)
            .unwrap()
            .graph
            .as_ref()
            .unwrap()
            .rank,
        5
    );
}

#[test]
fn semantic_source_bytes_stop_before_a_second_row_and_do_not_change_legacy_search() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    for (n, r) in [a, b].iter().enumerate() {
        db.apply_memory_embedding(
            "scope",
            &actor(),
            &attach(r.record_id, &format!("v{n}"), vec![1.0, 0.0, 0.0]),
        )
        .unwrap();
    }
    let old_query = super::super::super::semantic_tests::search();
    let old = db.search_memory_semantic("scope", &old_query).unwrap();
    assert!(old.exhaustive);
    let mut q = query("unused");
    q.seed = GraphSeedQuery::Semantic {
        space: space(),
        vector: vec![1.0, 0.0, 0.0],
        min_score: -1.0,
    };
    let snapshot = db.memory_snapshot();
    let first = old.hits[0].record.record_id;
    q.scan_bytes_limit = snapshot
        .record_with_bytes("scope", first)
        .unwrap()
        .unwrap()
        .1
        + snapshot
            .embedding("scope", &space(), first)
            .unwrap()
            .unwrap()
            .1;
    let cut = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(cut.seed.scanned_records, 1);
    assert_eq!(cut.seed.scanned_bytes, q.scan_bytes_limit);
    assert!(!cut.coverage.source_complete);
    assert_eq!(cut.hits.len(), 1);
    assert_eq!(
        serde_json::to_value(&old).unwrap(),
        serde_json::to_value(db.search_memory_semantic("scope", &old_query).unwrap()).unwrap()
    );
}

#[test]
fn clock_expiry_and_rejected_derivation_sources_cannot_bridge_a_graph() {
    for version in GraphRankingVersion::ALL {
        let mut q = query("anchor");
        q.ranking_version = version;
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let a = create(&db, "scope", "anchor");
        let source = create(&db, "scope", "source");
        let b = change(
            &db,
            MemoryOperation::Derive {
                record: input("derived bridge"),
                derivation: MemoryDerivation {
                    sources: vec![MemorySourceRef {
                        record_id: source.record_id,
                        revision: 1,
                    }],
                    method: "fixture".into(),
                    method_revision: "v1".into(),
                    evidence_ref: "trace://derivation".into(),
                },
            },
            "derived",
        );
        let c = create(&db, "scope", "leaf");
        link(&db, &a, &b, MemoryRelationKind::SameEntity, "ab");
        link(&db, &b, &c, MemoryRelationKind::SameEntity, "bc");
        assert_eq!(db.search_memory_graph("scope", &q).unwrap().hits.len(), 3);
        db.review_memory_record(
            "scope",
            &actor(),
            &MemoryReviewCommand {
                contract_version: 1,
                idempotency_key: "reject".into(),
                record_id: source.record_id,
                expected_revision: 1,
                disposition: MemoryReviewDisposition::Rejected,
                evidence_ref: "trace://rejection".into(),
            },
        )
        .unwrap();
        assert_eq!(
            ids(&db.search_memory_graph("scope", &q).unwrap()),
            [a.record_id].into()
        );
        let expiry = chrono::Utc::now().timestamp_millis() + 60000;
        let mut record = input("expiring");
        record.valid_until_millis = Some(expiry);
        let expiring = change(&db, MemoryOperation::Create { record }, "expiring");
        link(&db, &a, &expiring, MemoryRelationKind::SameEntity, "ae");
        let mut before = db.memory_snapshot();
        before.now = expiry - 1;
        assert!(ids(&before.search_graph("scope", &q).unwrap()).contains(&expiring.record_id));
        let mut after = db.memory_snapshot();
        after.now = expiry;
        assert!(!ids(&after.search_graph("scope", &q).unwrap()).contains(&expiring.record_id));
    }
}

#[test]
fn retired_and_rejected_assertions_stop_contributing_and_survive_reopen() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let b = create(&db, "scope", "b");
    let c = create(&db, "scope", "c");
    let ab = link(&db, &a, &b, MemoryRelationKind::Supports, "ab");
    let ac = link(&db, &a, &c, MemoryRelationKind::Supports, "ac");
    for (key, operation) in [
        (
            "retire",
            MemoryRelationOperation::Retire {
                relation_id: ab.relation_id,
                expected_revision: 1,
                evidence_ref: "trace://retire".into(),
            },
        ),
        (
            "reject",
            MemoryRelationOperation::Review {
                relation_id: ac.relation_id,
                expected_revision: 1,
                disposition: MemoryReviewDisposition::Rejected,
                evidence_ref: "trace://reject".into(),
            },
        ),
    ] {
        db.apply_memory_relation_command(
            "scope",
            &actor(),
            &MemoryRelationCommand {
                contract_version: 1,
                idempotency_key: key.into(),
                operation,
            },
        )
        .unwrap();
    }
    drop(db);
    let db = open(dir.path());
    let page = db.search_memory_graph("scope", &query("anchor")).unwrap();
    assert_eq!(ids(&page), [a.record_id].into());
    assert!(page.coverage.complete);
    assert!(page.relations.is_empty());
}

#[test]
fn episode_filter_and_corrupt_adjacency_never_produce_a_complete_success() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let mut record = input("different episode");
    record.episode_type = EpisodeType::Decision;
    let b = change(&db, MemoryOperation::Create { record }, "decision");
    let edge = link(&db, &a, &b, MemoryRelationKind::Supports, "ab");
    let mut q = query("anchor");
    q.episode_type = Some(EpisodeType::Observation);
    let page = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(ids(&page), [a.record_id].into());
    assert!(page.coverage.complete);
    // Remove both physical projections: the canonical/head checksum must still reject omission.
    let cf = db.cf(crate::storage::cf::AGENT_META).unwrap();
    for (tag, record) in [(0x54, &a), (0x55, &b)] {
        let mut key = record_key(tag, "scope", record.record_id);
        key.extend(1_u64.to_be_bytes());
        key.extend(edge.relation_id.as_bytes());
        db.db.delete_cf(cf, key).unwrap();
    }
    assert!(db.search_memory_graph("scope", &query("anchor")).is_err());
}

#[test]
fn semantic_affinity_prefers_a_relevant_neighbor_and_reports_stale_bindings() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let b = create(&db, "scope", "relevant");
    let c = create(&db, "scope", "orthogonal");
    for (r, key, vector) in [
        (&a, "va", vec![1.0, 0.0, 0.0]),
        (&b, "vb", vec![0.8, 0.6, 0.0]),
        (&c, "vc", vec![0.0, 1.0, 0.0]),
    ] {
        db.apply_memory_embedding("scope", &actor(), &attach(r.record_id, key, vector))
            .unwrap();
    }
    link(&db, &a, &b, MemoryRelationKind::SameEntity, "ab");
    link(&db, &a, &c, MemoryRelationKind::SameEntity, "ac");
    let mut q = query("unused");
    q.seed = GraphSeedQuery::Semantic {
        space: space(),
        vector: vec![1.0, 0.0, 0.0],
        min_score: 0.99,
    };
    let page = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(page.hits[1].record.record_id, b.record_id);
    assert_eq!(page.affinity_work.current_bindings, 2);
    assert!(page.coverage.embeddings_complete);
    let c2 = change(
        &db,
        MemoryOperation::Update {
            record_id: c.record_id,
            expected_revision: 1,
            record: input("changed"),
        },
        "change-c",
    );
    link(&db, &a, &c2, MemoryRelationKind::SameEntity, "ac2");
    let stale = db.search_memory_graph("scope", &q).unwrap();
    assert_eq!(stale.affinity_work.stale_bindings, 1);
    assert!(!stale.coverage.embeddings_complete);
    assert_eq!(
        stale.hits[2].graph.as_ref().unwrap().missing_affinity[0].revision,
        2
    );
}
