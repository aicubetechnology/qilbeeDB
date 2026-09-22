use super::super::semantic_tests::{actor, create, input, open};
use super::*;
use tempfile::TempDir;
fn source(r: &CommandReceipt) -> MemorySourceRef {
    MemorySourceRef {
        record_id: r.record_id,
        revision: r.revision,
    }
}
fn spec(sources: Vec<MemorySourceRef>) -> ConsolidationSpec {
    ConsolidationSpec {
        sources,
        objective: "Extract supported entity and evidence relations".into(),
        policy_ref: "policy://fixture/v1".into(),
        extractor: RelationProvenance {
            origin: RelationOrigin::ModelInference,
            method: "fixture-extraction".into(),
            method_revision: "prompt-v1".into(),
            evidence_ref: "trace://manifest".into(),
            model: Some(RelationModelIdentity {
                provider: "fixture".into(),
                model: "fixture-model".into(),
                revision: "v1".into(),
            }),
        },
        max_relations: 16,
        max_attempts: 3,
        lease_millis: 1000,
        max_attempt_millis: 5000,
    }
}
fn command(key: &str, operation: ConsolidationOperation) -> ConsolidationCommand {
    ConsolidationCommand {
        contract_version: 1,
        idempotency_key: key.into(),
        operation,
    }
}
fn setup(
    db: &RocksDbMemoryStorage,
    who: &RecordAuthor,
) -> (Vec<CommandReceipt>, ConsolidationReceipt) {
    let sources: Vec<_> = ["anchor", "neighbor", "context"]
        .iter()
        .map(|s| create(db, "scope", s))
        .collect();
    let r = db
        .apply_consolidation_command(
            "scope",
            who,
            &command(
                "create",
                ConsolidationOperation::Create {
                    spec: spec(sources.iter().map(source).collect()),
                },
            ),
        )
        .unwrap();
    (sources, r)
}
fn read(db: &RocksDbMemoryStorage, who: &RecordAuthor, id: Uuid) -> ConsolidationInspection {
    db.inspect_consolidation_job("scope", &who.subject_id, id)
        .unwrap()
        .unwrap()
}
fn claim(
    db: &RocksDbMemoryStorage,
    who: &RecordAuthor,
    id: Uuid,
    revision: u64,
) -> ConsolidationInspection {
    db.apply_consolidation_command(
        "scope",
        who,
        &command(
            &format!("claim-{revision}"),
            ConsolidationOperation::Claim {
                job_id: id,
                expected_revision: revision,
                worker_id: "worker-one".into(),
            },
        ),
    )
    .unwrap();
    read(db, who, id)
}
fn assertion(
    a: &CommandReceipt,
    b: &CommandReceipt,
    kind: MemoryRelationKind,
) -> ConsolidationAssertion {
    ConsolidationAssertion {
        source: source(a),
        target: source(b),
        kind,
        valid_from_millis: None,
        valid_until_millis: None,
        evidence_ref: "trace://output".into(),
    }
}
fn publish(
    job: &ConsolidationJob,
    assertions: Vec<ConsolidationAssertion>,
) -> ConsolidationCommand {
    command(
        "publish",
        ConsolidationOperation::Publish {
            job_id: job.job_id,
            expected_revision: job.revision,
            fence: job.attempts.last().unwrap().fence,
            assertions,
            usage: ConsolidationUsage::Unknown,
            evidence_ref: "trace://execution".into(),
        },
    )
}
fn graph(db: &RocksDbMemoryStorage, id: Uuid) -> TypedMemoryGraph {
    db.read_memory_typed_graph(
        "scope",
        &serde_json::from_value(serde_json::json!({"root_record_ids":[id],"max_depth":8})).unwrap(),
    )
    .unwrap()
}
fn journal(db: &RocksDbMemoryStorage) -> RelationChangesPage {
    db.relation_changes(
        "scope",
        &RelationChangesQuery {
            after: None,
            through: None,
            limit: 100,
        },
    )
    .unwrap()
}
fn correct(db: &RocksDbMemoryStorage, id: Uuid) {
    db.apply_memory_command(
        "scope",
        &actor(),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "correction".into(),
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
fn publication_is_atomic_across_shared_adjacency_heads_journal_job_and_receipt() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let (s, r) = setup(&db, &who);
    let lease = claim(&db, &who, r.job_id, 1);
    assert!(lease.lease_active);
    let assertions = vec![
        assertion(&s[0], &s[1], MemoryRelationKind::SameEntity),
        assertion(&s[0], &s[2], MemoryRelationKind::Supports),
        assertion(&s[1], &s[2], MemoryRelationKind::Contradicts),
        assertion(&s[0], &s[1], MemoryRelationKind::Supports),
    ];
    let command = publish(&lease.job, assertions);
    let receipt = db
        .apply_consolidation_command("scope", &who, &command)
        .unwrap();
    assert_eq!(receipt.revision, 3);
    let stored = read(&db, &who, r.job_id);
    assert_eq!(stored.job.status, ConsolidationStatus::Published);
    assert!(!stored.lease_active);
    assert_eq!(stored.job.output_receipts.len(), 4);
    assert_eq!(stored.job.attempts[0].usage, ConsolidationUsage::Unknown);
    let g = graph(&db, s[0].record_id);
    assert!(g.coverage.complete);
    assert_eq!(g.edges.len(), 4);
    assert_eq!(g.nodes.len(), 3);
    for edge in &g.edges {
        let expected: Vec<_> = s
            .iter()
            .map(source)
            .filter(|r| {
                r.record_id != edge.input.source.record_id
                    && r.record_id != edge.input.target.record_id
            })
            .collect();
        assert_eq!(edge.input.evidence_sources, expected);
        assert_eq!(edge.input.provenance.model, lease.job.spec.extractor.model);
    }
    let feed = journal(&db);
    assert!(feed.complete);
    assert_eq!(feed.changes.len(), 4);
    for (i, event) in feed.changes.iter().enumerate() {
        assert_eq!(event.cursor.sequence, i as u64 + 1);
        assert_eq!(
            event.change.relation_id,
            stored.job.output_receipts[i].relation_id
        );
    }
    assert_eq!(
        db.apply_consolidation_command("scope", &who, &command)
            .unwrap(),
        receipt
    );
    assert_eq!(journal(&db).changes.len(), 4);
    drop(db);
    let db = open(dir.path());
    assert_eq!(
        db.apply_consolidation_command("scope", &who, &command)
            .unwrap(),
        receipt
    );
    assert_eq!(graph(&db, s[0].record_id).edges.len(), 4);
    correct(&db, s[2].record_id);
    assert!(graph(&db, s[0].record_id).edges.is_empty());
    let historic = db
        .consolidation_job_revision("scope", &who.subject_id, r.job_id, 3)
        .unwrap()
        .unwrap();
    assert_eq!(historic.receipt, receipt);
    assert_eq!(historic.job.output_receipts.len(), 4);
}

#[test]
fn invalid_last_output_drops_every_prepared_relation_and_does_not_reserve_receipt() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let (s, r) = setup(&db, &who);
    let lease = claim(&db, &who, r.job_id, 1);
    db.activate_relation_changes("scope").unwrap();
    let invalid = publish(
        &lease.job,
        vec![
            assertion(&s[0], &s[1], MemoryRelationKind::SameEntity),
            assertion(&s[1], &s[1], MemoryRelationKind::Supports),
        ],
    );
    assert!(matches!(
        db.apply_consolidation_command("scope", &who, &invalid),
        Err(Error::ValidationError(_))
    ));
    assert_eq!(read(&db, &who, r.job_id).job.revision, 2);
    assert!(journal(&db).changes.is_empty());
    assert!(graph(&db, s[0].record_id).edges.is_empty());
    assert!(
        db.consolidation_job_revision("scope", &who.subject_id, r.job_id, 3)
            .unwrap()
            .is_none()
    );
    let valid = publish(
        &lease.job,
        vec![assertion(&s[0], &s[1], MemoryRelationKind::SameEntity)],
    );
    db.apply_consolidation_command("scope", &who, &valid)
        .unwrap();
    assert_eq!(journal(&db).changes.len(), 1);
}

#[test]
fn six_competing_workers_have_one_claim_and_the_fence_binds_its_credential() {
    let dir = TempDir::new().unwrap();
    let db = std::sync::Arc::new(open(dir.path()));
    let who = actor();
    let (s, r) = setup(&db, &who);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
    let workers: Vec<_> = (0..6)
        .map(|n| {
            let db = db.clone();
            let who = who.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                db.apply_consolidation_command(
                    "scope",
                    &who,
                    &command(
                        &format!("claim-{n}"),
                        ConsolidationOperation::Claim {
                            job_id: r.job_id,
                            expected_revision: 1,
                            worker_id: format!("worker-{n}"),
                        },
                    ),
                )
            })
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter(|r| r.is_err())
            .all(|r| matches!(r, Err(Error::TransactionConflict(_))))
    );
    let current = read(&db, &who, r.job_id);
    let cmd = publish(
        &current.job,
        vec![assertion(&s[0], &s[1], MemoryRelationKind::SameEntity)],
    );
    let another = RecordAuthor {
        credential_id: Uuid::new_v4(),
        subject_id: who.subject_id.clone(),
    };
    assert!(matches!(
        db.apply_consolidation_command("scope", &another, &cmd),
        Err(Error::TransactionConflict(_))
    ));
    db.apply_consolidation_command("scope", &who, &cmd).unwrap();
}

#[test]
fn restart_fences_previous_leases_and_recovery_preserves_unknown_consumption() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let (s, r) = setup(&db, &who);
    let lease = claim(&db, &who, r.job_id, 1);
    let old = publish(
        &lease.job,
        vec![assertion(&s[0], &s[1], MemoryRelationKind::SameEntity)],
    );
    drop(db);
    let db = open(dir.path());
    let restart = read(&db, &who, r.job_id);
    assert!(!restart.lease_active);
    assert!(restart.recoverable);
    assert!(matches!(
        db.apply_consolidation_command("scope", &who, &old),
        Err(Error::TransactionConflict(_))
    ));
    let recovery = command(
        "recover",
        ConsolidationOperation::RecoverExpired {
            job_id: r.job_id,
            expected_revision: 2,
            evidence_ref: "trace://restart".into(),
        },
    );
    db.apply_consolidation_command("scope", &who, &recovery)
        .unwrap();
    let closed = read(&db, &who, r.job_id);
    assert_eq!(closed.job.status, ConsolidationStatus::Ready);
    assert_eq!(
        closed.job.attempts[0].outcome,
        ConsolidationOutcome::Unknown
    );
    assert_eq!(closed.job.attempts[0].usage, ConsolidationUsage::Unknown);
    let next = claim(&db, &who, r.job_id, 3);
    assert_ne!(next.job.attempts[1].fence, lease.job.attempts[0].fence);
    let mut stale = old;
    if let ConsolidationOperation::Publish {
        ref mut expected_revision,
        ..
    } = stale.operation
    {
        *expected_revision = 4;
    }
    assert!(matches!(
        db.apply_consolidation_command("scope", &who, &stale),
        Err(Error::TransactionConflict(_))
    ));
    db.apply_consolidation_command("scope", &who, &publish(&next.job, Vec::new()))
        .unwrap();
    let usage = ConsolidationUsage::Reported {
        model_calls: 1,
        input_tokens: 100,
        output_tokens: 10,
        cost_microusd: Some(25),
    };
    db.apply_consolidation_command(
        "scope",
        &who,
        &command(
            "provider-reconciliation",
            ConsolidationOperation::ReconcileUsage {
                job_id: r.job_id,
                expected_revision: 5,
                attempt_number: 1,
                usage: usage.clone(),
                evidence_ref: "provider-receipt://request-1".into(),
            },
        ),
    )
    .unwrap();
    let current = read(&db, &who, r.job_id);
    assert_eq!(current.job.status, ConsolidationStatus::Published);
    assert_eq!(current.job.attempts[0].usage, usage);
    assert_eq!(
        current.job.attempts[0].outcome,
        ConsolidationOutcome::Unknown
    );
    assert_eq!(
        db.consolidation_job_revision("scope", &who.subject_id, r.job_id, 3)
            .unwrap()
            .unwrap()
            .job
            .attempts[0]
            .usage,
        ConsolidationUsage::Unknown
    );
}

#[test]
fn source_changes_block_publication_without_losing_observed_failure_or_usage() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let (s, r) = setup(&db, &who);
    let lease = claim(&db, &who, r.job_id, 1);
    correct(&db, s[2].record_id);
    assert!(matches!(
        db.apply_consolidation_command(
            "scope",
            &who,
            &publish(
                &lease.job,
                vec![assertion(&s[0], &s[1], MemoryRelationKind::SameEntity)]
            )
        ),
        Err(Error::TransactionConflict(_))
    ));
    assert!(journal(&db).changes.is_empty());
    assert_eq!(read(&db, &who, r.job_id).job.revision, 2);
    let usage = ConsolidationUsage::Reported {
        model_calls: 1,
        input_tokens: 100,
        output_tokens: 50,
        cost_microusd: None,
    };
    db.apply_consolidation_command(
        "scope",
        &who,
        &command(
            "failed",
            ConsolidationOperation::Fail {
                job_id: r.job_id,
                expected_revision: 2,
                fence: lease.job.attempts[0].fence,
                usage: usage.clone(),
                evidence_ref: "trace://source-revision-conflict".into(),
            },
        ),
    )
    .unwrap();
    let current = read(&db, &who, r.job_id);
    assert_eq!(current.job.attempts[0].usage, usage);
    assert!(current.source_failure.is_some());
    assert!(matches!(
        db.apply_consolidation_command(
            "scope",
            &who,
            &command(
                "reclaim",
                ConsolidationOperation::Claim {
                    job_id: r.job_id,
                    expected_revision: 3,
                    worker_id: "worker".into()
                }
            )
        ),
        Err(Error::TransactionConflict(_))
    ));
}

#[test]
fn cancellation_fences_publication_without_claiming_external_execution_stopped() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let (s, r) = setup(&db, &who);
    let lease = claim(&db, &who, r.job_id, 1);
    db.apply_consolidation_command(
        "scope",
        &who,
        &command(
            "cancel",
            ConsolidationOperation::Cancel {
                job_id: r.job_id,
                expected_revision: 2,
                evidence_ref: "policy://cancel".into(),
            },
        ),
    )
    .unwrap();
    let job = read(&db, &who, r.job_id).job;
    assert_eq!(job.status, ConsolidationStatus::Cancelled);
    assert_eq!(job.attempts[0].outcome, ConsolidationOutcome::Unknown);
    assert_eq!(job.attempts[0].usage, ConsolidationUsage::Unknown);
    assert!(matches!(
        db.apply_consolidation_command(
            "scope",
            &who,
            &publish(
                &job,
                vec![assertion(&s[0], &s[1], MemoryRelationKind::Supports)]
            )
        ),
        Err(Error::TransactionConflict(_))
    ));
    assert!(journal(&db).changes.is_empty());
    assert_eq!(lease.job.revision, 2);
}

#[test]
fn exhausted_attempts_cannot_be_reclaimed_and_cross_owner_reads_are_unavailable() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let mut specification = spec(vec![source(&a), source(&b)]);
    specification.max_attempts = 1;
    let r = db
        .apply_consolidation_command(
            "scope",
            &who,
            &command(
                "create",
                ConsolidationOperation::Create {
                    spec: specification,
                },
            ),
        )
        .unwrap();
    let lease = claim(&db, &who, r.job_id, 1);
    db.apply_consolidation_command(
        "scope",
        &who,
        &command(
            "failure",
            ConsolidationOperation::Fail {
                job_id: r.job_id,
                expected_revision: 2,
                fence: lease.job.attempts[0].fence,
                usage: ConsolidationUsage::Unknown,
                evidence_ref: "trace://failure".into(),
            },
        ),
    )
    .unwrap();
    assert_eq!(
        read(&db, &who, r.job_id).job.status,
        ConsolidationStatus::Exhausted
    );
    assert!(
        db.inspect_consolidation_job("other", &who.subject_id, r.job_id)
            .unwrap()
            .is_none()
    );
    assert!(
        db.inspect_consolidation_job("scope", "another-subject", r.job_id)
            .unwrap()
            .is_none()
    );
    assert!(
        db.consolidation_job_revision("scope", "another-subject", r.job_id, 1)
            .unwrap()
            .is_none()
    );
    let retry = command(
        "retry",
        ConsolidationOperation::Claim {
            job_id: r.job_id,
            expected_revision: 3,
            worker_id: "worker".into(),
        },
    );
    assert!(matches!(
        db.apply_consolidation_command("scope", &who, &retry),
        Err(Error::TransactionConflict(_))
    ));
}

#[test]
fn leases_expire_at_the_upper_bound_and_renewal_retains_the_attempt_fence() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let (_, r) = setup(&db, &who);
    let lease = claim(&db, &who, r.job_id, 1);
    let attempt = &lease.job.attempts[0];
    assert!(lease_active(
        &lease.job,
        db.consolidation_incarnation,
        attempt.expires_at_millis - 1
    ));
    assert!(!lease_active(
        &lease.job,
        db.consolidation_incarnation,
        attempt.expires_at_millis
    ));
    std::thread::sleep(std::time::Duration::from_millis(5));
    db.apply_consolidation_command(
        "scope",
        &who,
        &command(
            "renew",
            ConsolidationOperation::Renew {
                job_id: r.job_id,
                expected_revision: 2,
                fence: attempt.fence,
            },
        ),
    )
    .unwrap();
    let renewed = read(&db, &who, r.job_id);
    assert_eq!(renewed.job.attempts.len(), 1);
    assert_eq!(renewed.job.attempts[0].fence, attempt.fence);
    assert!(renewed.job.attempts[0].expires_at_millis > attempt.expires_at_millis);
    let recovery = command(
        "recover",
        ConsolidationOperation::RecoverExpired {
            job_id: r.job_id,
            expected_revision: 3,
            evidence_ref: "trace://timeout".into(),
        },
    );
    assert!(matches!(
        db.apply_consolidation_command("scope", &who, &recovery),
        Err(Error::TransactionConflict(_))
    ));
    let until = renewed.job.attempts[0].expires_at_millis - chrono::Utc::now().timestamp_millis();
    if until > 0 {
        std::thread::sleep(std::time::Duration::from_millis(until as u64 + 1));
    }
    db.apply_consolidation_command("scope", &who, &recovery)
        .unwrap();
    assert_eq!(
        read(&db, &who, r.job_id).job.attempts[0].outcome,
        ConsolidationOutcome::Unknown
    );
}

#[test]
fn discovery_advances_across_filtered_jobs_and_never_scans_other_owners() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let (s, first) = setup(&db, &who);
    let mut all = BTreeSet::from([first.job_id]);
    for i in 0..8 {
        let r = db
            .apply_consolidation_command(
                "scope",
                &who,
                &command(
                    &format!("create-{i}"),
                    ConsolidationOperation::Create {
                        spec: spec(s.iter().map(source).collect()),
                    },
                ),
            )
            .unwrap();
        all.insert(r.job_id);
    }
    let foreign = RecordAuthor {
        credential_id: Uuid::new_v4(),
        subject_id: "another-owner".into(),
    };
    for i in 0..4 {
        db.apply_consolidation_command(
            "scope",
            &foreign,
            &command(
                &format!("foreign-{i}"),
                ConsolidationOperation::Create {
                    spec: spec(s.iter().map(source).collect()),
                },
            ),
        )
        .unwrap();
    }
    let mut query = ConsolidationQuery {
        limit: 2,
        scan_limit: 2,
        after: None,
        status: None,
    };
    let mut found = BTreeSet::new();
    let mut work = 0;
    loop {
        let page = db
            .query_consolidation_jobs("scope", &who.subject_id, &query)
            .unwrap();
        work += page.records_examined;
        for job in page.jobs {
            assert!(found.insert(job.job_id));
        }
        let Some(next) = page.next_after else { break };
        assert_ne!(Some(next), query.after);
        query.after = Some(next);
    }
    assert_eq!(found, all);
    assert_eq!(work, 9);
    query.after = None;
    query.status = Some(ConsolidationStatus::Published);
    let mut work = 0;
    loop {
        let page = db
            .query_consolidation_jobs("scope", &who.subject_id, &query)
            .unwrap();
        assert!(page.jobs.is_empty());
        work += page.records_examined;
        let Some(next) = page.next_after else { break };
        assert_ne!(Some(next), query.after);
        query.after = Some(next);
    }
    assert_eq!(work, 9);
    assert!(
        db.query_consolidation_jobs("foreign-scope", &who.subject_id, &query)
            .unwrap()
            .jobs
            .is_empty()
    );
}

#[test]
fn context_read_requires_current_revision_credential_and_live_fence() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let (s, r) = setup(&db, &who);
    let lease = claim(&db, &who, r.job_id, 1);
    let fence = lease.job.attempts[0].fence;
    let context = db
        .read_consolidation_context("scope", &who, r.job_id, 2, fence)
        .unwrap();
    assert_eq!(
        context
            .records
            .iter()
            .map(|r| r.record_id)
            .collect::<Vec<_>>(),
        s.iter().map(|r| r.record_id).collect::<Vec<_>>()
    );
    assert_eq!(context.dependency_work.records_examined, 3);
    let another = RecordAuthor {
        credential_id: Uuid::new_v4(),
        subject_id: who.subject_id.clone(),
    };
    assert!(matches!(
        db.read_consolidation_context("scope", &another, r.job_id, 2, fence),
        Err(Error::TransactionConflict(_))
    ));
    assert!(matches!(
        db.read_consolidation_context("scope", &who, r.job_id, 1, fence),
        Err(Error::TransactionConflict(_))
    ));
    assert!(matches!(
        db.read_consolidation_context("scope", &who, r.job_id, 2, Uuid::new_v4()),
        Err(Error::TransactionConflict(_))
    ));
    correct(&db, s[2].record_id);
    assert!(matches!(
        db.read_consolidation_context("scope", &who, r.job_id, 2, fence),
        Err(Error::TransactionConflict(_))
    ));
}

#[test]
fn job_manifest_bounds_and_foreign_sources_fail_before_reserving_commands() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let foreign = create(&db, "other", "foreign");
    for case in 0..8 {
        let mut specification = spec(vec![source(&a), source(&b)]);
        match case {
            0 => specification.sources.pop().map(|_| ()).unwrap(),
            1 => specification.sources[1] = source(&a),
            2 => specification.lease_millis = 999,
            3 => specification.max_attempt_millis = 999,
            4 => specification.max_attempts = 33,
            5 => specification.max_relations = 17,
            6 => specification.extractor.model = None,
            _ => specification.policy_ref = " ".into(),
        }
        assert!(matches!(
            db.apply_consolidation_command(
                "scope",
                &who,
                &command(
                    "validate",
                    ConsolidationOperation::Create {
                        spec: specification
                    }
                )
            ),
            Err(Error::ValidationError(_))
        ));
    }
    assert!(matches!(
        db.apply_consolidation_command(
            "scope",
            &who,
            &command(
                "validate",
                ConsolidationOperation::Create {
                    spec: spec(vec![source(&a), source(&foreign)])
                }
            )
        ),
        Err(Error::TransactionConflict(_))
    ));
    let accepted = command(
        "validate",
        ConsolidationOperation::Create {
            spec: spec(vec![source(&a), source(&b)]),
        },
    );
    let receipt = db
        .apply_consolidation_command("scope", &who, &accepted)
        .unwrap();
    assert_eq!(
        db.apply_consolidation_command("scope", &who, &accepted)
            .unwrap(),
        receipt
    );
    let mut changed = accepted;
    if let ConsolidationOperation::Create { ref mut spec } = changed.operation {
        spec.objective = "Changed intent".into();
    }
    assert!(matches!(
        db.apply_consolidation_command("scope", &who, &changed),
        Err(Error::ConstraintViolation(_))
    ));
}

#[test]
fn an_undeliverable_context_is_rejected_at_creation_with_no_job() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let mut sources = Vec::new();
    for i in 0..3 {
        let receipt = db
            .apply_memory_command(
                "scope",
                &who,
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: format!("large-{i}"),
                    operation: MemoryOperation::Create {
                        record: input(&"x".repeat(3 * 1024 * 1024)),
                    },
                },
            )
            .unwrap();
        sources.push(source(&receipt));
    }
    let result = db.apply_consolidation_command(
        "scope",
        &who,
        &command(
            "too-large",
            ConsolidationOperation::Create {
                spec: spec(sources),
            },
        ),
    );
    assert!(matches!(result,Err(Error::ValidationError(ref e)) if e.contains("8 MiB")));
    assert!(
        db.query_consolidation_jobs(
            "scope",
            &who.subject_id,
            &ConsolidationQuery {
                limit: 10,
                scan_limit: 100,
                after: None,
                status: None
            }
        )
        .unwrap()
        .jobs
        .is_empty()
    );
}

#[test]
fn missing_history_or_changed_canonical_job_fails_closed_on_read_and_replay() {
    for damage in ["history", "record"] {
        let dir = TempDir::new().unwrap();
        let db = open(dir.path());
        let who = actor();
        let (s, r) = setup(&db, &who);
        let cf = db.cf(crate::storage::cf::AGENT_META).unwrap();
        if damage == "history" {
            db.db
                .delete_cf(
                    cf,
                    history_key("scope", &who.subject_id, r.job_id, 1).unwrap(),
                )
                .unwrap();
        } else {
            let mut job = read(&db, &who, r.job_id).job;
            job.spec.objective = "Replaced objective".into();
            db.db
                .put_cf(
                    cf,
                    job_key(JOB, "scope", &who.subject_id, r.job_id).unwrap(),
                    encode(&job).unwrap(),
                )
                .unwrap();
        }
        assert!(matches!(
            db.inspect_consolidation_job("scope", &who.subject_id, r.job_id),
            Err(Error::DataCorruption(_))
        ));
        let original = command(
            "create",
            ConsolidationOperation::Create {
                spec: spec(s.iter().map(source).collect()),
            },
        );
        assert!(matches!(
            db.apply_consolidation_command("scope", &who, &original),
            Err(Error::DataCorruption(_))
        ));
    }
}

#[test]
#[ignore = "Owned subprocess fixture invoked by consolidation_crash_recovery"]
fn consolidation_crash_child() {
    use std::io::Write;
    let root =
        std::path::PathBuf::from(std::env::var_os("QILBEEDB_CONSOLIDATION_CRASH_DIR").unwrap());
    let published = std::env::var("QILBEEDB_CONSOLIDATION_CRASH_PHASE").unwrap() == "published";
    let db = open(&root.join("db"));
    let who = actor();
    let (s, r) = setup(&db, &who);
    let lease = claim(&db, &who, r.job_id, 1);
    let publication = publish(
        &lease.job,
        vec![
            assertion(&s[0], &s[1], MemoryRelationKind::SameEntity),
            assertion(&s[0], &s[2], MemoryRelationKind::Supports),
            assertion(&s[1], &s[2], MemoryRelationKind::Contradicts),
        ],
    );
    let acknowledged = if published {
        db.apply_consolidation_command("scope", &who, &publication)
            .unwrap()
    } else {
        r
    };
    let data = serde_json::to_vec(&(who, s, publication, acknowledged)).unwrap();
    let mut file = std::fs::File::create(root.join("ack.tmp")).unwrap();
    file.write_all(&data).unwrap();
    file.sync_all().unwrap();
    std::fs::rename(root.join("ack.tmp"), root.join("ack.json")).unwrap();
    loop {
        std::thread::park();
    }
}

#[test]
fn consolidation_crash_recovery_preserves_published_batches_and_fences_inflight_attempts() {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    struct Guard(std::process::Child);
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    for phase in ["claimed", "published"] {
        let dir = TempDir::new().unwrap();
        let mut child = Guard(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--ignored",
                    "--exact",
                    "storage::platform::consolidation::tests::consolidation_crash_child",
                    "--nocapture",
                ])
                .env("QILBEEDB_CONSOLIDATION_CRASH_DIR", dir.path())
                .env("QILBEEDB_CONSOLIDATION_CRASH_PHASE", phase)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let ack = dir.path().join("ack.json");
        let deadline = Instant::now() + Duration::from_secs(20);
        while !ack.exists() {
            assert!(child.0.try_wait().unwrap().is_none());
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(10));
        }
        child.0.kill().unwrap();
        assert!(!child.0.wait().unwrap().success());
        let (who, s, publication, receipt): (
            RecordAuthor,
            Vec<CommandReceipt>,
            ConsolidationCommand,
            ConsolidationReceipt,
        ) = serde_json::from_slice(&std::fs::read(ack).unwrap()).unwrap();
        let db = open(&dir.path().join("db"));
        let current = read(&db, &who, receipt.job_id);
        if phase == "published" {
            assert_eq!(current.job.status, ConsolidationStatus::Published);
            assert_eq!(current.job.output_receipts.len(), 3);
            assert_eq!(
                db.apply_consolidation_command("scope", &who, &publication)
                    .unwrap(),
                receipt
            );
            let g = graph(&db, s[0].record_id);
            assert_eq!(g.edges.len(), 3);
            assert!(g.coverage.complete);
            let feed = journal(&db);
            assert_eq!(feed.changes.len(), 3);
            assert_eq!(feed.high_watermark.unwrap().sequence, 3);
        } else {
            assert!(current.recoverable);
            assert_eq!(current.job.revision, 2);
            assert!(matches!(
                db.apply_consolidation_command("scope", &who, &publication),
                Err(Error::TransactionConflict(_))
            ));
            db.apply_consolidation_command(
                "scope",
                &who,
                &command(
                    "recover-crash",
                    ConsolidationOperation::RecoverExpired {
                        job_id: receipt.job_id,
                        expected_revision: 2,
                        evidence_ref: "trace://process-kill".into(),
                    },
                ),
            )
            .unwrap();
            let recovered = read(&db, &who, receipt.job_id);
            assert_eq!(recovered.job.attempts[0].usage, ConsolidationUsage::Unknown);
            assert_eq!(
                recovered.job.attempts[0].outcome,
                ConsolidationOutcome::Unknown
            );
            assert!(journal(&db).changes.is_empty());
            assert!(graph(&db, s[0].record_id).edges.is_empty());
        }
    }
}

#[test]
fn maximum_attempt_metadata_still_allows_final_publication_and_usage_correction() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let who = actor();
    let a = create(&db, "scope", "a");
    let b = create(&db, "scope", "b");
    let mut specification = spec(vec![source(&a), source(&b)]);
    specification.max_attempts = 32;
    specification.lease_millis = 30000;
    specification.max_attempt_millis = 60000;
    specification.objective = "\\".repeat(4096);
    specification.policy_ref = "\\".repeat(2048);
    specification.extractor.method = "\\".repeat(256);
    specification.extractor.method_revision = "\\".repeat(256);
    specification.extractor.evidence_ref = "\\".repeat(2048);
    let model = specification.extractor.model.as_mut().unwrap();
    model.provider = "\\".repeat(256);
    model.model = "\\".repeat(256);
    model.revision = "\\".repeat(256);
    let r = db
        .apply_consolidation_command(
            "scope",
            &who,
            &command(
                "create",
                ConsolidationOperation::Create {
                    spec: specification,
                },
            ),
        )
        .unwrap();
    let mut revision = 1;
    let usage = ConsolidationUsage::Reported {
        model_calls: u64::MAX,
        input_tokens: u64::MAX,
        output_tokens: u64::MAX,
        cost_microusd: Some(u64::MAX),
    };
    for attempt in 1..=32 {
        db.apply_consolidation_command(
            "scope",
            &who,
            &command(
                &format!("claim-{attempt}"),
                ConsolidationOperation::Claim {
                    job_id: r.job_id,
                    expected_revision: revision,
                    worker_id: "\\".repeat(256),
                },
            ),
        )
        .unwrap();
        revision += 1;
        let lease = read(&db, &who, r.job_id);
        let fence = lease.job.attempts.last().unwrap().fence;
        let operation = if attempt < 32 {
            ConsolidationOperation::Fail {
                job_id: r.job_id,
                expected_revision: revision,
                fence,
                usage: usage.clone(),
                evidence_ref: "\\".repeat(2048),
            }
        } else {
            let mut assertion = assertion(&a, &b, MemoryRelationKind::Supports);
            assertion.evidence_ref = "\\".repeat(2048);
            ConsolidationOperation::Publish {
                job_id: r.job_id,
                expected_revision: revision,
                fence,
                assertions: vec![assertion; 16],
                usage: usage.clone(),
                evidence_ref: "\\".repeat(2048),
            }
        };
        db.apply_consolidation_command(
            "scope",
            &who,
            &command(&format!("finish-{attempt}"), operation),
        )
        .unwrap();
        revision += 1;
    }
    let completed = read(&db, &who, r.job_id);
    assert_eq!(completed.job.status, ConsolidationStatus::Published);
    assert_eq!(completed.job.output_receipts.len(), 16);
    assert!(encode(&completed.job).unwrap().len() > 128 * 1024);
    db.apply_consolidation_command(
        "scope",
        &who,
        &command(
            "correction",
            ConsolidationOperation::ReconcileUsage {
                job_id: r.job_id,
                expected_revision: revision,
                attempt_number: 1,
                usage: usage.clone(),
                evidence_ref: "\\".repeat(2048),
            },
        ),
    )
    .unwrap();
    assert_eq!(read(&db, &who, r.job_id).job.attempts[0].usage, usage);
}

#[test]
fn administration_discovers_all_owners_and_cancels_with_its_real_identity() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = actor();
    let (sources, first) = setup(&db, &a);
    let b = RecordAuthor {
        subject_id: "second-owner".into(),
        credential_id: Uuid::new_v4(),
    };
    let second = db
        .apply_consolidation_command(
            "scope",
            &b,
            &command(
                "create",
                ConsolidationOperation::Create {
                    spec: spec(sources.iter().map(source).collect()),
                },
            ),
        )
        .unwrap();
    let running = claim(&db, &a, first.job_id, 1);
    let admin = RecordAuthor {
        subject_id: "company-admin".into(),
        credential_id: Uuid::new_v4(),
    };
    let mut query = AdminConsolidationQuery {
        limit: 1,
        scan_limit: 1,
        after: None,
        status: None,
    };
    let mut found = Vec::new();
    loop {
        let page = db.query_admin_consolidation_jobs("scope", &query).unwrap();
        found.extend(
            page.jobs
                .into_iter()
                .map(|v| (v.owner_id, v.summary.job_id)),
        );
        query.after = page.next_after;
        if query.after.is_none() {
            break;
        }
    }
    assert_eq!(found.len(), 2);
    assert!(found.contains(&(a.subject_id.clone(), first.job_id)));
    assert!(found.contains(&(b.subject_id.clone(), second.job_id)));
    assert!(
        db.query_admin_consolidation_jobs("other", &query)
            .unwrap()
            .jobs
            .is_empty()
    );
    assert!(
        db.cancel_admin_consolidation_job(
            "scope",
            &a.subject_id,
            &admin,
            &publish(&running.job, vec![])
        )
        .is_err()
    );
    let cancel = command(
        "admin-cancel",
        ConsolidationOperation::Cancel {
            job_id: first.job_id,
            expected_revision: 2,
            evidence_ref: "incident://cancel".into(),
        },
    );
    let receipt = db
        .cancel_admin_consolidation_job("scope", &a.subject_id, &admin, &cancel)
        .unwrap();
    assert_eq!(receipt.author, admin);
    assert_eq!(
        db.cancel_admin_consolidation_job("scope", &a.subject_id, &admin, &cancel)
            .unwrap(),
        receipt
    );
    let current = read(&db, &a, first.job_id);
    assert_eq!(current.job.created_by, a);
    assert_eq!(current.job.status, ConsolidationStatus::Cancelled);
    assert_eq!(current.job.attempts[0].usage, ConsolidationUsage::Unknown);
    assert!(
        db.apply_consolidation_command("scope", &a, &publish(&running.job, vec![]))
            .is_err()
    );
    assert_eq!(
        db.consolidation_job_revision("scope", &a.subject_id, first.job_id, 3)
            .unwrap()
            .unwrap()
            .receipt,
        receipt
    );
    assert_eq!(
        read(&db, &b, second.job_id).job.status,
        ConsolidationStatus::Ready
    );
    drop(db);
    assert_eq!(
        read(&open(dir.path()), &a, first.job_id).job.status,
        ConsolidationStatus::Cancelled
    );
}
