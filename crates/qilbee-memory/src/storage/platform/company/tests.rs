use super::*;

#[test]
fn canonical_company_addresses_preserve_the_existing_namespace_bytes() {
    let scope = MemoryResourceScope {
        project_id: "project".into(),
        mission_id: None,
        agent_id: "agent".into(),
        visibility: MemoryVisibility::Private,
    };
    let address = CompanyMemoryAddress::new("company", &scope, Some("subject")).unwrap();
    let expected = r#"qdb:scope:v1:["company",{"project_id":"project","mission_id":null,"agent_id":"agent","visibility":"private"},"subject"]"#;
    assert_eq!(address.namespace().unwrap(), expected);
    assert_eq!(
        CompanyMemoryAddress::from_namespace(expected).unwrap(),
        Some(address)
    );
    assert!(CompanyMemoryAddress::new("company", &scope, None).is_err());
    let mut shared = scope;
    shared.visibility = MemoryVisibility::Shared;
    assert!(CompanyMemoryAddress::new("company", &shared, Some("subject")).is_err());
}

use crate::storage::platform::semantic_tests::{input, open};
use serde_json::json;
use tempfile::TempDir;

fn address(company: &str, subject: Option<&str>) -> CompanyMemoryAddress {
    CompanyMemoryAddress::new(
        company,
        &MemoryResourceScope {
            project_id: "project/α".into(),
            mission_id: Some("mission".into()),
            agent_id: "external-agent".into(),
            visibility: if subject.is_some() {
                MemoryVisibility::Private
            } else {
                MemoryVisibility::Shared
            },
        },
        subject,
    )
    .unwrap()
}
fn actor_for(address: &CompanyMemoryAddress) -> RecordAuthor {
    RecordAuthor {
        credential_id: Uuid::new_v4(),
        subject_id: address
            .private_subject_id
            .clone()
            .unwrap_or_else(|| "writer".into()),
    }
}
fn create(db: &RocksDbMemoryStorage, address: &CompanyMemoryAddress, key: &str) -> CommandReceipt {
    db.apply_memory_command(
        &address.namespace().unwrap(),
        &actor_for(address),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: key.into(),
            operation: MemoryOperation::Create { record: input(key) },
        },
    )
    .unwrap()
}
fn query() -> CompanyMemoryQuery {
    serde_json::from_value(json!({})).unwrap()
}

#[test]
fn company_catalog_backfills_old_storage_including_deleted_only_workspaces() {
    let dir = TempDir::new().unwrap();
    let private = address("company", Some("owner"));
    let shared = address("company", None);
    {
        let db = open(dir.path());
        create(&db, &private, "private");
        let deleted = create(&db, &shared, "shared");
        db.apply_memory_command(
            &shared.namespace().unwrap(),
            &actor_for(&shared),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "delete".into(),
                operation: MemoryOperation::Delete {
                    record_id: deleted.record_id,
                    expected_revision: 1,
                },
            },
        )
        .unwrap();
        create(&db, &address("company-extra", Some("owner")), "foreign");
        for workspace in db
            .company_memory_workspaces("company", None, 100)
            .unwrap()
            .workspaces
        {
            // Model a pre-feature database: source records and journal/candidate
            // tips exist, while the new catalog has never been created.
            db.db
                .delete_cf(
                    db.cf(super::super::super::cf::AGENT_META).unwrap(),
                    workspace_key("company", &workspace.workspace_id),
                )
                .unwrap();
        }
        assert!(db
            .company_memory_workspaces("company", None, 100)
            .unwrap()
            .workspaces
            .is_empty());
    }
    let db = open(dir.path());
    let page = db.company_memory_workspaces("company", None, 100).unwrap();
    assert_eq!(page.workspaces.len(), 2);
    assert!(page.next_after_workspace_id.is_none());
    let deleted = db
        .query_company_memory("company", &shared.workspace_id().unwrap(), &query())
        .unwrap()
        .unwrap();
    assert_eq!(deleted.page.entries.len(), 1);
    assert!(deleted.page.entries[0].record.payload.is_none());
    assert_eq!(
        deleted.page.entries[0]
            .eligibility
            .first_failure
            .as_ref()
            .unwrap()
            .reason,
        MemoryEligibilityReason::Deleted
    );
    // Catalog migration is not a fabricated historical agent registration.
    assert!(db
        .list_observed_agents("company", None, 100)
        .unwrap()
        .agents
        .is_empty());
}

#[test]
fn retained_inventory_explains_ineligible_records_without_reenabling_agent_retrieval() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = address("company", Some("owner"));
    let ns = a.namespace().unwrap();
    let wid = a.workspace_id().unwrap();
    let live = create(&db, &a, "live");
    let deleted = create(&db, &a, "deleted");
    db.apply_memory_command(
        &ns,
        &actor_for(&a),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "delete".into(),
            operation: MemoryOperation::Delete {
                record_id: deleted.record_id,
                expected_revision: 1,
            },
        },
    )
    .unwrap();
    let rejected = create(&db, &a, "rejected");
    db.review_memory_record(
        &ns,
        &actor_for(&a),
        &MemoryReviewCommand {
            contract_version: 1,
            idempotency_key: "reject".into(),
            record_id: rejected.record_id,
            expected_revision: 1,
            disposition: MemoryReviewDisposition::Rejected,
            evidence_ref: "trace://company-audit".into(),
        },
    )
    .unwrap();
    let mut expired_input = input("expired");
    expired_input.valid_until_millis = Some(1);
    let expired = db
        .apply_memory_command(
            &ns,
            &actor_for(&a),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "expire".into(),
                operation: MemoryOperation::Create {
                    record: expired_input,
                },
            },
        )
        .unwrap();
    let source = create(&db, &a, "source");
    let derived = db
        .apply_memory_command(
            &ns,
            &actor_for(&a),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "derive".into(),
                operation: MemoryOperation::Derive {
                    record: input("derived"),
                    derivation: MemoryDerivation {
                        sources: vec![MemorySourceRef {
                            record_id: source.record_id,
                            revision: 1,
                        }],
                        method: "fixture".into(),
                        method_revision: "v1".into(),
                        evidence_ref: "trace://derivation".into(),
                    },
                },
            },
        )
        .unwrap();
    db.apply_memory_command(
        &ns,
        &actor_for(&a),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "update-source".into(),
            operation: MemoryOperation::Update {
                record_id: source.record_id,
                expected_revision: 1,
                record: input("corrected source"),
            },
        },
    )
    .unwrap();
    let retained = db
        .query_company_memory("company", &wid, &query())
        .unwrap()
        .unwrap();
    assert_eq!(retained.page.entries.len(), 6);
    assert_eq!(retained.page.scanned_records, 6);
    assert_eq!(
        retained.page.stop_reason,
        CompanyInventoryStopReason::Exhausted
    );
    for (receipt, reason) in [
        (deleted, MemoryEligibilityReason::Deleted),
        (rejected, MemoryEligibilityReason::Rejected),
        (expired, MemoryEligibilityReason::Expired),
        (derived, MemoryEligibilityReason::SourceRevisionChanged),
    ] {
        let inspection = db
            .inspect_company_memory("company", &wid, receipt.record_id)
            .unwrap()
            .unwrap();
        assert!(!inspection.entry.eligibility.eligible);
        assert_eq!(
            inspection.entry.eligibility.first_failure.unwrap().reason,
            reason
        );
        assert!(db
            .read_memory_record(&ns, receipt.record_id)
            .unwrap()
            .is_none());
    }
    let mut current = query();
    current.view = CompanyMemoryView::Current;
    let page = db
        .query_company_memory("company", &wid, &current)
        .unwrap()
        .unwrap()
        .page;
    assert_eq!(page.entries.len(), 2);
    assert!(page
        .entries
        .iter()
        .all(|e| e.eligibility.eligible
            && e.eligibility.evaluated_at_millis == page.evaluated_at_millis));
    assert!(page
        .entries
        .iter()
        .any(|e| e.record.record_id == live.record_id));
    assert_eq!(
        page.dependency_work.records_examined,
        page.entries
            .iter()
            .map(|e| e.eligibility.dependency_work.records_examined)
            .sum::<usize>()
            + 1
    );
}

#[test]
fn company_inventory_preserves_private_boundaries_and_ignores_foreign_corruption() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let one = address("company", Some("one"));
    let two = address("company", Some("two"));
    let foreign = address("other", Some("one"));
    let first = create(&db, &one, "same-content");
    let second = create(&db, &two, "same-content");
    create(&db, &foreign, "same-content");
    let before = db.company_memory_workspaces("company", None, 100).unwrap();
    db.db
        .put_cf(
            db.cf(super::super::super::cf::AGENT_META).unwrap(),
            workspace_key("other", &foreign.workspace_id().unwrap()),
            b"corrupt",
        )
        .unwrap();
    assert_eq!(
        db.company_memory_workspaces("company", None, 100)
            .unwrap()
            .workspaces,
        before.workspaces
    );
    assert!(db.company_memory_workspaces("other", None, 100).is_err());
    assert!(db
        .query_company_memory("company", &foreign.workspace_id().unwrap(), &query())
        .unwrap()
        .is_none());
    assert!(db
        .inspect_company_memory("company", &one.workspace_id().unwrap(), second.record_id)
        .unwrap()
        .is_none());
    let result = db
        .query_company_memory("company", &one.workspace_id().unwrap(), &query())
        .unwrap()
        .unwrap();
    assert_eq!(result.page.entries.len(), 1);
    assert_eq!(result.page.entries[0].record.record_id, first.record_id);
    assert_eq!(result.page.scanned_records, 1);
    let first_page = db.company_memory_workspaces("company", None, 1).unwrap();
    let next = db
        .company_memory_workspaces("company", first_page.next_after_workspace_id.as_deref(), 1)
        .unwrap();
    assert_eq!(next.workspaces.len(), 1);
    assert!(next.next_after_workspace_id.is_none());
    assert_ne!(
        next.workspaces[0].workspace_id,
        first_page.workspaces[0].workspace_id
    );
}

#[test]
fn inventory_scan_and_byte_continuations_preserve_forward_progress_without_duplicates() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = address("company", None);
    let wid = a.workspace_id().unwrap();
    let mut expected = std::collections::BTreeSet::new();
    for i in 0..5 {
        expected.insert(create(&db, &a, &format!("row-{i}")).record_id);
    }
    let mut q = query();
    q.limit = 1;
    q.scan_limit = 1;
    let mut seen = std::collections::BTreeSet::new();
    loop {
        let page = db
            .query_company_memory("company", &wid, &q)
            .unwrap()
            .unwrap()
            .page;
        assert_eq!(page.entries.len(), 1);
        assert!(seen.insert(page.entries[0].record.record_id));
        q.after = page.next_after;
        if q.after.is_none() {
            assert_eq!(page.stop_reason, CompanyInventoryStopReason::Exhausted);
            break;
        }
        assert_eq!(page.stop_reason, CompanyInventoryStopReason::RecordLimit);
    }
    assert_eq!(seen, expected);
    q = query();
    q.scan_limit = 1;
    q.text_contains = Some("no match".into());
    let mut scanned = 0;
    loop {
        let page = db
            .query_company_memory("company", &wid, &q)
            .unwrap()
            .unwrap()
            .page;
        assert!(page.entries.is_empty());
        scanned += page.scanned_records;
        if let Some(after) = page.next_after {
            assert!(q.after.is_none_or(|old| after > old));
            q.after = Some(after);
            assert_eq!(page.stop_reason, CompanyInventoryStopReason::ScanLimit);
        } else {
            break;
        }
    }
    assert_eq!(scanned, 5);
    let large = address("large", None);
    let ns = large.namespace().unwrap();
    let id = large.workspace_id().unwrap();
    for i in 0..3 {
        db.apply_memory_command(
            &ns,
            &actor_for(&large),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: format!("large-{i}"),
                operation: MemoryOperation::Create {
                    record: input(&"x".repeat(3_000_000)),
                },
            },
        )
        .unwrap();
    }
    let mut q = query();
    let first = db
        .query_company_memory("large", &id, &q)
        .unwrap()
        .unwrap()
        .page;
    assert_eq!(first.entries.len(), 2);
    assert_eq!(first.stop_reason, CompanyInventoryStopReason::ByteLimit);
    assert!(first.record_bytes <= MAX_MEMORY_READ_BYTES);
    q.after = first.next_after;
    let last = db
        .query_company_memory("large", &id, &q)
        .unwrap()
        .unwrap()
        .page;
    assert_eq!(last.entries.len(), 1);
    assert!(last.next_after.is_none());
    assert!(!first
        .entries
        .iter()
        .any(|e| e.record.record_id == last.entries[0].record.record_id));
}

#[test]
fn catalog_and_memory_share_atomic_failure_and_corrupt_catalog_fails_closed() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = address("company", Some("owner"));
    let ns = a.namespace().unwrap();
    let id = a.workspace_id().unwrap();
    db.db
        .put_cf(
            db.cf(super::super::super::cf::AGENT_META).unwrap(),
            record_prefix(0x20, &ns),
            b"corrupt",
        )
        .unwrap();
    assert!(db
        .apply_memory_command(
            &ns,
            &actor_for(&a),
            &MemoryCommand {
                contract_version: 1,
                idempotency_key: "failed".into(),
                operation: MemoryOperation::Create {
                    record: input("failed")
                }
            }
        )
        .is_err());
    assert!(db
        .company_memory_workspaces("company", None, 100)
        .unwrap()
        .workspaces
        .is_empty());
    db.db
        .delete_cf(
            db.cf(super::super::super::cf::AGENT_META).unwrap(),
            record_prefix(0x20, &ns),
        )
        .unwrap();
    let record = create(&db, &a, "valid");
    let mut workspace = db
        .company_memory_workspace("company", &id)
        .unwrap()
        .unwrap();
    workspace.private_subject_id = Some("different-owner".into());
    db.db
        .put_cf(
            db.cf(super::super::super::cf::AGENT_META).unwrap(),
            workspace_key("company", &id),
            encode(&StoredWorkspace {
                schema_version: 1,
                workspace,
            })
            .unwrap(),
        )
        .unwrap();
    assert!(db.company_memory_workspaces("company", None, 100).is_err());
    assert!(db.query_company_memory("company", &id, &query()).is_err());
    assert!(db
        .inspect_company_memory("company", &id, record.record_id)
        .is_err());
}

#[test]
fn inventory_rejects_invalid_bounds_and_preserves_unscoped_library_compatibility() {
    let dir = TempDir::new().unwrap();
    let db = open(dir.path());
    let a = address("company", None);
    let id = a.workspace_id().unwrap();
    create(&db, &a, "valid");
    for (limit, scan_limit) in [(0, 1), (101, 1), (1, 0), (1, 10001)] {
        let mut q = query();
        q.limit = limit;
        q.scan_limit = scan_limit;
        assert!(db.query_company_memory("company", &id, &q).is_err());
    }
    assert!(db.company_memory_workspaces("company", None, 101).is_err());
    assert!(db.company_memory_workspace("company", "not-an-id").is_err());
    assert!(CompanyMemoryAddress::from_namespace("qdb:scope:v1:malformed").is_err());
    let invalid = a.namespace().unwrap().replace("\"project/α\"", "\"\"");
    assert!(CompanyMemoryAddress::from_namespace(&invalid).is_err());
    db.apply_memory_command(
        "ordinary-library-namespace",
        &actor_for(&a),
        &MemoryCommand {
            contract_version: 1,
            idempotency_key: "legacy".into(),
            operation: MemoryOperation::Create {
                record: input("legacy"),
            },
        },
    )
    .unwrap();
    assert_eq!(
        db.company_memory_workspaces("company", None, 100)
            .unwrap()
            .workspaces
            .len(),
        1
    );
}
