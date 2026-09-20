use super::*;
use qilbee_memory::learning::{LearningPolicy, PolicyAlgorithm, PolicyDefinition};
fn scope() -> Value {
    json!({"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"})
}
fn policy() -> Value {
    json!({"contract_version":1,"id":"policy-v1","definition":PolicyDefinition { algorithm:PolicyAlgorithm::FixedBudgetHoeffdingV1,parameters:LearningPolicy { qualification_trials:32,evaluator_id:"evaluator".into(),evaluation_contract:"rubric-v1".into(),..Default::default() }}})
}
fn context() -> Value {
    json!({"contract_version":1,"id":"context-v1","context":{"task":"task-v1","baseline_revision":"baseline-v1","model_provider":"provider","model_revision":"model-v1","tools":{},"environment_revision":"env-v1","evaluation_contract":"rubric-v1","dataset_revision":"dataset-v1","harness_revision":"harness-v1","permissions_revision":"permissions-v1"}})
}
fn proposal(id: &str) -> Value {
    json!({"contract_version":1,"scope":scope(),"proposal":{"id":id,"policy_id":"policy-v1","context_id":"context-v1","instructions":"Check source evidence.","source_refs":["training-v1"]}})
}
fn read(id: &str) -> Value {
    json!({"contract_version":1,"scope":scope(),"procedure_id":id})
}
fn evaluation(id: &str, case: &str) -> Value {
    json!({"contract_version":1,"scope":scope(),"procedure_id":id,"submission":{"case_id":case,"phase":"Qualification","policy_id":"policy-v1","context_id":"context-v1","baseline_revision":"baseline-v1","evidence_ref":format!("held-out:{case}"),"status":"complete","baseline_utility":0.0,"candidate_utility":1.0,"candidate_cost_units":1,"candidate_latency_ms":1,"detail":null}})
}
fn selection() -> Value {
    json!({"contract_version":1,"scope":scope(),"policy_id":"policy-v1","context_id":"context-v1","max_instruction_bytes":4096})
}
async fn configure(router: &Router, admin: &str) {
    assert_eq!(
        request(router, "POST", "/api/v1/learning/policies", admin, policy())
            .await
            .0,
        StatusCode::OK
    );
    assert_eq!(
        request(
            router,
            "POST",
            "/api/v1/learning/contexts",
            admin,
            context()
        )
        .await
        .0,
        StatusCode::OK
    );
}
fn clients(identity: &IdentityStore, admin: &str) -> (String, String) {
    let proposer = identity.issue(admin, spec()).unwrap().secret;
    let mut evaluator = spec();
    evaluator.subject_id = "evaluator".into();
    evaluator.capabilities = [Capability::ProcedureEvaluate, Capability::MemoryRead].into();
    (proposer, identity.issue(admin, evaluator).unwrap().secret)
}
#[tokio::test]
async fn learning_http_registry_enforces_administrative_authority_and_strict_inputs() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let (proposer, _) = clients(&identity, &admin.secret);
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/policies",
            &proposer,
            policy()
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    configure(&router, &admin.secret).await;
    let first = request(
        &router,
        "GET",
        "/api/v1/learning/policies/policy-v1",
        &admin.secret,
        Value::Null,
    )
    .await;
    assert_eq!(first.0, StatusCode::OK);
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/policies",
            &admin.secret,
            policy()
        )
        .await
        .1,
        first.1
    );
    let mut changed = policy();
    changed["definition"]["parameters"]["min_improvement"] = 0.4.into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/policies",
            &admin.secret,
            changed
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let mut injected = policy();
    injected["tenant_id"] = "other".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/policies",
            &admin.secret,
            injected
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    let foreign = identity.bootstrap_tenant("other", "admin").unwrap();
    assert_eq!(
        request(
            &router,
            "GET",
            "/api/v1/learning/policies/policy-v1",
            &foreign.secret,
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}
#[tokio::test]
async fn learning_http_cycle_preserves_uncertainty_exact_context_and_restart_history() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let (proposer, evaluator) = clients(&identity, &admin.secret);
    configure(&router, &admin.secret).await;
    let original = request(
        &router,
        "POST",
        "/api/v1/learning/proposals",
        &proposer,
        proposal("candidate"),
    )
    .await;
    assert_eq!(original.0, StatusCode::OK);
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/select",
            &proposer,
            selection()
        )
        .await
        .1["selection"]["type"],
        "baseline"
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/evaluations",
            &proposer,
            evaluation("candidate", "denied")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut unknown = evaluation("candidate", "unknown");
    unknown["submission"]["candidate_cost_units"] = Value::Null;
    let uncertain = request(
        &router,
        "POST",
        "/api/v1/learning/evaluations",
        &evaluator,
        unknown.clone(),
    )
    .await;
    assert_eq!(uncertain.0, StatusCode::OK);
    assert_eq!(uncertain.1["receipt"]["outcome"], "unknown_consumption");
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/evaluations",
            &evaluator,
            unknown
        )
        .await
        .1,
        uncertain.1
    );
    for n in 0..32 {
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/learning/evaluations",
                &evaluator,
                evaluation("candidate", &n.to_string())
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let active = request(
        &router,
        "POST",
        "/api/v1/learning/procedures/read",
        &proposer,
        read("candidate"),
    )
    .await
    .1;
    assert_eq!(active["procedure"]["record"]["state"], "Active");
    assert_eq!(active["procedure"]["record"]["qualification_count"], 32);
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/proposals",
            &proposer,
            proposal("candidate")
        )
        .await
        .1,
        original.1
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/select",
            &proposer,
            selection()
        )
        .await
        .1["selection"]["type"],
        "procedure"
    );
    let mut next = context();
    next["id"] = "context-v2".into();
    next["context"]["model_revision"] = "model-v2".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/contexts",
            &admin.secret,
            next
        )
        .await
        .0,
        StatusCode::OK
    );
    let mut incompatible = selection();
    incompatible["context_id"] = "context-v2".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/select",
            &proposer,
            incompatible
        )
        .await
        .1["selection"]["type"],
        "baseline"
    );
    for n in 0..3 {
        let mut bad = evaluation("candidate", &format!("monitor-{n}"));
        bad["submission"]["phase"] = "Monitoring".into();
        bad["submission"]["candidate_utility"] = 0.0.into();
        assert_eq!(
            request(
                &router,
                "POST",
                "/api/v1/learning/evaluations",
                &evaluator,
                bad
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    drop(router);
    drop(identity);
    let (router, _) = app(dir.path());
    let suspended = request(
        &router,
        "POST",
        "/api/v1/learning/procedures/read",
        &proposer,
        read("candidate"),
    )
    .await
    .1;
    assert_eq!(suspended["procedure"]["record"]["state"], "Suspended");
    assert_eq!(
        suspended["procedure"]["record"]["decisions"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/select",
            &proposer,
            selection()
        )
        .await
        .1["selection"]["baseline_revision"],
        "baseline-v1"
    );
    let mut receipt = read("candidate");
    receipt["case_id"] = "unknown".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/evaluations/read",
            &proposer,
            receipt
        )
        .await
        .1,
        uncertain.1
    );
}
#[tokio::test]
async fn learning_http_isolates_scopes_and_rejects_ineffective_procedures() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let (proposer, evaluator) = clients(&identity, &admin.secret);
    configure(&router, &admin.secret).await;
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/proposals",
            &proposer,
            proposal("bad")
        )
        .await
        .0,
        StatusCode::OK
    );
    for n in 0..32 {
        let mut bad = evaluation("bad", &n.to_string());
        bad["submission"]["candidate_utility"] = 0.0.into();
        request(
            &router,
            "POST",
            "/api/v1/learning/evaluations",
            &evaluator,
            bad,
        )
        .await;
    }
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/procedures/read",
            &proposer,
            read("bad")
        )
        .await
        .1["procedure"]["record"]["state"],
        "Rejected"
    );
    let foreign = identity.bootstrap_tenant("other", "admin").unwrap();
    let (other, _) = clients(&identity, &foreign.secret);
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/procedures/read",
            &other,
            read("bad")
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut wrong = read("bad");
    wrong["scope"]["agent_id"] = "other-agent".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/procedures/read",
            &proposer,
            wrong
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let mut injected = evaluation("bad", "injected");
    injected["submission"]["evaluator_id"] = "evaluator".into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/evaluations",
            &evaluator,
            injected
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn learning_http_concurrent_evaluation_retries_count_once_and_changed_case_conflicts() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let (proposer, evaluator) = clients(&identity, &admin.secret);
    configure(&router, &admin.secret).await;
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/proposals",
            &proposer,
            proposal("candidate")
        )
        .await
        .0,
        StatusCode::OK
    );
    let workers: Vec<_> = (0..8)
        .map(|_| {
            let router = router.clone();
            let token = evaluator.clone();
            tokio::spawn(async move {
                request(
                    &router,
                    "POST",
                    "/api/v1/learning/evaluations",
                    &token,
                    evaluation("candidate", "same-case"),
                )
                .await
            })
        })
        .collect();
    let mut original = None;
    for worker in workers {
        let response = worker.await.unwrap();
        assert_eq!(response.0, StatusCode::OK);
        if let Some(first) = &original {
            assert_eq!(&response.1, first);
        } else {
            original = Some(response.1);
        }
    }
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/procedures/read",
            &proposer,
            read("candidate")
        )
        .await
        .1["procedure"]["record"]["qualification_count"],
        1
    );
    let mut changed = evaluation("candidate", "same-case");
    changed["submission"]["candidate_utility"] = 0.0.into();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/evaluations",
            &evaluator,
            changed
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let mut wrong_subject = spec();
    wrong_subject.capabilities = [Capability::ProcedureEvaluate].into();
    let key = identity.issue(&admin.secret, wrong_subject).unwrap();
    assert_eq!(
        request(
            &router,
            "POST",
            "/api/v1/learning/evaluations",
            &key.secret,
            evaluation("candidate", "other-case")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
}
