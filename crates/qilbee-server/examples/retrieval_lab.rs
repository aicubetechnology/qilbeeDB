//! An isolated loopback server using the real platform router for reproducible trials.
use qilbee_graph::Database;
use qilbee_server::security::identity::{
    Capability, CredentialSpec, IdentityStore, ResourceScope, Visibility,
};
use std::{fs, io::Write, path::PathBuf, sync::Arc};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 3 {
        return Err(
            "Usage: retrieval_lab NEW_DATA_DIRECTORY NEW_CREDENTIAL_FILE LOOPBACK_PORT".into(),
        );
    }
    let data = PathBuf::from(&args[0]);
    let credentials = PathBuf::from(&args[1]);
    if data.exists() || credentials.exists() {
        return Err(
            "Use fresh paths; this laboratory never overwrites an existing database or credential"
                .into(),
        );
    }
    let listener =
        tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, args[2].parse::<u16>()?))
            .await?;
    let database = Arc::new(Database::open(&data)?);
    let identity = IdentityStore::new(Arc::new(database.storage().clone()));
    let owner = identity.bootstrap_tenant("retrieval-laboratory", "operator")?;
    let scope = ResourceScope {
        project_id: "frozen-retrieval".into(),
        agent_id: "evaluator".into(),
        mission_id: None,
        visibility: Visibility::Private,
    };
    let key = identity.issue(
        &owner.secret,
        CredentialSpec {
            scope_policy: None,
            subject_id: "evaluator".into(),
            capabilities: [Capability::MemoryRead, Capability::MemoryWrite].into(),
            grants: vec![scope.clone()],
            expires_at_millis: Some(chrono::Utc::now().timestamp_millis() + 24 * 60 * 60 * 1000),
        },
    )?;
    let router = qilbee_server::http_server::create_router(database)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(credentials)?;
    serde_json::to_writer_pretty(
        &mut file,
        &serde_json::json!({"api_url":format!("http://{}",listener.local_addr()?),"secret":key.secret,"scope":scope,"credential":key.credential}),
    )?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    println!(
        "Retrieval laboratory listening on {}; PID {}",
        listener.local_addr()?,
        std::process::id()
    );
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
