use super::*;
use qilbee_storage::StorageOptions;
use tempfile::TempDir;

fn store(path: &std::path::Path) -> IdentityStore {
    IdentityStore::new(Arc::new(
        StorageEngine::open(StorageOptions::for_testing(path)).unwrap(),
    ))
}

fn provisioner() -> GlobalCredentialSpec {
    GlobalCredentialSpec {
        subject_id: "signup-backend".into(),
        capabilities: [GlobalCapability::TenantCreate].into(),
        expires_at_millis: None,
    }
}

#[test]
fn global_authority_is_separate_durable_and_redacts_secrets() {
    let dir = TempDir::new().unwrap();
    let (master, signup, tenant) = {
        let db = store(dir.path());
        let master = db.bootstrap_master("master").unwrap();
        assert!(!format!("{master:?}").contains(&master.secret));
        assert!(db.bootstrap_master("replacement").is_err());
        let signup = db.issue_global(&master.secret, provisioner()).unwrap();
        let (registration, tenant) = db
            .register_tenant(&signup.secret, "company-a", "company-owner")
            .unwrap();
        assert_eq!(registration.created_by, signup.credential.id);
        assert_eq!(tenant.credential.history[0].actor_id, signup.credential.id);
        assert!(tenant.credential.spec.grants.is_empty());
        assert!(db.authenticate(&master.secret).is_err());
        assert!(db.authenticate(&signup.secret).is_err());
        assert!(db.authenticate_global(&tenant.secret).is_err());
        assert!(db.issue_global(&tenant.secret, provisioner()).is_err());
        for issued in [&master, &signup] {
            let bytes = db
                .storage
                .get_meta(&global_key(issued.credential.id))
                .unwrap()
                .unwrap();
            assert!(!String::from_utf8(bytes).unwrap().contains(&issued.secret));
        }
        (master, signup, tenant)
    };
    let db = store(dir.path());
    assert!(db.authenticate_global(&master.secret).is_ok());
    assert!(db.authenticate_global(&signup.secret).is_ok());
    assert_eq!(
        db.authenticate(&tenant.secret).unwrap().tenant_id,
        "company-a"
    );
    let receipt = db
        .inspect_tenant(&master.secret, "company-a")
        .unwrap()
        .unwrap();
    assert_eq!(receipt.initial_admin_id, tenant.credential.id);
    assert!(
        db.register_tenant(&signup.secret, "company-a", "attacker")
            .is_err()
    );
    assert_eq!(
        db.inspect_tenant(&master.secret, "company-a")
            .unwrap()
            .unwrap()
            .initial_admin_id,
        tenant.credential.id
    );
}

#[test]
fn provisioner_cannot_take_over_existing_tenants_or_delegate_global_powers() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let master = db.bootstrap_master("master").unwrap();
    let signup = db.issue_global(&master.secret, provisioner()).unwrap();
    let existing = db.bootstrap_tenant("legacy", "owner").unwrap();
    assert!(
        db.register_tenant(&signup.secret, "legacy", "attacker")
            .is_err()
    );
    assert!(
        db.issue_tenant_admin(&signup.secret, "legacy", "attacker")
            .is_err()
    );
    assert!(db.inspect_tenant(&signup.secret, "legacy").is_err());
    assert!(db.issue_global(&signup.secret, provisioner()).is_err());
    assert!(
        db.rotate_global(&signup.secret, master.credential.id, 1)
            .is_err()
    );
    assert!(
        db.revoke_global(&signup.secret, master.credential.id, 1)
            .is_err()
    );
    assert!(
        db.inspect_global(&signup.secret, master.credential.id)
            .is_err()
    );
    assert!(db.authenticate(&existing.secret).is_ok());
    assert!(
        db.inspect_tenant(&master.secret, "legacy")
            .unwrap()
            .is_none()
    );
    let appointed = db
        .issue_tenant_admin(&master.secret, "legacy", "new-owner")
        .unwrap();
    assert_eq!(appointed.credential.tenant_id, "legacy");
    assert_eq!(
        appointed.credential.history[0].action,
        "admin_issued_by_global_authority"
    );
    assert!(
        db.issue_tenant_admin(&master.secret, "absent", "owner")
            .is_err()
    );
}

#[test]
fn global_delegation_cannot_expand_capabilities_or_outlive_issuer() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let master = db.bootstrap_master("master").unwrap();
    let expires = chrono::Utc::now().timestamp_millis() + 60_000;
    let limited = db
        .issue_global(
            &master.secret,
            GlobalCredentialSpec {
                subject_id: "limited-admin".into(),
                capabilities: [
                    GlobalCapability::TenantCreate,
                    GlobalCapability::GlobalCredentialAdmin,
                ]
                .into(),
                expires_at_millis: Some(expires),
            },
        )
        .unwrap();
    assert!(db.issue_global(&limited.secret, provisioner()).is_err());
    let mut spec = provisioner();
    spec.expires_at_millis = Some(expires);
    let child = db.issue_global(&limited.secret, spec.clone()).unwrap();
    spec.capabilities.insert(GlobalCapability::TenantAdmin);
    assert!(db.issue_global(&limited.secret, spec).is_err());
    assert!(db.authenticated_global(&child.secret, expires).is_err());
    assert!(db.authenticated_global(&child.secret, expires - 1).is_ok());
    assert!(
        db.inspect_global(&limited.secret, master.credential.id)
            .is_err()
    );
    // Even a full delegated administrator cannot replace or revoke the bootstrap master.
    let full = db
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
        db.rotate_global(&full.secret, master.credential.id, 1)
            .is_err()
    );
}

#[test]
fn global_rotation_revocation_and_explicit_offline_recovery_survive_reopen() {
    let dir = TempDir::new().unwrap();
    let (master, rotated, signup) = {
        let db = store(dir.path());
        let master = db.bootstrap_master("master").unwrap();
        let signup = db.issue_global(&master.secret, provisioner()).unwrap();
        assert!(
            db.rotate_global(&master.secret, master.credential.id, 99)
                .is_err()
        );
        let rotated = db
            .rotate_global(&master.secret, master.credential.id, 1)
            .unwrap();
        assert!(db.authenticate_global(&master.secret).is_err());
        assert!(
            db.register_tenant(&master.secret, "forbidden", "owner")
                .is_err()
        );
        db.revoke_global(&rotated.secret, signup.credential.id, 1)
            .unwrap();
        assert!(
            db.register_tenant(&signup.secret, "revoked", "owner")
                .is_err()
        );
        db.revoke_global(&rotated.secret, rotated.credential.id, 2)
            .unwrap();
        assert!(db.bootstrap_master("reset").is_err());
        (master, rotated, signup)
    };
    let db = store(dir.path());
    for token in [&master.secret, &rotated.secret, &signup.secret] {
        assert!(db.authenticate_global(token).is_err());
    }
    assert!(db.recover_master(2).is_err());
    let recovered = db.recover_master(3).unwrap();
    assert_eq!(recovered.credential.revision, 4);
    assert_eq!(
        recovered.credential.history[3].action,
        "recovered_by_local_operator"
    );
    assert!(db.authenticate_global(&recovered.secret).is_ok());
    assert!(db.authenticate_global(&signup.secret).is_err());
}

#[test]
fn concurrent_tenant_registration_has_one_atomic_winner_and_no_authority_reset() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(store(dir.path()));
    let master = db.bootstrap_master("master").unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let db = db.clone();
            let token = master.secret.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                db.register_tenant(&token, "raced-company", "owner")
            })
        })
        .collect();
    let results: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    let winner = results.into_iter().find_map(Result::ok).unwrap();
    assert_eq!(
        db.inspect_tenant(&master.secret, "raced-company")
            .unwrap()
            .unwrap()
            .initial_admin_id,
        winner.1.credential.id
    );
    assert!(db.authenticate(&winner.1.secret).is_ok());
}

#[test]
fn corrupt_global_authority_and_noncanonical_tokens_fail_closed() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let master = db.bootstrap_master("master").unwrap();
    for token in [
        "é".repeat(82),
        master.secret.replace("qdbg1_", "qdb1_"),
        format!("{}=", master.secret),
    ] {
        assert!(db.authenticate_global(&token).is_err());
    }
    let key = global_key(master.credential.id);
    let (mut record, previous) = db.read_global(master.credential.id).unwrap();
    record.credential.is_master = false;
    record.credential.revision = 10;
    db.commit(
        vec![condition(&key, Some(previous))],
        vec![write(&key, serialize(&record).unwrap())],
    )
    .unwrap();
    assert!(matches!(
        db.authenticate_global(&master.secret),
        Err(Error::DataCorruption(_))
    ));
}
