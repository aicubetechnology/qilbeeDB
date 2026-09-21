//! Context dependencies must fence publication and every relation-serving path.
use super::super::semantic_tests::{actor, create, input, open};
use super::tests::{assertion, change, reference};
use super::*;
use tempfile::TempDir;

fn contextual(
    a: &CommandReceipt,
    b: &CommandReceipt,
    sources: Vec<MemorySourceRef>,
) -> MemoryRelationCommand {
    let mut command = assertion(a, b);
    if let MemoryRelationOperation::Assert { relation } = &mut command.operation {
        relation.kind = MemoryRelationKind::SameEntity;
        relation.evidence_sources = sources;
    }
    command
}
fn graph(ids: Vec<Uuid>) -> TypedMemoryGraphQuery {
    serde_json::from_value(serde_json::json!({"root_record_ids":ids,"direction":"outgoing"}))
        .unwrap()
}
fn search() -> GraphRetrievalQuery {
    serde_json::from_value(serde_json::json!({"limit":10,"ranking_version":"typed_path_balanced_v1","seed":{"mode":"lexical","text":"anchor"}})).unwrap()
}
fn derive(db: &RocksDbMemoryStorage, key: &str, sources: Vec<MemorySourceRef>) -> CommandReceipt {
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: key.into(),
            operation: MemoryOperation::Derive {
                record: input(key),
                derivation: MemoryDerivation {
                    sources,
                    method: "fixture".into(),
                    method_revision: "v1".into(),
                    evidence_ref: "trace://context".into(),
                },
            },
        },
    )
    .unwrap()
}
fn update(db: &RocksDbMemoryStorage, id: Uuid) {
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "context-correction".into(),
            operation: MemoryOperation::Update {
                record_id: id,
                expected_revision: 1,
                record: input("corrected context"),
            },
        },
    )
    .unwrap();
}

#[test]
fn third_source_changes_remove_paths_preserve_audit_and_fence_late_publication() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let b = create(&db, "scope", "neighbor");
    let evidence = create(&db, "scope", "context");
    let command = contextual(&a, &b, vec![reference(&evidence)]);
    let receipt = db
        .apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    let original = db
        .memory_relation_revision("scope", receipt.relation_id, 1)
        .unwrap()
        .unwrap();
    let before = db.memory_snapshot();
    let g = db
        .read_memory_typed_graph("scope", &graph(vec![a.record_id]))
        .unwrap();
    assert_eq!(g.nodes.len(), 2); // Context is evidence, not an extra topology neighbor.
    assert_eq!(g.edges.len(), 1);
    assert_eq!(g.coverage.dependency_work.records_examined, 1);
    assert_eq!(
        db.search_memory_graph("scope", &search())
            .unwrap()
            .hits
            .len(),
        2
    );
    update(&db, evidence.record_id);
    assert!(
        before
            .relation_eligibility("scope", &original.relation)
            .unwrap()
            .eligible
    );
    drop(before);
    let inspected = db
        .inspect_memory_relation("scope", receipt.relation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        inspected.eligibility.reason,
        RelationEligibilityReason::EvidenceUnavailable
    );
    assert!(inspected.eligibility.endpoint.is_none());
    let failure = inspected.eligibility.evidence_failure.unwrap();
    assert_eq!(failure.record_id, evidence.record_id);
    assert_eq!(
        failure.reason,
        MemoryEligibilityReason::SourceRevisionChanged
    );
    assert_eq!(failure.expected_revision, Some(1));
    assert_eq!(failure.actual_revision, Some(2));
    assert!(
        db.read_memory_relation("scope", receipt.relation_id)
            .unwrap()
            .is_none()
    );
    for q in [
        graph(vec![a.record_id]),
        graph(vec![a.record_id, b.record_id]),
    ] {
        let g = db.read_memory_typed_graph("scope", &q).unwrap();
        assert!(g.edges.is_empty());
        assert!(g.coverage.complete);
    }
    let page = db.search_memory_graph("scope", &search()).unwrap();
    assert_eq!(page.hits.len(), 1);
    assert_eq!(page.hits[0].record.record_id, a.record_id);
    assert!(page.relations.is_empty());
    assert_eq!(
        db.memory_relation_revision("scope", receipt.relation_id, 1)
            .unwrap()
            .unwrap()
            .receipt,
        original.receipt
    );
    assert_eq!(
        db.apply_memory_relation_command("scope", &actor(), &command)
            .unwrap(),
        receipt
    );
    let mut stale = command.clone();
    stale.idempotency_key = "late-context-worker".into();
    assert!(matches!(
        db.apply_memory_relation_command("scope", &actor(), &stale),
        Err(Error::TransactionConflict(_))
    ));
    db.apply_memory_relation_command("scope", &actor(), &change(receipt.relation_id, 1, "retire"))
        .unwrap();
    assert!(matches!(
        db.apply_memory_relation_command(
            "scope",
            &actor(),
            &change(receipt.relation_id, 2, "restore")
        ),
        Err(Error::TransactionConflict(_))
    ));
    drop(db);
    let db = open(dir.path());
    assert_eq!(
        db.apply_memory_relation_command("scope", &actor(), &command)
            .unwrap(),
        receipt
    );
    assert_eq!(
        db.memory_relation_revision("scope", receipt.relation_id, 1)
            .unwrap()
            .unwrap()
            .relation,
        original.relation
    );
    assert!(
        db.memory_relation_revision("scope", receipt.relation_id, 3)
            .unwrap()
            .is_none()
    );
    assert!(
        db.read_memory_typed_graph("scope", &graph(vec![a.record_id]))
            .unwrap()
            .edges
            .is_empty()
    );
}

#[test]
fn legacy_wire_bytes_and_idempotency_digests_remain_unchanged() {
    // The exact field order and omitted field reflect the pre-extension encoder.
    let legacy = r#"{"contract_version":1,"idempotency_key":"old-command","operation":{"type":"assert","relation":{"source":{"record_id":"018f0000-0000-4000-8000-000000000001","revision":2},"target":{"record_id":"018f0000-0000-4000-8000-000000000002","revision":1},"kind":"supports","provenance":{"origin":"tool_observation","method":"fixture","method_revision":"v1","evidence_ref":"trace://fixture","model":null},"valid_from_millis":null,"valid_until_millis":null}}}"#;
    let decoded: MemoryRelationCommand = serde_json::from_str(legacy).unwrap();
    assert_eq!(encode(&decoded).unwrap(), legacy.as_bytes());
    let mut with_empty: serde_json::Value = serde_json::from_str(legacy).unwrap();
    with_empty["operation"]["relation"]["evidence_sources"] = serde_json::json!([]);
    let empty: MemoryRelationCommand = serde_json::from_value(with_empty).unwrap();
    assert_eq!(digest(&encode(&empty).unwrap()), digest(legacy.as_bytes()));
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let command = assertion(&a, &b);
    let receipt = db
        .apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    let old = db
        .memory_relation_revision("scope", receipt.relation_id, 1)
        .unwrap()
        .unwrap();
    assert!(
        !String::from_utf8(encode(&old).unwrap())
            .unwrap()
            .contains("evidence_sources")
    );
    drop(db);
    let db = open(dir.path());
    assert_eq!(
        db.apply_memory_relation_command("scope", &actor(), &command)
            .unwrap(),
        receipt
    );
    let inspected = db
        .inspect_memory_relation("scope", receipt.relation_id)
        .unwrap()
        .unwrap();
    assert!(inspected.eligibility.eligible);
    assert_eq!(inspected.eligibility.dependency_work.records_examined, 0);
    assert!(
        serde_json::to_value(inspected.eligibility)
            .unwrap()
            .get("evidence_failure")
            .is_none()
    );
}

#[test]
fn invalid_or_foreign_context_cannot_publish_and_context_binds_idempotency() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let c = create(&db, "scope", "c");
    let foreign = create(&db, "other", "foreign");
    let too_many = (0..17)
        .map(|_| MemorySourceRef {
            record_id: Uuid::new_v4(),
            revision: 1,
        })
        .collect();
    for refs in [
        vec![reference(&a)],
        vec![reference(&b)],
        vec![reference(&c), reference(&c)],
        vec![MemorySourceRef {
            record_id: c.record_id,
            revision: 0,
        }],
        too_many,
    ] {
        assert!(matches!(
            db.apply_memory_relation_command("scope", &actor(), &contextual(&a, &b, refs)),
            Err(Error::ValidationError(_))
        ));
    }
    for refs in [
        vec![reference(&foreign)],
        vec![MemorySourceRef {
            record_id: Uuid::new_v4(),
            revision: 1,
        }],
        vec![MemorySourceRef {
            record_id: c.record_id,
            revision: 2,
        }],
    ] {
        assert!(matches!(
            db.apply_memory_relation_command("scope", &actor(), &contextual(&a, &b, refs)),
            Err(Error::TransactionConflict(_))
        ));
    }
    let command = contextual(&a, &b, vec![reference(&c)]);
    db.apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    assert!(matches!(
        db.apply_memory_relation_command("scope", &actor(), &contextual(&a, &b, Vec::new())),
        Err(Error::ConstraintViolation(_))
    ));
}

#[test]
fn rejection_deletion_and_transitive_context_changes_invalidate_without_edge_mutation() {
    for action in ["reject", "delete", "ancestor"] {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let a = create(&db, "scope", "anchor");
        let b = create(&db, "scope", "neighbor");
        let source = create(&db, "scope", "source");
        let context = if action == "ancestor" {
            derive(&db, "derived", vec![reference(&source)])
        } else {
            source.clone()
        };
        let receipt = db
            .apply_memory_relation_command(
                "scope",
                &actor(),
                &contextual(&a, &b, vec![reference(&context)]),
            )
            .unwrap();
        if action == "delete" {
            db.apply_memory_command(
                "scope",
                &actor(),
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: "delete".into(),
                    operation: MemoryOperation::Delete {
                        record_id: source.record_id,
                        expected_revision: 1,
                    },
                },
            )
            .unwrap();
        } else {
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
        }
        let inspected = db
            .inspect_memory_relation("scope", receipt.relation_id)
            .unwrap()
            .unwrap();
        assert_eq!(inspected.relation.revision, 1);
        assert_eq!(
            inspected.eligibility.evidence_failure.unwrap().record_id,
            source.record_id
        );
        assert!(
            db.read_memory_typed_graph("scope", &graph(vec![a.record_id]))
                .unwrap()
                .edges
                .is_empty(),
            "{action}"
        );
        assert_eq!(
            db.search_memory_graph("scope", &search())
                .unwrap()
                .hits
                .len(),
            1,
            "{action}"
        );
        assert!(
            db.read_memory_relation("scope", receipt.relation_id)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn context_expiry_uses_snapshot_clock_even_without_a_change_event() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "anchor");
    let b = create(&db, "scope", "neighbor");
    let expiry = chrono::Utc::now().timestamp_millis() + 86_400_000;
    let mut payload = input("temporary context");
    payload.valid_until_millis = Some(expiry);
    let c = db
        .apply_memory_command(
            "scope",
            &actor(),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "temporary".into(),
                operation: MemoryOperation::Create { record: payload },
            },
        )
        .unwrap();
    let receipt = db
        .apply_memory_relation_command("scope", &actor(), &contextual(&a, &b, vec![reference(&c)]))
        .unwrap();
    for now in [expiry - 1, expiry] {
        let mut snapshot = db.memory_snapshot();
        snapshot.now = now;
        let relation = snapshot
            .relation("scope", receipt.relation_id)
            .unwrap()
            .unwrap();
        let eligibility = snapshot.relation_eligibility("scope", &relation).unwrap();
        assert_eq!(eligibility.eligible, now < expiry);
        let g = snapshot
            .typed_graph("scope", &graph(vec![a.record_id]))
            .unwrap();
        assert_eq!(g.edges.len(), usize::from(now < expiry));
        if now == expiry {
            assert_eq!(
                eligibility.evidence_failure.unwrap().reason,
                MemoryEligibilityReason::Expired
            );
        }
        assert_eq!(relation.revision, 1);
    }
}

#[test]
fn context_shares_dependency_cache_and_does_not_create_false_depth_cuts() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let c = create(&db, "scope", "c");
    for n in 0..8 {
        let mut command = contextual(&a, &b, vec![reference(&c)]);
        command.idempotency_key = format!("parallel-{n}");
        db.apply_memory_relation_command("scope", &actor(), &command)
            .unwrap();
    }
    let full = db
        .read_memory_typed_graph("scope", &graph(vec![a.record_id]))
        .unwrap();
    assert_eq!(full.edges.len(), 8);
    assert_eq!(full.coverage.dependency_work.records_examined, 1);
    let mut bounded = graph(vec![a.record_id]);
    bounded.max_depth = 0;
    assert!(
        !db.read_memory_typed_graph("scope", &bounded)
            .unwrap()
            .coverage
            .complete
    );
    update(&db, c.record_id);
    let after = db.read_memory_typed_graph("scope", &bounded).unwrap();
    assert!(after.coverage.complete); // Invalid context must be checked before deferred-edge cuts.
    assert!(after.edges.is_empty());
    assert_eq!(after.coverage.dependency_work.records_examined, 1);
}

#[test]
fn context_walk_preserves_depth_eight_and_combined_sixty_four_node_limit() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let mut chain = create(&db, "scope", "base");
    for n in 0..8 {
        chain = derive(&db, &format!("chain-{n}"), vec![reference(&chain)]);
    }
    let cmd = contextual(&a, &b, vec![reference(&chain)]);
    let r = db
        .apply_memory_relation_command("scope", &actor(), &cmd)
        .unwrap();
    assert_eq!(
        db.inspect_memory_relation("scope", r.relation_id)
            .unwrap()
            .unwrap()
            .eligibility
            .dependency_work
            .records_examined,
        9
    );
    let mut roots = Vec::new();
    for n in 0..16 {
        let leaves: Vec<_> = (0..3)
            .map(|i| reference(&create(&db, "scope", &format!("leaf-{n}-{i}"))))
            .collect();
        roots.push(reference(&derive(&db, &format!("root-{n}"), leaves)));
    }
    let mut at_limit = contextual(&a, &b, roots.clone());
    at_limit.idempotency_key = "at-node-limit".into();
    let r = db
        .apply_memory_relation_command("scope", &actor(), &at_limit)
        .unwrap();
    assert_eq!(
        db.inspect_memory_relation("scope", r.relation_id)
            .unwrap()
            .unwrap()
            .eligibility
            .dependency_work
            .records_examined,
        64
    );
    let last = &roots[15];
    let old = db
        .platform_record_locked("scope", last.record_id)
        .unwrap()
        .unwrap();
    let mut extended = old.derivation.unwrap().sources;
    extended.push(reference(&create(&db, "scope", "extra-leaf")));
    roots[15] = reference(&derive(&db, "larger-root", extended));
    let mut excessive = contextual(&a, &b, roots);
    excessive.idempotency_key = "over-node-limit".into();
    assert!(matches!(
        db.apply_memory_relation_command("scope", &actor(), &excessive),
        Err(Error::ValidationError(_))
    ));
}

#[test]
#[ignore = "Owned crash fixture; exercised by context_receipt_survives_abrupt_process_termination"]
fn evidence_crash_child() {
    use std::io::Write;
    let path =
        std::env::var_os("QILBEEDB_RELATION_EVIDENCE_CRASH_DIR").expect("owned fixture directory");
    let root = std::path::PathBuf::from(path);
    let db = open(&root.join("db"));
    let a = create(&db, "scope", "anchor");
    let b = create(&db, "scope", "neighbor");
    let c = create(&db, "scope", "context");
    let command = contextual(&a, &b, vec![reference(&c)]);
    let receipt = db
        .apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    let mut file = std::fs::File::create(root.join("ack.tmp")).unwrap();
    file.write_all(&serde_json::to_vec(&(a, c, command, receipt)).unwrap())
        .unwrap();
    file.sync_all().unwrap();
    std::fs::rename(root.join("ack.tmp"), root.join("ack.json")).unwrap();
    // The parent kills this process with RocksDB still open, after acknowledgement.
    loop {
        std::thread::park();
    }
}

#[test]
fn context_receipt_survives_abrupt_process_termination() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    struct ChildGuard(std::process::Child);
    impl Drop for ChildGuard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let dir = TempDir::new().unwrap();
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "storage::platform::relations::evidence_tests::evidence_crash_child",
                "--nocapture",
            ])
            .env("QILBEEDB_RELATION_EVIDENCE_CRASH_DIR", dir.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let ack = dir.path().join("ack.json");
    while !ack.exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "fixture exited before acknowledgement"
        );
        assert!(
            Instant::now() < deadline,
            "fixture did not acknowledge within 20 seconds"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    child.0.kill().unwrap();
    assert!(!child.0.wait().unwrap().success());
    let (a, c, command, receipt): (
        CommandReceipt,
        CommandReceipt,
        MemoryRelationCommand,
        MemoryRelationReceipt,
    ) = serde_json::from_slice(&std::fs::read(ack).unwrap()).unwrap();
    let db = open(&dir.path().join("db"));
    assert_eq!(
        db.apply_memory_relation_command("scope", &actor(), &command)
            .unwrap(),
        receipt
    );
    let stored = db
        .read_memory_relation("scope", receipt.relation_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.input.evidence_sources, vec![reference(&c)]);
    assert_eq!(
        db.memory_relation_revision("scope", receipt.relation_id, 1)
            .unwrap()
            .unwrap()
            .receipt,
        receipt
    );
    assert_eq!(
        db.read_memory_typed_graph("scope", &graph(vec![a.record_id]))
            .unwrap()
            .edges
            .len(),
        1
    );
    update(&db, c.record_id);
    assert!(
        db.read_memory_relation("scope", receipt.relation_id)
            .unwrap()
            .is_none()
    );
    assert!(
        db.read_memory_typed_graph("scope", &graph(vec![a.record_id]))
            .unwrap()
            .edges
            .is_empty()
    );
}
