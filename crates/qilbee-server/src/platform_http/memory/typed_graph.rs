//! Scoped typed neighbors retain canonical endpoint and assertion authority.
use super::*;
use qilbee_memory::storage::platform::TypedMemoryGraphQuery;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphRequest {
    contract_version: u32,
    scope: ResourceScope,
    query: TypedMemoryGraphQuery,
}
pub(super) fn routes() -> Router<PlatformState> {
    Router::new().route("/api/v1/memory/graph/typed", post(read_graph))
}
async fn read_graph(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<GraphRequest>, JsonRejection>,
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
            let graph = memory
                .read_memory_typed_graph(&scope.storage_namespace, &request.query)
                .map_err(ApiError::operation)?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"graph":graph}))
                    .into_response(),
            )
        })
        .await
}
