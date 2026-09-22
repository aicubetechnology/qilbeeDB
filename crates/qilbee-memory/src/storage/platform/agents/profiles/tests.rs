use super::*;
use crate::MemoryStorageConfig;
use std::sync::{Arc, Barrier};
use tempfile::TempDir;
fn open(path: &std::path::Path) -> RocksDbMemoryStorage {
    RocksDbMemoryStorage::open(MemoryStorageConfig::for_testing(path)).unwrap()
}
fn actor() -> RecordAuthor {
    RecordAuthor {
        credential_id: Uuid::from_u128(42),
        subject_id: "administrator".into(),
    }
}
fn register(db: &RocksDbMemoryStorage, company: &str, id: &str) {
    let scope = MemoryResourceScope {
        project_id: "external-project".into(),
        agent_id: id.into(),
        mission_id: None,
        visibility: MemoryVisibility::Private,
    };
    db.observe_agent(&AgentObservation {
        company_id: company.into(),
        agent_id: id.into(),
        namespace: CompanyMemoryAddress::new(company, &scope, Some("writer"))
            .unwrap()
            .namespace()
            .unwrap(),
        author: RecordAuthor {
            subject_id: "writer".into(),
            credential_id: Uuid::from_u128(2),
        },
    })
    .unwrap();
}
fn command(agent: &str, expected: u64, name: Option<&str>, id: &str) -> AgentProfileCommand {
    AgentProfileCommand {
        agent_id: agent.into(),
        expected_revision: expected,
        display_name: name.map(str::to_string),
        idempotency_key: id.into(),
    }
}
fn query() -> AgentDirectoryQuery {
    AgentDirectoryQuery {
        text: None,
        limit: 25,
        max_scanned_agents: 100,
        cursor: None,
    }
}
#[test]
fn agent_profiles_preserve_registration_and_replay_receipts_after_reopen_and_later_updates() {
    let dir = TempDir::new().unwrap();
    let first = command("agent", 0, Some("Research assistant"), "save-1");
    let (receipt, registration) = {
        let db = open(dir.path());
        register(&db, "company", "agent");
        let registration = db.profile_bytes(key("company", "agent")).unwrap();
        assert!(
            db.named_agent("company", "agent")
                .unwrap()
                .unwrap()
                .profile
                .is_none()
        );
        let saved = db
            .apply_agent_profile_command("company", &actor(), &first)
            .unwrap();
        assert!(!saved.replayed);
        db.apply_agent_profile_command(
            "company",
            &actor(),
            &command("agent", 1, Some("Research reviewer"), "save-2"),
        )
        .unwrap();
        assert_eq!(
            db.profile_bytes(key("company", "agent")).unwrap(),
            registration
        );
        (saved.receipt, registration)
    };
    let db = open(dir.path());
    let replay = db
        .apply_agent_profile_command("company", &actor(), &first)
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.receipt, receipt);
    assert_eq!(
        db.named_agent("company", "agent")
            .unwrap()
            .unwrap()
            .profile
            .unwrap()
            .revision,
        2
    );
    assert_eq!(
        db.profile_bytes(key("company", "agent")).unwrap(),
        registration
    );
    let first_page = db.agent_profile_history("company", "agent", 0, 1).unwrap();
    assert_eq!(first_page.receipts, vec![receipt]);
    assert_eq!(first_page.next_after_revision, Some(1));
    let second = db.agent_profile_history("company", "agent", 1, 1).unwrap();
    assert_eq!(second.receipts[0].profile.revision, 2);
    assert_eq!(second.next_after_revision, None);
    db.apply_agent_profile_command("company", &actor(), &command("agent", 2, None, "clear"))
        .unwrap();
    let cleared = db
        .named_agent("company", "agent")
        .unwrap()
        .unwrap()
        .profile
        .unwrap();
    assert_eq!(cleared.revision, 3);
    assert_eq!(cleared.display_name, None);
    assert!(
        db.agent_profile_history("company", "agent", 3, 1)
            .unwrap()
            .receipts
            .is_empty()
    );
}
#[test]
fn agent_profile_mutations_require_existing_registration_exact_revision_and_command_actor() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let first = command("agent", 0, Some("Analyst"), "save");
    assert!(matches!(
        db.apply_agent_profile_command("company", &actor(), &first),
        Err(Error::KeyNotFound(_))
    ));
    assert!(db.observed_agent("company", "agent").unwrap().is_none());
    register(&db, "company", "agent");
    register(&db, "company", "other");
    db.apply_agent_profile_command("company", &actor(), &first)
        .unwrap();
    assert!(matches!(
        db.apply_agent_profile_command(
            "company",
            &actor(),
            &command("agent", 0, Some("Stale"), "different")
        ),
        Err(Error::TransactionConflict(_))
    ));
    for changed in [
        command("agent", 0, Some("Changed"), "save"),
        command("other", 0, Some("Analyst"), "save"),
        command("agent", 1, Some("Analyst"), "save"),
    ] {
        assert!(matches!(
            db.apply_agent_profile_command("company", &actor(), &changed),
            Err(Error::ConstraintViolation(_))
        ));
    }
    let other = RecordAuthor {
        credential_id: Uuid::from_u128(43),
        ..actor()
    };
    assert!(matches!(
        db.apply_agent_profile_command("company", &other, &first),
        Err(Error::ConstraintViolation(_))
    ));
    for invalid in [
        "",
        " ",
        "Bad\nname",
        " leading",
        "trailing ",
        &"x".repeat(257),
    ] {
        assert!(matches!(
            db.apply_agent_profile_command(
                "company",
                &actor(),
                &command("agent", 1, Some(invalid), "invalid")
            ),
            Err(Error::ValidationError(_))
        ));
    }
    assert_eq!(
        db.agent_profile_history("company", "agent", 0, 100)
            .unwrap()
            .receipts
            .len(),
        1
    );
    assert!(matches!(
        db.agent_profile_history("company", "agent", 2, 10),
        Err(Error::TransactionConflict(_))
    ));
}
#[test]
fn agent_profile_compare_and_set_has_one_winner_under_concurrent_administrators() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(open(dir.path()));
    register(&db, "company", "agent");
    let barrier = Arc::new(Barrier::new(8));
    let workers: Vec<_> = (0..8)
        .map(|i| {
            let db = db.clone();
            let gate = barrier.clone();
            std::thread::spawn(move || {
                gate.wait();
                db.apply_agent_profile_command(
                    "company",
                    &actor(),
                    &command(
                        "agent",
                        0,
                        Some(&format!("Name {i}")),
                        &format!("attempt-{i}"),
                    ),
                )
            })
        })
        .collect();
    let results: Vec<_> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, Err(Error::TransactionConflict(_))))
            .count(),
        7
    );
    assert_eq!(
        db.agent_profile_history("company", "agent", 0, 100)
            .unwrap()
            .receipts
            .len(),
        1
    );
}
#[test]
fn named_agent_directory_is_company_bound_live_and_advances_through_filtered_empty_pages() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    for company in ["company", "company-extra", "other"] {
        for id in ["a", "b", "c"] {
            register(&db, company, id);
        }
    }
    for company in ["company", "other"] {
        db.apply_agent_profile_command(
            company,
            &actor(),
            &command("c", 0, Some("Revisão científica"), "same-command-id"),
        )
        .unwrap();
    }
    let mut q = query();
    q.text = Some("REVISÃO".into());
    q.max_scanned_agents = 1;
    let first = db.query_named_agents("company", &q).unwrap();
    assert!(first.agents.is_empty());
    assert_eq!(first.scanned_agents, 1);
    q.cursor = first.next_cursor;
    assert!(q.cursor.is_some());
    let second = db.query_named_agents("company", &q).unwrap();
    assert!(second.agents.is_empty());
    q.cursor = second.next_cursor;
    let third = db.query_named_agents("company", &q).unwrap();
    assert_eq!(third.agents.len(), 1);
    assert_eq!(third.agents[0].registration.company_id, "company");
    assert!(third.next_cursor.is_none());
    assert!(db.query_named_agents("other", &q).is_err());
    let mut changed = q.clone();
    changed.text = Some("different".into());
    assert!(db.query_named_agents("company", &changed).is_err());
    register(&db, "company", "aa");
    assert_eq!(
        db.query_named_agents("company", &q).unwrap().agents.len(),
        1
    );
    assert_eq!(
        db.query_named_agents("company", &query())
            .unwrap()
            .agents
            .len(),
        4
    );
    assert!(
        db.named_agent("company-extra", "c")
            .unwrap()
            .unwrap()
            .profile
            .is_none()
    );
    for limit in [0, 101] {
        let mut invalid = query();
        invalid.limit = limit;
        assert!(db.query_named_agents("company", &invalid).is_err());
    }
}
#[test]
fn agent_profile_inspection_rejects_missing_or_corrupt_atomic_companions() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    register(&db, "company", "agent");
    let request = command("agent", 0, Some("Analyst"), "save");
    db.apply_agent_profile_command("company", &actor(), &request)
        .unwrap();
    let cf = db.cf(super::super::super::super::cf::AGENT_META).unwrap();
    for target in [
        profile_key(PROFILE, "company", "agent"),
        history_key("company", "agent", 1),
        profile_key(RECEIPT, "company", "save"),
    ] {
        let original = db.db.get_cf(cf, &target).unwrap().unwrap();
        db.db.delete_cf(cf, &target).unwrap();
        assert!(db.named_agent("company", "agent").is_err());
        assert!(db.query_named_agents("company", &query()).is_err());
        assert!(
            db.apply_agent_profile_command("company", &actor(), &request)
                .is_err()
        );
        db.db.put_cf(cf, &target, &original).unwrap();
        assert!(db.named_agent("company", "agent").is_ok());
        db.db.put_cf(cf, &target, b"invalid").unwrap();
        assert!(db.named_agent("company", "agent").is_err());
        db.db.put_cf(cf, &target, &original).unwrap();
    }
}
#[test]
#[ignore = "Subprocess fixture invoked and killed by the durable profile parent test"]
fn agent_profile_crash_fixture() {
    let path = std::path::PathBuf::from(std::env::var("QILBEE_PROFILE_CRASH_PATH").unwrap());
    let db = open(&path.join("db"));
    register(&db, "company", "agent");
    let saved = db
        .apply_agent_profile_command(
            "company",
            &actor(),
            &command("agent", 0, Some("Durable analyst"), "save"),
        )
        .unwrap();
    std::fs::write(
        path.join("ack.json"),
        serde_json::to_vec(&saved.receipt).unwrap(),
    )
    .unwrap();
    loop {
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
#[test]
fn agent_profile_receipt_history_and_label_survive_abrupt_process_death() {
    let dir = TempDir::new().unwrap();
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "storage::platform::agents::profiles::tests::agent_profile_crash_fixture",
            "--ignored",
        ])
        .env("QILBEE_PROFILE_CRASH_PATH", dir.path())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !dir.path().join("ack.json").exists() && std::time::Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            panic!("Profile subprocess exited before acknowledgement");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    child.kill().unwrap();
    child.wait().unwrap();
    let receipt: AgentProfileReceipt =
        serde_json::from_slice(&std::fs::read(dir.path().join("ack.json")).unwrap()).unwrap();
    let db = open(&dir.path().join("db"));
    let replay = db
        .apply_agent_profile_command("company", &actor(), &receipt.command)
        .unwrap();
    assert!(replay.replayed);
    assert_eq!(replay.receipt, receipt);
    assert_eq!(
        db.named_agent("company", "agent").unwrap().unwrap().profile,
        Some(receipt.profile.clone())
    );
    assert_eq!(
        db.agent_profile_history("company", "agent", 0, 10)
            .unwrap()
            .receipts,
        vec![receipt]
    );
}

#[test]
fn agent_profile_tip_rollback_and_missing_clear_intent_are_rejected() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    register(&db, "company", "agent");
    let first = command("agent", 0, Some("First"), "first");
    db.apply_agent_profile_command("company", &actor(), &first)
        .unwrap();
    let tip = profile_key(PROFILE, "company", "agent");
    let old = db.profile_bytes(tip.clone()).unwrap().unwrap();
    db.apply_agent_profile_command(
        "company",
        &actor(),
        &command("agent", 1, Some("Second"), "second"),
    )
    .unwrap();
    db.db
        .put_cf(
            db.cf(super::super::super::super::cf::AGENT_META).unwrap(),
            tip,
            old,
        )
        .unwrap();
    assert!(db.named_agent("company", "agent").is_err());
    assert!(serde_json::from_value::<AgentProfileCommand>(serde_json::json!({"agent_id":"agent","expected_revision":0,"idempotency_key":"missing-clear"})).is_err());
}
