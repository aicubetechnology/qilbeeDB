use super::*;
use qilbee_storage::StorageOptions;
use tempfile::TempDir;

fn store(path: &std::path::Path) -> IdentityStore {
    IdentityStore::new(Arc::new(
        StorageEngine::open(StorageOptions::for_testing(path)).unwrap(),
    ))
}
fn policy() -> CompanyScopePolicy {
    CompanyScopePolicy {
        version: CompanyScopePolicyVersion::CompanyScopesV1,
        projects: IdSelector::All {},
        agents: IdSelector::All {},
        missions: IdSelector::All {},
        allow_unassigned_mission: true,
        visibilities: vec![Visibility::Private, Visibility::Shared],
    }
}
fn spec(subject: &str) -> CredentialSpec {
    CredentialSpec {
        subject_id: subject.into(),
        capabilities: [Capability::MemoryRead, Capability::MemoryWrite].into(),
        grants: vec![],
        scope_policy: Some(policy()),
        expires_at_millis: None,
    }
}
fn scope() -> ResourceScope {
    ResourceScope {
        project_id: "external/project:α".into(),
        mission_id: Some("external/mission:β".into()),
        agent_id: "external/agent:γ".into(),
        visibility: Visibility::Shared,
    }
}

#[test]
fn dynamic_scopes_preserve_company_subject_and_legacy_namespace_boundaries() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let a = db.bootstrap_tenant("a", "owner").unwrap();
    let b = db.bootstrap_tenant("b", "owner").unwrap();
    let alice = db.issue(&a.secret, spec("alice")).unwrap();
    let bob = db.issue(&a.secret, spec("bob")).unwrap();
    let other = db.issue(&b.secret, spec("alice")).unwrap();
    let shared = scope();
    let private = ResourceScope {
        visibility: Visibility::Private,
        ..shared.clone()
    };
    let authorized = |key: &str, scope: &ResourceScope| {
        db.authorize(key, Capability::MemoryRead, scope)
            .unwrap()
            .storage_namespace
    };
    assert_eq!(
        authorized(&alice.secret, &shared),
        authorized(&bob.secret, &shared)
    );
    assert_ne!(
        authorized(&alice.secret, &private),
        authorized(&bob.secret, &private)
    );
    assert_ne!(
        authorized(&alice.secret, &shared),
        authorized(&other.secret, &shared)
    );
    let exact = db
        .issue(
            &a.secret,
            CredentialSpec {
                scope_policy: None,
                grants: vec![shared.clone(), private.clone()],
                ..spec("alice")
            },
        )
        .unwrap();
    assert_eq!(
        authorized(&alice.secret, &shared),
        authorized(&exact.secret, &shared)
    );
    assert_eq!(
        authorized(&alice.secret, &private),
        authorized(&exact.secret, &private)
    );
    // A new ID remains consumer-owned; the authority derives no capability from it.
    let fresh = ResourceScope {
        agent_id: "new-agent-without-provisioning".into(),
        ..shared
    };
    assert!(
        db.authorize(&alice.secret, Capability::MemoryWrite, &fresh)
            .is_ok()
    );
    assert!(
        db.authorize(&alice.secret, Capability::CredentialAdmin, &fresh)
            .is_err()
    );
    let old: CredentialSpec =
        serde_json::from_str(r#"{"subject_id":"old","capabilities":["memory_read"],"grants":[]}"#)
            .unwrap();
    let old = db.issue(&a.secret, old).unwrap();
    assert!(
        db.authorize(&old.secret, Capability::MemoryRead, &fresh)
            .is_err()
    );
    assert!(
        serde_json::to_value(&old.credential.spec)
            .unwrap()
            .get("scope_policy")
            .is_none()
    );
}

#[test]
fn company_policy_restrictions_are_intersected_without_id_normalization() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let admin = db.bootstrap_tenant("company", "owner").unwrap();
    let allowed = scope();
    let definition = CredentialSpec {
        scope_policy: Some(CompanyScopePolicy {
            projects: IdSelector::Only {
                ids: vec![allowed.project_id.clone()],
            },
            agents: IdSelector::Only {
                ids: vec![allowed.agent_id.clone()],
            },
            missions: IdSelector::Only {
                ids: vec![allowed.mission_id.clone().unwrap()],
            },
            allow_unassigned_mission: false,
            visibilities: vec![Visibility::Shared],
            ..policy()
        }),
        ..spec("integration")
    };
    let key = db.issue(&admin.secret, definition.clone()).unwrap();
    assert!(
        db.authorize(&key.secret, Capability::MemoryWrite, &allowed)
            .is_ok()
    );
    for changed in [
        ResourceScope {
            project_id: format!("{} ", allowed.project_id),
            ..allowed.clone()
        },
        ResourceScope {
            agent_id: "other".into(),
            ..allowed.clone()
        },
        ResourceScope {
            mission_id: Some("other".into()),
            ..allowed.clone()
        },
        ResourceScope {
            mission_id: None,
            ..allowed.clone()
        },
        ResourceScope {
            visibility: Visibility::Private,
            ..allowed.clone()
        },
    ] {
        assert!(
            db.authorize(&key.secret, Capability::MemoryWrite, &changed)
                .is_err()
        );
    }
    let mut none = definition;
    none.scope_policy.as_mut().unwrap().agents = IdSelector::Only { ids: vec![] };
    let key = db.issue(&admin.secret, none).unwrap();
    assert!(
        db.authorize(&key.secret, Capability::MemoryWrite, &allowed)
            .is_err()
    );
}

#[test]
fn policy_mutations_preserve_secret_and_namespace_and_survive_restart() {
    let dir = TempDir::new().unwrap();
    let (admin, key, before) = {
        let db = store(dir.path());
        let admin = db.bootstrap_tenant("a", "owner").unwrap();
        let other = db.bootstrap_tenant("b", "owner").unwrap();
        let key = db.issue(&admin.secret, spec("integration")).unwrap();
        let before = db
            .authorize(&key.secret, Capability::MemoryRead, &scope())
            .unwrap()
            .storage_namespace;
        let narrow = ScopeAuthority {
            grants: vec![scope()],
            scope_policy: None,
        };
        assert!(
            db.set_scope_authority(&other.secret, key.credential.id, 1, narrow.clone())
                .is_err()
        );
        assert!(
            db.set_scope_authority(&key.secret, key.credential.id, 1, narrow.clone())
                .is_err()
        );
        let changed = db
            .set_scope_authority(&admin.secret, key.credential.id, 1, narrow.clone())
            .unwrap();
        assert_eq!(changed.revision, 2);
        assert_eq!(
            changed.history[1]
                .scope_authority_change
                .as_ref()
                .unwrap()
                .previous
                .scope_policy,
            Some(policy())
        );
        assert_eq!(
            changed.history[1]
                .scope_authority_change
                .as_ref()
                .unwrap()
                .current,
            narrow
        );
        assert!(
            db.set_scope_authority(&admin.secret, key.credential.id, 1, narrow)
                .is_err()
        );
        (admin, key, before)
    };
    let db = store(dir.path());
    assert_eq!(db.authenticate(&key.secret).unwrap().revision, 2);
    assert_eq!(
        before,
        db.authorize(&key.secret, Capability::MemoryRead, &scope())
            .unwrap()
            .storage_namespace
    );
    assert!(
        db.authorize(
            &key.secret,
            Capability::MemoryRead,
            &ResourceScope {
                agent_id: "new".into(),
                ..scope()
            }
        )
        .is_err()
    );
    let widened = ScopeAuthority {
        grants: vec![],
        scope_policy: Some(policy()),
    };
    db.set_scope_authority(&admin.secret, key.credential.id, 2, widened)
        .unwrap();
    let rotated = db.rotate(&admin.secret, key.credential.id, 3).unwrap();
    assert!(db.authenticate(&key.secret).is_err());
    assert_eq!(rotated.credential.spec.scope_policy, Some(policy()));
    assert!(
        db.authorize(&rotated.secret, Capability::MemoryRead, &scope())
            .is_ok()
    );
    db.revoke(&admin.secret, key.credential.id, 4).unwrap();
    assert!(db.authenticate(&rotated.secret).is_err());
    assert!(
        db.set_scope_authority(
            &admin.secret,
            key.credential.id,
            5,
            ScopeAuthority {
                grants: vec![],
                scope_policy: None
            }
        )
        .is_err()
    );
}

#[test]
fn concurrent_policy_changes_have_one_winner_and_one_complete_audit_event() {
    let dir = TempDir::new().unwrap();
    let db = Arc::new(store(dir.path()));
    let admin = db.bootstrap_tenant("company", "owner").unwrap();
    let key = db.issue(&admin.secret, spec("integration")).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let jobs: Vec<_> = (0..8)
        .map(|n| {
            let db = db.clone();
            let barrier = barrier.clone();
            let admin = admin.secret.clone();
            let id = key.credential.id;
            std::thread::spawn(move || {
                barrier.wait();
                db.set_scope_authority(
                    &admin,
                    id,
                    1,
                    ScopeAuthority {
                        grants: vec![ResourceScope {
                            agent_id: format!("agent-{n}"),
                            ..scope()
                        }],
                        scope_policy: None,
                    },
                )
            })
        })
        .collect();
    let wins: Vec<_> = jobs
        .into_iter()
        .filter_map(|job| job.join().unwrap().ok())
        .collect();
    assert_eq!(wins.len(), 1);
    let view = db.inspect(&admin.secret, key.credential.id).unwrap();
    assert_eq!(view.history.len(), 2);
    assert_eq!(
        view.history[1]
            .scope_authority_change
            .as_ref()
            .unwrap()
            .current
            .grants,
        wins[0].spec.grants
    );
}

#[test]
fn policies_reject_ambiguous_authority_and_unbounded_or_malformed_selectors() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let admin = db.bootstrap_tenant("company", "owner").unwrap();
    let mut invalid = Vec::new();
    let mut s = spec("integration");
    s.grants.push(scope());
    invalid.push(s);
    let mut s = spec("integration");
    s.capabilities.insert(Capability::CredentialAdmin);
    invalid.push(s);
    for ids in [
        vec!["same".into(), "same".into()],
        vec!["\n".into()],
        vec!["x".repeat(257)],
        (0..257).map(|n| n.to_string()).collect(),
    ] {
        let mut s = spec("integration");
        s.scope_policy.as_mut().unwrap().agents = IdSelector::Only { ids };
        invalid.push(s);
    }
    for visibilities in [vec![], vec![Visibility::Shared, Visibility::Shared]] {
        let mut s = spec("integration");
        s.scope_policy.as_mut().unwrap().visibilities = visibilities;
        invalid.push(s);
    }
    for s in invalid {
        assert!(db.issue(&admin.secret, s).is_err());
    }
    let value = serde_json::to_value(spec("integration")).unwrap();
    for field in [
        "projects",
        "agents",
        "missions",
        "allow_unassigned_mission",
        "visibilities",
        "version",
    ] {
        let mut missing = value.clone();
        missing["scope_policy"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(serde_json::from_value::<CredentialSpec>(missing).is_err());
    }
    let mut future = value.clone();
    future["scope_policy"]["version"] = "company_scopes_v2".into();
    assert!(serde_json::from_value::<CredentialSpec>(future).is_err());
    let mut forged = value.clone();
    forged["scope_policy"]["tenant_id"] = "other".into();
    assert!(serde_json::from_value::<CredentialSpec>(forged).is_err());
    let mut ambiguous = value;
    ambiguous["scope_policy"]["agents"]["ids"] = serde_json::json!(["ignored"]);
    assert!(serde_json::from_value::<CredentialSpec>(ambiguous).is_err());
}

#[test]
fn integration_expiry_is_checked_without_cached_authority() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let admin = db.bootstrap_tenant("company", "owner").unwrap();
    let expires = chrono::Utc::now().timestamp_millis() + 60_000;
    let key = db
        .issue(
            &admin.secret,
            CredentialSpec {
                expires_at_millis: Some(expires),
                ..spec("integration")
            },
        )
        .unwrap();
    assert!(db.authenticate_at(&key.secret, expires - 1).is_ok());
    assert!(db.authenticate_at(&key.secret, expires).is_err());
}

#[test]
fn inconsistent_policy_history_cannot_authorize_requests() {
    let dir = TempDir::new().unwrap();
    let db = store(dir.path());
    let admin = db.bootstrap_tenant("company", "owner").unwrap();
    let key = db.issue(&admin.secret, spec("integration")).unwrap();
    db.set_scope_authority(
        &admin.secret,
        key.credential.id,
        1,
        ScopeAuthority {
            grants: vec![scope()],
            scope_policy: None,
        },
    )
    .unwrap();
    db.set_scope_authority(
        &admin.secret,
        key.credential.id,
        2,
        ScopeAuthority {
            grants: vec![],
            scope_policy: Some(policy()),
        },
    )
    .unwrap();
    let storage_key = credential_key(key.credential.id);
    let bytes = db.storage.get_meta(&storage_key).unwrap().unwrap();
    let original: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let mut cases = Vec::new();
    let mut missing = original.clone();
    missing["credential"]["history"][1]
        .as_object_mut()
        .unwrap()
        .remove("scope_authority_change");
    cases.push(missing);
    let mut discontinuous = original.clone();
    discontinuous["credential"]["history"][2]["scope_authority_change"]["previous"]["grants"] =
        serde_json::json!([]);
    cases.push(discontinuous);
    let mut wrong_action = original.clone();
    wrong_action["credential"]["history"][1]["action"] = "rotated".into();
    cases.push(wrong_action);
    let mut current = original;
    current["credential"]["spec"]["scope_policy"]["projects"] =
        serde_json::json!({"mode":"only","ids":[]});
    cases.push(current);
    for corrupted in cases {
        db.storage
            .put_meta(&storage_key, &serde_json::to_vec(&corrupted).unwrap())
            .unwrap();
        assert!(matches!(
            db.authorize(&key.secret, Capability::MemoryRead, &scope()),
            Err(Error::DataCorruption(_))
        ));
    }
    db.storage.put_meta(&storage_key, &bytes).unwrap();
    assert!(
        db.authorize(&key.secret, Capability::MemoryRead, &scope())
            .is_ok()
    );
}
