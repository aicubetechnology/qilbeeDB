//! Discover retained resources through real HTTP with no delegated read credentials.
use super::administration::Client;
use super::*;
use qilbee_memory::learning::{LearningPolicy, PolicyAlgorithm, PolicyDefinition};

const QUERY: &str = "/api/v1/company/learning/query";
const READ: &str = "/api/v1/company/learning/read";

#[test]
fn company_learning_guide_examples_conform_to_published_request_schemas() {
    let api: Value =
        serde_json::from_str(include_str!("../../../../docs/api/openapi.json")).unwrap();
    let guide = include_str!("../../../../docs/api/company-learning-catalog.md");
    let mut count = 0;
    for block in guide.split("```json\n").skip(1) {
        let example: Value = serde_json::from_str(block.split("\n```").next().unwrap()).unwrap();
        let name = if example.get("query").is_some() {
            "CompanyLearningQueryRequest"
        } else {
            "CompanyLearningReadRequest"
        };
        let schema =
            json!({"$ref":format!("#/components/schemas/{name}"),"components":api["components"]});
        jsonschema::draft202012::options()
            .build(&schema)
            .unwrap()
            .validate(&example)
            .unwrap();
        count += 1;
    }
    assert_eq!(count, 2);
}
fn catalog(kind: &str) -> Value {
    json!({"contract_version":1,"query":{"kind":kind}})
}
fn scope() -> Value {
    json!({"project_id":"learning-only","agent_id":"agent","mission_id":null,"visibility":"private"})
}
async fn put(http: &Client, path: &str, token: &str, value: Value) -> Value {
    http.call("POST", path, path, token, value, 200).await
}

#[tokio::test]
async fn company_learning_real_http_catalogs_validate_every_kind_and_survive_writer_revocation() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("company", "owner").unwrap();
    let foreign = identity.bootstrap_tenant("other", "owner").unwrap();
    let catalog_admin = identity
        .issue(
            &admin.secret,
            CredentialSpec {
                subject_id: "catalog-administrator".into(),
                capabilities: [Capability::CredentialAdmin].into(),
                grants: vec![],
                scope_policy: None,
                expires_at_millis: None,
            },
        )
        .unwrap();
    let tool_admin = identity
        .issue(
            &admin.secret,
            CredentialSpec {
                subject_id: "tool-administrator".into(),
                capabilities: [Capability::ToolAdmin].into(),
                grants: vec![],
                scope_policy: None,
                expires_at_millis: None,
            },
        )
        .unwrap();
    let mut writers = vec![];
    for subject in ["alice", "bob"] {
        writers.push(
            identity
                .issue(
                    &admin.secret,
                    CredentialSpec {
                        subject_id: subject.into(),
                        capabilities: [
                            Capability::ExperienceWrite,
                            Capability::ExperienceReport,
                            Capability::ExperienceRead,
                            Capability::ProcedurePropose,
                            Capability::ToolDevelop,
                            Capability::ToolRead,
                        ]
                        .into(),
                        grants: vec![serde_json::from_value(scope()).unwrap()],
                        scope_policy: None,
                        expires_at_millis: None,
                    },
                )
                .unwrap(),
        );
    }
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
    put(&http, "/api/v1/learning/policies", &admin.secret, json!({"contract_version":1,"id":"policy","definition":PolicyDefinition {
        algorithm:PolicyAlgorithm::FixedBudgetHoeffdingV1,parameters:LearningPolicy { qualification_trials:32,evaluator_id:"judge".into(),evaluation_contract:"rubric".into(),..Default::default() }
    }})).await;
    let context = put(&http, "/api/v1/learning/contexts", &admin.secret, json!({"contract_version":1,"id":"context","context":{
        "task":"Recover an interrupted operation","baseline_revision":"baseline","model_provider":"external","model_revision":"v1","tools":{},
        "environment_revision":"v1","evaluation_contract":"rubric","dataset_revision":"v1","harness_revision":"v1","permissions_revision":"v1"
    }})).await;
    put(&http, "/api/v1/tools/executors", &tool_admin.secret, json!({"contract_version":1,"profile":{
        "id":"executor","subject_id":"alice","runtime_image_digest":format!("sha256:{}","a".repeat(64)),"environment_revision":"v1","permissions_revision":"v1","max_cost_units":10,"max_latency_ms":1000
    }})).await;
    for writer in &writers {
        let subject = &writer.credential.spec.subject_id;
        put(&http, "/api/v1/experiences", &writer.secret, json!({"contract_version":1,"scope":scope(),"request":{
            "id":"same-attempt","context_id":"context","reporter_subject_id":subject,"accounting_unit":"tokens","input":{"reference":"fixture:task","sha256":"b".repeat(64)},"parent":null
        }})).await;
        put(&http, "/api/v1/tools/artifacts", &writer.secret, json!({"contract_version":1,"scope":scope(),"artifact":{
            "id":"same-tool","source":"def run(): return True","dependency_lock":"","runtime_image_digest":format!("sha256:{}","a".repeat(64)),
            "entrypoint":"recovery:run","input_schema":true,"output_schema":true,"source_refs":["fixture:task"],"parent_artifact_id":null,"repair_evidence_ref":null
        }})).await;
    }
    let event = put(&http, "/api/v1/experiences/events", &writers[0].secret, json!({"contract_version":1,"scope":scope(),"attempt_id":"same-attempt","command":{
        "event_id":"observed","expected_revision":1,"context_digest":context["entry"]["payload_digest"],"outcome":"unknown",
        "evidence":{"reference":"fixture:result","sha256":"c".repeat(64)},"cost_units":null,"latency_ms":null
    }})).await;
    put(&http, "/api/v1/learning/strategies", &writers[0].secret, json!({"contract_version":1,"scope":scope(),"strategy":{
        "id":"strategy","policy_id":"policy","context_id":"context","instructions":"Inspect the original receipt before retrying",
        "preconditions":["A receipt exists"],"counterexamples":["An unknown response is not a failure"],
        "extractor":{"provider":"external","model":"extractor","model_revision":"v1","prompt_revision":"v1","evidence_ref":"fixture:extraction"},
        "selection":{"context_digest":context["entry"]["payload_digest"],"accounting_unit":"tokens","events":[{"attempt_id":"same-attempt","event_id":"observed","event_digest":event["event"]["event_digest"]}]}
    }})).await;
    put(&http, "/api/v1/tools/development/requests", &writers[0].secret, json!({"contract_version":1,"scope":scope(),"request":{
        "id":"development","executor_id":"executor","objective":"Build a recovery tool","parent_artifact_id":null,"repair_evidence_ref":null
    }})).await;
    http.call(
        "POST",
        QUERY,
        QUERY,
        &writers[0].secret,
        catalog("experience"),
        403,
    )
    .await;
    for writer in &writers {
        identity
            .revoke(
                &admin.secret,
                writer.credential.id,
                writer.credential.revision,
            )
            .unwrap();
    }
    let inventory = http
        .call(
            "GET",
            "/api/v1/company/memory/workspaces?contract_version=1",
            "/api/v1/company/memory/workspaces",
            &catalog_admin.secret,
            Value::Null,
            200,
        )
        .await;
    assert!(
        inventory["page"]["workspaces"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let credentials = http
        .call(
            "GET",
            "/api/v1/credentials?contract_version=1&limit=100",
            "/api/v1/credentials",
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    for kind in [
        "experience",
        "procedure",
        "strategy",
        "tool_artifact",
        "tool_development",
        "policy",
        "context",
        "executor",
    ] {
        let page = put(&http, QUERY, &catalog_admin.secret, catalog(kind)).await;
        assert_eq!(page["company_id"], "company");
        let entries = page["page"]["entries"].as_array().unwrap();
        assert_eq!(
            entries.len(),
            if ["experience", "tool_artifact"].contains(&kind) {
                2
            } else {
                1
            },
            "{kind}"
        );
        assert_eq!(page["page"]["stop_reason"], "exhausted");
        for entry in entries {
            let body = json!({"contract_version":1,"resource":entry["resource"]});
            let result = put(&http, READ, &catalog_admin.secret, body.clone()).await;
            assert_eq!(result["details"]["kind"], kind);
            assert_eq!(result["resource"], entry["resource"]);
            http.call("POST", READ, READ, &foreign.secret, body, 404)
                .await;
        }
    }
    let credentials_after = http
        .call(
            "GET",
            "/api/v1/credentials?contract_version=1&limit=100",
            "/api/v1/credentials",
            &admin.secret,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(credentials_after, credentials);
    let mut request = catalog("experience");
    request["query"]["limit"] = 1.into();
    let first = put(&http, QUERY, &catalog_admin.secret, request.clone()).await;
    request["query"]["cursor"] = first["page"]["next_cursor"].clone();
    let second = put(&http, QUERY, &catalog_admin.secret, request.clone()).await;
    assert_ne!(
        first["page"]["entries"][0]["resource"]["private_subject_id"],
        second["page"]["entries"][0]["resource"]["private_subject_id"]
    );
    assert!(second["page"]["next_cursor"].is_null());
    http.call("POST", QUERY, QUERY, &foreign.secret, request.clone(), 400)
        .await;
    request["query"]["filter"] = json!({"private_subject_id":"bob"});
    http.call("POST", QUERY, QUERY, &catalog_admin.secret, request, 400)
        .await;
    for invalid in [
        json!({"contract_version":1,"company_id":"other","query":{"kind":"experience"}}),
        json!({"contract_version":1,"query":{"kind":"experience","limit":51}}),
        json!({"contract_version":1,"query":{"kind":"policy","filter":{"project_id":"learning-only"}}}),
    ] {
        http.call("POST", QUERY, QUERY, &catalog_admin.secret, invalid, 400)
            .await;
    }
    http.call("POST", QUERY, QUERY, "", catalog("experience"), 401)
        .await;
    http.call("POST", QUERY, QUERY, &catalog_admin.secret,
        json!({"contract_version":1,"query":{"kind":"experience","filter":{"text":"x".repeat(70_000)}}}), 413).await;
    http.call(
        "POST",
        QUERY,
        QUERY,
        &writers[0].secret,
        catalog("experience"),
        401,
    )
    .await;
    identity
        .revoke(
            &admin.secret,
            catalog_admin.credential.id,
            catalog_admin.credential.revision,
        )
        .unwrap();
    http.call(
        "POST",
        QUERY,
        QUERY,
        &catalog_admin.secret,
        catalog("experience"),
        401,
    )
    .await;
    http.call(
        "POST",
        READ,
        READ,
        &catalog_admin.secret,
        json!({"contract_version":1,"resource":first["page"]["entries"][0]["resource"]}),
        401,
    )
    .await;
    server.abort();
    let _ = server.await;
}
