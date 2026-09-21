use super::*;
#[test]
fn typed_graph_http_keeps_atomic_adjacency_heads_and_claims_after_process_kill() {
    use crate::http_server_tests::{crash_server_for, wire_request};
    let dir = TempDir::new().unwrap();
    let key = {
        let (_, identity) = app(dir.path());
        let admin = identity.bootstrap_tenant("company", "owner").unwrap();
        token(&identity, &admin.secret, "alice")
    };
    let fixture = "platform_http_tests::platform_http_memory_child";
    let (mut child, port) = crash_server_for(dir.path(), fixture);
    let (status, a) = wire_request(
        port,
        "POST",
        MEMORY,
        &key,
        memory_create("a", "Source", "private"),
    );
    assert_eq!(status, 200);
    let (status, b) = wire_request(
        port,
        "POST",
        MEMORY,
        &key,
        memory_create("b", "Target", "private"),
    );
    assert_eq!(status, 200);
    let command = assertion(&a["receipt"], &b["receipt"], "edge", "supports");
    let (status, receipt) = wire_request(port, "POST", RELATIONS, &key, command.clone());
    assert_eq!(status, 200);
    let request = request_graph(json!([a["receipt"]["record_id"]]));
    let (status, before) = wire_request(port, "POST", GRAPH, &key, request.clone());
    assert_eq!(status, 200);
    assert_eq!(before["graph"]["coverage"]["complete"], true);
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let (mut child, port) = crash_server_for(dir.path(), fixture);
    let (status, after) = wire_request(port, "POST", GRAPH, &key, request.clone());
    assert_eq!(status, 200);
    assert_eq!(after["graph"]["edges"], before["graph"]["edges"]);
    assert_eq!(after["graph"]["nodes"], before["graph"]["nodes"]);
    assert_eq!(after["graph"]["coverage"], before["graph"]["coverage"]);
    assert_eq!(
        wire_request(port, "POST", RELATIONS, &key, command).1,
        receipt
    );
    let mut update = memory_create("correction", "New source", "private");
    update["operation"]["type"] = "update".into();
    update["operation"]["record_id"] = a["receipt"]["record_id"].clone();
    update["operation"]["expected_revision"] = 1.into();
    assert_eq!(wire_request(port, "POST", MEMORY, &key, update).0, 200);
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let (_child, port) = crash_server_for(dir.path(), fixture);
    let (status, changed) = wire_request(port, "POST", GRAPH, &key, request);
    assert_eq!(status, 200);
    assert_eq!(changed["graph"]["edges"], json!([]));
    assert_eq!(changed["graph"]["nodes"][0]["record"]["revision"], 2);
    assert_eq!(changed["graph"]["coverage"]["complete"], true);
}
