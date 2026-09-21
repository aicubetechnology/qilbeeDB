use super::*;
use crate::MemoryStorageConfig;
use std::sync::{Arc, Barrier};
use tempfile::TempDir;

fn open(path: &std::path::Path) -> RocksDbMemoryStorage {
    RocksDbMemoryStorage::open(MemoryStorageConfig::for_testing(path)).unwrap()
}
fn observation(company: &str, agent: &str) -> AgentObservation {
    AgentObservation {
        company_id: company.into(),
        agent_id: agent.into(),
        namespace: format!("{company}/{agent}"),
        author: RecordAuthor {
            credential_id: Uuid::new_v4(),
            subject_id: "integration".into(),
        },
    }
}
fn command() -> MemoryCommand {
    MemoryCommand {
        contract_version: 1,
        idempotency_key: "first-command".into(),
        operation: MemoryOperation::Create {
            record: RecordInput {
                episode_type: EpisodeType::Observation,
                content: EpisodeContent::new("Registered together with memory"),
                event_time_millis: 1700000000000,
                valid_until_millis: None,
                tags: vec![],
                metadata: Default::default(),
            },
        },
    }
}

#[test]
fn first_memory_and_registration_are_durable_and_replays_do_not_replace_registration() {
    let dir = TempDir::new().unwrap();
    let first = observation("company", "externally-managed/α");
    let (receipt, registration) = {
        let db = open(dir.path());
        let receipt = db
            .apply_observed_memory_command(&first, &command())
            .unwrap();
        let registration = db
            .observed_agent(&first.company_id, &first.agent_id)
            .unwrap()
            .unwrap();
        assert_eq!(registration.registered_by, first.author);
        assert_eq!(
            registration.trigger,
            AgentRegistrationTrigger::MemoryCommand {
                record_id: receipt.record_id,
                revision: receipt.revision,
                action: receipt.action.clone()
            }
        );
        (receipt, registration)
    };
    let db = open(dir.path());
    assert_eq!(
        db.observed_agent(&first.company_id, &first.agent_id)
            .unwrap()
            .unwrap(),
        registration
    );
    assert!(
        db.read_memory_record(&first.namespace, receipt.record_id)
            .unwrap()
            .is_some()
    );
    assert_eq!(
        db.apply_observed_memory_command(&first, &command())
            .unwrap(),
        receipt
    );
    let mut later = first.clone();
    later.author = observation("company", "different").author;
    later.namespace = "another-project/same-agent".into();
    db.observe_agent(&later).unwrap();
    assert_eq!(
        db.observed_agent(&first.company_id, &first.agent_id)
            .unwrap()
            .unwrap(),
        registration
    );
    let other = observation("other-company", &first.agent_id);
    db.observe_agent(&other).unwrap();
    assert_eq!(
        db.list_observed_agents("company", None, 100)
            .unwrap()
            .agents
            .len(),
        1
    );
    assert_eq!(
        db.list_observed_agents("other-company", None, 100)
            .unwrap()
            .agents
            .len(),
        1
    );
}

#[test]
fn invalid_or_failed_memory_commands_leave_no_successful_agent_registration() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let first = observation("company", "invalid-command");
    let mut bad = command();
    bad.contract_version = 99;
    assert!(db.apply_observed_memory_command(&first, &bad).is_err());
    assert!(
        db.observed_agent("company", "invalid-command")
            .unwrap()
            .is_none()
    );
    let failed = observation("company", "inconsistent-journal");
    db.db
        .put_cf(
            db.cf(super::super::super::cf::AGENT_META).unwrap(),
            record_prefix(0x20, &failed.namespace),
            b"corrupt",
        )
        .unwrap();
    assert!(
        db.apply_observed_memory_command(&failed, &command())
            .is_err()
    );
    assert!(
        db.observed_agent("company", "inconsistent-journal")
            .unwrap()
            .is_none()
    );
    // A failed batch must not leave a canonical memory behind either.
    let prefix = record_prefix(0x10, &failed.namespace);
    assert!(
        !db.db
            .iterator_cf(
                db.cf(super::super::super::cf::EPISODES).unwrap(),
                rocksdb::IteratorMode::From(&prefix, rocksdb::Direction::Forward)
            )
            .next()
            .is_some_and(|row| row.unwrap().0.starts_with(&prefix))
    );
}

#[test]
fn concurrent_first_writes_preserve_one_association_and_one_idempotent_memory() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(open(dir.path()));
    let observation = observation("company", "concurrent-agent");
    let barrier = Arc::new(Barrier::new(12));
    let jobs: Vec<_> = (0..12)
        .map(|_| {
            let db = db.clone();
            let observation = observation.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                db.apply_observed_memory_command(&observation, &command())
                    .unwrap()
            })
        })
        .collect();
    let receipts: Vec<_> = jobs.into_iter().map(|job| job.join().unwrap()).collect();
    assert!(receipts.iter().all(|receipt| receipt == &receipts[0]));
    let page = db.list_observed_agents("company", None, 100).unwrap();
    assert_eq!(page.agents.len(), 1);
    assert!(page.next_after_agent_id.is_none());
}

#[test]
fn directory_uses_exact_company_prefix_and_live_forward_agent_cursor() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for company in ["company", "company-extra", "other"] {
        for agent in ["α", "b", "a"] {
            db.observe_agent(&observation(company, agent)).unwrap();
        }
    }
    let mut after = None;
    let mut ids = vec![];
    loop {
        let page = db
            .list_observed_agents("company", after.as_deref(), 1)
            .unwrap();
        assert_eq!(page.agents.len(), 1);
        assert_eq!(page.agents[0].company_id, "company");
        ids.push(page.agents[0].agent_id.clone());
        after = page.next_after_agent_id;
        if after.is_none() {
            break;
        }
    }
    assert_eq!(ids, ["a", "b", "α"]);
    for limit in [0, 101] {
        assert!(db.list_observed_agents("company", None, limit).is_err());
    }
    assert!(db.list_observed_agents("company", Some("\n"), 1).is_err());
    assert!(
        db.list_observed_agents("empty", None, 1)
            .unwrap()
            .agents
            .is_empty()
    );
}

#[test]
fn an_old_receipt_can_register_its_first_post_upgrade_observation_without_new_memory() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let first = observation("company", "existing-agent");
    let receipt = db
        .apply_memory_command(&first.namespace, &first.author, &command())
        .unwrap();
    assert!(
        db.observed_agent("company", "existing-agent")
            .unwrap()
            .is_none()
    );
    let mut conflicting = command();
    conflicting.operation = MemoryOperation::Delete {
        record_id: receipt.record_id,
        expected_revision: 1,
    };
    assert!(
        db.apply_observed_memory_command(&first, &conflicting)
            .is_err()
    );
    assert!(
        db.observed_agent("company", "existing-agent")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.apply_observed_memory_command(&first, &command())
            .unwrap(),
        receipt
    );
    assert!(
        db.observed_agent("company", "existing-agent")
            .unwrap()
            .is_some()
    );
}

#[test]
fn inconsistent_agent_metadata_is_rejected_before_serving_or_mutating_memory() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let first = observation("company", "corrupted-agent");
    db.observe_agent(&first).unwrap();
    let original = db
        .observed_agent("company", "corrupted-agent")
        .unwrap()
        .unwrap();
    let mut mismatched = original.clone();
    mismatched.company_id = "other-company".into();
    let mut invalid_trigger = original.clone();
    invalid_trigger.trigger = AgentRegistrationTrigger::MemoryCommand {
        record_id: Uuid::new_v4(),
        revision: 0,
        action: "created".into(),
    };
    let mut invalid_action = original;
    invalid_action.trigger = AgentRegistrationTrigger::MemoryCommand {
        record_id: Uuid::new_v4(),
        revision: 1,
        action: "unrecognized".into(),
    };
    for corrupted in [mismatched, invalid_trigger, invalid_action] {
        db.db
            .put_cf(
                db.cf(super::super::super::cf::AGENT_META).unwrap(),
                key("company", "corrupted-agent"),
                encode(&corrupted).unwrap(),
            )
            .unwrap();
        assert!(db.observed_agent("company", "corrupted-agent").is_err());
        assert!(db.list_observed_agents("company", None, 25).is_err());
        assert!(
            db.apply_observed_memory_command(&first, &command())
                .is_err()
        );
    }
}
