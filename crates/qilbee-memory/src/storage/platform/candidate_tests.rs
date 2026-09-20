use super::semantic_tests::{actor, attach, create, input, open, search};
use super::*;
use tempfile::TempDir;

fn lexical() -> LexicalQuery {
    LexicalQuery {
        text: "term".into(),
        limit: 10,
        scan_limit: 16,
        scan_bytes_limit: 8_388_608,
        after: None,
        tag: Some("knowledge".into()),
        episode_type: None,
    }
}
fn hybrid() -> HybridQuery {
    let q = search();
    HybridQuery {
        text: "term".into(),
        space: q.space,
        vector: q.vector,
        limit: 10,
        ranking_version: HybridRankingVersion::WeightedRrfV1,
        min_score: -1.0,
        scan_limit: 16,
        scan_bytes_limit: 8_388_608,
        after: None,
        tag: Some("knowledge".into()),
        episode_type: None,
    }
}
fn command(db: &RocksDbMemoryStorage, key: &str, operation: MemoryOperation) -> CommandReceipt {
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
fn embed(db: &RocksDbMemoryStorage, receipt: &CommandReceipt) {
    let mut embedding = attach(
        receipt.record_id,
        &format!("{}-{}", receipt.record_id, receipt.revision),
        vec![1.0, 0.0, 0.0],
    );
    embedding.record_revision = receipt.revision;
    db.apply_memory_embedding("scope", &actor(), &embedding)
        .unwrap();
}
fn assert_coverage(db: &RocksDbMemoryStorage, expected: usize) {
    let lexical = db.search_memory_lexical("scope", &lexical()).unwrap();
    let hybrid = db.search_memory_hybrid("scope", &hybrid()).unwrap();
    let semantic = db
        .search_memory_semantic(
            "scope",
            &SemanticQuery {
                scan_limit: 16,
                tag: Some("knowledge".into()),
                ..search()
            },
        )
        .unwrap();
    assert!(lexical.exhaustive && hybrid.exhaustive && semantic.exhaustive);
    assert_eq!(
        (
            lexical.corpus_records,
            hybrid.corpus_records,
            hybrid.embedded_records,
            semantic.matched_records
        ),
        (expected, expected, expected, expected)
    );
    assert_eq!(
        (
            lexical.scanned_records,
            hybrid.scanned_records,
            semantic.scanned_records
        ),
        (expected, expected, expected)
    );
    assert_eq!(semantic.scanned_embeddings, expected);
    assert_eq!(
        lexical.candidate_selection_version,
        CANDIDATE_SELECTION_VERSION
    );
    assert!(semantic.hits.iter().all(|hit| hit.score == 1.0));
}
#[test]
fn current_candidates_preserve_full_coverage_across_three_history_cycles_and_reopen() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let mut current: Vec<_> = (0..16)
        .map(|i| create(&db, "scope", &format!("term {i}")))
        .collect();
    for receipt in &current {
        embed(&db, receipt);
    }
    for i in 0..32 {
        create(&db, "foreign", &format!("term foreign {i}"));
        let mut other = input("term background");
        other.tags = vec!["background".into()];
        command(
            &db,
            &format!("background {i}"),
            MemoryOperation::Create { record: other },
        );
    }
    assert_coverage(&db, 16);
    for cycle in 1..=3 {
        for (slot, receipt) in current.iter_mut().enumerate() {
            *receipt = command(
                &db,
                &format!("update {cycle} {slot}"),
                MemoryOperation::Update {
                    record_id: receipt.record_id,
                    expected_revision: receipt.revision,
                    record: input("term updated"),
                },
            );
        }
        let stale = db.search_memory_hybrid("scope", &hybrid()).unwrap();
        assert_eq!(stale.corpus_records, 16);
        assert_eq!(stale.embedded_records, 0);
        assert_eq!(stale.embedding_coverage, EmbeddingCoverage::Missing);
        for receipt in &current {
            embed(&db, receipt);
        }
        assert_coverage(&db, 16);
        for (slot, receipt) in current.iter_mut().enumerate() {
            command(
                &db,
                &format!("delete {cycle} {slot}"),
                MemoryOperation::Delete {
                    record_id: receipt.record_id,
                    expected_revision: receipt.revision,
                },
            );
            *receipt = create(&db, "scope", &format!("term replacement {cycle} {slot}"));
            embed(&db, receipt);
        }
        assert_coverage(&db, 16);
    }
    drop(db);
    assert_coverage(&open(dir.path()), 16);
}
#[test]
fn candidate_filters_reviews_missing_vectors_and_snapshot_revisions_remain_observable() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let original = create(&db, "scope", "term original");
    embed(&db, &original);
    let old = db.memory_snapshot();
    let mut moved = input("term moved");
    moved.tags = vec!["other".into()];
    moved.episode_type = EpisodeType::Decision;
    let updated = command(
        &db,
        "move",
        MemoryOperation::Update {
            record_id: original.record_id,
            expected_revision: 1,
            record: moved,
        },
    );
    assert_eq!(
        old.scan_corpus("scope", &lexical())
            .unwrap()
            .page
            .corpus_records,
        1
    );
    assert_coverage(&db, 0);
    let mut q = lexical();
    q.tag = Some("other".into());
    q.episode_type = Some(EpisodeType::Decision);
    assert_eq!(
        db.search_memory_lexical("scope", &q)
            .unwrap()
            .corpus_records,
        1
    );
    q.episode_type = Some(EpisodeType::Observation);
    assert_eq!(
        db.search_memory_lexical("scope", &q)
            .unwrap()
            .scanned_records,
        0
    );
    q.episode_type = None;
    let semantic = db.search_memory_semantic("scope", &search()).unwrap();
    assert_eq!(
        (
            semantic.scanned_records,
            semantic.scanned_embeddings,
            semantic.matched_records
        ),
        (1, 1, 0)
    );
    let mut missing = search();
    missing.space.revision = "missing".into();
    let semantic = db.search_memory_semantic("scope", &missing).unwrap();
    assert_eq!(
        (semantic.scanned_records, semantic.scanned_embeddings),
        (1, 0)
    );
    embed(&db, &updated);
    for (revision, disposition) in [
        (2, MemoryReviewDisposition::Rejected),
        (3, MemoryReviewDisposition::Approved),
    ] {
        db.review_memory_record(
            "scope",
            &actor(),
            &MemoryReviewCommand {
                contract_version: 1,
                idempotency_key: format!("review {revision}"),
                record_id: updated.record_id,
                expected_revision: revision,
                disposition,
                evidence_ref: "fixture:review".into(),
            },
        )
        .unwrap();
        let page = db.search_memory_lexical("scope", &q).unwrap();
        assert_eq!(
            page.scanned_records,
            usize::from(disposition == MemoryReviewDisposition::Approved)
        );
        assert!(
            db.search_memory_semantic("scope", &search())
                .unwrap()
                .hits
                .is_empty()
        );
    }
}
#[test]
fn candidate_pagination_has_no_duplicates_and_includes_unembedded_current_records_in_budget() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let mut ids = vec![];
    for i in 0..5 {
        ids.push(create(&db, "scope", &format!("term {i}")).record_id);
    }
    ids.sort();
    let mut query = search();
    query.scan_limit = 2;
    let mut scanned = 0;
    let mut cursor = None;
    loop {
        query.after = cursor;
        let page = db.search_memory_semantic("scope", &query).unwrap();
        assert_eq!(page.scanned_embeddings, 0);
        assert!(!page.exhaustive);
        scanned += page.scanned_records;
        assert!(page.scanned_records <= 2);
        if let Some(next) = page.next_after {
            assert!(cursor.is_none_or(|previous| previous < next));
            assert_eq!(next, ids[scanned - 1]);
            cursor = Some(next);
        } else {
            break;
        }
    }
    assert_eq!(scanned, 5);
}
#[test]
fn candidate_startup_repairs_missing_partial_and_old_writer_projections() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let first = create(&db, "scope", "term first");
    embed(&db, &first);
    let cf = db.cf(super::super::cf::EPISODE_INDEX).unwrap();
    let tip_key = record_prefix(0x31, "scope");
    let old_tip = db.db.get_cf(cf, &tip_key).unwrap().unwrap();
    let second = create(&db, "scope", "term second");
    embed(&db, &second);
    // Simulate a pre-index writer: newer canonical/journal state, obsolete projection fingerprint.
    let mut key = candidates::prefix("scope", Some("knowledge"), None).unwrap();
    key.extend_from_slice(second.record_id.as_bytes());
    db.db.delete_cf(cf, key).unwrap();
    db.db.put_cf(cf, &tip_key, old_tip).unwrap();
    drop(db);
    let db = open(dir.path());
    assert_coverage(&db, 2);
    let cf = db.cf(super::super::cf::EPISODE_INDEX).unwrap();
    // A rebuild interrupted before its final marker must not be mistaken for ready state.
    db.db.delete_cf(cf, &tip_key).unwrap();
    let mut key = candidates::prefix("scope", Some("knowledge"), None).unwrap();
    key.extend_from_slice(first.record_id.as_bytes());
    db.db.delete_cf(cf, key).unwrap();
    drop(db);
    assert_coverage(&open(dir.path()), 2);
}
#[test]
fn encountered_candidate_corruption_fails_closed() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let receipt = create(&db, "scope", "term");
    let cf = db.cf(super::super::cf::EPISODE_INDEX).unwrap();
    let mut key = candidates::prefix("scope", Some("knowledge"), None).unwrap();
    key.extend_from_slice(receipt.record_id.as_bytes());
    db.db.put_cf(cf, key, [0; 40]).unwrap();
    assert!(matches!(
        db.search_memory_lexical("scope", &lexical()),
        Err(Error::DataCorruption(_))
    ));
}

#[test]
fn four_deleted_leading_uuids_cannot_exhaust_a_four_candidate_budget() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let mut receipts: Vec<_> = (0..8)
        .map(|i| create(&db, "scope", &format!("term {i}")))
        .collect();
    for receipt in &receipts {
        embed(&db, receipt);
    }
    receipts.sort_by_key(|receipt| receipt.record_id);
    for receipt in &receipts[..4] {
        command(
            &db,
            &format!("delete {}", receipt.record_id),
            MemoryOperation::Delete {
                record_id: receipt.record_id,
                expected_revision: receipt.revision,
            },
        );
    }
    for receipt in &receipts[4..] {
        assert!(
            db.read_memory_record("scope", receipt.record_id)
                .unwrap()
                .is_some()
        );
    }
    let mut lexical = lexical();
    lexical.scan_limit = 4;
    let mut hybrid = hybrid();
    hybrid.scan_limit = 4;
    let semantic = SemanticQuery {
        scan_limit: 4,
        ..search()
    };
    let lexical = db.search_memory_lexical("scope", &lexical).unwrap();
    let hybrid = db.search_memory_hybrid("scope", &hybrid).unwrap();
    let semantic = db.search_memory_semantic("scope", &semantic).unwrap();
    for (scanned, hits, exhaustive) in [
        (
            lexical.scanned_records,
            lexical.hits.len(),
            lexical.exhaustive,
        ),
        (hybrid.scanned_records, hybrid.hits.len(), hybrid.exhaustive),
        (
            semantic.scanned_records,
            semantic.hits.len(),
            semantic.exhaustive,
        ),
    ] {
        assert_eq!((scanned, hits, exhaustive), (4, 4, true));
    }
    let changes = db
        .memory_changes(
            "scope",
            &MemoryChangesQuery {
                after: None,
                through: None,
                limit: 256,
            },
        )
        .unwrap();
    assert_eq!(
        changes
            .changes
            .iter()
            .filter(|e| e.kind == MemoryChangeKind::Deleted)
            .count(),
        4
    );
    assert!(
        db.memory_snapshot()
            .record("scope", receipts[0].record_id)
            .unwrap()
            .unwrap()
            .payload
            .is_none()
    );
}
