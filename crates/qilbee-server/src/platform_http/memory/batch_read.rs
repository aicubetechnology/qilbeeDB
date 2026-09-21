//! One authorized scope, one snapshot, and no partial-success fallback.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BatchRequest {
    contract_version: u32,
    scope: ResourceScope,
    record_ids: Vec<Uuid>,
}

pub(super) fn routes() -> Router<PlatformState> {
    Router::new().route("/api/v1/memory/records/batch", post(read_batch))
}

async fn read_batch(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<BatchRequest>, JsonRejection>,
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
            let batch = memory
                .read_memory_records(&scope.storage_namespace, &request.record_ids)
                .map_err(ApiError::operation)?;
            // Keep admission through response serialization, including large batches.
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"batch":batch}))
                    .into_response(),
            )
        })
        .await
}
