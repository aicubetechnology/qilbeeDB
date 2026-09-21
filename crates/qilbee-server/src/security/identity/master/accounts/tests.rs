use super::*;
use qilbee_storage::StorageOptions;
use tempfile::TempDir;

const PASSWORD: &str = "test-only-account-password";
fn store(path: &std::path::Path) -> IdentityStore {
    IdentityStore::new(Arc::new(
        StorageEngine::open(StorageOptions::for_testing(path)).unwrap(),
    ))
}
fn tenant(name: &str) -> LoginAuthority {
    LoginAuthority::Tenant {
        tenant_id: name.into(),
    }
}

#[test]
fn login_reopen_preserves_password_hash_sessions_and_authority_separation() {
    let dir = TempDir::new().unwrap();
    let (master, admin, account, global, session) = {
        let db = store(dir.path());
        let master = db.bootstrap_master("operator").unwrap();
        let (_, admin) = db
            .register_tenant(&master.secret, "company", "owner")
            .unwrap();
        let account = db
            .create_login_account(
                &admin.secret,
                false,
                "owner@example.test",
                PASSWORD,
                admin.credential.id,
            )
            .unwrap();
        db.create_login_account(
            &master.secret,
            true,
            "master",
            PASSWORD,
            master.credential.id,
        )
        .unwrap();
        let global = db
            .login(LoginAuthority::Global, "master", PASSWORD)
            .unwrap();
        let session = db
            .login(tenant("company"), "owner@example.test", PASSWORD)
            .unwrap();
        let bytes = String::from_utf8(
            db.storage
                .get_meta(&account_key(account.id))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert!(bytes.contains("$argon2id$v=19$m=19456,t=2,p=1$"));
        for secret in [
            PASSWORD,
            &admin.secret,
            &session.token,
            &session.token[73..],
        ] {
            assert!(!bytes.contains(secret));
        }
        assert!(!format!("{session:?}").contains(&session.token));
        (master, admin, account, global, session)
    };
    let db = store(dir.path());
    assert_eq!(
        db.authenticate(&session.token).unwrap().id,
        admin.credential.id
    );
    assert_eq!(
        db.authenticate_global(&global.token).unwrap().id,
        master.credential.id
    );
    assert!(db.authenticate_global(&session.token).is_err());
    assert!(db.authenticate(&global.token).is_err());
    assert!(db.register_tenant(&session.token, "evil", "owner").is_err());
    assert!(
        db.login(tenant("other"), "owner@example.test", PASSWORD)
            .is_err()
    );
    assert_eq!(
        db.inspect_login_account(&admin.secret, false, account.id)
            .unwrap()
            .username,
        "owner@example.test"
    );
}

#[test]
fn password_replacement_logout_disable_and_parent_rotation_invalidate_sessions() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let admin = db.bootstrap_tenant("company", "owner").unwrap();
    let account = db
        .create_login_account(&admin.secret, false, "owner", PASSWORD, admin.credential.id)
        .unwrap();
    let first = db.login(tenant("company"), "owner", PASSWORD).unwrap();
    let second = db.login(tenant("company"), "owner", PASSWORD).unwrap();
    db.logout(&first.token).unwrap();
    assert!(db.authenticate(&first.token).is_err());
    assert!(db.authenticate(&second.token).is_ok());
    assert!(db.logout(&admin.secret).is_err());
    db.change_login_account(
        &second.token,
        false,
        account.id,
        1,
        Some("replacement-password"),
    )
    .unwrap();
    assert!(db.authenticate(&second.token).is_err());
    assert!(db.authenticate(&admin.secret).is_ok());
    assert!(db.login(tenant("company"), "owner", PASSWORD).is_err());
    assert!(matches!(
        db.change_login_account(&admin.secret, false, account.id, 1, None),
        Err(Error::TransactionConflict(_))
    ));
    let third = db
        .login(tenant("company"), "owner", "replacement-password")
        .unwrap();
    let rotated = db.rotate(&admin.secret, admin.credential.id, 1).unwrap();
    assert!(db.authenticate(&third.token).is_err());
    let fourth = db
        .login(tenant("company"), "owner", "replacement-password")
        .unwrap();
    db.change_login_account(&rotated.secret, false, account.id, 2, None)
        .unwrap();
    assert!(db.authenticate(&fourth.token).is_err());
    assert!(
        db.login(tenant("company"), "owner", "replacement-password")
            .is_err()
    );
    assert!(db.authenticate(&rotated.secret).is_ok());
}

#[test]
fn five_failures_lock_durably_without_extending_lock_and_expiry_is_server_owned() {
    let dir = TempDir::new().unwrap();
    let now = chrono::Utc::now().timestamp_millis();
    {
        let db = store(dir.path());
        let admin = db.bootstrap_tenant("company", "owner").unwrap();
        db.create_login_account(&admin.secret, false, "owner", PASSWORD, admin.credential.id)
            .unwrap();
        for _ in 0..5 {
            assert!(
                db.login_at(tenant("company"), "owner", "wrong-password", now)
                    .is_err()
            );
        }
    }
    let db = store(dir.path());
    assert!(
        db.login_at(tenant("company"), "owner", PASSWORD, now + LOCK_MILLIS - 1)
            .is_err()
    );
    let session = db
        .login_at(tenant("company"), "owner", PASSWORD, now + LOCK_MILLIS)
        .unwrap();
    assert_eq!(
        session.expires_at_millis,
        now + LOCK_MILLIS + SESSION_MILLIS
    );
    assert!(
        db.authenticate_at(&session.token, session.expires_at_millis - 1)
            .is_ok()
    );
    assert!(
        db.authenticate_at(&session.token, session.expires_at_millis)
            .is_err()
    );
}

#[test]
fn account_management_cannot_cross_tenants_or_elevate_delegated_global_authority() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let master = db.bootstrap_master("master").unwrap();
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
    let a = db.bootstrap_tenant("a", "owner").unwrap();
    let b = db.bootstrap_tenant("b", "owner").unwrap();
    assert!(
        db.create_login_account(
            &delegated.secret,
            true,
            "evil",
            PASSWORD,
            master.credential.id
        )
        .is_err()
    );
    assert!(
        db.create_login_account(&a.secret, true, "evil", PASSWORD, master.credential.id)
            .is_err()
    );
    assert!(
        db.create_login_account(&a.secret, false, "evil", PASSWORD, b.credential.id)
            .is_err()
    );
    let account = db
        .create_login_account(&a.secret, false, "owner", PASSWORD, a.credential.id)
        .unwrap();
    assert!(
        db.inspect_login_account(&b.secret, false, account.id)
            .is_err()
    );
    assert!(
        db.change_login_account(&b.secret, false, account.id, 1, None)
            .is_err()
    );
    assert!(matches!(
        db.create_login_account(&a.secret, false, "owner", PASSWORD, a.credential.id),
        Err(Error::TransactionConflict(_))
    ));
    db.create_login_account(&b.secret, false, "owner", PASSWORD, b.credential.id)
        .unwrap();
    let session = db.login(tenant("a"), "owner", PASSWORD).unwrap();
    let mut forged = session.token.clone();
    forged.replace_range(4..5, "g");
    assert!(db.authenticate_global(&forged).is_err());
    db.revoke(&a.secret, a.credential.id, 1).unwrap();
    assert!(db.authenticate(&session.token).is_err());
    assert!(db.login(tenant("a"), "owner", PASSWORD).is_err());
}

#[test]
fn session_fence_prevents_administration_after_concurrent_account_disable() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let admin = db.bootstrap_tenant("company", "owner").unwrap();
    let account = db
        .create_login_account(&admin.secret, false, "owner", PASSWORD, admin.credential.id)
        .unwrap();
    let session = db.login(tenant("company"), "owner", PASSWORD).unwrap();
    let (_, old_bytes, _, parent_fence) = db
        .session_account(&session.token, chrono::Utc::now().timestamp_millis())
        .unwrap();
    db.change_login_account(&admin.secret, false, account.id, 1, None)
        .unwrap();
    let stale = vec![
        condition(&account_key(account.id), Some(old_bytes)),
        parent_fence,
    ];
    assert!(
        db.commit(stale, vec![write("test/stale-login-write", vec![1])])
            .is_err()
    );
    assert!(
        db.storage
            .get_meta("test/stale-login-write")
            .unwrap()
            .is_none()
    );
    assert!(
        db.commit_authenticated(
            &session.token,
            vec![],
            vec![write("test/stale-login-write", vec![1])]
        )
        .is_err()
    );
}

#[test]
fn sessions_are_bounded_and_eviction_does_not_invalidate_newer_sessions() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let admin = db.bootstrap_tenant("company", "owner").unwrap();
    let account = db
        .create_login_account(&admin.secret, false, "owner", PASSWORD, admin.credential.id)
        .unwrap();
    let first = db.login(tenant("company"), "owner", PASSWORD).unwrap();
    let (mut stored, bytes) = db.read_account(account.id).unwrap();
    // Fill the remaining slots with valid independent verifiers without repeating expensive hashing.
    for _ in 1..MAX_SESSIONS {
        stored.sessions.push(SessionVerifier {
            id: Uuid::new_v4(),
            verifier: vec![1; 32],
            parent_revision: 1,
            expires_at_millis: first.expires_at_millis,
        });
    }
    db.commit(
        vec![condition(&account_key(account.id), Some(bytes))],
        vec![write(&account_key(account.id), serialize(&stored).unwrap())],
    )
    .unwrap();
    let newest = db.login(tenant("company"), "owner", PASSWORD).unwrap();
    assert_eq!(
        db.read_account(account.id).unwrap().0.sessions.len(),
        MAX_SESSIONS
    );
    assert!(db.authenticate(&first.token).is_err());
    assert!(db.authenticate(&newest.token).is_ok());
    assert!(db.authenticate(&format!("{}A", newest.token)).is_err());
}

#[test]
fn global_master_sessions_preserve_bootstrap_rules_and_parent_expiry_caps_sessions() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let master = db.bootstrap_master("master").unwrap();
    let account = db
        .create_login_account(
            &master.secret,
            true,
            "master",
            PASSWORD,
            master.credential.id,
        )
        .unwrap();
    let session = db
        .login(LoginAuthority::Global, "master", PASSWORD)
        .unwrap();
    db.register_tenant(&session.token, "company", "owner")
        .unwrap();
    db.change_login_account(
        &session.token,
        true,
        account.id,
        1,
        Some("new-master-password"),
    )
    .unwrap();
    assert!(db.authenticate_global(&session.token).is_err());
    assert!(db.authenticate_global(&master.secret).is_ok());
    let now = chrono::Utc::now().timestamp_millis();
    let delegated = db
        .issue_global(
            &master.secret,
            GlobalCredentialSpec {
                subject_id: "temporary".into(),
                capabilities: [GlobalCapability::TenantInspect].into(),
                expires_at_millis: Some(now + 10000),
            },
        )
        .unwrap();
    db.create_login_account(
        &master.secret,
        true,
        "temporary",
        PASSWORD,
        delegated.credential.id,
    )
    .unwrap();
    let session = db
        .login(LoginAuthority::Global, "temporary", PASSWORD)
        .unwrap();
    assert_eq!(session.expires_at_millis, now + 10000);
    assert!(db.register_tenant(&session.token, "evil", "owner").is_err());
}
