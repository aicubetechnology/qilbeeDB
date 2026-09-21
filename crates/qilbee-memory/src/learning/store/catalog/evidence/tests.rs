use super::*;
use crate::learning::*;
use tempfile::TempDir;

fn configure(store: &LearningMemory, company: &str) {
    store
        .register_policy(
            company,
            "policy",
            PolicyDefinition {
                algorithm: PolicyAlgorithm::FixedBudgetHoeffdingV1,
                parameters: LearningPolicy {
                    qualification_trials: 32,
                    evaluator_id: "judge".into(),
                    evaluation_contract: "rubric".into(),
                    ..Default::default()
                },
            },
            "owner",
        )
        .unwrap();
    store
        .register_context(
            company,
            "context",
            EvaluationContext {
                task: "Recover an interrupted task".into(),
                baseline_revision: "baseline".into(),
                model_provider: "external".into(),
                model_revision: "v1".into(),
                tools: Default::default(),
                environment_revision: "v1".into(),
                evaluation_contract: "rubric".into(),
                dataset_revision: "v1".into(),
                harness_revision: "v1".into(),
                permissions_revision: "v1".into(),
            },
            "owner",
        )
        .unwrap();
}
fn resource(
    kind: LearningResourceKind,
    id: &str,
    project: &str,
    subject: Option<&str>,
) -> LearningResourceRef {
    LearningResourceRef {
        kind,
        id: id.into(),
        scope: Some(MemoryResourceScope {
            project_id: project.into(),
            agent_id: "agent".into(),
            mission_id: None,
            visibility: if subject.is_some() {
                MemoryVisibility::Private
            } else {
                MemoryVisibility::Shared
            },
        }),
        private_subject_id: subject.map(str::to_string),
    }
}
fn proposal(store: &LearningMemory, company: &str, r: &LearningResourceRef) -> RegisteredProcedure {
    let ns = r.namespace(company).unwrap().unwrap();
    store
        .propose_registered(
            company,
            &ns,
            RegisteredProposal {
                id: r.id.clone(),
                policy_id: "policy".into(),
                context_id: "context".into(),
                instructions: "Check the original receipt before retrying".into(),
                source_refs: vec!["fixture:source".into()],
            },
            "writer",
        )
        .unwrap();
    store
        .registered_procedure(company, &ns, &r.id)
        .unwrap()
        .unwrap()
}
fn query(resource: &LearningResourceRef, kind: LearningEvidenceKind) -> LearningEvidenceQuery {
    LearningEvidenceQuery {
        resource: resource.clone(),
        kind,
        text: None,
        limit: 25,
        max_scanned_records: 100,
        cursor: None,
    }
}
fn submission(id: &str, status: SubmissionStatus) -> EvaluationSubmission {
    EvaluationSubmission {
        case_id: id.into(),
        phase: EvaluationPhase::Qualification,
        policy_id: "policy".into(),
        context_id: "context".into(),
        baseline_revision: "baseline".into(),
        evidence_ref: format!("fixture:{id}"),
        status,
        baseline_utility: Some(0.2),
        candidate_utility: Some(0.8),
        candidate_cost_units: Some(3),
        candidate_latency_ms: Some(4),
        detail: None,
    }
}
fn admit(
    store: &LearningMemory,
    company: &str,
    r: &LearningResourceRef,
    s: EvaluationSubmission,
) -> AdmissionReceipt {
    store
        .admit_evaluation(
            company,
            &r.namespace(company).unwrap().unwrap(),
            &r.id,
            s,
            EvaluationActor {
                subject_id: "judge".into(),
                credential_id: "original-evaluator".into(),
            },
        )
        .unwrap()
}

#[test]
fn evidence_distinguishes_all_admission_outcomes_and_native_paired_receipts_after_reopen() {
    let dir = TempDir::new().unwrap();
    let db = LearningMemory::open(dir.path()).unwrap();
    configure(&db, "company");
    let r = resource(
        LearningResourceKind::Procedure,
        "procedure",
        "project",
        Some("writer"),
    );
    let bound = proposal(&db, "company", &r);
    for (id, status) in [
        ("a", SubmissionStatus::Complete),
        ("b", SubmissionStatus::Rejected),
        ("c", SubmissionStatus::Incomplete),
        ("d", SubmissionStatus::Cancelled),
        ("e", SubmissionStatus::PendingOrUnknown),
    ] {
        admit(&db, "company", &r, submission(id, status));
    }
    let mut unknown = submission("f", SubmissionStatus::Complete);
    unknown.candidate_cost_units = None;
    let original = admit(&db, "company", &r, unknown);
    assert_eq!(original.outcome, AdmissionOutcome::UnknownConsumption);
    db.record_evaluation(
        &bound.record.scope,
        &r.id,
        PairedEvaluation {
            case_id: "direct".into(),
            phase: EvaluationPhase::Qualification,
            evaluator_id: "judge".into(),
            evaluation_contract: "rubric".into(),
            evidence_ref: "fixture:direct-library-evaluation".into(),
            baseline_utility: 0.2,
            candidate_utility: 0.8,
            candidate_cost_units: 3,
            candidate_latency_ms: 4,
        },
    )
    .unwrap();
    drop(db);
    let db = LearningMemory::open(dir.path()).unwrap();
    let page = db
        .company_learning_evidence(
            "company",
            &query(&r, LearningEvidenceKind::EvaluationSubmission),
        )
        .unwrap();
    assert_eq!(
        page.entries
            .iter()
            .map(|v| v.status.as_str())
            .collect::<Vec<_>>(),
        vec![
            "accepted",
            "rejected",
            "incomplete",
            "cancelled",
            "pending_or_unknown",
            "unknown_consumption"
        ]
    );
    assert_eq!(page.stop_reason, LearningCatalogStop::Exhausted);
    assert!(page.next_cursor.is_none());
    let LearningEvidenceDetails::EvaluationSubmission(read) = db
        .inspect_company_learning_evidence("company", &page.entries[5].evidence)
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(read, original);
    assert_eq!(read.submission.candidate_cost_units, None);
    let paired = db
        .company_learning_evidence(
            "company",
            &query(&r, LearningEvidenceKind::PairedEvaluation),
        )
        .unwrap();
    assert_eq!(paired.entries.len(), 2);
    assert!(paired.entries.iter().any(|e| e.evidence.id == "direct"));
    assert_eq!(
        db.registered_procedure("company", &r.namespace("company").unwrap().unwrap(), &r.id)
            .unwrap()
            .unwrap()
            .record
            .qualification_count,
        2
    );
    for i in 2..32 {
        admit(
            &db,
            "company",
            &r,
            submission(&format!("promotion-{i:02}"), SubmissionStatus::Complete),
        );
    }
    assert_eq!(
        db.registered_procedure("company", &r.namespace("company").unwrap().unwrap(), &r.id)
            .unwrap()
            .unwrap()
            .record
            .state,
        ProcedureState::Active
    );
    let LearningEvidenceDetails::EvaluationSubmission(historical) = db
        .inspect_company_learning_evidence("company", &page.entries[5].evidence)
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(historical, original);
}

#[test]
fn evidence_pagination_filters_and_cursor_binding_preserve_scope_and_progress() {
    let dir = TempDir::new().unwrap();
    let db = LearningMemory::open(dir.path()).unwrap();
    for company in ["company", "other"] {
        configure(&db, company);
        let mut resources: Vec<_> = [
            ("project", Some("alice")),
            ("project", Some("bob")),
            ("elsewhere", Some("alice")),
            ("project", None),
        ]
        .into_iter()
        .map(|(project, subject)| {
            resource(LearningResourceKind::Procedure, "same", project, subject)
        })
        .collect();
        let mut other_agent = resources[0].clone();
        other_agent.scope.as_mut().unwrap().agent_id = "other-agent".into();
        resources.push(other_agent);
        let mut mission = resources[0].clone();
        mission.scope.as_mut().unwrap().mission_id = Some("mission".into());
        resources.push(mission);
        for r in resources {
            proposal(&db, company, &r);
            for id in ["a", "b", "c", "d"] {
                admit(
                    &db,
                    company,
                    &r,
                    submission(id, SubmissionStatus::Incomplete),
                );
            }
        }
    }
    let r = resource(
        LearningResourceKind::Procedure,
        "same",
        "project",
        Some("alice"),
    );
    let mut q = query(&r, LearningEvidenceKind::EvaluationSubmission);
    q.limit = 1;
    let first = db.company_learning_evidence("company", &q).unwrap();
    let cursor = first.next_cursor.unwrap();
    assert_eq!(first.entries[0].evidence.resource, r);
    assert_eq!(first.entries[0].evidence.id, "a");
    q.cursor = Some(cursor.clone());
    q.limit = 2;
    let second = db.company_learning_evidence("company", &q).unwrap();
    assert_eq!(
        second
            .entries
            .iter()
            .map(|v| v.evidence.id.as_str())
            .collect::<Vec<_>>(),
        vec!["b", "c"]
    );
    q.cursor = second.next_cursor;
    let last = db.company_learning_evidence("company", &q).unwrap();
    assert_eq!(last.entries[0].evidence.id, "d");
    assert!(last.next_cursor.is_none());
    q.cursor = Some(cursor);
    assert!(db.company_learning_evidence("other", &q).is_err());
    for foreign in [
        resource(
            LearningResourceKind::Procedure,
            "same",
            "project",
            Some("bob"),
        ),
        resource(
            LearningResourceKind::Procedure,
            "same",
            "elsewhere",
            Some("alice"),
        ),
        resource(LearningResourceKind::Procedure, "same", "project", None),
    ] {
        let mut wrong = q.clone();
        wrong.resource = foreign;
        assert!(db.company_learning_evidence("company", &wrong).is_err());
    }
    for (agent, mission) in [("other-agent", None), ("agent", Some("mission"))] {
        let mut changed = q.clone();
        changed.resource.scope.as_mut().unwrap().agent_id = agent.into();
        changed.resource.scope.as_mut().unwrap().mission_id = mission.map(str::to_string);
        assert!(db.company_learning_evidence("company", &changed).is_err());
        changed.cursor = None;
        let own_page = db.company_learning_evidence("company", &changed).unwrap();
        assert_eq!(own_page.entries.len(), 2);
        assert!(
            own_page
                .entries
                .iter()
                .all(|entry| entry.evidence.resource == changed.resource)
        );
    }
    let mut wrong = q.clone();
    wrong.kind = LearningEvidenceKind::PairedEvaluation;
    assert!(db.company_learning_evidence("company", &wrong).is_err());
    q.cursor = None;
    q.text = Some("absent".into());
    q.max_scanned_records = 1;
    let empty = db.company_learning_evidence("company", &q).unwrap();
    assert!(empty.entries.is_empty());
    assert_eq!(empty.scanned_records, 1);
    assert_eq!(empty.stop_reason, LearningCatalogStop::ScanLimit);
    q.cursor = empty.next_cursor;
    q.max_scanned_records = 3;
    let rest = db.company_learning_evidence("company", &q).unwrap();
    assert!(rest.entries.is_empty());
    assert!(rest.next_cursor.is_none());
    assert_eq!(rest.scanned_records, 3);
    q.cursor = None;
    q.text = Some("INCOMPLETE".into());
    assert_eq!(
        db.company_learning_evidence("company", &q)
            .unwrap()
            .entries
            .len(),
        2
    );
    q.limit = 0;
    assert!(db.company_learning_evidence("company", &q).is_err());
    q.limit = 51;
    assert!(db.company_learning_evidence("company", &q).is_err());
    q.limit = 1;
    q.max_scanned_records = 1001;
    assert!(db.company_learning_evidence("company", &q).is_err());
}

#[test]
fn evidence_rejects_mismatched_pair_identity_and_tampered_admission_actor_or_outcome() {
    let dir = TempDir::new().unwrap();
    let db = LearningMemory::open(dir.path()).unwrap();
    configure(&db, "company");
    let r = resource(
        LearningResourceKind::Procedure,
        "procedure",
        "project",
        Some("writer"),
    );
    proposal(&db, "company", &r);
    let original = admit(
        &db,
        "company",
        &r,
        submission("case", SubmissionStatus::Complete),
    );
    let mut q = query(&r, LearningEvidenceKind::EvaluationSubmission);
    let selection = db.evidence_selection("company", &r, q.kind).unwrap();
    let mut key = selection.prefix;
    append_component(&mut key, "case");
    for change in [0, 1, 2, 3] {
        let mut bad = original.clone();
        match change {
            0 => bad.actor.subject_id = "imposter".into(),
            1 => bad.outcome = AdmissionOutcome::UnknownConsumption,
            2 => bad.submission.candidate_utility = Some(0.4),
            _ => bad.reason = "fabricated".into(),
        };
        db.inner.db.put(&key, encode(&bad).unwrap()).unwrap();
        assert!(matches!(
            db.company_learning_evidence("company", &q),
            Err(Error::DataCorruption(_))
        ));
    }
    db.inner.db.put(&key, encode(&original).unwrap()).unwrap();
    q.kind = LearningEvidenceKind::PairedEvaluation;
    let selected = db.evidence_selection("company", &r, q.kind).unwrap();
    let mut paired_key = selected.prefix;
    append_component(&mut paired_key, "case");
    let mut bad = original.evaluation.clone().unwrap();
    bad.evaluation.case_id = "other".into();
    db.inner.db.put(&paired_key, encode(&bad).unwrap()).unwrap();
    assert!(db.company_learning_evidence("company", &q).is_err());
    db.inner
        .db
        .put(&paired_key, encode(&original.evaluation.unwrap()).unwrap())
        .unwrap();
    let mut malformed = key.clone();
    malformed.push(0);
    db.inner.db.put(malformed, b"{}").unwrap();
    q.kind = LearningEvidenceKind::EvaluationSubmission;
    assert!(db.company_learning_evidence("company", &q).is_err());
}

#[test]
fn evidence_exposes_historical_experience_and_development_events_without_claiming_current_state() {
    let dir = TempDir::new().unwrap();
    let db = LearningMemory::open(dir.path()).unwrap();
    configure(&db, "company");
    let r = resource(
        LearningResourceKind::Experience,
        "attempt",
        "project",
        Some("writer"),
    );
    let ns = r.namespace("company").unwrap().unwrap();
    let actor = ExperienceActor {
        subject_id: "writer".into(),
        credential_id: "original-writer".into(),
    };
    let receipt = db
        .create_experience(
            "company",
            &ns,
            ExperienceRequest {
                id: r.id.clone(),
                context_id: "context".into(),
                reporter_subject_id: "writer".into(),
                accounting_unit: "tokens".into(),
                input: ExperienceEvidence {
                    reference: "fixture:task".into(),
                    sha256: "a".repeat(64),
                },
                parent: None,
            },
            actor.clone(),
        )
        .unwrap();
    let first = db
        .observe_experience(
            "company",
            &ns,
            &r.id,
            ExperienceCommand {
                event_id: "z".into(),
                expected_revision: 1,
                context_digest: receipt.context_digest.clone(),
                outcome: ExperienceOutcome::Unknown,
                evidence: ExperienceEvidence {
                    reference: "fixture:pending".into(),
                    sha256: "b".repeat(64),
                },
                cost_units: Some(9),
                latency_ms: Some(10),
            },
            actor.clone(),
        )
        .unwrap();
    db.observe_experience(
        "company",
        &ns,
        &r.id,
        ExperienceCommand {
            event_id: "a".into(),
            expected_revision: 2,
            context_digest: receipt.context_digest.clone(),
            outcome: ExperienceOutcome::Succeeded,
            evidence: ExperienceEvidence {
                reference: "fixture:result".into(),
                sha256: "c".repeat(64),
            },
            cost_units: None,
            latency_ms: None,
        },
        actor,
    )
    .unwrap();
    let page = db
        .company_learning_evidence("company", &query(&r, LearningEvidenceKind::ExperienceEvent))
        .unwrap();
    assert_eq!(
        page.entries
            .iter()
            .map(|e| e.revision.unwrap())
            .collect::<Vec<_>>(),
        vec![3, 2]
    );
    let LearningEvidenceDetails::ExperienceEvent(old) = db
        .inspect_company_learning_evidence("company", &page.entries[1].evidence)
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(old, first);
    assert_eq!(old.command.outcome, ExperienceOutcome::Unknown);
    assert_eq!(
        db.experience("company", &ns, &r.id)
            .unwrap()
            .unwrap()
            .outcome,
        Some(ExperienceOutcome::Succeeded)
    );
    let tool_actor = ToolActor {
        subject_id: "writer".into(),
        credential_id: "original-writer".into(),
    };
    let executor = db
        .register_tool_executor(
            "company",
            ToolExecutorProfile {
                id: "executor".into(),
                subject_id: "worker".into(),
                runtime_image_digest: format!("sha256:{}", "d".repeat(64)),
                environment_revision: "v1".into(),
                permissions_revision: "v1".into(),
                max_cost_units: 10,
                max_latency_ms: 1000,
            },
            tool_actor.clone(),
        )
        .unwrap();
    let tool = resource(
        LearningResourceKind::ToolDevelopment,
        "development",
        "project",
        Some("writer"),
    );
    db.create_tool_development(
        "company",
        &ns,
        ToolDevelopmentRequest {
            id: tool.id.clone(),
            executor_id: "executor".into(),
            objective: "Build a recovery tool".into(),
            parent_artifact_id: None,
            repair_evidence_ref: None,
        },
        tool_actor.clone(),
    )
    .unwrap();
    let requested = db
        .apply_tool_development(
            "company",
            &ns,
            &tool.id,
            ToolDevelopmentCommand {
                event_id: "request".into(),
                expected_revision: 1,
                action: ToolDevelopmentAction::RequestCancellation {
                    reason: "Stop the previous attempt".into(),
                },
            },
            tool_actor,
        )
        .unwrap();
    db.apply_tool_development(
        "company",
        &ns,
        &tool.id,
        ToolDevelopmentCommand {
            event_id: "done".into(),
            expected_revision: 2,
            action: ToolDevelopmentAction::Report {
                report: ToolDevelopmentReport {
                    executor_profile_digest: executor.profile_digest,
                    outcome: ToolReportOutcome::Cancelled,
                    evidence_ref: "fixture:worker-cancellation".into(),
                    detail: None,
                    cost_units: None,
                    latency_ms: None,
                    artifact: None,
                },
            },
        },
        ToolActor {
            subject_id: "worker".into(),
            credential_id: "original-worker".into(),
        },
    )
    .unwrap();
    let page = db
        .company_learning_evidence(
            "company",
            &query(&tool, LearningEvidenceKind::ToolDevelopmentEvent),
        )
        .unwrap();
    assert_eq!(page.entries.len(), 2);
    let old = page
        .entries
        .iter()
        .find(|e| e.evidence.id == "request")
        .unwrap();
    let LearningEvidenceDetails::ToolDevelopmentEvent(read) = db
        .inspect_company_learning_evidence("company", &old.evidence)
        .unwrap()
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(read, requested);
    assert_eq!(
        read.record.state,
        ToolDevelopmentState::CancellationRequested
    );
    assert_eq!(
        db.tool_development("company", &ns, &tool.id)
            .unwrap()
            .unwrap()
            .state,
        ToolDevelopmentState::Cancelled
    );
    let selected = db
        .evidence_selection("company", &tool, LearningEvidenceKind::ToolDevelopmentEvent)
        .unwrap();
    let mut stored_key = selected.prefix;
    append_component(&mut stored_key, "done");
    let original: ToolDevelopmentEvent =
        decode(&db.inner.db.get(&stored_key).unwrap().unwrap()).unwrap();
    for mutation in [0, 1, 2] {
        let mut bad = original.clone();
        match mutation {
            0 => bad.actor.subject_id = "unbound-worker".into(),
            1 => {
                let ToolDevelopmentAction::Report { report } = &mut bad.command.action else {
                    panic!()
                };
                report.outcome = ToolReportOutcome::Failed;
            }
            _ => {
                let ToolDevelopmentAction::Report { report } = &mut bad.command.action else {
                    panic!()
                };
                report.cost_units = Some(1);
            }
        }
        db.inner.db.put(&stored_key, encode(&bad).unwrap()).unwrap();
        assert!(matches!(
            db.company_learning_evidence(
                "company",
                &query(&tool, LearningEvidenceKind::ToolDevelopmentEvent)
            ),
            Err(Error::DataCorruption(_))
        ));
    }
    db.inner
        .db
        .put(&stored_key, encode(&original).unwrap())
        .unwrap();
    let mut missing = old.evidence.clone();
    missing.id = "missing".into();
    assert!(
        db.inspect_company_learning_evidence("company", &missing)
            .unwrap()
            .is_none()
    );
    let mut wrong = query(&tool, LearningEvidenceKind::ExperienceEvent);
    assert!(db.company_learning_evidence("company", &wrong).is_err());
    wrong.resource.id = "unknown".into();
    wrong.kind = LearningEvidenceKind::ToolDevelopmentEvent;
    assert!(matches!(
        db.company_learning_evidence("company", &wrong),
        Err(Error::KeyNotFound(_))
    ));
}

#[test]
fn evidence_byte_budget_advances_filtered_pages_and_does_not_scan_foreign_corruption() {
    let dir = TempDir::new().unwrap();
    let db = LearningMemory::open(dir.path()).unwrap();
    configure(&db, "company");
    let r = resource(
        LearningResourceKind::Procedure,
        "procedure",
        "project",
        Some("writer"),
    );
    proposal(&db, "company", &r);
    let selected = db
        .evidence_selection("company", &r, LearningEvidenceKind::EvaluationSubmission)
        .unwrap();
    for i in 0..950 {
        let mut s = submission(&format!("case-{i:04}"), SubmissionStatus::Incomplete);
        s.detail = Some("d".repeat(2048));
        s.evidence_ref = "e".repeat(2048);
        admit(&db, "company", &r, s);
    }
    let mut foreign = vec![7];
    for value in ["other", "invalid namespace", "procedure", "case"] {
        append_component(&mut foreign, value);
    }
    db.inner.db.put(foreign, b"not JSON").unwrap();
    let mut q = query(&r, LearningEvidenceKind::EvaluationSubmission);
    q.text = Some("never matches".into());
    q.max_scanned_records = 1000;
    let first = db.company_learning_evidence("company", &q).unwrap();
    assert_eq!(first.stop_reason, LearningCatalogStop::ByteLimit);
    assert!(first.entries.is_empty());
    assert!(first.scanned_record_bytes <= SCAN_BYTES);
    assert!(first.scanned_records > 0 && first.scanned_records < 950);
    q.cursor = first.next_cursor;
    let next = db.company_learning_evidence("company", &q).unwrap();
    assert_eq!(first.scanned_records + next.scanned_records, 950);
    assert_eq!(next.stop_reason, LearningCatalogStop::Exhausted);
    let mut oversized = selected.prefix;
    append_component(&mut oversized, "huge");
    db.inner
        .db
        .put(oversized, vec![b' '; SCAN_BYTES + 1])
        .unwrap();
    q.cursor = None;
    assert!(matches!(
        db.company_learning_evidence("company", &q),
        Err(Error::DataCorruption(_))
    ));
}
