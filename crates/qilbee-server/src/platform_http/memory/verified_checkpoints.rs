//! Subject-owned change-feed checkpoints with current credential authorization.
use super::*;
use qilbee_memory::storage::platform::VerifiedMemoryCheckpointCommand;
pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v2/memory/checkpoints", post(commit))
        .route("/api/v2/memory/checkpoints/read", post(read_checkpoint))
        .route("/api/v2/memory/consumers/diagnose", post(diagnose))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitRequest {
    scope: ResourceScope,
    command: VerifiedMemoryCheckpointCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    contract_version: u32,
    scope: ResourceScope,
    consumer_id: String,
}
async fn commit(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CommitRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            super::verified_changes::version_two(request.command.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryCheckpoint, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = memory
                .commit_verified_memory_checkpoint(
                    &scope.storage_namespace,
                    &RecordAuthor {
                        credential_id: scope.credential_id,
                        subject_id: scope.subject_id,
                    },
                    &request.command,
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":2,"receipt":receipt})))
        })
        .await
}
async fn read_checkpoint(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ReadRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state.run(headers,move|identity,token,_|{
        let request=json_body(body)?;super::verified_changes::version_two(request.contract_version)?;
        let scope=identity.authorize(token,Capability::MemoryCheckpoint,&request.scope).map_err(ApiError::operation)?;
        identity.authorize(token,Capability::MemoryRead,&request.scope).map_err(ApiError::operation)?;
        let checkpoint=memory.read_verified_memory_checkpoint(&scope.storage_namespace,&scope.subject_id,&request.consumer_id).map_err(ApiError::operation)?.ok_or_else(||ApiError::new(StatusCode::NOT_FOUND,"checkpoint_not_found","No checkpoint exists for this subject and consumer in the authorized scope"))?;
        Ok(Json(json!({"contract_version":2,"scope":request.scope,"checkpoint":checkpoint})))
    }).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagnoseRequest {
    contract_version: u32,
    scope: ResourceScope,
    consumer_id: String,
    witness: Option<qilbee_memory::storage::platform::VerifiedMemoryCursor>,
}
async fn diagnose(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<DiagnoseRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            super::verified_changes::version_two(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryCheckpoint, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let diagnostics = memory
                .diagnose_memory_consumer(
                    &scope.storage_namespace,
                    &scope.subject_id,
                    &request.consumer_id,
                    request.witness.as_ref(),
                )
                .map_err(ApiError::operation)?;
            Ok(Json(
                json!({"contract_version":2,"scope":request.scope,"diagnostics":diagnostics}),
            ))
        })
        .await
}
