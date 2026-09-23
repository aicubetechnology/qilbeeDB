//! Date-ordered listing without changing the existing UUID continuation contract.
use super::*;
use qilbee_memory::storage::platform::{
    CompanyMemorySelection, CompanyMemoryView, OrderedMemoryQuery,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ScopedRequest {
    contract_version: u32,
    scope: ResourceScope,
    query: OrderedMemoryQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompanyRequest {
    contract_version: u32,
    #[serde(default)]
    selection: CompanyMemorySelection,
    #[serde(default)]
    view: CompanyMemoryView,
    query: OrderedMemoryQuery,
}

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/memory/query/ordered", post(scoped))
        .route("/api/v1/company/memory/query/ordered", post(company))
}
async fn scoped(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ScopedRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |identity, token, _| {
            let request = json_body(body)?;
            version(request.contract_version)?;
            let authorized = identity
                .authorize(token, Capability::MemoryRead, &request.scope)
                .map_err(ApiError::operation)?;
            let _permit = limits.acquire()?;
            let page = memory
                .query_ordered_memory(&authorized.storage_namespace, &request.query)
                .map_err(ApiError::operation)?;
            Ok(
                Json(json!({"contract_version":1,"scope":request.scope,"page":page}))
                    .into_response(),
            )
        })
        .await
}
async fn company(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CompanyRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |_, _, principal| {
            require_admin(&principal)?;
            let request = json_body(body)?;
            version(request.contract_version)?;
            let _permit = limits.acquire()?;
            let page = memory
                .query_ordered_company_memory(
                    &principal.tenant_id,
                    &request.selection,
                    request.view,
                    &request.query,
                )
                .map_err(ApiError::operation)?;
            Ok(
                Json(json!({"contract_version":1,"company_id":principal.tenant_id,"page":page}))
                    .into_response(),
            )
        })
        .await
}
