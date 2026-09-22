//! Real HTTP metadata discovery with current authority and published response schemas.
use super::administration::Client;
use super::*;
use qilbee_memory::learning::{LearningPolicy, PolicyAlgorithm, PolicyDefinition};
const QUERY: &str = "/api/v1/learning/metadata/query";

#[tokio::test]
async fn metadata_discovery_http_is_bounded_company_scoped_and_revocation_aware() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("one", "owner").unwrap();
    let other = identity.bootstrap_tenant("two", "owner").unwrap();
    let spec = |capability| CredentialSpec {
        subject_id: "reader".into(),
        capabilities: [capability].into(),
        grants: vec![],
        scope_policy: None,
        expires_at_millis: None,
    };
    let reader = identity
        .issue(&admin.secret, spec(Capability::LearningMetadataRead))
        .unwrap();
    let foreign = identity
        .issue(&other.secret, spec(Capability::LearningMetadataRead))
        .unwrap();
    let directory = identity
        .issue(&admin.secret, spec(Capability::CredentialAdmin))
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let api = client
        .get(format!("{base}/openapi.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let http = Client { client, base, api };
    for owner in [&admin, &other] {
        for id in ["a", "b"] {
            http.call("POST", "/api/v1/learning/policies", "/api/v1/learning/policies", &owner.secret,
                json!({"contract_version":1,"id":id,"definition":PolicyDefinition {
                    algorithm:PolicyAlgorithm::FixedBudgetHoeffdingV1,
                    parameters:LearningPolicy {qualification_trials:32,evaluator_id:"judge".into(),evaluation_contract:"rubric".into(),..Default::default()}
                }}), 200).await;
        }
    }
    let context = http.call("POST", "/api/v1/learning/contexts", "/api/v1/learning/contexts", &admin.secret,
        json!({"contract_version":1,"id":"context","context":{
            "task":"Inspect  incident evidence","baseline_revision":"baseline","model_provider":"external","model_revision":"v1","tools":{},
            "environment_revision":"v1","evaluation_contract":"rubric","dataset_revision":"v1","harness_revision":"v1","permissions_revision":"v1"
        }}),200).await;
    let contexts = http
        .call(
            "POST",
            QUERY,
            QUERY,
            &reader.secret,
            json!({"contract_version":1,"query":{"kind":"context"}}),
            200,
        )
        .await;
    assert_eq!(
        contexts["page"]["entries"][0]["title"],
        "Inspect incident evidence"
    );
    assert_eq!(
        contexts["page"]["entries"][0]["payload_digest"],
        context["entry"]["payload_digest"]
    );
    assert_eq!(contexts["page"]["entries"][0]["schema_version"], 1);
    let policy_admin = identity
        .issue(&admin.secret, spec(Capability::PolicyAdmin))
        .unwrap();
    http.call(
        "POST",
        QUERY,
        QUERY,
        &policy_admin.secret,
        json!({"contract_version":1,"query":{"kind":"context"}}),
        200,
    )
    .await;
    let body = json!({"contract_version":1,"query":{"kind":"policy","limit":1}});
    let first = http
        .call("POST", QUERY, QUERY, &reader.secret, body.clone(), 200)
        .await;
    assert_eq!(first["company_id"], "one");
    assert_eq!(first["page"]["entries"][0]["id"], "a");
    assert_eq!(first["page"]["stop_reason"], "entry_limit");
    assert!(first["page"]["entries"][0].get("payload").is_none());
    let exact = http
        .call(
            "GET",
            "/api/v1/learning/policies/a",
            "/api/v1/learning/policies/{id}",
            &reader.secret,
            Value::Null,
            200,
        )
        .await;
    assert_eq!(
        first["page"]["entries"][0]["payload_digest"],
        exact["entry"]["payload_digest"]
    );
    let mut next = body.clone();
    next["query"]["cursor"] = first["page"]["next_cursor"].clone();
    http.call("POST", QUERY, QUERY, &foreign.secret, next.clone(), 400)
        .await;
    http.call("POST", QUERY, QUERY, &directory.secret, body.clone(), 403)
        .await;
    let second = http
        .call("POST", QUERY, QUERY, &reader.secret, next.clone(), 200)
        .await;
    assert_eq!(second["page"]["entries"][0]["id"], "b");
    assert!(second["page"]["next_cursor"].is_null());
    for invalid in [
        json!({"kind":"procedure"}),
        json!({"kind":"policy","scope":{}}),
        json!({"kind":"policy","limit":51}),
        json!({"kind":"policy","max_scanned_records":0}),
    ] {
        http.call(
            "POST",
            QUERY,
            QUERY,
            &reader.secret,
            json!({"contract_version":1,"query":invalid}),
            400,
        )
        .await;
    }
    http.call(
        "POST",
        "/api/v1/company/learning/query",
        "/api/v1/company/learning/query",
        &reader.secret,
        body,
        403,
    )
    .await;
    identity
        .revoke(
            &admin.secret,
            reader.credential.id,
            reader.credential.revision,
        )
        .unwrap();
    http.call("POST", QUERY, QUERY, &reader.secret, next, 401)
        .await;
    let mut expiring_spec = spec(Capability::LearningMetadataRead);
    expiring_spec.expires_at_millis = Some(chrono::Utc::now().timestamp_millis() + 250);
    let expiring = identity.issue(&admin.secret, expiring_spec).unwrap();
    let first = http
        .call(
            "POST",
            QUERY,
            QUERY,
            &expiring.secret,
            json!({"contract_version":1,"query":{"kind":"policy","limit":1}}),
            200,
        )
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    http.call("POST", QUERY, QUERY, &expiring.secret,
        json!({"contract_version":1,"query":{"kind":"policy","cursor":first["page"]["next_cursor"]}}),401).await;
    server.abort();
}
