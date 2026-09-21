//! Native company administration without synthesizing delegated scope credentials.
use super::*;
use axum::extract::{Query, rejection::QueryRejection};
use qilbee_memory::storage::platform::CompanyMemoryQuery;
use qilbee_memory::storage::platform::MemoryGraphQuery;
mod relations;

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .merge(relations::routes())
        .route("/api/v1/company/memory/workspaces", get(workspaces))
        .route("/api/v1/company/memory/query", post(query))
        .route("/api/v1/company/memory/read", post(read))
        .route("/api/v1/company/memory/graph", post(graph))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryQuery {
    contract_version: u32,
    #[serde(default = "default_limit")]
    limit: usize,
    after_workspace_id: Option<String>,
}
fn default_limit() -> usize {
    25
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InventoryQuery {
    contract_version: u32,
    workspace_id: String,
    filter: CompanyMemoryQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectionRequest {
    contract_version: u32,
    workspace_id: String,
    record_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GraphRequest {
    contract_version: u32,
    workspace_id: String,
    query: MemoryGraphQuery,
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
        let result = memory.read_company_memory_graph(&principal.tenant_id, &request.workspace_id, &request.query).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"workspace":result.workspace,"graph":result.graph})).into_response())
    }).await
}
fn unavailable() -> ApiError {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "record_not_found",
        "No retained workspace or record exists in the authorized company",
    )
}
async fn workspaces(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    query: Result<Query<DirectoryQuery>, QueryRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |_, _, principal| {
            require_admin(&principal)?;
            let Query(query) = query.map_err(|_| {
                ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "Invalid company workspace query",
                )
            })?;
            version(query.contract_version)?;
            let _permit = limits.acquire()?;
            let page = memory
                .company_memory_workspaces(
                    &principal.tenant_id,
                    query.after_workspace_id.as_deref(),
                    query.limit,
                )
                .map_err(ApiError::operation)?;
            Ok(
                Json(json!({"contract_version":1,"company_id":principal.tenant_id,"page":page}))
                    .into_response(),
            )
        })
        .await
}
async fn query(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<InventoryQuery>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
        require_admin(&principal)?;
        let request = json_body(body)?;
        version(request.contract_version)?;
        let _permit = limits.acquire()?;
        let inventory = memory.query_company_memory(&principal.tenant_id, &request.workspace_id, &request.filter).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"workspace":inventory.workspace,"page":inventory.page})).into_response())
    }).await
}
async fn read(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<InspectionRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
        require_admin(&principal)?;
        let request = json_body(body)?;
        version(request.contract_version)?;
        let _permit = limits.acquire()?;
        let inspection = memory.inspect_company_memory(&principal.tenant_id, &request.workspace_id, request.record_id).map_err(ApiError::operation)?.ok_or_else(unavailable)?;
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"workspace":inspection.workspace,"entry":inspection.entry,"record_bytes":inspection.record_bytes})).into_response())
    }).await
}
