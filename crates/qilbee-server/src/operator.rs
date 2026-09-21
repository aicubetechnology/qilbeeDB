//! Local operator commands; these are never exposed as anonymous HTTP routes.
use qilbee_core::{Error, Result};
use serde_json::Value;

/// Parse an explicit local bootstrap command. A returned value contains secret
/// material and must be delivered only to the invoking operator, never logged.
pub fn bootstrap_command(args: &[String]) -> Result<Option<Value>> {
    if matches!(
        args.first().map(String::as_str),
        Some("bootstrap-master" | "recover-master")
    ) {
        if args.len() != 3 {
            return Err(Error::Configuration("Usage: qilbeedb bootstrap-master <data-directory> <subject-id> or recover-master <data-directory> <expected-revision>".into()));
        }
        let storage = qilbee_storage::StorageEngine::open(
            qilbee_storage::StorageOptions::for_production(&args[1]),
        )?;
        let identity = crate::security::identity::IdentityStore::new(std::sync::Arc::new(storage));
        let issued = if args[0] == "bootstrap-master" {
            identity.bootstrap_master(&args[2])?
        } else {
            identity.recover_master(args[2].parse().map_err(|_| {
                Error::Configuration("Expected a positive master revision".into())
            })?)?
        };
        return Ok(Some(
            serde_json::json!({"contract_version":1,"credential":issued.credential,"secret":issued.secret}),
        ));
    }
    if args.first().map(String::as_str) != Some("bootstrap-tenant") {
        return Ok(None);
    }
    if args.len() != 4 {
        return Err(Error::Configuration(
            "Usage: qilbeedb bootstrap-tenant <data-directory> <tenant-id> <subject-id>".into(),
        ));
    }
    let storage = qilbee_storage::StorageEngine::open(
        qilbee_storage::StorageOptions::for_production(&args[1]),
    )?;
    let identity = crate::security::identity::IdentityStore::new(std::sync::Arc::new(storage));
    let issued = identity.bootstrap_tenant(&args[2], &args[3])?;
    Ok(Some(serde_json::json!({
        "contract_version": 1, "credential": issued.credential, "secret": issued.secret
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::identity::IdentityStore;
    use qilbee_storage::{StorageEngine, StorageOptions};
    use std::sync::Arc;
    use tempfile::TempDir;

    #[test]
    fn operator_bootstrap_is_explicit_persistent_and_one_time() {
        let dir = TempDir::new().unwrap();
        let args = vec![
            "bootstrap-tenant".into(),
            dir.path().to_str().unwrap().into(),
            "company".into(),
            "operator".into(),
        ];
        let result = bootstrap_command(&args).unwrap().unwrap();
        let secret = result["secret"].as_str().unwrap();
        assert_eq!(result["contract_version"], 1);
        assert_eq!(result["credential"]["tenant_id"], "company");
        assert!(bootstrap_command(&args).is_err());
        let store = IdentityStore::new(Arc::new(
            StorageEngine::open(StorageOptions::for_testing(dir.path())).unwrap(),
        ));
        assert_eq!(store.authenticate(secret).unwrap().tenant_id, "company");
    }

    #[test]
    fn operator_bootstrap_rejects_incomplete_arguments_without_implicit_defaults() {
        assert!(bootstrap_command(&[]).unwrap().is_none());
        assert!(bootstrap_command(&["./data".into()]).unwrap().is_none());
        assert!(bootstrap_command(&["bootstrap-tenant".into()]).is_err());
        assert!(
            bootstrap_command(&["bootstrap-tenant".into(), "data".into(), "company".into()])
                .is_err()
        );
    }

    #[test]
    fn operator_master_bootstrap_and_recovery_require_explicit_local_commands() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().to_str().unwrap().to_owned();
        let args = vec!["bootstrap-master".into(), path.clone(), "master".into()];
        let first = bootstrap_command(&args).unwrap().unwrap();
        assert_eq!(first["credential"]["is_master"], true);
        assert!(bootstrap_command(&args).is_err());
        assert!(bootstrap_command(&["bootstrap-master".into(), path.clone()]).is_err());
        assert!(
            bootstrap_command(&["recover-master".into(), path.clone(), "invalid".into()]).is_err()
        );
        assert!(bootstrap_command(&["recover-master".into(), path.clone(), "2".into()]).is_err());
        let recovered = bootstrap_command(&["recover-master".into(), path.clone(), "1".into()])
            .unwrap()
            .unwrap();
        let store = IdentityStore::new(Arc::new(
            StorageEngine::open(StorageOptions::for_testing(dir.path())).unwrap(),
        ));
        assert!(
            store
                .authenticate_global(first["secret"].as_str().unwrap())
                .is_err()
        );
        assert!(
            store
                .authenticate_global(recovered["secret"].as_str().unwrap())
                .is_ok()
        );
        assert_eq!(recovered["credential"]["revision"], 2);
    }
}
