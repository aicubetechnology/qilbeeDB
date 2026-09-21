use super::*;
#[test]
fn relation_feed_and_checkpoint_replay_survive_abrupt_process_termination() {
    use crate::http_server_tests::{crash_server_for, wire_request};
    let dir = TempDir::new().unwrap();
    let key = {
        let (_, identity) = app(dir.path());
        let admin = identity.bootstrap_tenant("company", "owner").unwrap();
        token(&identity, &admin.secret, "alice")
    };
    let fixture = "platform_http_tests::platform_http_memory_child";
    let (mut child, port) = crash_server_for(dir.path(), fixture);
    let (status, api) = wire_request(port, "GET", "/openapi.json", "", Value::Null);
    assert_eq!(status, 200);
    let call = |port, path: &str, body: Value| {
        let (status, value) = wire_request(port, "POST", path, &key, body);
        assert_eq!(status, 200, "{path}: {value}");
        let schema = json!({"allOf":[api["paths"][path]["post"]["responses"]["200"]["content"]["application/json"]["schema"]],"components":api["components"]});
        let validator = jsonschema::draft202012::options().build(&schema).unwrap();
        assert!(validator.is_valid(&value), "{path}: {value}");
        value
    };
    let baseline = call(port, ACTIVATE, scoped())["baseline"].clone();
    let a = call(port, MEMORY, memory_create("a", "Source", "private"))["receipt"].clone();
    let b = call(port, MEMORY, memory_create("b", "Target", "private"))["receipt"].clone();
    let relation_command = assertion(&a, &b, "edge", "supports");
    let relation = call(port, RELATIONS, relation_command.clone());
    let initial = call(
        port,
        CHECKPOINT,
        checkpoint(baseline, &Value::Null, "initial"),
    )["receipt"]
        .clone();
    let page = call(port, CHANGES, query(Value::Null, Value::Null, 256))["page"].clone();
    let advance = checkpoint(
        page["next_cursor"].clone(),
        &initial["checkpoint"],
        "applied-durable-effects",
    );
    let ack = call(port, CHECKPOINT, advance.clone());
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let (mut child, port) = crash_server_for(dir.path(), fixture);
    assert_eq!(
        call(port, CHANGES, query(Value::Null, Value::Null, 256))["page"],
        page
    );
    assert_eq!(call(port, CHECKPOINT, advance.clone()), ack);
    assert_eq!(call(port, RELATIONS, relation_command), relation);
    let mut reject = scoped();
    reject["idempotency_key"] = "reject".into();
    reject["operation"] = json!({"type":"review","relation_id":relation["receipt"]["relation_id"],"expected_revision":1,"disposition":"rejected","evidence_ref":"trace://rejected"});
    call(port, RELATIONS, reject);
    child.0.kill().unwrap();
    child.0.wait().unwrap();
    let (_child, port) = crash_server_for(dir.path(), fixture);
    let pending = call(
        port,
        CHANGES,
        query(page["next_cursor"].clone(), Value::Null, 256),
    )["page"]
        .clone();
    assert_eq!(pending["changes"].as_array().unwrap().len(), 1);
    assert_eq!(pending["changes"][0]["change"]["kind"], "reviewed");
    let current = call(port, READ, read_body());
    assert_eq!(current["checkpoint"], ack["receipt"]["checkpoint"]);
    let done = call(
        port,
        CHECKPOINT,
        checkpoint(
            pending["next_cursor"].clone(),
            &current["checkpoint"],
            "final",
        ),
    );
    assert_eq!(done["receipt"]["checkpoint"]["revision"], 3);
    assert_eq!(call(port, CHECKPOINT, advance), ack);
    let status = call(port, DIAGNOSE, read_body());
    assert_eq!(status["diagnostics"]["pending_positions"], 0);
    assert_eq!(status["diagnostics"]["checkpoint"]["revision"], 3);
}
