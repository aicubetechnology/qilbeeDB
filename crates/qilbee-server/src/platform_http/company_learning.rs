//! Native company learning catalogs. Caller-selected tenants are never accepted.
use super::*;
use qilbee_memory::learning::{LearningCatalogQuery, LearningResourceRef};

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/company/learning/query", post(query))
        .route("/api/v1/company/learning/read", post(read))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogRequest {
    contract_version: u32,
    query: LearningCatalogQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InspectionRequest {
    contract_version: u32,
    resource: LearningResourceRef,
}

async fn query(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<CatalogRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let learning = state.learning.clone();
    let limits = state.retrieval_limits.clone();
    state
        .run(headers, move |_, _, principal| {
            require_admin(&principal)?;
            let request = json_body(body)?;
            version(request.contract_version)?;
            let _permit = limits.acquire()?;
            let page = learning
                .company_learning_catalog(&principal.tenant_id, &request.query)
                .map_err(ApiError::operation)?;
            Ok(
                Json(json!({"contract_version":1,"company_id":principal.tenant_id,"page":page}))
                    .into_response(),
            )
        })
        .await
}

async fn read(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<InspectionRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let learning = state.learning.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
        require_admin(&principal)?;
        let request = json_body(body)?;
        version(request.contract_version)?;
        let _permit = limits.acquire()?;
        let details = learning.inspect_company_learning_resource(&principal.tenant_id, &request.resource)
            .map_err(ApiError::operation)?.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "record_not_found", "No learning resource exists in the authorized company"))?;
        Ok(Json(json!({"contract_version":1,"company_id":principal.tenant_id,"resource":request.resource,"details":details})).into_response())
    }).await
}
