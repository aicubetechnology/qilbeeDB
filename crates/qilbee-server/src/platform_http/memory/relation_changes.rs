//! History-bound typed relation delivery and subject-owned consumer progress.
use super::*;
use qilbee_memory::storage::platform::{
    RelationChangeCursor, RelationChangesQuery, RelationCheckpointCommand,
};
pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/memory/relations/changes", post(changes))
        .route("/api/v1/memory/relations/changes/activate", post(activate))
        .route("/api/v1/memory/relations/checkpoints", post(commit))
        .route("/api/v1/memory/relations/checkpoints/read", post(read))
        .route(
            "/api/v1/memory/relations/checkpoints/revision",
            post(revision),
        )
        .route(
            "/api/v1/memory/relations/consumers/diagnose",
            post(diagnose),
        )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangesRequest {
    contract_version: u32,
    scope: ResourceScope,
    query: RelationChangesQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ActivateRequest {
    contract_version: u32,
    scope: ResourceScope,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommitRequest {
    scope: ResourceScope,
    command: RelationCheckpointCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    contract_version: u32,
    scope: ResourceScope,
    consumer_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionRequest {
    contract_version: u32,
    scope: ResourceScope,
    consumer_id: String,
    revision: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DiagnoseRequest {
    contract_version: u32,
    scope: ResourceScope,
    consumer_id: String,
    witness: Option<RelationChangeCursor>,
}

async fn changes(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ChangesRequest>, JsonRejection>,
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

            let _permit = limits.acquire()?;
            let page = memory
                .relation_changes(&scope.storage_namespace, &request.query)
                .map_err(ApiError::operation)?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"page":page}))
                    .into_response(),
            )
        })
        .await
}

async fn activate(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ActivateRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryWrite, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let baseline = memory
                .activate_relation_changes(&scope.storage_namespace)
                .map_err(ApiError::operation)?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"baseline":baseline}))
                    .into_response(),
            )
        })
        .await
}

async fn commit(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CommitRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
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
            let _permit = limits.acquire()?;
            let receipt = memory
                .commit_relation_checkpoint(
                    &scope.storage_namespace,
                    &RecordAuthor {
                        credential_id: scope.credential_id,
                        subject_id: scope.subject_id,
                    },
                    &request.command,
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
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryCheckpoint, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let checkpoint = memory
                .read_relation_checkpoint(
                    &scope.storage_namespace,
                    &scope.subject_id,
                    &request.consumer_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::NOT_FOUND,
                        "checkpoint_not_found",
                        "No relation checkpoint exists for this subject and consumer",
                    )
                })?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"checkpoint":checkpoint}))
                    .into_response(),
            )
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
                .authorize(token, Capability::MemoryCheckpoint, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let receipt = memory
                .relation_checkpoint_revision(
                    &scope.storage_namespace,
                    &scope.subject_id,
                    &request.consumer_id,
                    request.revision,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::NOT_FOUND,
                        "checkpoint_not_found",
                        "No relation checkpoint revision exists for this subject and consumer",
                    )
                })?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"receipt":receipt}))
                    .into_response(),
            )
        })
        .await
}

async fn diagnose(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<DiagnoseRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryCheckpoint, &request.scope)
                .map_err(ApiError::operation)?;
            identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let diagnostics = memory
                .diagnose_relation_consumer(
                    &scope.storage_namespace,
                    &scope.subject_id,
                    &request.consumer_id,
                    request.witness.as_ref(),
                )
                .map_err(ApiError::operation)?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"diagnostics":diagnostics}))
                    .into_response(),
            )
        })
        .await
}
