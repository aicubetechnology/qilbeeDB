//! Scoped transport adapter for atomic versioned memory commands.
use super::*;
use crate::security::identity::{ResourceScope, Visibility};
use axum::extract::{Query, rejection::QueryRejection};
use qilbee_memory::storage::platform::{MemoryCommand, MemoryOperation, MemoryQuery, RecordAuthor};

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/memory/commands", post(command))
        .route("/api/v1/memory/records/:id", get(read))
        .route("/api/v1/memory/query", post(query))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CommandRequest {
    contract_version: u32,
    idempotency_key: String,
    scope: ResourceScope,
    operation: MemoryOperation,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRequest {
    contract_version: u32,
    scope: ResourceScope,
    filter: MemoryQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadQuery {
    contract_version: u32,
    project_id: String,
    mission_id: Option<String>,
    agent_id: String,
    visibility: Visibility,
}
async fn command(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CommandRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryWrite, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = memory
                .apply_memory_command(
                    &scope.storage_namespace,
                    &RecordAuthor {
                        credential_id: scope.credential_id,
                        subject_id: scope.subject_id,
                    },
                    &MemoryCommand {
                        contract_version: request.contract_version,
                        idempotency_key: request.idempotency_key,
                        operation: request.operation,
                    },
                )
                .map_err(ApiError::operation)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
        })
        .await
}
async fn read(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: Result<Query<ReadQuery>, QueryRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let Query(request) = request.map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Invalid memory scope query",
                )
            })?;
            version(request.contract_version)?;
            let resource = ResourceScope {
                project_id: request.project_id,
                mission_id: request.mission_id,
                agent_id: request.agent_id,
                visibility: request.visibility,
            };
            let scope = identity
                .authorize(token, Capability::MemoryRead, &resource)
                .map_err(ApiError::operation)?;
            let id = Uuid::parse_str(&id).map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Expected a record UUID",
                )
            })?;
            let record = memory
                .read_memory_record(&scope.storage_namespace, id)
                .map_err(ApiError::operation)?
                .ok_or_else(|| {
                    ApiError::new(
                        StatusCode::NOT_FOUND,
                        "record_not_found",
                        "No current record exists in the authorized scope",
                    )
                })?;
            Ok(Json(
                json!({"contract_version":1,"scope":resource,"record":record}),
            ))
        })
        .await
}
async fn query(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<QueryRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let page = memory
                .query_memory_records(&scope.storage_namespace, &request.filter)
                .map_err(ApiError::operation)?;
            Ok(Json(
                json!({"contract_version":1,"scope":request.scope,"page":page}),
            ))
        })
        .await
}
