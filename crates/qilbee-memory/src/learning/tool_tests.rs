use super::*;
use tempfile::TempDir;

fn actor() -> ToolActor {
    ToolActor {
        subject_id: "developer".into(),
        credential_id: "credential-v1".into(),
    }
}
fn artifact(id: &str) -> ToolArtifactProposal {
    ToolArtifactProposal {
        id: id.into(),
        source: "def run(value):\n    return value\n".into(),
        dependency_lock: "".into(),
        runtime_image_digest: format!("sha256:{}", "a".repeat(64)),
        entrypoint: "tool:run".into(),
        input_schema: serde_json::json!({"type":"object"}),
        output_schema: serde_json::json!(true),
        source_refs: vec!["request:one".into()],
        parent_artifact_id: None,
        repair_evidence_ref: None,
    }
}
#[test]
fn tool_artifact_is_immutable_scoped_and_durable() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let first = store
        .register_tool_artifact("tenant", "scope", artifact("one"), actor())
        .unwrap();
    assert_eq!(first.source_digest.len(), 64);
    assert_eq!(
        first.dependency_digest,
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        first,
        store
            .register_tool_artifact("tenant", "scope", artifact("one"), actor())
            .unwrap()
    );
    let mut changed = artifact("one");
    changed.source.push(' ');
    assert!(
        store
            .register_tool_artifact("tenant", "scope", changed, actor())
            .is_err()
    );
    assert!(
        store
            .tool_artifact("other", "scope", "one")
            .unwrap()
            .is_none()
    );
    assert!(
        store
            .tool_artifact("tenant", "other", "one")
            .unwrap()
            .is_none()
    );
    drop(store);
    let store = LearningMemory::open(dir.path()).unwrap();
    assert_eq!(
        store.tool_artifact("tenant", "scope", "one").unwrap(),
        Some(first)
    );
}
#[test]
fn tool_artifact_repair_requires_local_parent_and_evidence() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let mut repair = artifact("repair");
    repair.parent_artifact_id = Some("one".into());
    assert!(
        store
            .register_tool_artifact("tenant", "scope", repair.clone(), actor())
            .is_err()
    );
    repair.repair_evidence_ref = Some("failure:one".into());
    store
        .register_tool_artifact("tenant", "foreign", artifact("one"), actor())
        .unwrap();
    assert!(
        store
            .register_tool_artifact("tenant", "scope", repair.clone(), actor())
            .is_err()
    );
    store
        .register_tool_artifact("tenant", "scope", artifact("one"), actor())
        .unwrap();
    let record = store
        .register_tool_artifact("tenant", "scope", repair, actor())
        .unwrap();
    assert_eq!(record.proposal.parent_artifact_id.as_deref(), Some("one"));
}
#[test]
fn tool_artifact_rejects_unbounded_or_malformed_declarations() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let mut bad = artifact("one");
    bad.runtime_image_digest = "python:latest".into();
    assert!(
        store
            .register_tool_artifact("tenant", "scope", bad, actor())
            .is_err()
    );
    let mut bad = artifact("one");
    bad.source = "x".repeat(24 * 1024 + 1);
    assert!(
        store
            .register_tool_artifact("tenant", "scope", bad, actor())
            .is_err()
    );
    let mut bad = artifact("one");
    bad.input_schema = serde_json::json!([]);
    assert!(
        store
            .register_tool_artifact("tenant", "scope", bad, actor())
            .is_err()
    );
    let mut bad = serde_json::to_value(artifact("one")).unwrap();
    bad["approved"] = true.into();
    assert!(serde_json::from_value::<ToolArtifactProposal>(bad).is_err());
}
#[test]
fn tool_artifact_concurrent_retries_have_one_original_record() {
    let dir = TempDir::new().unwrap();
    let store = LearningMemory::open(dir.path()).unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store
                    .register_tool_artifact("tenant", "scope", artifact("one"), actor())
                    .unwrap()
            })
        })
        .collect();
    let records: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert!(records.iter().all(|record| record == &records[0]));
}
