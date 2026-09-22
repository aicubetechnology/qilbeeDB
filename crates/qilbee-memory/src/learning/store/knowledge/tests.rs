use super::*;
use uuid::Uuid;
fn tool(name: &str, implementation: Option<&str>) -> ExternalToolKnowledge {
    ExternalToolKnowledge {
        name: name.into(),
        schema_revision: "schema:r1".into(),
        implementation_revision: implementation.map(str::to_owned),
        environment_revision: implementation.map(|_| "env:r1".into()),
        usage_contract: "Read one authorized document".into(),
    }
}
fn proposal() -> KnowledgeProposal {
    KnowledgeProposal {
        id: "knowledge-r1".into(),
        policy_id: "policy".into(),
        context_id: "context".into(),
        title: "Lookup recovery".into(),
        instructions: "Use only for transient errors".into(),
        memory_sources: vec![
            MemorySourceRef {
                record_id: Uuid::from_u128(2),
                revision: u64::MAX,
            },
            MemorySourceRef {
                record_id: Uuid::from_u128(1),
                revision: 3,
            },
        ],
        external_tools: vec![tool("z", None), tool("a", Some("code:r1"))],
    }
}
#[test]
fn order_is_canonical_for_retry_and_digest_without_losing_revision_precision() {
    let a = proposal().canonicalize().unwrap();
    let mut b = proposal();
    b.memory_sources.reverse();
    b.external_tools.reverse();
    let b = b.canonicalize().unwrap();
    assert_eq!(a, b);
    assert_eq!(
        super::super::registry::digest(&a).unwrap(),
        super::super::registry::digest(&b).unwrap()
    );
    let bytes = serde_json::to_vec(&a).unwrap();
    let decoded: KnowledgeProposal = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, a);
    assert_eq!(decoded.memory_sources[1].revision, u64::MAX);
}
#[test]
fn implementation_null_and_schema_identity_are_not_wildcards() {
    let bound = tool("lookup", Some("code:r1")).identity();
    let other = tool("lookup", Some("code:r2")).identity();
    let unbound = tool("lookup", None).identity();
    assert_ne!(bound, other);
    assert_ne!(bound, unbound);
    assert!(canonical_external_tool_identities(vec![bound.clone(), other]).is_err());
    let mut invalid = bound;
    invalid.environment_revision = None;
    assert!(canonical_external_tool_identities(vec![invalid]).is_err());
    assert!(
        canonical_external_tool_identities(vec![])
            .unwrap()
            .is_empty()
    );
}
#[test]
fn proposals_reject_duplicate_sources_and_unbounded_or_injected_data() {
    let mut p = proposal();
    p.memory_sources[1] = p.memory_sources[0].clone();
    assert!(p.canonicalize().is_err());
    let mut p = proposal();
    p.instructions = "é".repeat(16385);
    assert!(p.canonicalize().is_err());
    let mut p = proposal();
    p.external_tools = vec![tool("same", None); 33];
    assert!(p.canonicalize().is_err());
    let mut wire = serde_json::to_value(proposal()).unwrap();
    wire["source_code"] = serde_json::json!("print('not knowledge')");
    assert!(serde_json::from_value::<KnowledgeProposal>(wire).is_err());
}
#[test]
fn optional_identity_must_be_explicit_null_not_missing() {
    let mut wire = serde_json::to_value(tool("lookup", None)).unwrap();
    wire.as_object_mut()
        .unwrap()
        .remove("implementation_revision");
    assert!(serde_json::from_value::<ExternalToolKnowledge>(wire).is_err());
}

#[test]
fn knowledge_receipt_is_atomic_immutable_and_survives_reopen() {
    let dir = tempfile::TempDir::new().unwrap();
    let db = LearningMemory::open(dir.path()).unwrap();
    install_contracts(&db);
    let request = proposal();
    let receipt = db
        .propose_knowledge("company", "scope", request.clone(), "original-author")
        .unwrap();
    assert_eq!(receipt.record.state, ProcedureState::Candidate);
    assert!(
        db.registered_procedure("company", "scope", &request.id)
            .unwrap()
            .is_some()
    );
    drop(db);
    let db = LearningMemory::open(dir.path()).unwrap();
    let mut reordered = request.clone();
    reordered.external_tools.reverse();
    reordered.memory_sources.reverse();
    assert_eq!(
        db.propose_knowledge("company", "scope", reordered, "another-author")
            .unwrap(),
        receipt
    );
    let mut changed = request.clone();
    changed.title = "Another meaning".into();
    assert!(matches!(
        db.propose_knowledge("company", "scope", changed, "author"),
        Err(Error::ConstraintViolation(_))
    ));
    assert!(
        db.knowledge_receipt("other-company", "scope", &request.id)
            .unwrap()
            .is_none()
    );
    assert!(
        db.knowledge_receipt("company", "another-subject", &request.id)
            .unwrap()
            .is_none()
    );
    // Synthetic active state isolates selector routing from evaluation efficacy.
    let mut active = receipt.record.clone();
    active.state = ProcedureState::Active;
    db.inner
        .db
        .put(
            procedure_key(&active.scope, &request.id),
            encode(&active).unwrap(),
        )
        .unwrap();
    assert!(
        db.select_registered("company", "scope", "policy", "context", 65536)
            .unwrap()
            .is_none()
    );
    assert!(
        db.select(
            &active.scope,
            &active.proposal.task,
            &active.proposal.baseline_revision,
            &active.proposal.policy.evaluation_contract,
            65536
        )
        .unwrap()
        .is_none()
    );
    let memory_dir = tempfile::TempDir::new().unwrap();
    let memory = crate::RocksDbMemoryStorage::open(crate::MemoryStorageConfig::for_testing(
        memory_dir.path(),
    ))
    .unwrap();
    let observed = db
        .inspect_knowledge(&memory, "company", "scope", &request.id)
        .unwrap()
        .unwrap();
    assert!(observed.qualification_active);
    assert!(!observed.evidence.eligible);
    assert!(!observed.eligible_for_knowledge_reuse);
    assert_eq!(observed.receipt, receipt);
    assert!(matches!(
        db.propose_registered(
            "company",
            "scope",
            observed.procedure.receipt.request,
            "writer"
        ),
        Err(Error::ValidationError(_))
    ));
    let mut forged = receipt.clone();
    forged.request.instructions = "tampered".into();
    db.inner
        .db
        .put(
            key("company", "scope", &request.id).unwrap(),
            encode(&forged).unwrap(),
        )
        .unwrap();
    assert!(matches!(
        db.knowledge_receipt("company", "scope", &request.id),
        Err(Error::DataCorruption(_))
    ));
}

fn install_contracts(db: &LearningMemory) {
    use super::super::registry::{EvaluationContext, PolicyAlgorithm, PolicyDefinition};
    db.register_policy(
        "company",
        "policy",
        PolicyDefinition {
            algorithm: PolicyAlgorithm::FixedBudgetHoeffdingV1,
            parameters: LearningPolicy {
                evaluator_id: "evaluator".into(),
                evaluation_contract: "contract".into(),
                ..Default::default()
            },
        },
        "admin",
    )
    .unwrap();
    db.register_context(
        "company",
        "context",
        EvaluationContext {
            task: "lookup".into(),
            baseline_revision: "baseline".into(),
            model_provider: "provider".into(),
            model_revision: "model".into(),
            tools: [
                ("a".into(), "schema:r1".into()),
                ("z".into(), "schema:r1".into()),
            ]
            .into(),
            environment_revision: "env:r1".into(),
            evaluation_contract: "contract".into(),
            dataset_revision: "dataset".into(),
            harness_revision: "harness".into(),
            permissions_revision: "permissions".into(),
        },
        "admin",
    )
    .unwrap();
}

#[test]
fn selection_filters_evidence_and_identity_before_ranking_and_never_returns_partial_winner() {
    use crate::storage::platform::{MemoryCommand, MemoryOperation, RecordAuthor, RecordInput};
    let learning_dir = tempfile::TempDir::new().unwrap();
    let db = LearningMemory::open(learning_dir.path()).unwrap();
    install_contracts(&db);
    let memory_dir = tempfile::TempDir::new().unwrap();
    let memory = crate::RocksDbMemoryStorage::open(crate::MemoryStorageConfig::for_testing(
        memory_dir.path(),
    ))
    .unwrap();
    let source = memory
        .apply_memory_command(
            "scope",
            &RecordAuthor {
                credential_id: Uuid::new_v4(),
                subject_id: "writer".into(),
            },
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "source".into(),
                operation: MemoryOperation::Create {
                    record: RecordInput {
                        episode_type: crate::EpisodeType::Observation,
                        content: crate::EpisodeContent::new("authorized knowledge"),
                        event_time_millis: 1,
                        valid_until_millis: None,
                        tags: vec![],
                        metadata: Default::default(),
                    },
                },
            },
        )
        .unwrap();
    let mut good = proposal();
    good.id = "b-valid".into();
    good.memory_sources = vec![MemorySourceRef {
        record_id: source.record_id,
        revision: source.revision,
    }];
    let mut invalid = good.clone();
    invalid.id = "a-invalid".into();
    invalid.memory_sources[0].record_id = Uuid::new_v4();
    let mut other = good.clone();
    other.id = "c-other-code".into();
    other.external_tools[1].implementation_revision = Some("code:r2".into());
    for (request, score) in [(invalid, 0.9), (good.clone(), 0.5), (other, 0.99)] {
        let receipt = db
            .propose_knowledge("company", "scope", request, "writer")
            .unwrap();
        let mut active = receipt.record;
        active.state = ProcedureState::Active;
        active.lower_improvement_bound = Some(score);
        // Synthetic ledger state tests routing, not statistical qualification.
        db.inner
            .db
            .put(
                procedure_key(&active.scope, &active.proposal.id),
                encode(&active).unwrap(),
            )
            .unwrap();
    }
    let query = KnowledgeSelectRequest {
        policy_id: "policy".into(),
        context_id: "context".into(),
        max_instruction_bytes: 65536,
        candidate_limit: 1000,
        external_tool_identities: good.external_identities().unwrap(),
    };
    let selected = db
        .select_knowledge(&memory, "company", "scope", query.clone())
        .unwrap();
    assert!(selected.coverage.complete);
    assert_eq!(selected.coverage.candidates_eligible, 1);
    match selected.selection {
        KnowledgeSelectionOutcome::Procedure { knowledge } => {
            assert_eq!(knowledge.receipt.request.id, "b-valid")
        }
        _ => panic!("eligible runner-up must survive"),
    }
    let mut limited = query.clone();
    limited.candidate_limit = 1;
    let partial = db
        .select_knowledge(&memory, "company", "scope", limited)
        .unwrap();
    assert!(!partial.coverage.complete);
    assert!(
        matches!(partial.selection,KnowledgeSelectionOutcome::Baseline {reason,..} if reason=="selection_incomplete")
    );
    let mut unbound = query.clone();
    unbound.external_tool_identities[0].implementation_revision = None;
    unbound.external_tool_identities[0].environment_revision = None;
    assert!(
        matches!(db.select_knowledge(&memory,"company","scope",unbound).unwrap().selection,KnowledgeSelectionOutcome::Baseline {reason,..} if reason=="no_eligible_bound_procedure")
    );
    for i in 0..1000 {
        let mut inactive = good.clone();
        inactive.id = format!("inactive-{i:04}");
        db.propose_knowledge("company", "scope", inactive, "writer")
            .unwrap();
    }
    let over_limit = db
        .select_knowledge(&memory, "company", "scope", query)
        .unwrap();
    assert_eq!(over_limit.coverage.records_examined, 1000);
    assert!(!over_limit.coverage.complete);
    assert!(
        matches!(over_limit.selection,KnowledgeSelectionOutcome::Baseline {reason,..} if reason=="selection_incomplete")
    );
}

#[test]
fn proposal_digest_matches_independent_cross_language_vector() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("proposal-digest-v2.json")).unwrap();
    let proposal: KnowledgeProposal = serde_json::from_value(fixture["input"].clone()).unwrap();
    let canonical = proposal.canonicalize().unwrap();
    assert_eq!(
        serde_json::to_string(&canonical).unwrap(),
        fixture["canonical_json"].as_str().unwrap()
    );
    assert_eq!(
        super::super::registry::digest(&canonical).unwrap(),
        fixture["sha256"].as_str().unwrap()
    );
    assert_eq!(canonical.memory_sources[1].revision, u64::MAX);
    let mut changed = canonical;
    changed.instructions = changed.instructions.replace("e\u{301}", "é");
    assert_ne!(
        super::super::registry::digest(&changed).unwrap(),
        fixture["sha256"].as_str().unwrap()
    );
}

#[test]
fn acknowledged_knowledge_survives_process_exit_without_database_destructors() {
    const CHILD_DIRECTORY: &str = "QILBEEDB_KNOWLEDGE_CRASH_FIXTURE";
    if let Some(directory) = std::env::var_os(CHILD_DIRECTORY) {
        let directory = std::path::PathBuf::from(directory);
        let db = LearningMemory::open(directory.join("database")).unwrap();
        install_contracts(&db);
        let receipt = db
            .propose_knowledge("company", "scope", proposal(), "original-author")
            .unwrap();
        let mut acknowledged = std::fs::File::create(directory.join("acknowledged.json")).unwrap();
        use std::io::Write;
        acknowledged
            .write_all(&serde_json::to_vec(&receipt).unwrap())
            .unwrap();
        acknowledged.sync_all().unwrap();
        // No Rust destructors or graceful RocksDB shutdown run on this path.
        // This simulates process loss after acknowledgement, not host power loss.
        std::process::exit(77);
    }
    let directory = tempfile::TempDir::new().unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "learning::store::knowledge::tests::acknowledged_knowledge_survives_process_exit_without_database_destructors", "--nocapture"])
        .env(CHILD_DIRECTORY, directory.path())
        .output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(77),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let receipt: KnowledgeReceipt =
        serde_json::from_slice(&std::fs::read(directory.path().join("acknowledged.json")).unwrap())
            .unwrap();
    let db = LearningMemory::open(directory.path().join("database")).unwrap();
    assert_eq!(
        db.knowledge_receipt("company", "scope", &receipt.request.id)
            .unwrap(),
        Some(receipt.clone())
    );
    let retried = db
        .propose_knowledge("company", "scope", proposal(), "retry-author")
        .unwrap();
    assert_eq!(retried, receipt);
    assert_eq!(
        db.registered_procedure("company", "scope", &receipt.request.id)
            .unwrap()
            .unwrap()
            .record,
        receipt.record
    );
}

#[test]
fn receipt_digest_matches_cross_language_original_candidate_vector() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("receipt-digest-v2.json")).unwrap();
    let receipt: KnowledgeReceipt = serde_json::from_value(fixture["receipt"].clone()).unwrap();
    let bytes = serde_json::to_string(&(
        receipt.schema_version,
        &receipt.tenant,
        &receipt.namespace,
        &receipt.request,
        &receipt.policy_digest,
        &receipt.context_digest,
        &receipt.actor,
        &receipt.record,
    ))
    .unwrap();
    assert_eq!(bytes, fixture["canonical_json"].as_str().unwrap());
    assert_eq!(
        receipt.digest().unwrap(),
        fixture["sha256"].as_str().unwrap()
    );
    assert_eq!(receipt.record.proposal.policy.max_cost_units, u64::MAX);
    let mut changed = receipt.clone();
    changed.actor = "retry-author".into();
    assert_ne!(changed.digest().unwrap(), receipt.receipt_digest);
    let mut changed = receipt.clone();
    changed.record.state = ProcedureState::Active;
    assert_ne!(changed.digest().unwrap(), receipt.receipt_digest);
}

#[test]
fn selection_shares_dependency_budget_across_candidates_and_never_returns_early_winner() {
    use crate::storage::platform::{MemoryCommand, MemoryOperation, RecordAuthor, RecordInput};
    let learning_dir = tempfile::TempDir::new().unwrap();
    let db = LearningMemory::open(learning_dir.path()).unwrap();
    install_contracts(&db);
    let memory_dir = tempfile::TempDir::new().unwrap();
    let memory = crate::RocksDbMemoryStorage::open(crate::MemoryStorageConfig::for_testing(
        memory_dir.path(),
    ))
    .unwrap();
    let actor = RecordAuthor {
        credential_id: Uuid::new_v4(),
        subject_id: "writer".into(),
    };
    let mut sources = Vec::new();
    for index in 0..4097 {
        let receipt = memory
            .apply_memory_command(
                "scope",
                &actor,
                &MemoryCommand {
                    contract_version: 1,
                    idempotency_key: format!("budget-source-{index}"),
                    operation: MemoryOperation::Create {
                        record: RecordInput {
                            episode_type: crate::EpisodeType::Observation,
                            content: crate::EpisodeContent::new("Synthetic budget fixture"),
                            event_time_millis: 1,
                            valid_until_millis: None,
                            tags: vec![],
                            metadata: Default::default(),
                        },
                    },
                },
            )
            .unwrap();
        sources.push(MemorySourceRef {
            record_id: receipt.record_id,
            revision: receipt.revision,
        });
    }
    let activate = |id: String, roots: Vec<MemorySourceRef>| {
        let mut request = proposal();
        request.id = id;
        request.memory_sources = roots;
        let receipt = db
            .propose_knowledge("company", "scope", request, "writer")
            .unwrap();
        let mut record = receipt.record;
        record.state = ProcedureState::Active;
        record.lower_improvement_bound = Some(0.5);
        // Synthetic state isolates bounded selection, not statistical efficacy.
        db.inner
            .db
            .put(
                procedure_key(&record.scope, &record.proposal.id),
                encode(&record).unwrap(),
            )
            .unwrap();
    };
    for (index, roots) in sources[..4096].chunks(16).enumerate() {
        activate(format!("candidate-{index:04}"), roots.to_vec());
    }
    let query = KnowledgeSelectRequest {
        policy_id: "policy".into(),
        context_id: "context".into(),
        max_instruction_bytes: 65536,
        candidate_limit: 1000,
        external_tool_identities: proposal().external_identities().unwrap(),
    };
    let exact = db
        .select_knowledge(&memory, "company", "scope", query.clone())
        .unwrap();
    assert!(exact.coverage.complete);
    assert_eq!(exact.coverage.candidates_eligible, 256);
    assert!(matches!(
        exact.selection,
        KnowledgeSelectionOutcome::Procedure { .. }
    ));
    // Reusing an already observed dependency must not spend another distinct-read slot.
    activate("candidate-0256".into(), vec![sources[0].clone()]);
    let cached = db
        .select_knowledge(&memory, "company", "scope", query.clone())
        .unwrap();
    assert!(cached.coverage.complete);
    assert_eq!(cached.coverage.candidates_eligible, 257);
    activate("candidate-0257".into(), vec![sources[4096].clone()]);
    assert!(
        matches!(db.select_knowledge(&memory,"company","scope",query),Err(Error::ValidationError(message)) if message.contains("Dependency record budget exhausted"))
    );
}
