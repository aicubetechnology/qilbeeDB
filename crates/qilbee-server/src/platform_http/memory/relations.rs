//! Scoped typed assertions; review authority is independent from ordinary writes.
use super::*;
use qilbee_memory::storage::platform::{MemoryRelationCommand, MemoryRelationOperation};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandRequest {
    contract_version: u32,
    scope: ResourceScope,
    idempotency_key: String,
    operation: MemoryRelationOperation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    contract_version: u32,
    scope: ResourceScope,
    relation_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionRequest {
    contract_version: u32,
    scope: ResourceScope,
    relation_id: Uuid,
    revision: u64,
}
pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/memory/relations/commands", post(command))
        .route("/api/v1/memory/relations/read", post(read))
        .route("/api/v1/memory/relations/inspect", post(inspect))
        .route("/api/v1/memory/relations/revision", post(revision))
}
fn missing() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "record_not_found",
        "No relation is available in the authorized scope",
    )
}
async fn command(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CommandRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let capability = if matches!(&request.operation, MemoryRelationOperation::Review { .. })
            {
                Capability::MemoryReview
            } else {
                Capability::MemoryWrite
            };
            identity
                .authorize(token, capability, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let receipt = memory
                .apply_memory_relation_command(
                    &scope.storage_namespace,
                    &RecordAuthor {
                        credential_id: scope.credential_id,
                        subject_id: scope.subject_id,
                    },
                    &MemoryRelationCommand {
                        contract_version: request.contract_version,
                        idempotency_key: request.idempotency_key,
                        operation: request.operation,
                    },
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})).into_response())
        })
        .await
}
async fn read(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ReadRequest>, JsonRejection>,
) -> ApiResult<Response> {
    load(state, headers, body, false).await
}
async fn inspect(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ReadRequest>, JsonRejection>,
) -> ApiResult<Response> {
    load(state, headers, body, true).await
}
async fn load(
    state: PlatformState,
    headers: HeaderMap,
    body: Result<Json<ReadRequest>, JsonRejection>,
    inspect: bool,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            if inspect {
                identity
                    .authorize(token, Capability::MemoryReview, &request.scope)
                    .map_err(ApiError::operation)?;
            }
            let _permit = limits.acquire()?;
            let response = if inspect {
                let value = memory
                    .inspect_memory_relation(&scope.storage_namespace, request.relation_id)
                    .map_err(ApiError::operation)?
                    .ok_or_else(missing)?;
                json!({"contract_version":1,"scope":request.scope,"inspection":value})
            } else {
                let value = memory
                    .read_memory_relation(&scope.storage_namespace, request.relation_id)
                    .map_err(ApiError::operation)?
                    .ok_or_else(missing)?;
                json!({"contract_version":1,"scope":request.scope,"relation":value})
            };
            Ok(Json(response).into_response())
        })
        .await
}
async fn revision(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<RevisionRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::MemoryReview, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let history = memory
                .memory_relation_revision(
                    &scope.storage_namespace,
                    request.relation_id,
                    request.revision,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"history":history}))
                    .into_response(),
            )
        })
        .await
}
