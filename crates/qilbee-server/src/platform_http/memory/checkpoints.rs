//! Subject-owned change-feed checkpoints with current credential authorization.
use super::*;
use qilbee_memory::storage::platform::MemoryCheckpointCommand;
pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/memory/checkpoints", post(commit))
        .route("/api/v1/memory/checkpoints/read", post(read_checkpoint))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitRequest {
    scope: ResourceScope,
    command: MemoryCheckpointCommand,
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
            version(request.command.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryCheckpoint, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = memory
                .commit_memory_checkpoint(
                    &scope.storage_namespace,
                    &RecordAuthor {
                        credential_id: scope.credential_id,
                        subject_id: scope.subject_id,
                    },
                    &request.command,
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
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
        let request=json_body(body)?;version(request.contract_version)?;
        let scope=identity.authorize(token,Capability::MemoryCheckpoint,&request.scope).map_err(ApiError::operation)?;
        identity.authorize(token,Capability::MemoryRead,&request.scope).map_err(ApiError::operation)?;
        let checkpoint=memory.read_memory_checkpoint(&scope.storage_namespace,&scope.subject_id,&request.consumer_id).map_err(ApiError::operation)?.ok_or_else(||ApiError::new(StatusCode::NOT_FOUND,"checkpoint_not_found","No checkpoint exists for this subject and consumer in the authorized scope"))?;
        Ok(Json(json!({"contract_version":1,"scope":request.scope,"checkpoint":checkpoint})))
    }).await
}
