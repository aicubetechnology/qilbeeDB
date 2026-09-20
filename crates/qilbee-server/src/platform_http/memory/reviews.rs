//! Review authority is separate from content creation and retrieval.
use super::*;
use qilbee_memory::storage::platform::MemoryReviewCommand;

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/memory/reviews", post(review))
        .route("/api/v1/memory/reviews/state", post(current))
        .route("/api/v1/memory/reviews/read", post(read_review))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewRequest {
    scope: ResourceScope,
    command: MemoryReviewCommand,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StateRequest {
    contract_version: u32,
    scope: ResourceScope,
    record_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    contract_version: u32,
    scope: ResourceScope,
    record_id: Uuid,
    revision: u64,
}
fn missing() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "review_not_found",
        "No matching memory review state exists in the authorized scope",
    )
}
async fn review(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ReviewRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.command.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryReview, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = memory
                .review_memory_record(
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
async fn current(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<StateRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryReview, &request.scope)
                .map_err(ApiError::operation)?;
            let current = memory
                .memory_review_state(&scope.storage_namespace, request.record_id)
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(
                json!({"contract_version":1,"scope":request.scope,"state":current}),
            ))
        })
        .await
}
async fn read_review(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ReadRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryReview, &request.scope)
                .map_err(ApiError::operation)?;
            let receipt = memory
                .read_memory_review(
                    &scope.storage_namespace,
                    request.record_id,
                    request.revision,
                )
                .map_err(ApiError::operation)?
                .ok_or_else(missing)?;
            Ok(Json(json!({"contract_version":1,"receipt":receipt})))
        })
        .await
}
