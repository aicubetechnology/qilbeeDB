//! Native HTTP and published-schema qualification for company evidence discovery.
use super::administration::Client;
use super::*;
use qilbee_memory::learning::{LearningPolicy, PolicyAlgorithm, PolicyDefinition};
const QUERY: &str = "/api/v1/company/learning/evidence/query";
const READ: &str = "/api/v1/company/learning/evidence/read";
fn scope() -> Value {
    json!({"project_id":"learning-only","agent_id":"agent","mission_id":null,"visibility":"private"})
}
fn resource(kind: &str, id: &str, owner: &str) -> Value {
    json!({"kind":kind,"id":id,"scope":scope(),"private_subject_id":owner})
}
fn query(resource: Value, kind: &str) -> Value {
    json!({"contract_version":1,"query":{"resource":resource,"kind":kind}})
}
async fn put(http: &Client, path: &str, token: &str, body: Value) -> Value {
    http.call("POST", path, path, token, body, 200).await
}

#[tokio::test]
async fn learning_evidence_real_http_preserves_receipt_kinds_company_isolation_and_revocation() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "owner").unwrap();
    let reader = identity
        .issue(
            &admin.secret,
            CredentialSpec {
                subject_id: "auditor".into(),
                capabilities: [Capability::CredentialAdmin].into(),
                grants: vec![],
                scope_policy: None,
                expires_at_millis: None,
            },
        )
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .unwrap();
    let api = client
        .get(format!("{base}/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let http = Client { client, base, api };
    let mut original = Value::Null;
    for (company, owner) in [("company", &admin), ("foreign", &foreign)] {
        put(&http,"/api/v1/learning/policies",&owner.secret,json!({"contract_version":1,"id":"policy","definition":PolicyDefinition {algorithm:PolicyAlgorithm::FixedBudgetHoeffdingV1,parameters:LearningPolicy {qualification_trials:32,evaluator_id:"writer".into(),evaluation_contract:"rubric".into(),..Default::default()}}})).await;
        let context=put(&http,"/api/v1/learning/contexts",&owner.secret,json!({"contract_version":1,"id":"context","context":{"task":"Inspect learning evidence","baseline_revision":"baseline","model_provider":"external","model_revision":"v1","tools":{},"environment_revision":"v1","evaluation_contract":"rubric","dataset_revision":"v1","harness_revision":"v1","permissions_revision":"v1"}})).await;
        let writer = identity
            .issue(
                &owner.secret,
                CredentialSpec {
                    subject_id: "writer".into(),
                    capabilities: [
                        Capability::ProcedurePropose,
                        Capability::ProcedureEvaluate,
                        Capability::ExperienceWrite,
                        Capability::ExperienceReport,
                        Capability::ToolDevelop,
                        Capability::ToolAdmin,
                        Capability::ToolReport,
                    ]
                    .into(),
                    grants: vec![serde_json::from_value(scope()).unwrap()],
                    scope_policy: None,
                    expires_at_millis: None,
                },
            )
            .unwrap();
        put(&http,"/api/v1/learning/proposals",&writer.secret,json!({"contract_version":1,"scope":scope(),"proposal":{"id":"procedure","policy_id":"policy","context_id":"context","instructions":"Inspect the original evidence","source_refs":["fixture:source"]}})).await;
        for (case, status, cost) in [
            ("accepted", "complete", Some(3)),
            ("unknown", "complete", None),
            ("rejected", "rejected", None),
            ("incomplete", "incomplete", None),
            ("cancelled", "cancelled", None),
            ("pending", "pending_or_unknown", None),
        ] {
            let saved=put(&http,"/api/v1/learning/evaluations",&writer.secret,json!({"contract_version":1,"scope":scope(),"procedure_id":"procedure","submission":{"case_id":case,"phase":"Qualification","policy_id":"policy","context_id":"context","baseline_revision":"baseline","evidence_ref":format!("fixture:{company}:{case}"),"status":status,"baseline_utility":0.2,"candidate_utility":0.8,"candidate_cost_units":cost,"candidate_latency_ms":4,"detail":null}})).await;
            if company == "company" && case == "unknown" {
                original = saved["receipt"].clone();
            }
        }
        put(&http,"/api/v1/experiences",&writer.secret,json!({"contract_version":1,"scope":scope(),"request":{"id":"attempt","context_id":"context","reporter_subject_id":"writer","accounting_unit":"tokens","input":{"reference":"fixture:task","sha256":"a".repeat(64)},"parent":null}})).await;
        put(&http,"/api/v1/experiences/events",&writer.secret,json!({"contract_version":1,"scope":scope(),"attempt_id":"attempt","command":{"event_id":"observed","expected_revision":1,"context_digest":context["entry"]["payload_digest"],"outcome":"unknown","evidence":{"reference":format!("fixture:{company}:event"),"sha256":"b".repeat(64)},"cost_units":null,"latency_ms":null}})).await;
        let executor=put(&http,"/api/v1/tools/executors",&writer.secret,json!({"contract_version":1,"profile":{"id":"executor","subject_id":"writer","runtime_image_digest":format!("sha256:{}","c".repeat(64)),"environment_revision":"v1","permissions_revision":"v1","max_cost_units":10,"max_latency_ms":1000}})).await;
        put(&http,"/api/v1/tools/development/requests",&writer.secret,json!({"contract_version":1,"scope":scope(),"request":{"id":"development","executor_id":"executor","objective":"Build a recovery tool","parent_artifact_id":null,"repair_evidence_ref":null}})).await;
        put(&http,"/api/v1/tools/development/commands",&writer.secret,json!({"contract_version":1,"scope":scope(),"request_id":"development","command":{"event_id":"observed","expected_revision":1,"action":{"type":"report","report":{"executor_profile_digest":executor["executor"]["profile_digest"],"outcome":"pending_or_unknown","evidence_ref":"fixture:worker","detail":null,"cost_units":null,"latency_ms":null,"artifact":null}}}})).await;
        http.call(
            "POST",
            QUERY,
            QUERY,
            &writer.secret,
            query(
                resource("procedure", "procedure", "writer"),
                "evaluation_submission",
            ),
            403,
        )
        .await;
        identity
            .revoke(
                &owner.secret,
                writer.credential.id,
                writer.credential.revision,
            )
            .unwrap();
        http.call(
            "POST",
            QUERY,
            QUERY,
            &writer.secret,
            query(
                resource("procedure", "procedure", "writer"),
                "evaluation_submission",
            ),
            401,
        )
        .await;
    }
    for (parent, id, kind, count) in [
        ("procedure", "procedure", "evaluation_submission", 6),
        ("procedure", "procedure", "paired_evaluation", 1),
        ("experience", "attempt", "experience_event", 1),
        (
            "tool_development",
            "development",
            "tool_development_event",
            1,
        ),
    ] {
        let page = put(
            &http,
            QUERY,
            &reader.secret,
            query(resource(parent, id, "writer"), kind),
        )
        .await;
        assert_eq!(page["company_id"], "company");
        let entries = page["page"]["entries"].as_array().unwrap();
        assert_eq!(entries.len(), count);
        for e in entries {
            let detail = put(
                &http,
                READ,
                &reader.secret,
                json!({"contract_version":1,"evidence":e["evidence"]}),
            )
            .await;
            assert_eq!(detail["details"]["kind"], kind);
            if e["evidence"]["id"] == "unknown" {
                assert_eq!(detail["details"]["record"], original);
            }
        }
    }
    let mut limited = query(
        resource("procedure", "procedure", "writer"),
        "evaluation_submission",
    );
    limited["query"]["limit"] = json!(1);
    let page = put(&http, QUERY, &reader.secret, limited.clone()).await;
    limited["query"]["cursor"] = page["page"]["next_cursor"].clone();
    http.call("POST", QUERY, QUERY, &foreign.secret, limited.clone(), 400)
        .await;
    let second = put(&http, QUERY, &reader.secret, limited).await;
    assert_ne!(
        page["page"]["entries"][0]["evidence"]["id"],
        second["page"]["entries"][0]["evidence"]["id"]
    );
    let mut empty = query(
        resource("procedure", "procedure", "writer"),
        "evaluation_submission",
    );
    empty["query"]["text"] = json!("no matching evidence");
    empty["query"]["max_scanned_records"] = json!(1);
    let empty = put(&http, QUERY, &reader.secret, empty).await;
    assert!(empty["page"]["entries"].as_array().unwrap().is_empty());
    assert!(!empty["page"]["next_cursor"].is_null());
    let foreign_page = put(
        &http,
        QUERY,
        &foreign.secret,
        query(
            resource("procedure", "procedure", "writer"),
            "evaluation_submission",
        ),
    )
    .await;
    assert_eq!(foreign_page["company_id"], "foreign");
    assert!(
        foreign_page["page"]["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["title"].as_str().unwrap().starts_with("fixture:foreign:"))
    );
    http.call(
        "POST",
        QUERY,
        QUERY,
        &reader.secret,
        query(
            resource("procedure", "procedure", "someone-else"),
            "evaluation_submission",
        ),
        404,
    )
    .await;
    http.call("POST",READ,READ,&reader.secret,json!({"contract_version":1,"evidence":{"resource":resource("procedure","procedure","writer"),"kind":"evaluation_submission","id":"missing"}}),404).await;
    http.call(
        "POST",
        QUERY,
        QUERY,
        &reader.secret,
        query(
            resource("procedure", "procedure", "writer"),
            "experience_event",
        ),
        400,
    )
    .await;
    let mut oversized = query(
        resource("procedure", "procedure", "writer"),
        "evaluation_submission",
    );
    oversized["query"]["text"] = json!("x".repeat(65537));
    http.call("POST", QUERY, QUERY, &reader.secret, oversized, 413)
        .await;
    identity
        .revoke(
            &admin.secret,
            reader.credential.id,
            reader.credential.revision,
        )
        .unwrap();
    http.call(
        "POST",
        QUERY,
        QUERY,
        &reader.secret,
        query(
            resource("procedure", "procedure", "writer"),
            "evaluation_submission",
        ),
        401,
    )
    .await;
    http.call("POST",READ,READ,&reader.secret,json!({"contract_version":1,"evidence":{"resource":resource("procedure","procedure","writer"),"kind":"evaluation_submission","id":"unknown"}}),401).await;
    server.abort();
}

#[test]
fn learning_evidence_guide_examples_match_published_request_contracts() {
    let api: Value =
        serde_json::from_str(include_str!("../../../../docs/api/openapi.json")).unwrap();
    let guide = include_str!("../../../../docs/api/learning-evidence-history.md");
    let examples: Vec<_> = guide
        .split("```json\n")
        .skip(1)
        .map(|s| serde_json::from_str::<Value>(s.split("```").next().unwrap()).unwrap())
        .collect();
    assert_eq!(examples.len(), 2);
    for (example, schema) in examples.iter().zip([
        "CompanyLearningEvidenceQueryRequest",
        "CompanyLearningEvidenceReadRequest",
    ]) {
        let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","allOf":[{"$ref":format!("#/components/schemas/{schema}")}],"components":api["components"]});
        jsonschema::draft202012::options()
            .build(&schema)
            .unwrap()
            .validate(example)
            .unwrap();
    }
}
