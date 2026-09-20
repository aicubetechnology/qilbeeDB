//! Versioned, scope-authorized journal continuations.
use super::*;
use qilbee_memory::storage::platform::VerifiedMemoryChangesQuery;
pub(super) fn routes() -> Router<PlatformState> {
    Router::new().route("/api/v2/memory/changes", post(changes))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangesRequest {
    contract_version: u32,
    scope: ResourceScope,
    query: VerifiedMemoryChangesQuery,
}
fn version_two(value: u32) -> ApiResult<()> {
    if value != 2 {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "Expected contract version 2",
        ));
    }
    Ok(())
}
async fn changes(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ChangesRequest>, JsonRejection>,
) -> ApiResult<Json<Value>> {
    let memory = state.memory.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version_two(request.contract_version)?;
            let scope = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let page = memory
                .verified_memory_changes(&scope.storage_namespace, &request.query)
                .map_err(ApiError::operation)?;
            Ok(Json(
                json!({"contract_version":2,"scope":request.scope,"page":page}),
            ))
        })
        .await
}
