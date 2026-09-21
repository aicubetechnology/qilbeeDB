use super::super::semantic_tests::{actor, create, input, open};
use super::*;
use tempfile::TempDir;
pub(super) fn reference(r: &CommandReceipt) -> MemorySourceRef {
    MemorySourceRef {
        record_id: r.record_id,
        revision: r.revision,
    }
}
pub(super) fn assertion(source: &CommandReceipt, target: &CommandReceipt) -> MemoryRelationCommand {
    MemoryRelationCommand {
        contract_version: 1,
        idempotency_key: "assert-claim".into(),
        operation: MemoryRelationOperation::Assert {
            relation: MemoryRelationInput {
                evidence_sources: Vec::new(),
                source: reference(source),
                target: reference(target),
                kind: MemoryRelationKind::CausalClaim,
                provenance: RelationProvenance {
                    origin: RelationOrigin::ModelInference,
                    method: "external-extractor".into(),
                    method_revision: "prompt-v3".into(),
                    evidence_ref: "trace://fixture/observations".into(),
                    model: Some(RelationModelIdentity {
                        provider: "fixture-provider".into(),
                        model: "fixture-model".into(),
                        revision: "fixture-v1".into(),
                    }),
                },
                valid_from_millis: None,
                valid_until_millis: None,
            },
        },
    }
}
pub(super) fn change(id: Uuid, rev: u64, action: &str) -> MemoryRelationCommand {
    let evidence_ref = "trace://fixture/decision".into();
    let operation = match action {
        "retire" => MemoryRelationOperation::Retire {
            relation_id: id,
            expected_revision: rev,
            evidence_ref,
        },
        "restore" => MemoryRelationOperation::Restore {
            relation_id: id,
            expected_revision: rev,
            evidence_ref,
        },
        _ => MemoryRelationOperation::Review {
            relation_id: id,
            expected_revision: rev,
            evidence_ref,
            disposition: if action == "reject" {
                MemoryReviewDisposition::Rejected
            } else {
                MemoryReviewDisposition::Approved
            },
        },
    };
    MemoryRelationCommand {
        contract_version: 1,
        idempotency_key: format!("{action}-{rev}"),
        operation,
    }
}
#[test]
fn typed_relation_replay_history_retirement_and_review_survive_reopen() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let source = create(&db, "scope", "source");
    let target = create(&db, "scope", "target");
    let command = assertion(&source, &target);
    let first = db
        .apply_memory_relation_command("scope", &who, &command)
        .unwrap();
    let original = db
        .read_memory_relation("scope", first.relation_id)
        .unwrap()
        .unwrap();
    assert_eq!(original.reported_by, who);
    assert_eq!(original.input.kind, MemoryRelationKind::CausalClaim);
    assert_eq!(
        original.input.provenance.model.as_ref().unwrap().revision,
        "fixture-v1"
    );
    assert!(original.review.is_none());
    for (rev, action, visible) in [
        (1, "retire", false),
        (2, "restore", true),
        (3, "reject", false),
        (4, "retire", false),
        (5, "restore", false),
        (6, "approve", true),
    ] {
        let receipt = db
            .apply_memory_relation_command("scope", &who, &change(first.relation_id, rev, action))
            .unwrap();
        assert_eq!(receipt.revision, rev + 1);
        assert_eq!(
            db.read_memory_relation("scope", first.relation_id)
                .unwrap()
                .is_some(),
            visible
        );
        assert_eq!(
            db.memory_relation_revision("scope", first.relation_id, rev + 1)
                .unwrap()
                .unwrap()
                .receipt,
            receipt
        );
    }
    assert_eq!(
        db.memory_relation_revision("scope", first.relation_id, 1)
            .unwrap()
            .unwrap()
            .relation,
        original
    );
    assert_eq!(
        db.apply_memory_relation_command("scope", &actor(), &command)
            .unwrap(),
        first
    );
    drop(db);
    let db = open(dir.path());
    assert_eq!(
        db.apply_memory_relation_command("scope", &who, &command)
            .unwrap(),
        first
    );
    assert_eq!(
        db.read_memory_relation("scope", first.relation_id)
            .unwrap()
            .unwrap()
            .revision,
        7
    );
    assert!(
        db.inspect_memory_relation("other", first.relation_id)
            .unwrap()
            .is_none()
    );
    assert!(
        db.memory_relation_revision("other", first.relation_id, 1)
            .unwrap()
            .is_none()
    );
    let mut different = command;
    different.idempotency_key = "assert-claim".into();
    if let MemoryRelationOperation::Assert { ref mut relation } = different.operation {
        relation.kind = MemoryRelationKind::Supports;
    }
    assert!(matches!(
        db.apply_memory_relation_command("scope", &who, &different),
        Err(Error::ConstraintViolation(_))
    ));
}
#[test]
fn stale_worker_cannot_assert_or_restore_and_old_receipts_remain_exact() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let source = create(&db, "scope", "source");
    let target = create(&db, "scope", "target");
    let command = assertion(&source, &target);
    let first = db
        .apply_memory_relation_command("scope", &who, &command)
        .unwrap();
    let before = db.memory_snapshot();
    db.apply_memory_command(
        "scope",
        &who,
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "correction".into(),
            operation: MemoryOperation::Update {
                record_id: source.record_id,
                expected_revision: 1,
                record: input("corrected source"),
            },
        },
    )
    .unwrap();
    assert!(
        before
            .relation_eligibility(
                "scope",
                &before
                    .relation("scope", first.relation_id)
                    .unwrap()
                    .unwrap()
            )
            .unwrap()
            .eligible
    );
    let inspection = db
        .inspect_memory_relation("scope", first.relation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        inspection.eligibility.reason,
        RelationEligibilityReason::EndpointRevisionChanged
    );
    assert!(
        db.read_memory_relation("scope", first.relation_id)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.apply_memory_relation_command("scope", &who, &command)
            .unwrap(),
        first
    );
    let mut stale = command.clone();
    stale.idempotency_key = "late-worker".into();
    assert!(matches!(
        db.apply_memory_relation_command("scope", &who, &stale),
        Err(Error::TransactionConflict(_))
    ));
    db.apply_memory_relation_command("scope", &who, &change(first.relation_id, 1, "retire"))
        .unwrap();
    assert!(matches!(
        db.apply_memory_relation_command("scope", &who, &change(first.relation_id, 2, "restore")),
        Err(Error::TransactionConflict(_))
    ));
    assert_eq!(
        db.inspect_memory_relation("scope", first.relation_id)
            .unwrap()
            .unwrap()
            .relation
            .revision,
        2
    );
    assert!(
        db.memory_relation_revision("scope", first.relation_id, 3)
            .unwrap()
            .is_none()
    );
    assert!(
        db.apply_memory_relation_command("foreign", &who, &stale)
            .is_err()
    );
}
#[test]
fn invalid_model_identity_endpoints_temporal_order_and_validity_publish_nothing() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let source = create(&db, "scope", "source");
    let target = create(&db, "scope", "target");
    let who = actor();
    for case in 0..7 {
        let mut command = assertion(&source, &target);
        if let MemoryRelationOperation::Assert { ref mut relation } = command.operation {
            match case {
                0 => relation.provenance.model = None,
                1 => relation.target = relation.source.clone(),
                2 => relation.source.revision = 0,
                3 => relation.kind = MemoryRelationKind::TemporalBefore,
                4 => {
                    relation.valid_from_millis = Some(10);
                    relation.valid_until_millis = Some(10);
                }
                5 => relation.provenance.method = "\n".into(),
                _ => relation.target.record_id = Uuid::new_v4(),
            }
        }
        assert!(
            db.apply_memory_relation_command("scope", &who, &command)
                .is_err(),
            "case {case}"
        );
    }
    let first = db
        .apply_memory_relation_command("scope", &who, &assertion(&source, &target))
        .unwrap();
    assert_eq!(first.revision, 1);
}
#[test]
fn rejected_expired_deleted_and_transitively_invalid_endpoints_are_not_served() {
    for action in ["reject", "delete", "expire", "ancestor"] {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let who = actor();
        let source = create(&db, "scope", "source");
        let endpoint = if action == "ancestor" {
            db.apply_memory_command(
                "scope",
                &who,
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: "derived".into(),
                    operation: MemoryOperation::Derive {
                        record: input("derived"),
                        derivation: MemoryDerivation {
                            sources: vec![reference(&source)],
                            method: "fixture".into(),
                            method_revision: "v1".into(),
                            evidence_ref: "trace://derived".into(),
                        },
                    },
                },
            )
            .unwrap()
        } else {
            source.clone()
        };
        let target = create(&db, "scope", "target");
        let relation = db
            .apply_memory_relation_command("scope", &who, &assertion(&endpoint, &target))
            .unwrap();
        if action == "reject" || action == "ancestor" {
            db.review_memory_record(
                "scope",
                &who,
                &MemoryReviewCommand {
                    contract_version: 1,
                    idempotency_key: "review-source".into(),
                    record_id: source.record_id,
                    expected_revision: 1,
                    disposition: MemoryReviewDisposition::Rejected,
                    evidence_ref: "trace://rejection".into(),
                },
            )
            .unwrap();
        } else if action == "delete" {
            db.apply_memory_command(
                "scope",
                &who,
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
            let mut record = db
                .platform_record_locked("scope", source.record_id)
                .unwrap()
                .unwrap();
            record.payload.as_mut().unwrap().valid_until_millis =
                Some(chrono::Utc::now().timestamp_millis() + 1000);
            // A controlled server clock, not a waiting timing test, exercises expiration without a new write.
            let bytes = encode(&record).unwrap();
            db.db
                .put_cf(
                    db.cf(super::super::super::cf::EPISODES).unwrap(),
                    record_key(0x10, "scope", source.record_id),
                    &bytes,
                )
                .unwrap();
            db.db
                .put_cf(
                    db.cf(super::super::super::cf::EPISODE_INDEX).unwrap(),
                    record_key(0x11, "scope", source.record_id),
                    encode(&RecordIndex {
                        schema_version: 1,
                        revision: 1,
                        record_digest: digest(&bytes),
                    })
                    .unwrap(),
                )
                .unwrap();
            let mut snapshot = db.memory_snapshot();
            snapshot.now += 2000;
            let r = snapshot
                .relation("scope", relation.relation_id)
                .unwrap()
                .unwrap();
            assert!(!snapshot.relation_eligibility("scope", &r).unwrap().eligible);
            continue;
        }
        assert!(
            db.read_memory_relation("scope", relation.relation_id)
                .unwrap()
                .is_none(),
            "{action}"
        );
    }
}
#[test]
fn relation_clock_bounds_are_half_open_and_do_not_replace_endpoint_checks() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let source = create(&db, "scope", "s");
    let target = create(&db, "scope", "t");
    let mut command = assertion(&source, &target);
    if let MemoryRelationOperation::Assert { ref mut relation } = command.operation {
        relation.valid_from_millis = Some(100);
        relation.valid_until_millis = Some(200);
    }
    let receipt = db
        .apply_memory_relation_command("scope", &actor(), &command)
        .unwrap();
    for (now, reason) in [
        (99, RelationEligibilityReason::NotYetValid),
        (100, RelationEligibilityReason::Eligible),
        (199, RelationEligibilityReason::Eligible),
        (200, RelationEligibilityReason::Expired),
    ] {
        let mut snapshot = db.memory_snapshot();
        snapshot.now = now;
        let relation = snapshot
            .relation("scope", receipt.relation_id)
            .unwrap()
            .unwrap();
        assert_eq!(
            snapshot
                .relation_eligibility("scope", &relation)
                .unwrap()
                .reason,
            reason
        );
    }
}

#[test]
fn deepest_eligible_endpoint_retains_its_full_derivation_depth() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let base = create(&db, "scope", "base");
    let mut endpoint = base.clone();
    for depth in 1..=MAX_DERIVATION_DEPTH {
        endpoint = db
            .apply_memory_command(
                "scope",
                &who,
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: format!("depth-{depth}"),
                    operation: MemoryOperation::Derive {
                        record: input("derived endpoint"),
                        derivation: MemoryDerivation {
                            sources: vec![reference(&endpoint)],
                            method: "fixture".into(),
                            method_revision: "v1".into(),
                            evidence_ref: "trace://depth-boundary".into(),
                        },
                    },
                },
            )
            .unwrap();
    }
    let target = create(&db, "scope", "other");
    let receipt = db
        .apply_memory_relation_command("scope", &who, &assertion(&endpoint, &target))
        .unwrap();
    let inspection = db
        .inspect_memory_relation("scope", receipt.relation_id)
        .unwrap()
        .unwrap();
    assert!(inspection.eligibility.eligible);
    assert_eq!(
        inspection.eligibility.dependency_work.records_examined,
        MAX_DERIVATION_DEPTH
    );
    db.review_memory_record(
        "scope",
        &who,
        &MemoryReviewCommand {
            contract_version: 1,
            idempotency_key: "reject-base".into(),
            record_id: base.record_id,
            expected_revision: 1,
            disposition: MemoryReviewDisposition::Rejected,
            evidence_ref: "trace://review".into(),
        },
    )
    .unwrap();
    assert!(
        db.read_memory_relation("scope", receipt.relation_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn temporal_assertion_checks_event_time_not_creation_order_and_preserves_claim_kind() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    // The later event is persisted first; insertion order is not temporal evidence.
    let later = create(&db, "scope", "later");
    let mut earlier_input = input("earlier");
    earlier_input.event_time_millis -= 1;
    let earlier = db
        .apply_memory_command(
            "scope",
            &who,
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "earlier".into(),
                operation: MemoryOperation::Create {
                    record: earlier_input,
                },
            },
        )
        .unwrap();
    let mut command = assertion(&earlier, &later);
    if let MemoryRelationOperation::Assert { ref mut relation } = command.operation {
        relation.kind = MemoryRelationKind::TemporalBefore;
        relation.provenance.origin = RelationOrigin::ToolObservation;
        relation.provenance.model = None;
    }
    let receipt = db
        .apply_memory_relation_command("scope", &who, &command)
        .unwrap();
    let stored = db
        .read_memory_relation("scope", receipt.relation_id)
        .unwrap()
        .unwrap();
    assert_eq!(stored.input.kind, MemoryRelationKind::TemporalBefore);
    assert!(stored.input.provenance.model.is_none());
    command.idempotency_key = "reversed".into();
    if let MemoryRelationOperation::Assert { ref mut relation } = command.operation {
        std::mem::swap(&mut relation.source, &mut relation.target);
    }
    assert!(matches!(
        db.apply_memory_relation_command("scope", &who, &command),
        Err(Error::ValidationError(_))
    ));
    assert!(matches!(
        db.apply_memory_relation_command("scope", &who, &change(receipt.relation_id, 1, "restore")),
        Err(Error::ValidationError(_))
    ));
    db.apply_memory_relation_command("scope", &who, &change(receipt.relation_id, 1, "retire"))
        .unwrap();
    assert!(matches!(
        db.apply_memory_relation_command("scope", &who, &change(receipt.relation_id, 2, "retire")),
        Err(Error::ValidationError(_))
    ));
    assert!(
        db.memory_relation_revision("scope", receipt.relation_id, 3)
            .unwrap()
            .is_none()
    );
}

#[test]
fn relation_endpoint_byte_exhaustion_returns_error_and_does_not_reserve_receipt() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let a = create(&db, "scope", "large");
    let b = create(&db, "scope", "small");
    let original = db
        .platform_record_locked("scope", a.record_id)
        .unwrap()
        .unwrap();
    let mut oversized = original.clone();
    oversized.payload.as_mut().unwrap().content.primary = "x".repeat(MAX_MEMORY_READ_BYTES);
    let replace = |record: &MemoryRecord| {
        let bytes = encode(record).unwrap();
        db.db
            .put_cf(
                db.cf(super::super::super::cf::EPISODES).unwrap(),
                record_key(0x10, "scope", a.record_id),
                &bytes,
            )
            .unwrap();
        db.db
            .put_cf(
                db.cf(super::super::super::cf::EPISODE_INDEX).unwrap(),
                record_key(0x11, "scope", a.record_id),
                encode(&RecordIndex {
                    schema_version: 1,
                    revision: 1,
                    record_digest: digest(&bytes),
                })
                .unwrap(),
            )
            .unwrap();
    };
    // Trusted storage callers can persist records larger than the HTTP body limit.
    replace(&oversized);
    let command = assertion(&a, &b);
    assert!(matches!(
        db.apply_memory_relation_command("scope", &who, &command),
        Err(Error::ValidationError(_))
    ));
    replace(&original);
    let receipt = db
        .apply_memory_relation_command("scope", &who, &command)
        .unwrap();
    replace(&oversized);
    assert!(matches!(
        db.read_memory_relation("scope", receipt.relation_id),
        Err(Error::ValidationError(_))
    ));
    // A committed receipt remains an exact acknowledgement, not a current read.
    assert_eq!(
        db.apply_memory_relation_command("scope", &who, &command)
            .unwrap(),
        receipt
    );
}
#[test]
fn missing_adjacency_or_corrupt_historical_provenance_fails_closed() {
    for history in [false, true] {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let source = create(&db, "scope", "s");
        let target = create(&db, "scope", "t");
        let who = actor();
        let first = db
            .apply_memory_relation_command("scope", &who, &assertion(&source, &target))
            .unwrap();
        let cf = db.cf(super::super::super::cf::AGENT_META).unwrap();
        if history {
            db.apply_memory_relation_command(
                "scope",
                &who,
                &change(first.relation_id, 1, "retire"),
            )
            .unwrap();
            let mut old = db
                .memory_relation_revision("scope", first.relation_id, 1)
                .unwrap()
                .unwrap();
            old.relation.input.provenance.evidence_ref = "trace://forged".into();
            db.db
                .put_cf(
                    cf,
                    history_key("scope", first.relation_id, 1),
                    encode(&old).unwrap(),
                )
                .unwrap();
            assert!(matches!(
                db.memory_relation_revision("scope", first.relation_id, 1),
                Err(Error::DataCorruption(_))
            ));
            assert!(matches!(
                db.apply_memory_relation_command("scope", &who, &assertion(&source, &target)),
                Err(Error::DataCorruption(_))
            ));
        } else {
            db.db
                .delete_cf(
                    cf,
                    adjacency_key(INCOMING, "scope", &reference(&target), first.relation_id),
                )
                .unwrap();
            assert!(matches!(
                db.read_memory_relation("scope", first.relation_id),
                Err(Error::DataCorruption(_))
            ));
            assert!(matches!(
                db.apply_memory_relation_command(
                    "scope",
                    &who,
                    &change(first.relation_id, 1, "retire")
                ),
                Err(Error::DataCorruption(_))
            ));
            assert!(
                db.memory_relation_revision("scope", first.relation_id, 2)
                    .unwrap()
                    .is_none()
            );
        }
    }
}
#[test]
fn concurrent_relation_updates_have_one_winner_and_retries_do_not_duplicate_edges() {
    let dir = TempDir::new().unwrap();
    let db = std::sync::Arc::new(open(dir.path()));
    let who = actor();
    let source = create(&db, "scope", "s");
    let target = create(&db, "scope", "t");
    let command = assertion(&source, &target);
    let receipts = std::thread::scope(|threads| {
        (0..8)
            .map(|_| {
                let db = &db;
                let who = &who;
                let command = &command;
                threads.spawn(move || {
                    db.apply_memory_relation_command("scope", who, command)
                        .unwrap()
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(receipts.iter().all(|r| r == &receipts[0]));
    let id = receipts[0].relation_id;
    let winners = std::thread::scope(|threads| {
        (0..8)
            .map(|i| {
                let db = &db;
                let who = &who;
                threads.spawn(move || {
                    let mut c = change(id, 1, "retire");
                    c.idempotency_key = format!("competing-{i}");
                    db.apply_memory_relation_command("scope", who, &c)
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|t| t.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(winners.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        winners
            .iter()
            .filter(|r| matches!(r, Err(Error::TransactionConflict(_))))
            .count(),
        7
    );
}
