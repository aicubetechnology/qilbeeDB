use super::*;
use qilbee_memory::learning::{LearningPolicy, PolicyAlgorithm, PolicyDefinition};

struct ContractHttp {
    client: reqwest::Client,
    base: String,
    api: Value,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for ContractHttp {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl ContractHttp {
    async fn new(router: Router) -> Self {
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
        assert_eq!(
            api,
            serde_json::from_str::<Value>(include_str!("../../../../docs/api/openapi.json"))
                .unwrap()
        );
        Self {
            client,
            base,
            api,
            server,
        }
    }
}
async fn request(
    http: &ContractHttp,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = http
        .client
        .request(method.parse().unwrap(), format!("{}{path}", http.base))
        .bearer_auth(token)
        .json(&body)
        .send()
        .await
        .unwrap();
    let status = response.status();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let value: Value = response.json().await.unwrap();
    if status.is_success() {
        let mut request_schema = http.api["paths"][path][method.to_ascii_lowercase()]["requestBody"]["content"]["application/json"]["schema"].clone();
        request_schema["components"] = http.api["components"].clone();
        jsonschema::draft202012::options()
            .should_validate_formats(true)
            .build(&request_schema)
            .unwrap()
            .validate(&body)
            .unwrap();
    }

    let mut schema = http.api["paths"][path][method.to_ascii_lowercase()]["responses"]
        [status.as_str()]["content"]["application/json"]["schema"]
        .clone();
    assert!(!schema.is_null(), "Undocumented {path}: {status}");
    schema["components"] = http.api["components"].clone();
    jsonschema::draft202012::options()
        .should_validate_formats(true)
        .build(&schema)
        .unwrap()
        .validate(&value)
        .unwrap_or_else(|error| panic!("{path} {status}: {error}"));
    (status, value)
}

fn scope() -> Value {
    json!({"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"shared"})
}
fn policy() -> Value {
    json!({"contract_version":1,"id":"policy-v1","definition":PolicyDefinition { algorithm:PolicyAlgorithm::FixedBudgetHoeffdingV1,parameters:LearningPolicy { qualification_trials:32,evaluator_id:"evaluator".into(),evaluation_contract:"rubric-v1".into(),..Default::default() }}})
}
fn context() -> Value {
    json!({"contract_version":1,"id":"context-v1","context":{"task":"task-v1","baseline_revision":"baseline-v1","model_provider":"provider","model_revision":"model-v1","tools":{},"environment_revision":"env-v1","evaluation_contract":"rubric-v1","dataset_revision":"dataset-v1","harness_revision":"harness-v1","permissions_revision":"permissions-v1"}})
}

#[tokio::test]
async fn knowledge_real_http_matches_served_openapi_and_enforces_identity() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let writer = identity.issue(&admin.secret, spec()).unwrap();
    let http = ContractHttp::new(router).await;
    for (path, body) in [
        ("/api/v1/learning/policies", policy()),
        ("/api/v1/learning/contexts", context()),
    ] {
        assert_eq!(
            request(&http, "POST", path, &admin.secret, body).await.0,
            StatusCode::OK
        );
    }
    let proposal = json!({"contract_version":2,"scope":scope(),"proposal":{"id":"knowledge-r1","policy_id":"policy-v1","context_id":"context-v1","title":"Lookup recovery","instructions":"Verify source applicability before use.","memory_sources":[{"record_id":"00000000-0000-4000-8000-000000000002","revision":1},{"record_id":"00000000-0000-4000-8000-000000000001","revision":2}],"external_tools":[]}});
    let path = "/api/v1/learning/knowledge/proposals";
    let (status, original) = request(&http, "POST", path, &writer.secret, proposal.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let mut reordered = proposal.clone();
    reordered["proposal"]["memory_sources"]
        .as_array_mut()
        .unwrap()
        .reverse();
    assert_eq!(
        request(&http, "POST", path, &writer.secret, reordered)
            .await
            .1,
        original
    );
    let mut changed = proposal.clone();
    changed["proposal"]["title"] = "Different meaning".into();
    assert_eq!(
        request(&http, "POST", path, &writer.secret, changed)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let mut injected = proposal.clone();
    injected["proposal"]["code"] = "forbidden field".into();
    assert_eq!(
        request(&http, "POST", path, &writer.secret, injected)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let inspection = json!({"contract_version":2,"scope":scope(),"procedure_id":"knowledge-r1"});
    let inspect_path = "/api/v1/learning/knowledge/inspect";
    let (status, current) = request(
        &http,
        "POST",
        inspect_path,
        &writer.secret,
        inspection.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(current["inspection"]["eligible_for_knowledge_reuse"], false);
    assert_eq!(current["inspection"]["receipt"], original["receipt"]);
    let mut other = inspection.clone();
    other["scope"]["agent_id"] = "other".into();
    assert_eq!(
        request(&http, "POST", inspect_path, &writer.secret, other)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let mut missing = inspection.clone();
    missing["procedure_id"] = "missing".into();
    assert_eq!(
        request(&http, "POST", inspect_path, &writer.secret, missing)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let select = json!({"contract_version":2,"scope":scope(),"policy_id":"policy-v1","context_id":"context-v1","max_instruction_bytes":65536,"candidate_limit":1000,"external_tool_identities":[]});
    let select_path = "/api/v1/learning/knowledge/select";
    let (status, result) =
        request(&http, "POST", select_path, &writer.secret, select.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        result["result"]["selection"]["reason"],
        "no_eligible_bound_procedure"
    );
    assert_eq!(result["result"]["coverage"]["complete"], true);
    let mut wrong_version = select.clone();
    wrong_version["contract_version"] = 1.into();
    assert_eq!(
        request(&http, "POST", select_path, &writer.secret, wrong_version)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut missing_identity = select;
    missing_identity
        .as_object_mut()
        .unwrap()
        .remove("external_tool_identities");
    assert_eq!(
        request(&http, "POST", select_path, &writer.secret, missing_identity)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    identity
        .revoke(
            &admin.secret,
            writer.credential.id,
            writer.credential.revision,
        )
        .unwrap();
    assert_eq!(
        request(&http, "POST", inspect_path, &writer.secret, inspection)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn knowledge_real_http_qualification_invalidates_reuse_without_rewriting_history() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let mut definition = spec();
    definition.capabilities.insert(Capability::MemoryWrite);
    let writer = identity.issue(&admin.secret, definition).unwrap();
    let mut definition = spec();
    definition.subject_id = "evaluator".into();
    definition.capabilities = [Capability::ProcedureEvaluate, Capability::MemoryRead].into();
    let evaluator = identity.issue(&admin.secret, definition).unwrap();
    let http = ContractHttp::new(router).await;
    for (path, body) in [
        ("/api/v1/learning/policies", policy()),
        ("/api/v1/learning/contexts", context()),
    ] {
        assert_eq!(
            request(&http, "POST", path, &admin.secret, body).await.0,
            StatusCode::OK
        );
    }
    let (status, source) = request(
        &http,
        "POST",
        "/api/v1/memory/commands",
        &writer.secret,
        memory_create(
            "knowledge-source",
            "A verified applicability observation",
            "shared",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let source_id = source["receipt"]["record_id"].clone();
    assert!(source_id.is_string());
    let proposal = json!({"contract_version":2,"scope":scope(),"proposal":{"id":"qualified-knowledge","policy_id":"policy-v1","context_id":"context-v1","title":"Source-aware procedure","instructions":"Consult the applicable evidence before answering.","memory_sources":[{"record_id":source_id,"revision":1}],"external_tools":[]}});
    let path = "/api/v1/learning/knowledge/proposals";
    let (status, original) = request(&http, "POST", path, &writer.secret, proposal.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let inspect =
        json!({"contract_version":2,"scope":scope(),"procedure_id":"qualified-knowledge"});
    let inspect_path = "/api/v1/learning/knowledge/inspect";
    let candidate = request(&http, "POST", inspect_path, &writer.secret, inspect.clone())
        .await
        .1;
    assert_eq!(candidate["inspection"]["evidence"]["eligible"], true);
    assert_eq!(candidate["inspection"]["qualification_active"], false);
    assert_eq!(
        candidate["inspection"]["eligible_for_knowledge_reuse"],
        false
    );
    // Synthetic paired outcomes exercise qualification mechanics, not agent efficacy.
    for i in 0..32 {
        let evaluation = json!({"contract_version":1,"scope":scope(),"procedure_id":"qualified-knowledge","submission":{"case_id":format!("case-{i}"),"phase":"Qualification","policy_id":"policy-v1","context_id":"context-v1","baseline_revision":"baseline-v1","evidence_ref":format!("synthetic:case-{i}"),"status":"complete","baseline_utility":0.0,"candidate_utility":1.0,"candidate_cost_units":1,"candidate_latency_ms":1,"detail":null}});
        assert_eq!(
            request(
                &http,
                "POST",
                "/api/v1/learning/evaluations",
                &evaluator.secret,
                evaluation
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let qualified = request(&http, "POST", inspect_path, &writer.secret, inspect.clone())
        .await
        .1;
    assert_eq!(qualified["inspection"]["qualification_active"], true);
    assert_eq!(
        qualified["inspection"]["eligible_for_knowledge_reuse"],
        true
    );
    assert_eq!(qualified["inspection"]["receipt"], original["receipt"]);
    let mut schema = http.api["components"]["schemas"]["KnowledgeInspectResponse"].clone();
    schema["components"] = http.api["components"].clone();
    let validator = jsonschema::draft202012::options().build(&schema).unwrap();
    let mut forged = qualified.clone();
    forged["inspection"]["qualification_active"] = false.into();
    assert!(!validator.is_valid(&forged));
    let mut forged = qualified.clone();
    forged["inspection"]["eligible_for_knowledge_reuse"] = false.into();
    assert!(!validator.is_valid(&forged));
    let selection = json!({"contract_version":2,"scope":scope(),"policy_id":"policy-v1","context_id":"context-v1","max_instruction_bytes":65536,"candidate_limit":1000,"external_tool_identities":[]});
    let select_path = "/api/v1/learning/knowledge/select";
    let selected = request(
        &http,
        "POST",
        select_path,
        &writer.secret,
        selection.clone(),
    )
    .await
    .1;
    assert_eq!(selected["result"]["selection"]["type"], "procedure");
    let legacy = json!({"contract_version":1,"scope":scope(),"policy_id":"policy-v1","context_id":"context-v1","max_instruction_bytes":65536});
    assert_eq!(
        request(
            &http,
            "POST",
            "/api/v1/learning/select",
            &writer.secret,
            legacy
        )
        .await
        .1["selection"]["type"],
        "baseline"
    );
    let deletion = json!({"contract_version":1,"scope":scope(),"idempotency_key":"delete-knowledge-source","operation":{"type":"delete","record_id":source_id,"expected_revision":1}});
    assert_eq!(
        request(
            &http,
            "POST",
            "/api/v1/memory/commands",
            &writer.secret,
            deletion
        )
        .await
        .0,
        StatusCode::OK
    );
    let invalid = request(&http, "POST", inspect_path, &writer.secret, inspect)
        .await
        .1;
    assert_eq!(invalid["inspection"]["qualification_active"], true);
    assert_eq!(invalid["inspection"]["evidence"]["eligible"], false);
    assert_eq!(invalid["inspection"]["eligible_for_knowledge_reuse"], false);
    assert_eq!(
        request(&http, "POST", path, &writer.secret, proposal)
            .await
            .1,
        original
    );
    assert_eq!(
        request(&http, "POST", select_path, &writer.secret, selection)
            .await
            .1["result"]["selection"]["reason"],
        "no_eligible_bound_procedure"
    );
}

#[tokio::test]
async fn knowledge_real_http_company_discovery_survives_writer_revocation() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "admin").unwrap();
    let foreign = identity.bootstrap_tenant("foreign", "admin").unwrap();
    let http = ContractHttp::new(router).await;
    for (path, body) in [
        ("/api/v1/learning/policies", policy()),
        ("/api/v1/learning/contexts", context()),
    ] {
        assert_eq!(
            request(&http, "POST", path, &admin.secret, body).await.0,
            StatusCode::OK
        );
    }
    let private_scope =
        json!({"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"private"});
    for subject in ["alice", "bob"] {
        let mut credential = spec();
        credential.subject_id = subject.into();
        credential.grants = vec![serde_json::from_value(private_scope.clone()).unwrap()];
        let writer = identity.issue(&admin.secret, credential).unwrap();
        let proposal = json!({"contract_version":2,"scope":private_scope,"proposal":{"id":"knowledge-r1","policy_id":"policy-v1","context_id":"context-v1","title":"Recovery instructions","instructions":"Check current evidence.","memory_sources":[{"record_id":"00000000-0000-4000-8000-000000000001","revision":1}],"external_tools":[]}});
        assert_eq!(
            request(
                &http,
                "POST",
                "/api/v1/learning/knowledge/proposals",
                &writer.secret,
                proposal
            )
            .await
            .0,
            StatusCode::OK
        );
        identity
            .revoke(
                &admin.secret,
                writer.credential.id,
                writer.credential.revision,
            )
            .unwrap();
    }
    let query_path = "/api/v1/company/learning/query";
    let query = json!({"contract_version":1,"query":{"kind":"knowledge"}});
    let (status, page) = request(&http, "POST", query_path, &admin.secret, query.clone()).await;
    assert_eq!(status, StatusCode::OK);
    let items = page["page"]["entries"].as_array().expect("catalog entries");
    assert_eq!(items.len(), 2);
    let mut subjects = vec![];
    for item in items {
        let resource = item["resource"].clone();
        subjects.push(resource["private_subject_id"].as_str().unwrap().to_owned());
        assert_eq!(item["title"], "Recovery instructions");
        let inspection = json!({"contract_version":2,"resource":resource});
        let path = "/api/v1/company/learning/knowledge/inspect";
        let (status, value) = request(&http, "POST", path, &admin.secret, inspection.clone()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(value["inspection"]["eligible_for_knowledge_reuse"], false);
        assert_eq!(
            request(&http, "POST", path, &foreign.secret, inspection)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(
                &http,
                "POST",
                "/api/v1/company/learning/read",
                &admin.secret,
                json!({"contract_version":1,"resource":resource})
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    // Pagination is a live traversal scoped to company, kind and filters.
    let first_query = json!({"contract_version":1,"query":{"kind":"knowledge","limit":1}});
    let (_, first) = request(
        &http,
        "POST",
        query_path,
        &admin.secret,
        first_query.clone(),
    )
    .await;
    assert_eq!(first["page"]["entries"].as_array().unwrap().len(), 1);
    assert!(!first["page"]["next_cursor"].is_null());
    let mut next_query = first_query.clone();
    next_query["query"]["cursor"] = first["page"]["next_cursor"].clone();
    let (_, next) = request(&http, "POST", query_path, &admin.secret, next_query.clone()).await;
    assert_eq!(next["page"]["entries"].as_array().unwrap().len(), 1);
    assert_ne!(
        first["page"]["entries"][0]["resource"],
        next["page"]["entries"][0]["resource"]
    );
    assert!(next["page"]["next_cursor"].is_null());
    assert_eq!(
        request(
            &http,
            "POST",
            query_path,
            &foreign.secret,
            next_query.clone()
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    for replacement in [
        json!({"text":"Recovery"}),
        json!({"private_subject_id":"alice"}),
    ] {
        let mut changed = next_query.clone();
        changed["query"]["filter"] = replacement;
        assert_eq!(
            request(&http, "POST", query_path, &admin.secret, changed)
                .await
                .0,
            StatusCode::BAD_REQUEST
        );
    }
    next_query["query"]["kind"] = "procedure".into();
    assert_eq!(
        request(&http, "POST", query_path, &admin.secret, next_query)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    // Missing or incompatible ownership never broadens an administrative selection.
    let inspect_path = "/api/v1/company/learning/knowledge/inspect";
    let selected = items[0]["resource"].clone();
    for (field, value, status) in [
        ("private_subject_id", Value::Null, StatusCode::BAD_REQUEST),
        (
            "private_subject_id",
            json!("unknown-owner"),
            StatusCode::NOT_FOUND,
        ),
        ("kind", json!("procedure"), StatusCode::BAD_REQUEST),
    ] {
        let mut resource = selected.clone();
        resource[field] = value;
        assert_eq!(
            request(
                &http,
                "POST",
                inspect_path,
                &admin.secret,
                json!({"contract_version":2,"resource":resource})
            )
            .await
            .0,
            status
        );
    }
    let reader = identity.issue(&admin.secret, spec()).unwrap();
    assert_eq!(
        request(&http, "POST", query_path, &reader.secret, query.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &http,
            "POST",
            inspect_path,
            &reader.secret,
            json!({"contract_version":2,"resource":selected})
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let administrator = identity
        .issue(
            &admin.secret,
            CredentialSpec {
                subject_id: "inventory-admin".into(),
                capabilities: [Capability::CredentialAdmin].into(),
                grants: vec![],
                scope_policy: None,
                expires_at_millis: None,
            },
        )
        .unwrap();
    assert_eq!(
        request(
            &http,
            "POST",
            query_path,
            &administrator.secret,
            query.clone()
        )
        .await
        .0,
        StatusCode::OK
    );
    identity
        .revoke(
            &admin.secret,
            administrator.credential.id,
            administrator.credential.revision,
        )
        .unwrap();
    assert_eq!(
        request(
            &http,
            "POST",
            query_path,
            &administrator.secret,
            query.clone()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &http,
            "POST",
            inspect_path,
            &administrator.secret,
            json!({"contract_version":2,"resource":selected})
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    subjects.sort();
    assert_eq!(subjects, vec!["alice", "bob"]);
    let (_, other) = request(&http, "POST", query_path, &foreign.secret, query).await;
    assert!(other["page"]["entries"].as_array().unwrap().is_empty());
}
