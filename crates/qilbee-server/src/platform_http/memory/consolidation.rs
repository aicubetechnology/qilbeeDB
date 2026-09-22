//! Durable external consolidation transport; provider execution stays outside the database.
use super::*;
use qilbee_memory::storage::platform::{
    ConsolidationCommand, ConsolidationOperation, ConsolidationQuery,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandRequest {
    contract_version: u32,
    scope: ResourceScope,
    idempotency_key: String,
    operation: ConsolidationOperation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    contract_version: u32,
    scope: ResourceScope,
    job_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionRequest {
    contract_version: u32,
    scope: ResourceScope,
    job_id: Uuid,
    revision: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRequest {
    contract_version: u32,
    scope: ResourceScope,
    query: ConsolidationQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextRequest {
    contract_version: u32,
    scope: ResourceScope,
    job_id: Uuid,
    expected_revision: u64,
    fence: Uuid,
}

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/memory/consolidation/commands", post(command))
        .route("/api/v1/memory/consolidation/inspect", post(inspect))
        .route("/api/v1/memory/consolidation/revision", post(revision))
        .route("/api/v1/memory/consolidation/query", post(query))
        .route("/api/v1/memory/consolidation/context", post(context))
}
fn unavailable() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "record_not_found",
        "No consolidation job or revision is available in the authorized scope and subject",
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
            identity
                .authorize(token, Capability::MemoryWrite, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let receipt = memory
                .apply_consolidation_command(
                    &scope.storage_namespace,
                    &RecordAuthor {
                        credential_id: scope.credential_id,
                        subject_id: scope.subject_id,
                    },
                    &ConsolidationCommand {
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
async fn inspect(
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
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let inspection = memory
                .inspect_consolidation_job(
                    &scope.storage_namespace,
                    &scope.subject_id,
                    request.job_id,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(unavailable)?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"inspection":inspection}))
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
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let history = memory
                .consolidation_job_revision(
                    &scope.storage_namespace,
                    &scope.subject_id,
                    request.job_id,
                    request.revision,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(unavailable)?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"history":history}))
                    .into_response(),
            )
        })
        .await
}
async fn query(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<QueryRequest>, JsonRejection>,
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
                .query_consolidation_jobs(
                    &scope.storage_namespace,
                    &scope.subject_id,
                    &request.query,
                )
                .map_err(ApiError::operation)?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"page":page}))
                    .into_response(),
            )
        })
        .await
}
async fn context(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ContextRequest>, JsonRejection>,
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
                .authorize(token, Capability::MemoryWrite, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let context = memory
                .read_consolidation_context(
                    &scope.storage_namespace,
                    &RecordAuthor {
                        credential_id: scope.credential_id,
                        subject_id: scope.subject_id,
                    },
                    request.job_id,
                    request.expected_revision,
                    request.fence,
                )
                .map_err(ApiError::operation)?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"context":context}))
                    .into_response(),
            )
        })
        .await
}
