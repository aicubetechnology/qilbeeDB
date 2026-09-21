use super::*;
use qilbee_storage::StorageOptions;
use tempfile::TempDir;

fn store(path: &std::path::Path) -> IdentityStore {
    IdentityStore::new(Arc::new(
        StorageEngine::open(StorageOptions::for_testing(path)).unwrap(),
    ))
}

#[test]
fn directory_pages_are_scoped_before_selection_and_legacy_indexes_rebuild_on_reopen() {
    let dir = TempDir::new().unwrap();
    let (master, a, b, extra) = {
        let db = store(dir.path());
        let master = db.bootstrap_master("master").unwrap();
        let (_, a) = db
            .register_tenant_named(&master.secret, "a", "owner", Some("Company A"))
            .unwrap();
        let b = db.bootstrap_tenant("a\"/other", "owner").unwrap();
        let extra = db.issue(&a.secret, tenant_admin_spec("second")).unwrap();
        let key = tenant_credential_index("a", extra.credential.id).unwrap();
        let existing = db.storage.get_meta(&key).unwrap();
        db.storage
            .compare_and_write_meta(
                &[condition(&key, existing)],
                &[qilbee_storage::MetadataWrite { key, value: None }],
            )
            .unwrap();
        (master, a, b, extra)
    };
    let db = store(dir.path());
    db.prepare_administration_directory().unwrap();
    let first = db.list_tenant_credentials(&a.secret, None, 1).unwrap();
    let second = db
        .list_tenant_credentials(&a.secret, first.next_cursor.as_deref(), 1)
        .unwrap();
    assert!(second.next_cursor.is_none());
    let ids: BTreeSet<_> = first
        .items
        .iter()
        .chain(second.items.iter())
        .map(|c| c.id)
        .collect();
    assert_eq!(ids, [a.credential.id, extra.credential.id].into());
    assert!(
        db.list_tenant_credentials(&b.secret, first.next_cursor.as_deref(), 1)
            .is_err()
    );
    assert_eq!(
        db.list_tenant_credentials(&b.secret, None, 100)
            .unwrap()
            .items
            .len(),
        1
    );
    let tenants = db.list_tenants(&master.secret, None, 100).unwrap();
    assert_eq!(tenants.items.len(), 2);
    assert_eq!(
        tenants
            .items
            .iter()
            .find(|t| t.tenant_id == "a")
            .unwrap()
            .registration
            .as_ref()
            .unwrap()
            .display_name
            .as_deref(),
        Some("Company A")
    );
    assert!(
        tenants
            .items
            .iter()
            .find(|t| t.tenant_id == "a\"/other")
            .unwrap()
            .registration
            .is_none()
    );
    assert!(db.list_tenants(&a.secret, None, 100).is_err());
    assert!(db.list_tenant_credentials(&a.secret, None, 0).is_err());
    assert!(
        db.list_tenant_credentials(&a.secret, Some("invalid"), 10)
            .is_err()
    );
    db.revoke(&a.secret, extra.credential.id, 1).unwrap();
    assert!(
        db.list_tenant_credentials(&a.secret, None, 100)
            .unwrap()
            .items
            .iter()
            .find(|c| c.id == extra.credential.id)
            .unwrap()
            .revoked_at_millis
            .is_some()
    );
    db.revoke(&b.secret, b.credential.id, 1).unwrap();
    assert!(db.list_tenant_credentials(&b.secret, None, 100).is_err());
}

#[test]
fn accounts_and_global_credentials_never_disclose_password_or_api_verifiers() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let master = db.bootstrap_master("master").unwrap();
    let a = db.bootstrap_tenant("a", "owner").unwrap();
    let b = db.bootstrap_tenant("b", "owner").unwrap();
    for key in [&a, &b] {
        db.create_login_account(
            &key.secret,
            false,
            "same-user",
            "test-password",
            key.credential.id,
        )
        .unwrap();
    }
    db.create_login_account(
        &master.secret,
        true,
        "same-user",
        "test-password",
        master.credential.id,
    )
    .unwrap();
    let accounts = db.list_login_accounts(&a.secret, false, None, 100).unwrap();
    assert_eq!(accounts.items.len(), 1);
    assert_eq!(accounts.items[0].credential_id, a.credential.id);
    let json = String::from_utf8(serialize(&accounts).unwrap()).unwrap();
    for forbidden in [
        "password",
        "verifier",
        "sessions",
        "test-password",
        a.secret.as_str(),
        b.secret.as_str(),
    ] {
        assert!(!json.contains(forbidden));
    }
    assert_eq!(
        db.list_login_accounts(&master.secret, true, None, 100)
            .unwrap()
            .items
            .len(),
        1
    );
    let delegated = db
        .issue_global(
            &master.secret,
            GlobalCredentialSpec {
                subject_id: "delegate".into(),
                capabilities: all_capabilities(),
                expires_at_millis: None,
            },
        )
        .unwrap();
    assert!(
        db.list_login_accounts(&delegated.secret, true, None, 100)
            .is_err()
    );
    assert!(
        db.list_global_credentials(&delegated.secret, None, 100)
            .is_err()
    );
    assert_eq!(
        db.list_global_credentials(&master.secret, None, 100)
            .unwrap()
            .items
            .len(),
        2
    );
    let id = accounts.items[0].id;
    db.change_login_account(&a.secret, false, id, 1, None)
        .unwrap();
    assert!(
        db.list_login_accounts(&a.secret, false, None, 100)
            .unwrap()
            .items[0]
            .disabled_at_millis
            .is_some()
    );
}
