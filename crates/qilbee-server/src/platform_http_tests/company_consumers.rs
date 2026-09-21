//! Real HTTP discovery and inspection use current company authority and published schemas.
use super::administration::Client;
use super::*;
const QUERY: &str = "/api/v1/company/memory/consumers/query";
const READ: &str = "/api/v1/company/memory/consumers/read";
async fn post(http: &Client, path: &str, token: &str, body: Value) -> Value {
    http.call("POST", path, path, token, body, 200).await
}

#[tokio::test]
async fn company_consumers_real_http_preserves_owners_and_revoked_writer_progress() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity
        .bootstrap_tenant("company", "administrator")
        .unwrap();
    let foreign = identity
        .bootstrap_tenant("foreign", "administrator")
        .unwrap();
    let mut writer_keys = vec![];
    for subject in ["alice", "bob"] {
        let mut spec = spec();
        spec.subject_id = subject.into();
        spec.capabilities = [
            Capability::MemoryRead,
            Capability::MemoryWrite,
            Capability::MemoryCheckpoint,
        ]
        .into();
        spec.grants.push(ResourceScope {
            visibility: Visibility::Private,
            ..spec.grants[0].clone()
        });
        writer_keys.push(identity.issue(&admin.secret, spec).unwrap());
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
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
    let kinds = ["memory_v1", "memory_v2", "relations_v1"];
    let q = json!({"contract_version":1,"query":{"kind":"memory_v2"}});
    http.call("POST", QUERY, QUERY, "", q.clone(), 401).await;
    http.call("POST", QUERY, QUERY, &writer_keys[0].secret, q.clone(), 403)
        .await;
    for writer in &writer_keys {
        for visibility in ["private", "shared"] {
            let scope = memory_scope(visibility);
            post(
                &http,
                "/api/v1/memory/commands",
                &writer.secret,
                memory_create(
                    &format!("create-{}-{visibility}", writer.credential.spec.subject_id),
                    "Consumer fixture",
                    visibility,
                ),
            )
            .await;
            for kind in kinds {
                let (path, command) = match kind {
                    "memory_v1" => {
                        let page = post(
                            &http,
                            "/api/v1/memory/changes",
                            &writer.secret,
                            json!({"contract_version":1,"scope":scope,"query":{"limit":10}}),
                        )
                        .await;
                        (
                            "/api/v1/memory/checkpoints",
                            json!({"contract_version":1,"idempotency_key":"checkpoint-legacy","consumer_id":"cache","expected_revision":0,"cursor":page["page"]["high_watermark"]}),
                        )
                    }
                    "memory_v2" => {
                        let baseline = post(
                            &http,
                            "/api/v2/memory/changes/activate",
                            &writer.secret,
                            json!({"contract_version":2,"scope":scope}),
                        )
                        .await;
                        (
                            "/api/v2/memory/checkpoints",
                            json!({"contract_version":2,"idempotency_key":"checkpoint-verified","consumer_id":"cache","expected_revision":0,"expected_checkpoint_digest":null,"cursor":baseline["baseline"]}),
                        )
                    }
                    _ => {
                        let baseline = post(
                            &http,
                            "/api/v1/memory/relations/changes/activate",
                            &writer.secret,
                            json!({"contract_version":1,"scope":scope}),
                        )
                        .await;
                        (
                            "/api/v1/memory/relations/checkpoints",
                            json!({"contract_version":1,"idempotency_key":"checkpoint-relations","consumer_id":"cache","expected_revision":0,"expected_checkpoint_digest":null,"operation":{"type":"advance","cursor":baseline["baseline"]}}),
                        )
                    }
                };
                post(
                    &http,
                    path,
                    &writer.secret,
                    json!({"scope":scope,"command":command}),
                )
                .await;
            }
        }
        let current = identity.authenticate(&writer.secret).unwrap();
        identity
            .revoke(&admin.secret, current.id, current.revision)
            .unwrap();
    }
    for kind in kinds {
        let mut body = json!({"contract_version":1,"query":{"kind":kind,"limit":1}});
        let mut found = vec![];
        loop {
            let page = post(&http, QUERY, &admin.secret, body.clone()).await;
            found.extend(page["page"]["entries"].as_array().unwrap().iter().cloned());
            if page["page"]["next_cursor"].is_null() {
                break;
            }
            body["query"]["cursor"] = page["page"]["next_cursor"].clone();
            assert!(found.len() < 5);
        }
        assert_eq!(found.len(), 4);
        let owners: std::collections::BTreeSet<_> = found
            .iter()
            .map(|e| {
                (
                    e["consumer"]["subject_id"].as_str().unwrap(),
                    e["consumer"]["private_subject_id"].as_str(),
                )
            })
            .collect();
        assert_eq!(owners.len(), 4);
        for entry in found {
            let read = json!({"contract_version":1,"consumer":entry["consumer"]});
            let result = post(&http, READ, &admin.secret, read.clone()).await;
            assert_eq!(result["details"]["kind"], kind);
            assert_eq!(
                result["details"]["observation"]["checkpoint"]["revision"],
                1
            );
            http.call("POST", READ, READ, &foreign.secret, read.clone(), 404)
                .await;
            http.call(
                "POST",
                READ,
                READ,
                &writer_keys[0].secret,
                read.clone(),
                401,
            )
            .await;
            if kind != "memory_v1" {
                let mut witnessed = read.clone();
                witnessed["witness"] = json!({"kind":kind,"cursor":result["details"]["observation"]["checkpoint"]["cursor"]});
                let compatible = post(&http, READ, &admin.secret, witnessed.clone()).await;
                assert_eq!(
                    compatible["details"]["observation"]["witness_status"],
                    "compatible"
                );
                witnessed["witness"]["cursor"]["prefix_digest"] = json!("0".repeat(64));
                let incompatible = post(&http, READ, &admin.secret, witnessed.clone()).await;
                assert_eq!(
                    incompatible["details"]["observation"]["witness_status"],
                    "history_incompatible"
                );
                assert_eq!(
                    incompatible["details"]["observation"]["checkpoint_relative_to_witness"],
                    Value::Null
                );
                witnessed["consumer"]["kind"] = json!("memory_v1");
                http.call("POST", READ, READ, &admin.secret, witnessed, 400)
                    .await;
            }
            let mut absent = read.clone();
            absent["consumer"]["consumer_id"] = json!("absent");
            http.call("POST", READ, READ, &admin.secret, absent, 404)
                .await;
        }
        let empty = post(
            &http,
            QUERY,
            &foreign.secret,
            json!({"contract_version":1,"query":{"kind":kind}}),
        )
        .await;
        assert_eq!(empty["page"]["scanned_records"], 0);
    }
    for body in [
        json!({"contract_version":1,"query":{"kind":"memory_v2","limit":0}}),
        json!({"contract_version":1,"company_id":"foreign","query":{"kind":"memory_v2"}}),
        json!({"contract_version":2,"query":{"kind":"memory_v2"}}),
    ] {
        http.call("POST", QUERY, QUERY, &admin.secret, body, 400)
            .await;
    }
    http.call(
        "POST",
        QUERY,
        QUERY,
        &admin.secret,
        json!({"contract_version":1,"query":{"kind":"memory_v2","text":"x".repeat(70000)}}),
        413,
    )
    .await;
    let first = post(
        &http,
        QUERY,
        &admin.secret,
        json!({"contract_version":1,"query":{"kind":"memory_v2","limit":1}}),
    )
    .await;
    let mut continuation = json!({"contract_version":1,"query":{"kind":"memory_v2","cursor":first["page"]["next_cursor"]}});
    continuation["query"]["cursor"]["position"] = json!("not-hex");
    http.call("POST", QUERY, QUERY, &admin.secret, continuation, 400)
        .await;
    let current = identity.authenticate(&admin.secret).unwrap();
    identity
        .revoke(&admin.secret, current.id, current.revision)
        .unwrap();
    http.call("POST", QUERY, QUERY, &admin.secret, q, 401).await;
    server.abort();
}

#[test]
fn company_consumer_guide_examples_match_published_request_schema() {
    let api: Value =
        serde_json::from_str(include_str!("../../../../docs/api/openapi.json")).unwrap();
    let schema = json!({"$ref":"#/components/schemas/CompanyConsumerQueryRequest","components":api["components"]});
    let validator = jsonschema::draft202012::options().build(&schema).unwrap();
    let guide = include_str!("../../../../docs/api/company-consumer-directory.md");
    let examples: Vec<_> = guide.split("```json\n").skip(1).collect();
    assert_eq!(examples.len(), 1);
    for block in examples {
        let value: Value = serde_json::from_str(block.split("\n```").next().unwrap()).unwrap();
        validator.validate(&value).unwrap();
    }
}
