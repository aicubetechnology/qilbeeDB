//! Read-only company administration of retained consumer progress.
use super::*;
use qilbee_memory::storage::platform::{
    CompanyConsumerQuery, CompanyConsumerRef, CompanyConsumerWitness,
};

pub(super) fn routes() -> Router<PlatformState> {
    Router::new()
        .route("/api/v1/company/memory/consumers/query", post(query))
        .route("/api/v1/company/memory/consumers/read", post(read))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRequest {
    contract_version: u32,
    query: CompanyConsumerQuery,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    contract_version: u32,
    consumer: CompanyConsumerRef,
    witness: Option<CompanyConsumerWitness>,
}
async fn query(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<QueryRequest>, JsonRejection>,
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
                .company_consumers(&principal.tenant_id, &request.query)
                .map_err(ApiError::operation)?;
            Ok(Json(
                json!({"contract_version": 1, "company_id": principal.tenant_id, "page": page}),
            )
            .into_response())
        })
        .await
}
async fn read(
    State(state): State<PlatformState>,
    headers: HeaderMap,
    body: Result<Json<ReadRequest>, JsonRejection>,
) -> ApiResult<Response> {
    let memory = state.memory.clone();
    let limits = state.retrieval_limits.clone();
    state.run(headers, move |_, _, principal| {
        require_admin(&principal)?;
        let request = json_body(body)?;
        version(request.contract_version)?;
        let _permit = limits.acquire()?;
        let details = memory.inspect_company_consumer(&principal.tenant_id, &request.consumer, request.witness.as_ref()).map_err(ApiError::operation)?.ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "record_not_found", "No retained consumer checkpoint exists in the authorized company"))?;
        Ok(Json(json!({"contract_version": 1, "company_id": principal.tenant_id, "consumer": request.consumer, "details": details})).into_response())
    }).await
}
