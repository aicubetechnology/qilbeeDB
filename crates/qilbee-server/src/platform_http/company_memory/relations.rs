//! Native company graph and retained relation audit without delegated agent keys.
use super::*;
use qilbee_memory::storage::platform::TypedMemoryGraphQuery;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphRequest {
    contract_version: u32,
    workspace_id: String,
    query: TypedMemoryGraphQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectRequest {
    contract_version: u32,
    workspace_id: String,
    relation_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionRequest {
    contract_version: u32,
    workspace_id: String,
    relation_id: Uuid,
    revision: u64,
}
pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/company/memory/graph/typed", post(graph))
        .route("/api/v1/company/memory/relations/inspect", post(inspect))
        .route("/api/v1/company/memory/relations/revision", post(revision))
}
async fn graph(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<GraphRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
        require_admin(&principal)?;
        let request = json_body(body)?;
        version(request.contract_version)?;
        let _permit = limits.acquire()?;
        let result = memory.read_company_memory_typed_graph(&principal.tenant_id, &request.workspace_id, &request.query).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"workspace":result.workspace,"graph":result.graph})).into_response())
    }).await
}
async fn inspect(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<InspectRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
        require_admin(&principal)?;
        let request = json_body(body)?;
        version(request.contract_version)?;
        let _permit = limits.acquire()?;
        let result = memory.inspect_company_memory_relation(&principal.tenant_id, &request.workspace_id, request.relation_id).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"workspace":result.workspace,"inspection":result.inspection})).into_response())
    }).await
}
async fn revision(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<RevisionRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
        require_admin(&principal)?;
        let request = json_body(body)?;
        version(request.contract_version)?;
        let _permit = limits.acquire()?;
        let result = memory.company_memory_relation_revision(&principal.tenant_id, &request.workspace_id, request.relation_id, request.revision).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"workspace":result.workspace,"history":result.history})).into_response())
    }).await
}
