use super::semantic_tests::{actor, attach, create, input, open, search};
use super::*;
use tempfile::TempDir;
fn derive(
    db: &RocksDbMemoryStorage,
    key: &str,
    sources: &[CommandReceipt],
) -> Result<CommandReceipt> {
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: key.into(),
            operation: MemoryOperation::Derive {
                record: input(key),
                derivation: MemoryDerivation {
                    sources: sources
                        .iter()
                        .map(|r| MemorySourceRef {
                            record_id: r.record_id,
                            revision: r.revision,
                        })
                        .collect(),
                    method: "synthesis".into(),
                    method_revision: "v1".into(),
                    evidence_ref: "trace://test".into(),
                },
            },
        },
    )
}
fn query() -> LexicalQuery {
    LexicalQuery {
        text: "derived".into(),
        limit: 10,
        scan_limit: 1000,
        scan_bytes_limit: 8_388_608,
        after: None,
        episode_type: None,
        tag: None,
    }
}
#[test]
fn transitive_retrieval_and_explanation_follow_the_exact_invalidating_ancestor() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let leaf = create(&db, "scope", "leaf");
    let mid = derive(&db, "derived-middle", &[leaf.clone()]).unwrap();
    let top = derive(&db, "derived-top", &[mid.clone()]).unwrap();
    db.apply_memory_embedding(
        "scope",
        &actor(),
        &attach(top.record_id, "vector", vec![1.0, 0.0, 0.0]),
    )
    .unwrap();
    let original = db
        .explain_memory_eligibility("scope", top.record_id)
        .unwrap()
        .unwrap();
    assert!(original.eligible);
    assert!(original.all_dependencies_checked);
    assert_eq!(original.dependencies_checked, 2);
    assert_eq!(original.max_depth_examined, 2);
    let old = db.memory_snapshot();
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "correct".into(),
            operation: MemoryOperation::Update {
                record_id: leaf.record_id,
                expected_revision: 1,
                record: input("corrected"),
            },
        },
    )
    .unwrap();
    assert!(
        db.read_memory_record("scope", top.record_id)
            .unwrap()
            .is_none()
    );
    assert!(
        db.search_memory_semantic("scope", &search())
            .unwrap()
            .hits
            .is_empty()
    );
    assert_eq!(
        db.search_memory_lexical("scope", &query())
            .unwrap()
            .corpus_records,
        1
    );
    assert!(
        old.eligible(
            "scope",
            &old.record("scope", top.record_id).unwrap().unwrap()
        )
        .unwrap()
    );
    let reason = db
        .explain_memory_eligibility("scope", top.record_id)
        .unwrap()
        .unwrap();
    assert!(!reason.eligible);
    assert!(!reason.all_dependencies_checked);
    let failure = reason.first_failure.unwrap();
    assert_eq!(failure.record_id, leaf.record_id);
    assert_eq!(
        failure.reason,
        MemoryEligibilityReason::SourceRevisionChanged
    );
    assert_eq!(failure.actual_revision, Some(2));
    assert!(derive(&db, "stale-transitive", &[top.clone()]).is_err());
    drop(old);
    drop(db);
    let db = open(dir.path());
    assert!(
        !db.explain_memory_eligibility("scope", top.record_id)
            .unwrap()
            .unwrap()
            .eligible
    );
    assert!(
        db.explain_memory_eligibility("foreign", top.record_id)
            .unwrap()
            .is_none()
    );
}
#[test]
fn transitive_depth_and_unique_node_bounds_hold_for_shared_subgraphs() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let root = create(&db, "scope", "root");
    let mut chain = root.clone();
    for n in 1..=MAX_DERIVATION_DEPTH {
        chain = derive(&db, &format!("depth-{n}"), &[chain]).unwrap();
    }
    assert_eq!(
        db.explain_memory_eligibility("scope", chain.record_id)
            .unwrap()
            .unwrap()
            .max_depth_examined,
        8
    );
    assert!(matches!(
        derive(&db, "depth-9", &[chain]),
        Err(Error::ValidationError(_))
    ));
    let mut chain = root.clone();
    for n in 1..8 {
        chain = derive(&db, &format!("memo-depth-{n}"), &[chain]).unwrap();
    }
    // The same subtree appears shallow first and deeper later; memoization must
    // preserve its remaining height instead of incorrectly skipping the deep path.
    let parent = derive(&db, "memo-parent", &[chain.clone()]).unwrap();
    assert!(matches!(
        derive(&db, "memo-too-deep", &[chain, parent]),
        Err(Error::ValidationError(_))
    ));
    let leaves: Vec<_> = (0..48)
        .map(|n| create(&db, "scope", &format!("leaf-{n}")))
        .collect();
    let mids: Vec<_> = (0..16)
        .map(|n| derive(&db, &format!("mid-{n}"), &leaves[n * 3..n * 3 + 3]).unwrap())
        .collect();
    let top = derive(&db, "exactly-64", &mids).unwrap();
    let info = db
        .explain_memory_eligibility("scope", top.record_id)
        .unwrap()
        .unwrap();
    assert!(info.eligible);
    assert_eq!(info.dependencies_checked, 64);
    assert_eq!(info.dependency_work.records_examined, 64);
    assert!(matches!(
        derive(&db, "too-many-nodes", &[top]),
        Err(Error::ValidationError(_))
    ));
    let shared: Vec<_> = (0..16)
        .map(|n| derive(&db, &format!("shared-{n}"), &[root.clone()]).unwrap())
        .collect();
    let merged = derive(&db, "shared-top", &shared).unwrap();
    let info = db
        .explain_memory_eligibility("scope", merged.record_id)
        .unwrap()
        .unwrap();
    assert_eq!(info.dependencies_checked, 17);
    assert_eq!(info.dependency_work.records_examined, 17);
}
#[test]
fn eligibility_explains_current_visibility_missing_sources_and_cycles_without_payloads() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let s = create(&db, "scope", "sensitive payload");
    let d = derive(&db, "derived-sensitive", &[s.clone()]).unwrap();
    db.review_memory_record(
        "scope",
        &actor(),
        &MemoryReviewCommand {
            contract_version: 1,
            idempotency_key: "reject".into(),
            record_id: s.record_id,
            expected_revision: 1,
            disposition: MemoryReviewDisposition::Rejected,
            evidence_ref: "sensitive evidence".into(),
        },
    )
    .unwrap();
    let info = db
        .explain_memory_eligibility("scope", s.record_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        info.first_failure.unwrap().reason,
        MemoryEligibilityReason::Rejected
    );
    let mut raw = db
        .platform_record_locked("scope", d.record_id)
        .unwrap()
        .unwrap();
    raw.derivation.as_mut().unwrap().sources = vec![MemorySourceRef {
        record_id: d.record_id,
        revision: 1,
    }];
    let bytes = encode(&raw).unwrap();
    let index = RecordIndex {
        schema_version: 1,
        revision: 1,
        record_digest: digest(&bytes),
    };
    db.db
        .put_cf(
            db.cf(super::super::cf::EPISODES).unwrap(),
            record_key(0x10, "scope", d.record_id),
            bytes,
        )
        .unwrap();
    db.db
        .put_cf(
            db.cf(super::super::cf::EPISODE_INDEX).unwrap(),
            record_key(0x11, "scope", d.record_id),
            encode(&index).unwrap(),
        )
        .unwrap();
    let info = db
        .explain_memory_eligibility("scope", d.record_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        info.first_failure.as_ref().unwrap().reason,
        MemoryEligibilityReason::DependencyCycle
    );
    assert!(
        !String::from_utf8(encode(&info).unwrap())
            .unwrap()
            .contains("sensitive")
    );
    assert!(
        db.read_memory_record("scope", d.record_id)
            .unwrap()
            .is_none()
    );
    raw.derivation.as_mut().unwrap().sources = vec![MemorySourceRef {
        record_id: Uuid::new_v4(),
        revision: 1,
    }];
    let bytes = encode(&raw).unwrap();
    let index = RecordIndex {
        schema_version: 1,
        revision: 1,
        record_digest: digest(&bytes),
    };
    db.db
        .put_cf(
            db.cf(super::super::cf::EPISODES).unwrap(),
            record_key(0x10, "scope", d.record_id),
            bytes,
        )
        .unwrap();
    db.db
        .put_cf(
            db.cf(super::super::cf::EPISODE_INDEX).unwrap(),
            record_key(0x11, "scope", d.record_id),
            encode(&index).unwrap(),
        )
        .unwrap();
    assert_eq!(
        db.explain_memory_eligibility("scope", d.record_id)
            .unwrap()
            .unwrap()
            .first_failure
            .unwrap()
            .reason,
        MemoryEligibilityReason::SourceMissing
    );
}
