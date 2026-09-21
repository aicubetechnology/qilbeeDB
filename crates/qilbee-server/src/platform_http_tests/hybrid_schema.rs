//! Validate actual loopback HTTP responses against the schema served by that process.
use super::*;
async fn post(
    client: &reqwest::Client,
    base: &str,
    route: &str,
    token: &str,
    body: &Value,
) -> Value {
    let response = client
        .post(format!("{base}{route}"))
        .bearer_auth(token)
        .json(body)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), 200);
    response.json().await.unwrap()
}
#[tokio::test]
async fn hybrid_profiles_validate_real_http_scores_against_the_served_openapi() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let token = memory_key(&identity, &admin.secret, "schema-evaluator", true);
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
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let response_schema = api["paths"]["/api/v1/memory/search/hybrid"]["post"]["responses"]["200"]
        ["content"]["application/json"]["schema"]
        .clone();
    let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema","allOf":[response_schema],"components":api["components"]});
    let validator = jsonschema::draft202012::options().build(&schema).unwrap();
    let scope = memory_scope("shared");
    let space =
        json!({"provider":"fixture","model":"schema-conformance","revision":"v1","dimensions":3});
    for n in 0..10 {
        let created = post(
            &client,
            &base,
            "/api/v1/memory/commands",
            &token,
            &memory_create(
                &format!("record-{n}"),
                "ZX17 identical synthetic record",
                "shared",
            ),
        )
        .await;
        post(&client,&base,"/api/v1/memory/embeddings",&token,&json!({"contract_version":1,"scope":scope,"idempotency_key":format!("vector-{n}"),"record_id":created["receipt"]["record_id"],"record_revision":1,"space":space,"vector":[1,0,0]})).await;
    }
    let catalog: Value = client
        .get(format!("{base}/api/v1/memory/ranking-profiles"))
        .bearer_auth(&token)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(catalog["hybrid_profiles"].as_array().unwrap().len(), 2);
    for (version, constant, lexical_weight, semantic_weight) in [
        ("weighted_rrf_v1", 60, 0.5, 0.5),
        ("weighted_rrf_v2", 2, 0.25, 0.75),
    ] {
        let profile = catalog["hybrid_profiles"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["version"] == version)
            .unwrap();
        assert_eq!(profile["rank_constant"], constant);
        assert_eq!(profile["lexical_weight"], lexical_weight);
        assert_eq!(profile["semantic_weight"], semantic_weight);
        let maximum = (lexical_weight + semantic_weight) / f64::from(constant + 1);
        for (channel, text, vector, threshold, weight) in [
            ("both", "ZX17", json!([1, 0, 0]), -1.0, 1.0),
            (
                "semantic",
                "unmatched-token",
                json!([1, 0, 0]),
                -1.0,
                semantic_weight,
            ),
            ("lexical", "ZX17", json!([-1, 0, 0]), 0.0, lexical_weight),
            ("empty", "unmatched-token", json!([-1, 0, 0]), 0.0, 0.0),
        ] {
            let body = json!({"contract_version":1,"scope":scope,"mode":"hybrid","query":{"text":text,"space":space,"vector":vector,"limit":10,"min_score":threshold,"ranking_version":version}});
            let response = post(
                &client,
                &base,
                "/api/v1/memory/search/hybrid",
                &token,
                &body,
            )
            .await;
            let hits = response["page"]["hits"].as_array().unwrap();
            assert_eq!(hits.len(), if channel == "empty" { 0 } else { 10 });
            if channel != "empty" {
                let expected = weight / f64::from(constant + 1);
                assert!((hits[0]["score"].as_f64().unwrap() - expected).abs() < 1e-15);
            }
            let errors: Vec<_> = validator
                .iter_errors(&response)
                .map(|error| error.to_string())
                .collect();
            assert!(errors.is_empty(), "{version}/{channel}: {errors:?}");
            if channel == "both" {
                let mut invalid = response.clone();
                invalid["page"]["hits"][0]["score"] = json!(maximum + 0.000001);
                assert!(
                    !validator.is_valid(&invalid),
                    "Profile-specific combined maximum must remain enforced"
                );
                for (name, weight) in [("lexical", lexical_weight), ("semantic", semantic_weight)] {
                    let mut invalid = response.clone();
                    invalid["page"]["hits"][0][name]["contribution"] =
                        json!(weight / f64::from(constant + 1) + 0.000001);
                    assert!(
                        !validator.is_valid(&invalid),
                        "Profile and channel-specific contribution maximum must remain enforced"
                    );
                }
                let mut invalid = response.clone();
                invalid["ranking_version"] = json!(if version == "weighted_rrf_v1" {
                    "weighted_rrf_v2"
                } else {
                    "weighted_rrf_v1"
                });
                assert!(
                    !validator.is_valid(&invalid),
                    "Envelope and page ranking versions must agree"
                );
            }
        }
    }
    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn current_candidate_http_coverage_and_work_validate_against_served_openapi() {
    let dir = TempDir::new().unwrap();
    let (router, identity) = app(dir.path());
    let admin = identity.bootstrap_tenant("tenant", "operator").unwrap();
    let token = memory_key(&identity, &admin.secret, "candidate-evaluator", true);
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
    let scope = memory_scope("shared");
    let space =
        json!({"provider":"fixture","model":"candidate-schema","revision":"v1","dimensions":3});
    let mut target = Value::Null;
    for n in 0..8 {
        let mut command = memory_create(&format!("history-{n}"), "term identical", "shared");
        command["operation"]["record"]["tags"] =
            json!([if n < 4 { "target" } else { "background" }]);
        let created = post(&client, &base, "/api/v1/memory/commands", &token, &command).await;
        let id = created["receipt"]["record_id"].clone();
        post(&client, &base, "/api/v1/memory/embeddings", &token, &json!({"contract_version":1,"scope":scope,
            "idempotency_key":format!("vector-{n}"),"record_id":id,"record_revision":1,"space":space,"vector":[1,0,0]})).await;
        if n < 3 {
            post(&client, &base, "/api/v1/memory/commands", &token, &json!({"contract_version":1,"scope":scope,
                "idempotency_key":format!("delete-{n}"),"operation":{"type":"delete","record_id":id,"expected_revision":1}})).await;
        } else if n == 3 {
            target = id;
        }
    }
    for mode in ["lexical", "semantic", "hybrid"] {
        let route = match mode {
            "semantic" => "/api/v1/memory/search".to_owned(),
            _ => format!("/api/v1/memory/search/{mode}"),
        };
        let mut body = json!({"contract_version":1,"scope":scope,"query":{"limit":10,"scan_limit":1,"tag":"target"}});
        if mode != "semantic" {
            body["mode"] = mode.into();
            body["query"]["text"] = "term".into();
        }
        if mode != "lexical" {
            body["query"]["space"] = space.clone();
            body["query"]["vector"] = json!([1, 0, 0]);
        }
        if mode == "hybrid" {
            body["query"]["ranking_version"] = "weighted_rrf_v2".into();
        }
        let http = client
            .post(format!("{base}{route}"))
            .bearer_auth(&token)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(http.status().as_u16(), 200);
        let headers = http.headers().clone();
        let response: Value = http.json().await.unwrap();
        let schema = json!({"$schema":"https://json-schema.org/draft/2020-12/schema",
            "allOf":[api["paths"][&route]["post"]["responses"]["200"]["content"]["application/json"]["schema"]],"components":api["components"]});
        let validator = jsonschema::draft202012::options().build(&schema).unwrap();
        assert!(validator.is_valid(&response), "{mode}: {response}");
        let page = &response["page"];
        assert!(page.get("candidate_selection_version").is_none());
        assert!(page.get("candidate_index_bytes").is_none());
        if mode == "semantic" {
            assert!(page.get("scanned_records").is_none());
            assert!(page.get("scanned_bytes").is_none());
        }
        assert_eq!(
            headers["x-qilbee-candidate-selection-version"],
            "current_records_v1"
        );
        assert_eq!(page["exhaustive"], true);
        assert_eq!(headers["x-qilbee-scanned-records"], "1");
        assert!(
            headers["x-qilbee-candidate-index-bytes"]
                .to_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 40
        );
        assert!(
            headers["x-qilbee-scanned-bytes"]
                .to_str()
                .unwrap()
                .parse::<u64>()
                .unwrap()
                > 0
        );
        assert_eq!(page["hits"][0]["record"]["record_id"], target);
        if mode == "semantic" {
            assert_eq!(page["hits"][0]["score"], 1.0);
        }
    }
    server.abort();
    let _ = server.await;
}
