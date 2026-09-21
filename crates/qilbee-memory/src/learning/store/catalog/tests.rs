use super::*;
use crate::learning::*;
use std::collections::{BTreeMap, BTreeSet};
use tempfile::TempDir;

fn query(kind: LearningResourceKind) -> LearningCatalogQuery {
    LearningCatalogQuery {
        kind,
        filter: Default::default(),
        limit: 25,
        max_scanned_records: 100,
        cursor: None,
    }
}
fn address(company: &str, project: &str, subject: Option<&str>) -> CompanyMemoryAddress {
    CompanyMemoryAddress::new(
        company,
        &MemoryResourceScope {
            project_id: project.into(),
            agent_id: "agent".into(),
            mission_id: None,
            visibility: if subject.is_some() {
                MemoryVisibility::Private
            } else {
                MemoryVisibility::Shared
            },
        },
        subject,
    )
    .unwrap()
}
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
            "admin",
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
                tools: BTreeMap::new(),
                environment_revision: "v1".into(),
                evaluation_contract: "rubric".into(),
                dataset_revision: "v1".into(),
                harness_revision: "v1".into(),
                permissions_revision: "v1".into(),
            },
            "admin",
        )
        .unwrap();
}
fn artifact(id: &str) -> ToolArtifactProposal {
    ToolArtifactProposal {
        id: id.into(),
        source: "def run(): return True".into(),
        dependency_lock: "".into(),
        runtime_image_digest: format!("sha256:{}", "a".repeat(64)),
        entrypoint: "recover:run".into(),
        input_schema: serde_json::json!(true),
        output_schema: serde_json::json!(true),
        source_refs: vec!["fixture:task".into()],
        parent_artifact_id: None,
        repair_evidence_ref: None,
    }
}
fn actor() -> ToolActor {
    ToolActor {
        subject_id: "writer".into(),
        credential_id: "old-writer".into(),
    }
}
fn experience(
    store: &LearningMemory,
    company: &str,
    namespace: &str,
    id: &str,
) -> ExperienceReceipt {
    store
        .create_experience(
            company,
            namespace,
            ExperienceRequest {
                id: id.into(),
                context_id: "context".into(),
                reporter_subject_id: "writer".into(),
                accounting_unit: "tokens".into(),
                input: ExperienceEvidence {
                    reference: "fixture:recover-interrupted-task".into(),
                    sha256: "b".repeat(64),
                },
                parent: None,
            },
            ExperienceActor {
                subject_id: "writer".into(),
                credential_id: "old-writer".into(),
            },
        )
        .unwrap()
}

#[test]
fn company_catalog_discovers_every_learning_kind_without_any_memory_inventory() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    configure(&store, "company");
    let ns = address("company", "learning-only", Some("writer"))
        .namespace()
        .unwrap();
    let attempt = experience(&store, "company", &ns, "attempt");
    let event = store
        .observe_experience(
            "company",
            &ns,
            "attempt",
            ExperienceCommand {
                event_id: "observed".into(),
                expected_revision: 1,
                context_digest: attempt.context_digest.clone(),
                outcome: ExperienceOutcome::Unknown,
                evidence: ExperienceEvidence {
                    reference: "fixture:observation".into(),
                    sha256: "c".repeat(64),
                },
                cost_units: None,
                latency_ms: None,
            },
            ExperienceActor {
                subject_id: "writer".into(),
                credential_id: "old-writer".into(),
            },
        )
        .unwrap();
    store
        .propose_strategy(
            "company",
            &ns,
            StrategyCandidateRequest {
                id: "strategy".into(),
                policy_id: "policy".into(),
                context_id: "context".into(),
                instructions: "Inspect the original receipt before retrying".into(),
                preconditions: vec!["A receipt exists".into()],
                counterexamples: vec!["An unknown response is not a failure".into()],
                extractor: StrategyExtractor {
                    provider: "external".into(),
                    model: "extractor".into(),
                    model_revision: "v1".into(),
                    prompt_revision: "v1".into(),
                    evidence_ref: "fixture:extraction".into(),
                },
                selection: ExperienceExportRequest {
                    context_digest: attempt.context_digest,
                    accounting_unit: "tokens".into(),
                    events: vec![ExperienceExportRef {
                        attempt_id: "attempt".into(),
                        event_id: "observed".into(),
                        event_digest: event.event_digest,
                    }],
                },
            },
            "writer",
        )
        .unwrap();
    store
        .register_tool_artifact("company", &ns, artifact("tool"), actor())
        .unwrap();
    store
        .register_tool_executor(
            "company",
            ToolExecutorProfile {
                id: "executor".into(),
                subject_id: "writer".into(),
                runtime_image_digest: format!("sha256:{}", "a".repeat(64)),
                environment_revision: "v1".into(),
                permissions_revision: "v1".into(),
                max_cost_units: 10,
                max_latency_ms: 1000,
            },
            actor(),
        )
        .unwrap();
    store
        .create_tool_development(
            "company",
            &ns,
            ToolDevelopmentRequest {
                id: "development".into(),
                executor_id: "executor".into(),
                objective: "Build a recovery tool".into(),
                parent_artifact_id: None,
                repair_evidence_ref: None,
            },
            actor(),
        )
        .unwrap();
    drop(store);
    let store = LearningMemory::open(dir.path()).unwrap();
    for kind in [
        LearningResourceKind::Experience,
        LearningResourceKind::Procedure,
        LearningResourceKind::Strategy,
        LearningResourceKind::ToolArtifact,
        LearningResourceKind::ToolDevelopment,
        LearningResourceKind::Policy,
        LearningResourceKind::Context,
        LearningResourceKind::Executor,
    ] {
        let page = store
            .company_learning_catalog("company", &query(kind))
            .unwrap();
        assert_eq!(page.entries.len(), 1, "{kind:?}");
        assert_eq!(page.stop_reason, LearningCatalogStop::Exhausted);
        assert!(page.next_cursor.is_none());
        let entry = &page.entries[0];
        assert!(!entry.title.is_empty());
        let details = store
            .inspect_company_learning_resource("company", &entry.resource)
            .unwrap()
            .unwrap();
        assert_eq!(
            store
                .catalog_summary("company", &details, entry.resource.clone())
                .unwrap(),
            *entry
        );
        assert!(
            store
                .inspect_company_learning_resource("foreign", &entry.resource)
                .unwrap()
                .is_none()
        );
        assert_eq!(entry.resource.scope.is_some(), kind.scoped());
        if kind.scoped() {
            assert_eq!(entry.resource.private_subject_id.as_deref(), Some("writer"));
        }
    }
    assert_eq!(
        store
            .company_learning_catalog("company", &query(LearningResourceKind::Experience))
            .unwrap()
            .entries[0]
            .status,
        "unknown"
    );
    let scope = store
        .registered_scope("company", &ns, "policy", "context")
        .unwrap();
    for case in 0..32 {
        store
            .record_evaluation(
                &scope,
                "strategy",
                PairedEvaluation {
                    case_id: format!("case-{case}"),
                    phase: EvaluationPhase::Qualification,
                    evaluator_id: "judge".into(),
                    evaluation_contract: "rubric".into(),
                    evidence_ref: format!("held-out:{case}"),
                    baseline_utility: 1.0,
                    candidate_utility: 0.0,
                    candidate_cost_units: 1,
                    candidate_latency_ms: 1,
                },
            )
            .unwrap();
    }
    for kind in [
        LearningResourceKind::Procedure,
        LearningResourceKind::Strategy,
    ] {
        let page = store
            .company_learning_catalog("company", &query(kind))
            .unwrap();
        assert_eq!(page.entries[0].status, "rejected");
    }
}

#[test]
fn company_catalog_filters_scopes_and_keeps_live_cursor_progress_through_empty_pages() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    for company in ["one", "two", "one-suffix"] {
        configure(&store, company);
        for project in ["first", "second"] {
            for subject in [Some("alice"), Some("bob"), None] {
                experience(
                    &store,
                    company,
                    &address(company, project, subject).namespace().unwrap(),
                    "same-id",
                );
            }
        }
    }
    // Trusted-library namespaces cannot be assigned a platform scope by assumption.
    experience(&store, "one", "legacy", "legacy");
    let mut q = query(LearningResourceKind::Experience);
    q.max_scanned_records = 1;
    q.limit = 1;
    let mut seen = BTreeSet::new();
    let mut scanned = 0;
    let mut skipped = 0;
    loop {
        let page = store.company_learning_catalog("one", &q).unwrap();
        scanned += page.scanned_records;
        skipped += page.skipped_non_platform_records;
        for entry in &page.entries {
            assert!(seen.insert(serde_json::to_string(&entry.resource).unwrap()));
        }
        q.cursor = page.next_cursor;
        if q.cursor.is_none() {
            break;
        }
        assert_ne!(page.stop_reason, LearningCatalogStop::Exhausted);
    }
    assert_eq!(seen.len(), 6);
    assert_eq!(scanned, 7);
    assert_eq!(skipped, 1);
    q.cursor = None;
    q.filter.project_id = Some("second".into());
    q.filter.private_subject_id = Some("bob".into());
    let mut entries = vec![];
    let mut empty_partial = false;
    loop {
        let page = store.company_learning_catalog("one", &q).unwrap();
        empty_partial |= page.entries.is_empty() && page.next_cursor.is_some();
        entries.extend(page.entries);
        q.cursor = page.next_cursor;
        if q.cursor.is_none() {
            break;
        }
    }
    assert!(empty_partial);
    assert_eq!(entries.len(), 1);
    assert_eq!(
        entries[0].resource.scope.as_ref().unwrap().project_id,
        "second"
    );
    assert_eq!(
        entries[0].resource.private_subject_id.as_deref(),
        Some("bob")
    );
}

#[test]
fn company_catalog_rejects_cursor_rebinding_invalid_bounds_and_ambiguous_resource_selections() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    configure(&store, "company");
    let ns = address("company", "project", None).namespace().unwrap();
    for id in ["one", "two"] {
        experience(&store, "company", &ns, id);
    }
    let mut q = query(LearningResourceKind::Experience);
    q.limit = 1;
    q.cursor = store
        .company_learning_catalog("company", &q)
        .unwrap()
        .next_cursor;
    assert!(q.cursor.is_some());
    assert!(store.company_learning_catalog("foreign", &q).is_err());
    let mut changed = q.clone();
    changed.kind = LearningResourceKind::Procedure;
    assert!(store.company_learning_catalog("company", &changed).is_err());
    changed = q.clone();
    changed.filter.text = Some("changed".into());
    assert!(store.company_learning_catalog("company", &changed).is_err());
    changed = q.clone();
    changed.cursor.as_mut().unwrap().position = "00".into();
    assert!(store.company_learning_catalog("company", &changed).is_err());
    for (limit, scanned) in [(0, 1), (51, 1), (1, 0), (1, 1001)] {
        changed = query(LearningResourceKind::Experience);
        changed.limit = limit;
        changed.max_scanned_records = scanned;
        assert!(store.company_learning_catalog("company", &changed).is_err());
    }
    let selection = LearningResourceRef {
        kind: LearningResourceKind::Policy,
        id: "policy".into(),
        scope: Some(address("company", "p", None).scope),
        private_subject_id: None,
    };
    assert!(
        store
            .inspect_company_learning_resource("company", &selection)
            .is_err()
    );
    let selection = LearningResourceRef {
        kind: LearningResourceKind::Experience,
        id: "one".into(),
        scope: None,
        private_subject_id: None,
    };
    assert!(
        store
            .inspect_company_learning_resource("company", &selection)
            .is_err()
    );
}

#[test]
fn company_catalog_verifies_retained_records_and_never_reads_other_companies() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let ns = address("company", "project", None).namespace().unwrap();
    let a = store
        .register_tool_artifact("company", &ns, artifact("one"), actor())
        .unwrap();
    let foreign_ns = address("foreign", "project", None).namespace().unwrap();
    store
        .inner
        .db
        .put(
            tools::tool_key(8, "foreign", &foreign_ns, "one").unwrap(),
            b"corrupt foreign payload",
        )
        .unwrap();
    let q = query(LearningResourceKind::ToolArtifact);
    assert_eq!(
        store
            .company_learning_catalog("company", &q)
            .unwrap()
            .entries
            .len(),
        1
    );
    let mut corrupted = a;
    corrupted.proposal.source = "tampered source".into();
    store
        .inner
        .db
        .put(
            tools::tool_key(8, "company", &ns, "one").unwrap(),
            encode(&corrupted).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        store.company_learning_catalog("company", &q),
        Err(Error::DataCorruption(_))
    ));
    // Mismatched canonical company inside a scoped key fails closed.
    store
        .inner
        .db
        .delete(tools::tool_key(8, "company", &ns, "one").unwrap())
        .unwrap();
    store
        .inner
        .db
        .put(
            tools::tool_key(8, "company", &foreign_ns, "one").unwrap(),
            b"{}",
        )
        .unwrap();
    assert!(matches!(
        store.company_learning_catalog("company", &q),
        Err(Error::DataCorruption(_))
    ));
}

#[test]
fn company_catalog_byte_cutoff_advances_over_filtered_and_legacy_records() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    // Synthetic library-only entries exercise byte accounting without allocating
    // unbounded production objects. They must never become platform resources.
    for id in ["one", "two", "tri"] {
        store
            .inner
            .db
            .put(
                tools::tool_key(8, "company", "legacy", id).unwrap(),
                vec![0; 2 * 1024 * 1024],
            )
            .unwrap();
    }
    let ns = address("company", "project", None).namespace().unwrap();
    store
        .register_tool_artifact("company", &ns, artifact("real"), actor())
        .unwrap();
    let mut q = query(LearningResourceKind::ToolArtifact);
    let mut found = vec![];
    let mut cuts = 0;
    let mut scanned = 0;
    loop {
        let page = store.company_learning_catalog("company", &q).unwrap();
        assert!(page.scanned_record_bytes <= SCAN_BYTES);
        scanned += page.scanned_records;
        cuts += usize::from(page.stop_reason == LearningCatalogStop::ByteLimit);
        found.extend(page.entries);
        q.cursor = page.next_cursor;
        if q.cursor.is_none() {
            break;
        }
    }
    assert_eq!(scanned, 4);
    assert_eq!(cuts, 2);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].resource.id, "real");
}

#[test]
fn company_catalog_filters_agent_mission_and_text_without_losing_current_identity() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let mut expected = None;
    for agent in ["writer-one", "writer-two"] {
        for mission in [None, Some("mission-one"), Some("mission-two")] {
            let mut a = address("company", "project", None);
            a.scope.agent_id = agent.into();
            a.scope.mission_id = mission.map(str::to_owned);
            let mut proposal = artifact("same-id");
            proposal.entrypoint = "Résumé: recover_task".into();
            store
                .register_tool_artifact("company", &a.namespace().unwrap(), proposal, actor())
                .unwrap();
            if agent == "writer-two" && mission == Some("mission-two") {
                expected = Some(a.scope);
            }
        }
    }
    let mut q = query(LearningResourceKind::ToolArtifact);
    q.filter.agent_id = Some("writer-two".into());
    q.filter.mission_id = Some("mission-two".into());
    q.filter.visibility = Some(MemoryVisibility::Shared);
    q.filter.text = Some("RÉSUMÉ".into());
    let page = store.company_learning_catalog("company", &q).unwrap();
    assert_eq!(page.scanned_records, 6);
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].resource.scope, expected);
    q.filter.text = Some("def run".into());
    assert!(
        store
            .company_learning_catalog("company", &q)
            .unwrap()
            .entries
            .is_empty()
    );
    q.filter.text = Some("SAME-ID".into());
    assert_eq!(
        store
            .company_learning_catalog("company", &q)
            .unwrap()
            .entries
            .len(),
        1
    );
}

#[test]
fn company_catalog_resumes_after_reopen_and_reports_live_updates_without_duplicates() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    configure(&store, "company");
    let ns = address("company", "project", None).namespace().unwrap();
    experience(&store, "company", &ns, "aaa");
    let b = experience(&store, "company", &ns, "bbb");
    let mut q = query(LearningResourceKind::Experience);
    q.limit = 1;
    let first = store.company_learning_catalog("company", &q).unwrap();
    assert_eq!(first.entries[0].resource.id, "aaa");
    q.cursor = first.next_cursor;
    let serialized = serde_json::to_vec(&q).unwrap();
    drop(store);
    let store = LearningMemory::open(dir.path()).unwrap();
    let q: LearningCatalogQuery = serde_json::from_slice(&serialized).unwrap();
    // Insert behind the cursor and update a not-yet-read row. Continuation is
    // deliberately live; restarting is necessary to discover the earlier insert.
    experience(&store, "company", &ns, "000");
    store
        .observe_experience(
            "company",
            &ns,
            "bbb",
            ExperienceCommand {
                event_id: "done".into(),
                expected_revision: 1,
                context_digest: b.context_digest,
                outcome: ExperienceOutcome::Succeeded,
                evidence: ExperienceEvidence {
                    reference: "fixture:result".into(),
                    sha256: "c".repeat(64),
                },
                cost_units: Some(2),
                latency_ms: Some(4),
            },
            ExperienceActor {
                subject_id: "writer".into(),
                credential_id: "rotated-writer".into(),
            },
        )
        .unwrap();
    let next = store.company_learning_catalog("company", &q).unwrap();
    assert_eq!(next.entries.len(), 1);
    assert_eq!(next.entries[0].resource.id, "bbb");
    assert_eq!(next.entries[0].status, "succeeded");
    assert_eq!(next.entries[0].revision, Some(2));
    assert!(next.next_cursor.is_none());
    assert_eq!(
        store
            .company_learning_catalog("company", &query(LearningResourceKind::Experience))
            .unwrap()
            .entries
            .len(),
        3
    );
}
