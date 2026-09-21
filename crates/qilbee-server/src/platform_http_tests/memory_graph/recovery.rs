use super::*;
use crate::http_server_tests::{crash_server_for, wire_request};

#[test]
fn evidence_graph_http_after_process_kill_keeps_links_and_rechecks_source_revisions() {
    let dir = TempDir::new().unwrap();
    let (admin, key) = {
        let (_, identity) = app(dir.path());
        let admin = identity.bootstrap_tenant("company", "owner").unwrap();
        let key = memory_key(&identity, &admin.secret, "subject", true);
        (admin, key)
    };
    let fixture = "platform_http_tests::platform_http_memory_child";
    let (mut process, port) = crash_server_for(dir.path(), fixture);
    let commands = "/api/v1/memory/commands";
    let (status, source) = wire_request(
        port,
        "POST",
        commands,
        &key,
        memory_create("source", "Before crash", "private"),
    );
    assert_eq!(status, 200);
    let mut command = memory_create("derived", "Derived before crash", "private");
    command["operation"]["type"] = "derive".into();
    command["operation"]["derivation"] = json!({"sources":[{"record_id":source["receipt"]["record_id"],"revision":1}],"method":"fixture","method_revision":"v1","evidence_ref":"trace://crash"});
    let (status, derived) = wire_request(port, "POST", commands, &key, command);
    assert_eq!(status, 200);
    let request = scoped(json!([derived["receipt"]["record_id"]]), "private");
    let (status, before) = wire_request(port, "POST", SCOPED, &key, request.clone());
    assert_eq!(status, 200);
    assert_eq!(before["graph"]["edges"].as_array().unwrap().len(), 1);
    let (status, workspaces) = wire_request(
        port,
        "GET",
        "/api/v1/company/memory/workspaces?contract_version=1",
        &admin.secret,
        Value::Null,
    );
    assert_eq!(status, 200);
    let company = json!({"contract_version":1,"workspace_id":workspaces["page"]["workspaces"][0]["workspace_id"],"query":request["query"]});
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    let (_restarted, port) = crash_server_for(dir.path(), fixture);
    for (route, token, body) in [
        (SCOPED, &key, request.clone()),
        (COMPANY, &admin.secret, company.clone()),
    ] {
        let (status, after) = wire_request(port, "POST", route, token, body);
        assert_eq!(status, 200);
        for field in ["nodes", "edges", "roots", "coverage"] {
            assert_eq!(after["graph"][field], before["graph"][field]);
        }
    }
    let mut update = memory_create("edit", "After crash", "private");
    update["operation"]["type"] = "update".into();
    update["operation"]["record_id"] = source["receipt"]["record_id"].clone();
    update["operation"]["expected_revision"] = 1.into();
    assert_eq!(wire_request(port, "POST", commands, &key, update).0, 200);
    for (route, token, body) in [(SCOPED, &key, request), (COMPANY, &admin.secret, company)] {
        let (status, after) = wire_request(port, "POST", route, token, body);
        assert_eq!(status, 200);
        assert_eq!(after["graph"]["nodes"], json!([]));
        assert_eq!(after["graph"]["roots"][0]["status"], "unavailable");
    }
}
