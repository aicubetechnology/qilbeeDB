//! Exact strategy evidence and scope isolation over real HTTP with served schema validation.
use super::history_errors::call;
use super::*;

#[tokio::test]
async fn strategy_http_binds_evidence_checks_roles_and_preserves_original_receipts() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "operator").unwrap();
    let mut spec = spec();
    spec.subject_id = "observer".into();
    spec.capabilities = [
        Capability::MemoryRead,
        Capability::ProcedurePropose,
        Capability::ExperienceRead,
        Capability::ExperienceWrite,
        Capability::ExperienceReport,
    ]
    .into();
    spec.grants
        .push(serde_json::from_value(memory_scope("private")).unwrap());
    let full = identity.issue(&admin.secret, spec.clone()).unwrap();
    let outside = identity.issue(&foreign.secret, spec.clone()).unwrap();
    let mut other_spec = spec.clone();
    other_spec.subject_id = "other".into();
    let other = identity.issue(&admin.secret, other_spec).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();
    let api: Value = client
        .get(format!("{base}/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let context = json!({"contract_version":1,"id":"context","context":{"task":"task","baseline_revision":"baseline","model_provider":"external","model_revision":"v1","tools":{},"environment_revision":"v1","evaluation_contract":"rubric","dataset_revision":"v1","harness_revision":"v1","permissions_revision":"v1"}});
    let registered = call(
        &client,
        &base,
        &api,
        "/api/v1/learning/contexts",
        &admin.secret,
        context,
        200,
        None,
    )
    .await;
    let digest = registered["entry"]["payload_digest"].clone();
    let policy = json!({"contract_version":1,"id":"policy","definition":{"algorithm":"fixed_budget_hoeffding_v1","parameters":{"qualification_trials":32,"confidence_delta":0.01,"min_improvement":0.05,"min_candidate_utility":0.7,"max_cost_units":10000,"max_latency_ms":60000,"max_failure_streak":3,"evaluator_id":"judge","evaluation_contract":"rubric"}}});
    call(
        &client,
        &base,
        &api,
        "/api/v1/learning/policies",
        &admin.secret,
        policy,
        200,
        None,
    )
    .await;
    for visibility in ["shared", "private"] {
        let scope = memory_scope(visibility);
        let mut refs = Vec::new();
        for (n, outcome) in ["succeeded", "failed", "unknown"].into_iter().enumerate() {
            let attempt = format!("attempt-{n}");
            call(&client,&base,&api,"/api/v1/experiences",&full.secret,json!({"contract_version":1,"scope":scope,"request":{"id":attempt,"context_id":"context","reporter_subject_id":"observer","accounting_unit":"tokens","input":{"reference":"fixture:input","sha256":"a".repeat(64)},"parent":null}}),200,None).await;
            let event=call(&client,&base,&api,"/api/v1/experiences/events",&full.secret,json!({"contract_version":1,"scope":scope,"attempt_id":attempt,"command":{"event_id":"event","expected_revision":1,"context_digest":digest,"outcome":outcome,"evidence":{"reference":format!("fixture:{n}"),"sha256":"b".repeat(64)},"cost_units":null,"latency_ms":null}}),200,None).await;
            refs.push(json!({"attempt_id":attempt,"event_id":"event","event_digest":event["event"]["event_digest"]}));
        }
        let body = json!({"contract_version":1,"scope":scope,"strategy":{"id":"strategy","policy_id":"policy","context_id":"context","instructions":"Verify the receipt before retrying.","preconditions":["A receipt can be read."],"counterexamples":["An unknown result is not a failed result."],"extractor":{"provider":"external","model":"extractor","model_revision":"v1","prompt_revision":"v1","evidence_ref":"fixture:extraction"},"selection":{"context_digest":digest,"accounting_unit":"tokens","events":refs}}});
        for missing in [Capability::ProcedurePropose, Capability::ExperienceRead] {
            let mut limited = spec.clone();
            limited.capabilities.remove(&missing);
            let limited = identity.issue(&admin.secret, limited).unwrap();
            call(
                &client,
                &base,
                &api,
                "/api/v1/learning/strategies",
                &limited.secret,
                body.clone(),
                403,
                Some("forbidden"),
            )
            .await;
        }
        call(
            &client,
            &base,
            &api,
            "/api/v1/learning/strategies",
            &outside.secret,
            body.clone(),
            404,
            Some("record_not_found"),
        )
        .await;
        if visibility == "private" {
            call(
                &client,
                &base,
                &api,
                "/api/v1/learning/strategies",
                &other.secret,
                body.clone(),
                404,
                Some("record_not_found"),
            )
            .await;
        }
        let accepted = call(
            &client,
            &base,
            &api,
            "/api/v1/learning/strategies",
            &full.secret,
            body.clone(),
            200,
            None,
        )
        .await;
        assert_eq!(accepted["receipt"]["summary"]["failed"], 1);
        assert_eq!(accepted["receipt"]["summary"]["unknown"], 1);
        assert_eq!(
            accepted["receipt"]["proposal"]["record"]["qualification_count"],
            0
        );
        assert_eq!(
            accepted,
            call(
                &client,
                &base,
                &api,
                "/api/v1/learning/strategies",
                &full.secret,
                body.clone(),
                200,
                None
            )
            .await
        );
        let read = json!({"contract_version":1,"scope":scope,"strategy_id":"strategy"});
        assert_eq!(
            accepted,
            call(
                &client,
                &base,
                &api,
                "/api/v1/learning/strategies/read",
                &full.secret,
                read.clone(),
                200,
                None
            )
            .await
        );
        call(
            &client,
            &base,
            &api,
            "/api/v1/learning/strategies/read",
            &outside.secret,
            read.clone(),
            404,
            Some("record_not_found"),
        )
        .await;
        if visibility == "private" {
            call(
                &client,
                &base,
                &api,
                "/api/v1/learning/strategies/read",
                &other.secret,
                read.clone(),
                404,
                Some("record_not_found"),
            )
            .await;
        }
        for missing in [Capability::MemoryRead, Capability::ExperienceRead] {
            let mut limited = spec.clone();
            limited.capabilities.remove(&missing);
            let limited = identity.issue(&admin.secret, limited).unwrap();
            call(
                &client,
                &base,
                &api,
                "/api/v1/learning/strategies/read",
                &limited.secret,
                read.clone(),
                403,
                Some("forbidden"),
            )
            .await;
        }
        let mut changed = body.clone();
        changed["strategy"]["extractor"]["prompt_revision"] = "v2".into();
        call(
            &client,
            &base,
            &api,
            "/api/v1/learning/strategies",
            &full.secret,
            changed,
            409,
            Some("idempotency_conflict"),
        )
        .await;
        for field in ["project_id", "agent_id", "mission_id"] {
            let mut wrong = body.clone();
            wrong["scope"][field] = "outside".into();
            call(
                &client,
                &base,
                &api,
                "/api/v1/learning/strategies",
                &full.secret,
                wrong,
                403,
                Some("forbidden"),
            )
            .await;
        }
    }
    identity
        .revoke(&admin.secret, full.credential.id, full.credential.revision)
        .unwrap();
    call(
        &client,
        &base,
        &api,
        "/api/v1/learning/strategies/read",
        &full.secret,
        json!({"contract_version":1,"scope":memory_scope("shared"),"strategy_id":"strategy"}),
        401,
        Some("unauthorized"),
    )
    .await;
    server.abort();
}
