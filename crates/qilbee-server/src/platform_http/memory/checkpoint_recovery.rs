//! Explicit subject-owned recovery with immutable before/after receipts.
use super::*;
use qilbee_memory::storage::platform::VerifiedCheckpointRecoveryCommand;
pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v2/memory/checkpoints/recover", post(recover))
        .route("/api/v2/memory/checkpoints/recoveries/read", post(read))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecoverRequest {
    scope: ResourceScope,
    command: VerifiedCheckpointRecoveryCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    contract_version: u32,
    scope: ResourceScope,
    consumer_id: String,
    revision: u64,
}
async fn recover(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<RecoverRequest>, JsonRejection>,
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
                .recover_verified_memory_checkpoint(
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
async fn read(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ReadRequest>, JsonRejection>,
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
            let receipt = memory
                .read_verified_checkpoint_recovery(
                    &scope.storage_namespace,
                    &scope.subject_id,
                    &request.consumer_id,
                    request.revision,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::NOT_FOUND,
                        "recovery_not_found",
                        "No recovery receipt exists for this subject, consumer and revision",
                    )
                })?;
            Ok(Json(json!({"contract_version":2,"receipt":receipt})))
        })
        .await
}
